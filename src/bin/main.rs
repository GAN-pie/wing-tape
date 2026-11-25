use audio_capture::AudioCaptureConfig;
use clap::Parser;
use signal_hook::consts::{SIGINT, SIGTERM};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use wing_tape::pipeline::birdnet::BirdNetClassifier;
use wing_tape::pipeline::microphone::MicrophoneSource;
use wing_tape::pipeline::orchestrator::Orchestrator;
use wing_tape::pipeline::{
    CaptureActor, CaptureCommand, InferenceActor, InferenceCommand, PipelineState, SharedStatus,
    SystemEvent, UserCommand,
};
use wing_tape::utils::labels::Labels;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    model_path: String,

    #[arg(short, long, default_value_t = 0)]
    duration: u64,

    #[arg(short('D'), long, default_value = "alsa:default")]
    device_id: String,

    /// Start the pipeline idle (paused) instead of running.
    #[arg(long, default_value_t = false)]
    start_idle: bool,

    /// Address the web control server will bind to (server lands separately).
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: String,

    /// Optional zero-indexed, single-column species labels file.
    #[arg(long)]
    labels_path: Option<String>,
}

/// Handles reserved for the upcoming web control actor.
///
/// The server will use `event_tx` to inject user commands, `status` to answer
/// `/api/status`, `shutdown` to stop its accept loop, and `bind` for its listen
/// address. Keeping this clone alive until shutdown also guarantees the
/// orchestrator's event channel does not close while a web request is in flight.
#[allow(dead_code)]
struct WebControlSeam {
    bind: String,
    status: SharedStatus,
    event_tx: mpsc::Sender<SystemEvent>,
    shutdown: Arc<AtomicBool>,
}

fn main() {
    let args = Args::parse();

    let labels = match &args.labels_path {
        Some(path) => match Labels::from_path(path) {
            Ok(labels) => Some(Arc::new(labels)),
            Err(e) => {
                eprintln!("Failed to load labels from {path}: {e}");
                std::process::exit(1);
            }
        },
        None => None,
    };

    // SIGINT/SIGTERM flip this flag; the main loop below turns it into an orderly
    // shutdown instead of letting the default handler kill the process mid-write.
    let shutdown = Arc::new(AtomicBool::new(false));
    for signal in [SIGINT, SIGTERM] {
        signal_hook::flag::register(signal, Arc::clone(&shutdown))
            .expect("failed to install signal handler");
    }

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
    )
    .with_start_paused(args.start_idle);

    if let Some(labels) = labels {
        orchestrator = orchestrator.with_labels(labels);
    }

    let status = orchestrator.status();

    // The external command seam for the web control actor. Built before the
    // orchestrator moves into its thread, and kept alive until shutdown.
    let web_seam = WebControlSeam {
        bind: args.bind,
        status: Arc::clone(&status),
        event_tx: system_event_tx.clone(),
        shutdown: Arc::clone(&shutdown),
    };
    println!("Web control will bind to {}", web_seam.bind);

    println!("Starting Wing-Tape pipeline...");
    // `main` owns the lifecycle. The orchestrator runs on its own thread so
    // `main` can react to signals and close the event channel when it is time to
    // exit; otherwise the orchestrator would wait forever on a sender `main`
    // itself still holds.
    let orchestrator_handle = thread::spawn(move || {
        let mut orchestrator = orchestrator;
        orchestrator.run();
    });

    wait_for_shutdown(&shutdown, &status);

    println!("Shutting down...");
    // Ask the orchestrator to stop both actors, then close every event sender.
    // std::mpsc delivers queued messages before reporting disconnect, so the
    // Stop is always observed, after which `run()` returns.
    let _ = system_event_tx.send(SystemEvent::User(UserCommand::Stop));
    drop(web_seam);
    drop(system_event_tx);
    let _ = orchestrator_handle.join();

    // Belt and braces: the orchestrator already forwarded Stop, but send again in
    // case it exited before processing it. Sends to dead actors are ignored.
    let _ = capture_cmd_tx.send(CaptureCommand::Stop);
    let _ = inference_cmd_tx.send(InferenceCommand::Stop);
    let _ = capture_handle.join();
    let _ = inference_handle.join();
    println!("Goodbye!");
}

/// Blocks until a shutdown signal arrives or the pipeline reaches a terminal
/// state (for example a `--duration` source running out).
fn wait_for_shutdown(shutdown: &AtomicBool, status: &SharedStatus) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }

        let terminal = status
            .lock()
            .map(|status| matches!(status.state, PipelineState::Stopped | PipelineState::Failed))
            .unwrap_or(false);
        if terminal {
            return;
        }

        thread::sleep(Duration::from_millis(200));
    }
}
