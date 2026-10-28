use crate::pipeline::{
    CaptureCommand, CaptureEvent, InferenceCommand, InferenceEvent, SystemEvent, UserCommand,
};
use std::sync::mpsc;

pub struct Orchestrator {
    event_rx: mpsc::Receiver<SystemEvent>,
    capture_tx: mpsc::Sender<CaptureCommand>,
    inference_tx: mpsc::Sender<InferenceCommand>,
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
        }
    }

    pub fn run(&mut self) {
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
                // Route window to inference
                let _ = self.inference_tx.send(InferenceCommand::Predict(window));
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
                let _ = self.capture_tx.send(CaptureCommand::Start);
            }
            UserCommand::Stop => {
                let _ = self.capture_tx.send(CaptureCommand::Stop);
                let _ = self.inference_tx.send(InferenceCommand::Stop);
            }
        }
    }
}
