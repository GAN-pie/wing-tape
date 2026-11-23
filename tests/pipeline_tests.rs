use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use wing_tape::pipeline::capture::{AudioSource, CaptureActor};
use wing_tape::pipeline::inference::{InferenceActor, InferenceEngine};
use wing_tape::pipeline::orchestrator::Orchestrator;
use wing_tape::pipeline::{
    AudioWindow, CaptureCommand, CaptureEvent, InferenceCommand, InferenceEvent, Prediction,
    SystemEvent, UserCommand,
};

struct MockAudioSource {
    sample_rate: u32,
    chunks_to_send: usize,
}

impl AudioSource for MockAudioSource {
    fn next_chunk(&mut self) -> Option<Vec<f32>> {
        if self.chunks_to_send > 0 {
            self.chunks_to_send -= 1;
            Some(vec![0.1; 1024])
        } else {
            None
        }
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

struct MockInferenceEngine {
    should_error: bool,
}

impl InferenceEngine for MockInferenceEngine {
    fn predict(&mut self, _window: &AudioWindow) -> Result<Prediction, String> {
        if self.should_error {
            return Err("Mock inference error".to_string());
        }
        Ok(Prediction {
            class_id: 42,
            probability: 0.99,
            timestamp: Instant::now(),
        })
    }
}

/// How long to wait for a command we expect the orchestrator to emit.
const EXPECT_TIMEOUT: Duration = Duration::from_millis(500);
/// How long to wait before concluding that no command was emitted.
const QUIET_TIMEOUT: Duration = Duration::from_millis(100);

fn sample_window() -> AudioWindow {
    AudioWindow {
        data: vec![0.0; 10],
        sample_rate: 48000,
        timestamp: Instant::now(),
    }
}

/// Drives the orchestrator in a background thread and gives tests typed access to
/// the commands it emits to the capture and inference actors.
struct OrchestratorHarness {
    event_tx: mpsc::Sender<SystemEvent>,
    capture_cmd_rx: mpsc::Receiver<CaptureCommand>,
    inference_cmd_rx: mpsc::Receiver<InferenceCommand>,
    handle: Option<thread::JoinHandle<()>>,
}

impl OrchestratorHarness {
    fn new() -> Self {
        Self::new_with_start_paused(false)
    }

    fn new_with_start_paused(start_paused: bool) -> Self {
        let (event_tx, event_rx) = mpsc::channel::<SystemEvent>();
        let (capture_cmd_tx, capture_cmd_rx) = mpsc::channel::<CaptureCommand>();
        let (inference_cmd_tx, inference_cmd_rx) = mpsc::channel::<InferenceCommand>();

        let orchestrator = Orchestrator::new(event_rx, capture_cmd_tx, inference_cmd_tx)
            .with_start_paused(start_paused);
        let handle = thread::spawn(move || {
            let mut orchestrator = orchestrator;
            orchestrator.run();
        });

        Self {
            event_tx,
            capture_cmd_rx,
            inference_cmd_rx,
            handle: Some(handle),
        }
    }

    fn send_user(&self, command: UserCommand) {
        self.event_tx
            .send(SystemEvent::User(command))
            .expect("orchestrator event channel closed");
    }

    fn send_window(&self) {
        self.event_tx
            .send(SystemEvent::Capture(CaptureEvent::WindowReady(
                sample_window(),
            )))
            .expect("orchestrator event channel closed");
    }

    fn expect_capture(&self) -> CaptureCommand {
        self.capture_cmd_rx
            .recv_timeout(EXPECT_TIMEOUT)
            .expect("expected a capture command")
    }

    fn expect_inference(&self) -> InferenceCommand {
        self.inference_cmd_rx
            .recv_timeout(EXPECT_TIMEOUT)
            .expect("expected an inference command")
    }

    fn expect_no_capture(&self) {
        match self.capture_cmd_rx.recv_timeout(QUIET_TIMEOUT) {
            Err(RecvTimeoutError::Timeout) => {}
            other => panic!("expected no capture command, got {other:?}"),
        }
    }

    fn expect_no_inference(&self) {
        match self.inference_cmd_rx.recv_timeout(QUIET_TIMEOUT) {
            Err(RecvTimeoutError::Timeout) => {}
            other => panic!("expected no inference command, got {other:?}"),
        }
    }
}

impl Drop for OrchestratorHarness {
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
fn test_orchestrator_runs_by_default() {
    let harness = OrchestratorHarness::new();

    // Running the orchestrator must start the capture actor without waiting for
    // an explicit Start command.
    assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

    harness.send_window();
    match harness.expect_inference() {
        InferenceCommand::Predict(window) => assert_eq!(window.data.len(), 10),
        other => panic!("expected Predict, got {other:?}"),
    }
}

#[test]
fn test_orchestrator_can_start_paused() {
    let harness = OrchestratorHarness::new_with_start_paused(true);

    // Capture still starts so that Resume can take effect immediately.
    assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

    // No prediction happens until the pipeline is resumed.
    harness.send_window();
    harness.expect_no_inference();

    harness.send_user(UserCommand::Resume);
    harness.expect_no_capture();

    harness.send_window();
    match harness.expect_inference() {
        InferenceCommand::Predict(_) => {}
        other => panic!("expected Predict after resume, got {other:?}"),
    }
}

#[test]
fn test_orchestrator_start_is_idempotent() {
    let harness = OrchestratorHarness::new();
    assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

    // The pipeline is already running, so Start must not start it twice.
    harness.send_user(UserCommand::Start);
    harness.expect_no_capture();

    harness.send_window();
    harness.expect_inference();
}

#[test]
fn test_orchestrator_pause_drops_windows_without_stopping_actors() {
    let harness = OrchestratorHarness::new();
    assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

    harness.send_user(UserCommand::Pause);

    // Pausing must not stop either actor: the capture stream stays open and the
    // inference thread (with its loaded model) stays alive for a fast resume.
    harness.expect_no_capture();
    harness.expect_no_inference();

    // Windows produced while paused are dropped instead of being predicted.
    harness.send_window();
    harness.expect_no_inference();
}

#[test]
fn test_orchestrator_resume_routes_windows_again() {
    let harness = OrchestratorHarness::new();
    assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

    harness.send_user(UserCommand::Pause);
    harness.expect_no_capture();
    harness.expect_no_inference();
    harness.send_window();
    harness.expect_no_inference();

    harness.send_user(UserCommand::Resume);
    harness.expect_no_capture();

    harness.send_window();
    match harness.expect_inference() {
        InferenceCommand::Predict(_) => {}
        other => panic!("expected Predict after resume, got {other:?}"),
    }
}

#[test]
fn test_orchestrator_stop_stops_actors_and_drops_windows() {
    let harness = OrchestratorHarness::new();
    assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

    harness.send_user(UserCommand::Stop);

    // Stop is the hard shutdown: both actors must receive it.
    assert!(matches!(harness.expect_capture(), CaptureCommand::Stop));
    assert!(matches!(harness.expect_inference(), InferenceCommand::Stop));

    harness.send_window();
    harness.expect_no_inference();
}

#[test]
fn test_orchestrator_start_after_stop_is_ignored() {
    let harness = OrchestratorHarness::new();
    assert!(matches!(harness.expect_capture(), CaptureCommand::Start));

    harness.send_user(UserCommand::Stop);
    assert!(matches!(harness.expect_capture(), CaptureCommand::Stop));
    assert!(matches!(harness.expect_inference(), InferenceCommand::Stop));

    // The inference actor has exited, so a new Start must not restart the capture
    // actor (which would queue Predict commands nobody consumes).
    harness.send_user(UserCommand::Start);
    harness.expect_no_capture();

    harness.send_window();
    harness.expect_no_inference();
}

#[test]
fn test_capture_actor_emits_window() {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let source = Box::new(MockAudioSource {
        sample_rate: 48000,
        chunks_to_send: 2,
    });

    let actor = CaptureActor::new(cmd_rx, event_tx, source);

    let actor_handle = thread::spawn(move || {
        actor.run();
    });

    // Start the actor
    cmd_tx.send(CaptureCommand::Start).unwrap();

    // Check for first window
    let event = event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Expected WindowReady event");
    if let SystemEvent::Capture(CaptureEvent::WindowReady(window)) = event {
        assert_eq!(window.data.len(), 1024);
    } else {
        panic!("Unexpected event type");
    }

    // Check for second window
    let event = event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Expected second WindowReady event");
    if let SystemEvent::Capture(CaptureEvent::WindowReady(_)) = event {
        // ok
    } else {
        panic!("Unexpected event type");
    }

    // Check for error (source exhausted)
    let event = event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Expected Error event");
    if let SystemEvent::Capture(CaptureEvent::Error(msg)) = event {
        assert_eq!(msg, "Source exhausted");
    } else {
        panic!("Unexpected event type");
    }

    let _ = cmd_tx.send(CaptureCommand::Stop);
    let _ = actor_handle.join();
}

#[test]
fn test_inference_actor_emits_prediction() {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let engine = Box::new(MockInferenceEngine {
        should_error: false,
    });

    let actor = InferenceActor::new(cmd_rx, event_tx, engine);

    let actor_handle = thread::spawn(move || {
        actor.run();
    });

    let window = AudioWindow {
        data: vec![0.0; 10],
        sample_rate: 48000,
        timestamp: Instant::now(),
    };

    cmd_tx.send(InferenceCommand::Predict(window)).unwrap();

    let event = event_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Expected Prediction event");
    if let SystemEvent::Inference(InferenceEvent::Prediction(prediction)) = event {
        assert_eq!(prediction.class_id, 42);
        assert_eq!(prediction.probability, 0.99);
    } else {
        panic!("Unexpected event type");
    }

    cmd_tx.send(InferenceCommand::Stop).unwrap();
    let _ = actor_handle.join();
}
