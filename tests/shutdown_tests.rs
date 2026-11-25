//! End-to-end shutdown handshake: orchestrator + capture actor + inference actor.
//!
//! These tests mirror the lifecycle `main` implements without touching the
//! audio hardware or the model: `main` owns the lifecycle, sends
//! `UserCommand::Stop`, drops every `SystemEvent` sender, and then joins.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use wing_tape::pipeline::capture::{AudioSource, CaptureActor};
use wing_tape::pipeline::inference::{InferenceActor, InferenceEngine};
use wing_tape::pipeline::orchestrator::Orchestrator;
use wing_tape::pipeline::{
    AudioWindow, CaptureCommand, InferenceCommand, PipelineState, Prediction, SystemEvent,
    UserCommand,
};

/// A source that never runs out, so only an explicit `Stop` ends the pipeline.
struct EndlessSource;

impl AudioSource for EndlessSource {
    fn next_chunk(&mut self) -> Option<Vec<f32>> {
        thread::sleep(Duration::from_millis(5));
        Some(vec![0.0; 8])
    }

    fn sample_rate(&self) -> u32 {
        48000
    }
}

struct FixedEngine;

impl InferenceEngine for FixedEngine {
    fn predict(&mut self, _window: &AudioWindow) -> Result<Prediction, String> {
        Ok(Prediction {
            class_id: 0,
            probability: 0.5,
            timestamp: Instant::now(),
        })
    }
}

fn join_with_timeout(handle: thread::JoinHandle<()>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !handle.is_finished() {
        if Instant::now() > deadline {
            panic!("thread did not exit within {timeout:?}; shutdown would hang");
        }
        thread::sleep(Duration::from_millis(5));
    }
    handle.join().expect("thread panicked");
}

#[test]
fn shutdown_handshake_joins_orchestrator_and_actors() {
    let (event_tx, event_rx) = mpsc::channel::<SystemEvent>();
    let (capture_tx, capture_rx) = mpsc::channel::<CaptureCommand>();
    let (inference_tx, inference_rx) = mpsc::channel::<InferenceCommand>();

    let capture_event_tx = event_tx.clone();
    let capture_handle = thread::spawn(move || {
        CaptureActor::new(capture_rx, capture_event_tx, Box::new(EndlessSource)).run();
    });
    let inference_event_tx = event_tx.clone();
    let inference_handle = thread::spawn(move || {
        InferenceActor::new(inference_rx, inference_event_tx, Box::new(FixedEngine)).run();
    });

    let orchestrator = Orchestrator::new(event_rx, capture_tx.clone(), inference_tx.clone());
    let status = orchestrator.status();
    let orchestrator_handle = thread::spawn(move || {
        let mut orchestrator = orchestrator;
        orchestrator.run();
    });

    // Let the pipeline do some work before shutting down.
    thread::sleep(Duration::from_millis(100));

    // Emulate `main`: send the terminal Stop, then drop every event sender (the
    // owner's sender plus the reserved web-control seam clone). std::mpsc drains
    // queued messages before reporting disconnect, so Stop is always observed.
    let web_seam = event_tx.clone();
    event_tx
        .send(SystemEvent::User(UserCommand::Stop))
        .expect("orchestrator event channel closed early");
    drop(web_seam);
    drop(event_tx);

    join_with_timeout(orchestrator_handle, Duration::from_secs(2));

    // Belt and braces, exactly as `main` does.
    let _ = capture_tx.send(CaptureCommand::Stop);
    let _ = inference_tx.send(InferenceCommand::Stop);
    join_with_timeout(capture_handle, Duration::from_secs(2));
    join_with_timeout(inference_handle, Duration::from_secs(2));

    assert_eq!(status.lock().unwrap().state, PipelineState::Stopped);
}

#[test]
fn shutdown_without_stop_also_terminates_once_senders_drop() {
    // Documents the other half of the contract: `run()` returns when every
    // SystemEvent sender is gone, even if no Stop command was processed.
    let (event_tx, event_rx) = mpsc::channel::<SystemEvent>();
    let (capture_tx, _capture_rx) = mpsc::channel::<CaptureCommand>();
    let (inference_tx, _inference_rx) = mpsc::channel::<InferenceCommand>();

    let orchestrator = Orchestrator::new(event_rx, capture_tx, inference_tx);
    let orchestrator_handle = thread::spawn(move || {
        let mut orchestrator = orchestrator;
        orchestrator.run();
    });

    drop(event_tx);
    join_with_timeout(orchestrator_handle, Duration::from_secs(2));
}
