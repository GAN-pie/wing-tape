pub mod birdnet;
pub mod capture;
pub mod inference;
pub mod microphone;
pub mod orchestrator;
pub mod wav_source;

pub use capture::{AudioSource, CaptureActor};
pub use inference::{InferenceActor, InferenceEngine};
pub use wav_source::WAVFileSource;

use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

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

/// High-level lifecycle state of the pipeline, published for external readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineState {
    Running,
    Paused,
    /// Terminal: the actors have been asked to stop. Resuming requires a process
    /// restart (in-process restart is deferred).
    Stopped,
    /// Terminal: the audio source or inference path failed.
    Failed,
}

/// A single prediction as recorded in the status snapshot.
#[derive(Debug, Clone)]
pub struct Detection {
    pub class_id: usize,
    pub probability: f32,
    /// Wall-clock time the orchestrator recorded the prediction.
    ///
    /// The internal [`Prediction`] keeps a monotonic `Instant`; this is the
    /// serialization-friendly counterpart written to the shared snapshot.
    pub detected_at: SystemTime,
    /// Human-readable species label, when a labels file was supplied.
    pub label: Option<String>,
}

/// Point-in-time view of the pipeline, shared between the orchestrator (writer)
/// and external consumers such as the web control actor (readers).
#[derive(Debug, Clone)]
pub struct PipelineStatus {
    pub state: PipelineState,
    pub started_at: SystemTime,
    pub updated_at: SystemTime,
    pub last_prediction: Option<Detection>,
    pub last_error: Option<String>,
}

impl PipelineStatus {
    pub fn new() -> Self {
        let now = SystemTime::now();
        Self {
            state: PipelineState::Running,
            started_at: now,
            updated_at: now,
            last_prediction: None,
            last_error: None,
        }
    }
}

impl Default for PipelineStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared, thread-safe handle to the pipeline status snapshot.
pub type SharedStatus = Arc<Mutex<PipelineStatus>>;

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
