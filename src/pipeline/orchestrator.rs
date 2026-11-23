use crate::pipeline::{
    CaptureCommand, CaptureEvent, InferenceCommand, InferenceEvent, SystemEvent, UserCommand,
};
use std::sync::mpsc;

pub struct Orchestrator {
    event_rx: mpsc::Receiver<SystemEvent>,
    capture_tx: mpsc::Sender<CaptureCommand>,
    inference_tx: mpsc::Sender<InferenceCommand>,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Paused,
    Stopped,
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
            state: State::Running,
        }
    }

    /// Overrides the default running state. When `start_paused` is true the
    /// orchestrator begins in `Paused` and drops captured windows until a
    /// `Resume` (or `Start`) command is received. Capture still starts so that
    /// resuming takes effect immediately.
    pub fn with_start_paused(mut self, start_paused: bool) -> Self {
        self.state = if start_paused {
            State::Paused
        } else {
            State::Running
        };
        self
    }

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

    fn handle_capture(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::WindowReady(window) => {
                // Pause gate: only feed the inference engine while the pipeline is
                // running. Dropping windows here keeps both actors alive, so a
                // resume does not require reopening the microphone or reloading
                // the model.
                if self.state == State::Running {
                    let _ = self.inference_tx.send(InferenceCommand::Predict(window));
                }
            }
            CaptureEvent::Error(e) => {
                eprintln!("Capture Error: {}", e);
            }
        }
    }

    fn handle_inference(&mut self, event: InferenceEvent) {
        match event {
            InferenceEvent::Prediction(prediction) => {
                // This is where business logic lives.
                // For now, we just print it.
                println!(
                    "Prediction: Class {}, Prob {:.4}",
                    prediction.class_id, prediction.probability
                );
            }
            InferenceEvent::Error(e) => {
                eprintln!("Inference Error: {}", e);
            }
        }
    }

    fn handle_user(&mut self, command: UserCommand) {
        match command {
            UserCommand::Start => {
                if self.state != State::Stopped {
                    // The pipeline already runs by default, so Start only needs to
                    // bring a paused pipeline back to running.
                    self.state = State::Running;
                }
            }
            UserCommand::Pause => {
                if self.state == State::Running {
                    self.state = State::Paused;
                }
            }
            UserCommand::Resume => {
                if self.state == State::Paused {
                    self.state = State::Running;
                }
            }
            UserCommand::Stop => {
                if self.state != State::Stopped {
                    let _ = self.capture_tx.send(CaptureCommand::Stop);
                    let _ = self.inference_tx.send(InferenceCommand::Stop);
                    // Stop is terminal: the inference thread exits, so it cannot
                    // be restarted without recreating the actor.
                    self.state = State::Stopped;
                }
            }
        }
    }
}
