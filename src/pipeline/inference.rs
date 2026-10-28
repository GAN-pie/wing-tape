use crate::pipeline::{AudioWindow, InferenceCommand, InferenceEvent, Prediction, SystemEvent};
use std::sync::mpsc;

pub trait InferenceEngine: Send + 'static {
    fn predict(&mut self, window: &AudioWindow) -> Result<Prediction, String>;
}

pub struct InferenceActor {
    command_rx: mpsc::Receiver<InferenceCommand>,
    event_tx: mpsc::Sender<SystemEvent>,
    engine: Box<dyn InferenceEngine>,
}

impl InferenceActor {
    pub fn new(
        command_rx: mpsc::Receiver<InferenceCommand>,
        event_tx: mpsc::Sender<SystemEvent>,
        engine: Box<dyn InferenceEngine>,
    ) -> Self {
        Self {
            command_rx,
            event_tx,
            engine,
        }
    }

    pub fn run(mut self) {
        while let Ok(cmd) = self.command_rx.recv() {
            match cmd {
                InferenceCommand::Predict(window) => match self.engine.predict(&window) {
                    Ok(prediction) => {
                        if self
                            .event_tx
                            .send(SystemEvent::Inference(InferenceEvent::Prediction(
                                prediction,
                            )))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        if self
                            .event_tx
                            .send(SystemEvent::Inference(InferenceEvent::Error(e)))
                            .is_err()
                        {
                            break;
                        }
                    }
                },
                InferenceCommand::Stop => {
                    break;
                }
            }
        }
    }
}
