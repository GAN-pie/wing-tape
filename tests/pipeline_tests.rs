use std::sync::mpsc;
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

#[test]
fn test_orchestrator_routing() {
    let (system_event_tx, system_event_rx) = mpsc::channel();
    let (capture_cmd_tx, capture_cmd_rx) = mpsc::channel();
    let (inference_cmd_tx, inference_cmd_rx) = mpsc::channel();

    let orchestrator = Orchestrator::new(
        system_event_rx,
        capture_cmd_tx.clone(),
        inference_cmd_tx.clone(),
    );

    // We'll use a thread for the orchestrator so we can send events to it.
    let orchestrator_handle = thread::spawn(move || {
        let mut orch = orchestrator;
        orch.run();
    });

    // Test 1: Route WindowReady from Capture to Inference
    let window = AudioWindow {
        data: vec![0.0; 10],
        sample_rate: 48000,
        timestamp: Instant::now(),
    };
    system_event_tx
        .send(SystemEvent::Capture(CaptureEvent::WindowReady(
            window.clone(),
        )))
        .unwrap();

    // Check if inference command was sent
    let cmd = inference_cmd_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Expected Predict command");
    if let InferenceCommand::Predict(received_window) = cmd {
        assert_eq!(received_window.data.len(), 10);
    } else {
        panic!("Wrong command type");
    }

    // Test 2: User command Stop
    system_event_tx
        .send(SystemEvent::User(UserCommand::Stop))
        .unwrap();

    // Give it a bit of time to process
    thread::sleep(Duration::from_millis(50));

    // Drop the sender to stop the orchestrator loop
    drop(system_event_tx);
    let _ = orchestrator_handle.join();
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
