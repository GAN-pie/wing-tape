use crate::pipeline::{
    CaptureCommand, CaptureEvent, Detection, InferenceCommand, InferenceEvent, PipelineState,
    PipelineStatus, Prediction, SharedStatus, SystemEvent, UserCommand,
};
use crate::utils::labels::Labels;
use std::sync::{Arc, Mutex, mpsc};
use std::time::SystemTime;

pub struct Orchestrator {
    event_rx: mpsc::Receiver<SystemEvent>,
    capture_tx: mpsc::Sender<CaptureCommand>,
    inference_tx: mpsc::Sender<InferenceCommand>,
    state: PipelineState,
    status: SharedStatus,
    labels: Option<Arc<Labels>>,
}

impl Orchestrator {
    pub fn new(
        event_rx: mpsc::Receiver<SystemEvent>,
        capture_tx: mpsc::Sender<CaptureCommand>,
        inference_tx: mpsc::Sender<InferenceCommand>,
    ) -> Self {
        Self {
            event_rx,
            capture_tx,
            inference_tx,
            state: PipelineState::Running,
            status: Arc::new(Mutex::new(PipelineStatus::new())),
            labels: None,
        }
    }

    /// Overrides the default running state. When `start_paused` is true the
    /// orchestrator begins in `Paused` and drops captured windows until a
    /// `Resume` (or `Start`) command is received. Capture still starts so that
    /// resuming takes effect immediately.
    ///
    /// This writes through the same single transition path as every other state
    /// change, so the shared snapshot never disagrees with the internal state.
    pub fn with_start_paused(mut self, start_paused: bool) -> Self {
        self.set_state(if start_paused {
            PipelineState::Paused
        } else {
            PipelineState::Running
        });
        self
    }

    /// Supplies labels used to annotate recorded predictions.
    pub fn with_labels(mut self, labels: Arc<Labels>) -> Self {
        self.labels = Some(labels);
        self
    }

    /// Returns a handle to the shared status snapshot.
    ///
    /// Call this before moving the orchestrator into its thread; the handle can
    /// then be read from any thread (e.g. the web control actor).
    pub fn status(&self) -> SharedStatus {
        Arc::clone(&self.status)
    }

    /// Runs the event loop until every [`SystemEvent`] sender has been dropped.
    ///
    /// The orchestrator does not decide when the process shuts down: the owner
    /// (`main`) owns the lifecycle. On shutdown the owner sends
    /// [`UserCommand::Stop`] and then drops its sender, which lets this loop drain
    /// the queued command and exit. `Stop` itself is terminal (it stops the
    /// actors) but does not return from this loop, so a late event cannot be
    /// mistaken for a restart request.
    pub fn run(&mut self) {
        // Running by default: start capture as soon as the orchestrator runs.
        let _ = self.capture_tx.send(CaptureCommand::Start);

        while let Ok(event) = self.event_rx.recv() {
            match event {
                SystemEvent::Capture(capture_event) => self.handle_capture(capture_event),
                SystemEvent::Inference(inference_event) => self.handle_inference(inference_event),
                SystemEvent::User(user_command) => self.handle_user(user_command),
            }
        }
    }

    /// The single write path for state transitions: keeps `self.state` and the
    /// shared snapshot in sync.
    fn set_state(&mut self, state: PipelineState) {
        let changed = self.state != state;
        self.state = state;
        if let Ok(mut status) = self.status.lock() {
            status.state = state;
            if changed {
                status.updated_at = SystemTime::now();
            }
        }
    }

