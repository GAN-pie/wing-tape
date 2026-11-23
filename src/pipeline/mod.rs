pub mod birdnet;
pub mod capture;
pub mod inference;
pub mod microphone;
pub mod orchestrator;
pub mod wav_source;

pub use capture::{AudioSource, CaptureActor};
pub use inference::{InferenceActor, InferenceEngine};
pub use wav_source::WAVFileSource;

use std::time::Instant;

#[derive(Debug, Clone)]
pub struct AudioWindow {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub struct Prediction {
    pub class_id: usize,
    pub probability: f32,
    pub timestamp: Instant,
}

#[derive(Debug)]
pub enum CaptureEvent {
    WindowReady(AudioWindow),
    Error(String),
}

#[derive(Debug)]
pub enum CaptureCommand {
    Start,
    Stop,
}

#[derive(Debug)]
pub enum InferenceEvent {
    Prediction(Prediction),
    Error(String),
}

#[derive(Debug)]
pub enum InferenceCommand {
    Predict(AudioWindow),
    Stop,
}

#[derive(Debug)]
pub enum UserCommand {
    Start,
    Pause,
    Resume,
    Stop,
}

#[derive(Debug)]
pub enum SystemEvent {
    Capture(CaptureEvent),
    Inference(InferenceEvent),
    User(UserCommand),
}
