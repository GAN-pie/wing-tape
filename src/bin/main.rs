use audio_capture::AudioCaptureConfig;
use clap::Parser;
use std::sync::mpsc;
use std::thread;
use wing_tape::pipeline::birdnet::BirdNetClassifier;
use wing_tape::pipeline::microphone::MicrophoneSource;
use wing_tape::pipeline::orchestrator::Orchestrator;
use wing_tape::pipeline::{
    CaptureActor, CaptureCommand, InferenceActor, InferenceCommand, SystemEvent, UserCommand,
};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    model_path: String,

    #[arg(short, long, default_value_t = 0)]
    duration: u64,

    #[arg(short('D'), long, default_value = "alsa:default")]
    device_id: String,
}

fn main() {
    let args = Args::parse();

    let (system_event_tx, system_event_rx) = mpsc::channel::<SystemEvent>();
    let (capture_cmd_tx, capture_cmd_rx) = mpsc::channel::<CaptureCommand>();
    let (inference_cmd_tx, inference_cmd_rx) = mpsc::channel::<InferenceCommand>();

    // 1. Setup Inference Actor
    let classifier =
        BirdNetClassifier::new(&args.model_path).expect("Failed to initialize classifier");
    let inference_actor = InferenceActor::new(
        inference_cmd_rx,
        system_event_tx.clone(),
        Box::new(classifier),
    );

    let inference_handle = thread::spawn(move || {
        inference_actor.run();
    });

    // 2. Setup Capture Actor
    let capture_config = AudioCaptureConfig {
        device_id: Some(args.device_id),
        duration: args.duration,
        ..Default::default()
    };

    let mic_source = MicrophoneSource::new(
        capture_config,
        3.0, // window duration
        1.0, // overlap
    )
    .expect("Failed to initialize microphone source");

    let capture_actor = CaptureActor::new(
        capture_cmd_rx,
        system_event_tx.clone(),
        Box::new(mic_source),
    );

    let capture_handle = thread::spawn(move || {
        capture_actor.run();
    });

    // 3. Setup Orchestrator
    let mut orchestrator = Orchestrator::new(
        system_event_rx,
        capture_cmd_tx.clone(),
        inference_cmd_tx.clone(),
    );

    // Start the process
    println!("Starting Wing-Tape pipeline...");
    system_event_tx
        .send(SystemEvent::User(UserCommand::Start))
        .unwrap();

    // Run the orchestrator (this blocks until the loop ends)
    orchestrator.run();

    // Cleanup
    println!("Shutting down...");
    let _ = capture_cmd_tx.send(CaptureCommand::Stop);
    let _ = inference_cmd_tx.send(InferenceCommand::Stop);

    let _ = capture_handle.join();
    let _ = inference_handle.join();
    println!("Goodbye!");
}