    fn handle_capture(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::WindowReady(window) => {
                // Pause gate: only feed the inference engine while the pipeline is
                // running. Dropping windows here keeps both actors alive, so a
                // resume does not require reopening the microphone or reloading
                // the model.
                if self.state == PipelineState::Running {
                    let _ = self.inference_tx.send(InferenceCommand::Predict(window));
                }
            }
            CaptureEvent::Error(e) => {
                eprintln!("Capture Error: {}", e);
                // The capture actor exits after reporting an error, so no audio can
                // flow again without a restart. Release the inference actor too and
                // mark the pipeline as failed.
                let _ = self.inference_tx.send(InferenceCommand::Stop);
                self.fail(e);
            }
        }
    }

    fn handle_inference(&mut self, event: InferenceEvent) {
        match event {
            InferenceEvent::Prediction(prediction) => {
                println!(
                    "Prediction: Class {}, Prob {:.4}",
                    prediction.class_id, prediction.probability
                );
                self.record_prediction(prediction);
            }
            InferenceEvent::Error(e) => {
                eprintln!("Inference Error: {}", e);
                // Non-fatal: record the error and keep the pipeline running. A
                // single bad window should not take down the microphone.
                if self.state != PipelineState::Stopped {
                    self.record_error(e);
                }
            }
        }
    }

    fn handle_user(&mut self, command: UserCommand) {
        match command {
            // `Start` is retained as an alias for `Resume` (HTTP exposes only
            // Pause/Resume). Both are no-ops while running, and both are rejected
            // from the terminal `Stopped`/`Failed` states.
            UserCommand::Start | UserCommand::Resume => {
                if self.state == PipelineState::Paused {
                    self.set_state(PipelineState::Running);
                }
            }
            UserCommand::Pause => {
                if self.state == PipelineState::Running {
                    self.set_state(PipelineState::Paused);
                }
            }
            // `Stop` is terminal: it stops both actors and cannot be undone
            // without a process restart. Not exposed over HTTP.
            UserCommand::Stop => {
                if self.state != PipelineState::Stopped {
                    let _ = self.capture_tx.send(CaptureCommand::Stop);
                    let _ = self.inference_tx.send(InferenceCommand::Stop);
                    self.set_state(PipelineState::Stopped);
                }
            }
        }
    }

    /// Records a terminal failure. Late errors after an explicit `Stop` are
    /// ignored so a stopped pipeline never resurrects as `Failed`.
    fn fail(&mut self, message: String) {
        if self.state == PipelineState::Stopped {
            return;
        }
        self.record_error(message);
        self.set_state(PipelineState::Failed);
    }

    fn record_error(&mut self, message: String) {
        if let Ok(mut status) = self.status.lock() {
            status.last_error = Some(message);
            status.updated_at = SystemTime::now();
        }
    }

    fn record_prediction(&mut self, prediction: Prediction) {
        if self.state == PipelineState::Stopped {
            return;
        }
        let label = self
            .labels
            .as_ref()
            .and_then(|labels| labels.get(prediction.class_id))
            .map(str::to_string);
        let detection = Detection {
            class_id: prediction.class_id,
            probability: prediction.probability,
            detected_at: SystemTime::now(),
            label,
        };
        if let Ok(mut status) = self.status.lock() {
            status.last_prediction = Some(detection);
            // A successful prediction means any transient inference error has
            // cleared.
            status.last_error = None;
            status.updated_at = SystemTime::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::AudioWindow;
    use std::thread;
    use std::time::Duration;

    const WAIT: Duration = Duration::from_millis(500);

    fn sample_window() -> AudioWindow {
        AudioWindow {
            data: vec![0.0; 10],
            sample_rate: 48000,
            timestamp: std::time::Instant::now(),
        }
    }

    /// Drives the orchestrator and exposes its emitted commands plus the shared
    /// status snapshot.
    struct Harness {
        event_tx: mpsc::Sender<SystemEvent>,
        capture_rx: mpsc::Receiver<CaptureCommand>,
        inference_rx: mpsc::Receiver<InferenceCommand>,
        status: SharedStatus,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl Harness {
        fn new(start_paused: bool, labels: Option<Arc<Labels>>) -> Self {
            let (event_tx, event_rx) = mpsc::channel();
            let (capture_tx, capture_rx) = mpsc::channel();
            let (inference_tx, inference_rx) = mpsc::channel();

            let mut orchestrator = Orchestrator::new(event_rx, capture_tx, inference_tx)
                .with_start_paused(start_paused);
            if let Some(labels) = labels {
                orchestrator = orchestrator.with_labels(labels);
            }
            let status = orchestrator.status();
            let handle = thread::spawn(move || {
                let mut orchestrator = orchestrator;
                orchestrator.run();
            });

            Self {
                event_tx,
                capture_rx,
                inference_rx,
                status,
                handle: Some(handle),
            }
        }

        fn state(&self) -> PipelineState {
            self.status.lock().unwrap().state
        }

        fn send_user(&self, command: UserCommand) {
            self.event_tx.send(SystemEvent::User(command)).unwrap();
        }

        fn send_window(&self) {
            self.event_tx
                .send(SystemEvent::Capture(CaptureEvent::WindowReady(
                    sample_window(),
                )))
                .unwrap();
        }

        fn send_capture_error(&self, message: &str) {
            self.event_tx
                .send(SystemEvent::Capture(CaptureEvent::Error(
                    message.to_string(),
                )))
                .unwrap();
        }

        fn send_prediction(&self, class_id: usize, probability: f32) {
            self.event_tx
                .send(SystemEvent::Inference(InferenceEvent::Prediction(
                    Prediction {
                        class_id,
                        probability,
                        timestamp: std::time::Instant::now(),
                    },
                )))
                .unwrap();
        }

        fn expect_capture(&self) -> CaptureCommand {
            self.capture_rx
                .recv_timeout(WAIT)
                .expect("expected capture command")
        }

        fn expect_inference(&self) -> InferenceCommand {
            self.inference_rx
                .recv_timeout(WAIT)
                .expect("expected inference command")
        }

        fn wait_for_state(&self, expected: PipelineState) {
            let deadline = std::time::Instant::now() + WAIT;
            loop {
                if self.state() == expected {
                    return;
                }
                if std::time::Instant::now() > deadline {
                    panic!(
                        "state did not become {expected:?}; still {:?}",
                        self.state()
                    );
                }
                thread::sleep(Duration::from_millis(5));
            }
        }

        fn wait_for_prediction(&self) -> Detection {
            let deadline = std::time::Instant::now() + WAIT;
            loop {
                if let Some(detection) = self.status.lock().unwrap().last_prediction.clone() {
                    return detection;
                }
                if std::time::Instant::now() > deadline {
                    panic!("no prediction was recorded");
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            // Dropping every SystemEvent sender makes Orchestrator::run return.
            let (event_tx, _) = mpsc::channel();
            let old = std::mem::replace(&mut self.event_tx, event_tx);
            drop(old);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    #[test]
    fn initial_snapshot_matches_start_paused() {
        let running = Harness::new(false, None);
        assert_eq!(running.state(), PipelineState::Running);
        assert!(matches!(running.expect_capture(), CaptureCommand::Start));

        let paused = Harness::new(true, None);
        assert_eq!(paused.state(), PipelineState::Paused);
        assert!(matches!(paused.expect_capture(), CaptureCommand::Start));
    }

    #[test]
    fn pause_and_resume_update_snapshot() {
        let harness = Harness::new(false, None);
        assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

        harness.send_user(UserCommand::Pause);
        harness.wait_for_state(PipelineState::Paused);

        // Pausing again is a no-op (idempotent, as required for the HTTP layer).
        harness.send_user(UserCommand::Pause);
        assert_eq!(harness.state(), PipelineState::Paused);

        harness.send_user(UserCommand::Resume);
        harness.wait_for_state(PipelineState::Running);

        // Resuming again is a no-op.
        harness.send_user(UserCommand::Resume);
        assert_eq!(harness.state(), PipelineState::Running);
    }

    #[test]
    fn prediction_is_recorded_with_wall_clock_and_label() {
        let labels = Arc::new(Labels::from_iter(["American Robin", "House Sparrow"]));
        let harness = Harness::new(false, Some(labels));
        assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

        let before = SystemTime::now();
        harness.send_prediction(1, 0.75);

        let detection = harness.wait_for_prediction();
        assert_eq!(detection.class_id, 1);
        assert_eq!(detection.probability, 0.75);
        assert_eq!(detection.label.as_deref(), Some("House Sparrow"));
        assert!(detection.detected_at >= before);
        assert!(detection.detected_at <= SystemTime::now());
    }

    #[test]
    fn capture_error_marks_failed_and_stops_inference() {
        let harness = Harness::new(false, None);
        assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

        harness.send_capture_error("Source exhausted");
        harness.wait_for_state(PipelineState::Failed);

        assert_eq!(
            harness.status.lock().unwrap().last_error.as_deref(),
            Some("Source exhausted")
        );
        assert!(matches!(harness.expect_inference(), InferenceCommand::Stop));
    }

    #[test]
    fn stop_is_terminal_and_cannot_be_resumed() {
        let harness = Harness::new(true, None);
        assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

        harness.send_user(UserCommand::Stop);
        assert!(matches!(harness.expect_capture(), CaptureCommand::Stop));
        assert!(matches!(harness.expect_inference(), InferenceCommand::Stop));
        harness.wait_for_state(PipelineState::Stopped);

        // Resume/Start after Stop must be ignored, and windows must be dropped.
        harness.send_user(UserCommand::Resume);
        harness.send_user(UserCommand::Start);
        harness.send_window();
        thread::sleep(Duration::from_millis(50));
        assert_eq!(harness.state(), PipelineState::Stopped);
        assert!(matches!(
            harness.inference_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }
}
