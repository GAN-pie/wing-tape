use audio_capture::AudioCaptureConfig;
use clap::Parser;
use signal_hook::consts::{SIGINT, SIGTERM};
use std::net::SocketAddr;
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
use wing_tape::webui::WebControlActor;

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

    /// Exit when the pipeline reaches a terminal state (failed or stopped)
    /// instead of continuing to serve status until SIGINT/SIGTERM. Implied when
    /// `--duration` is greater than 0.
    #[arg(long, default_value_t = false)]
    exit_on_failure: bool,

    /// Address the web control server binds to.
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: SocketAddr,

    /// Optional zero-indexed, single-column species labels file.
    #[arg(long)]
    labels_path: Option<String>,
}

fn main() {
    let args = Args::parse();
    let bind = args.bind;

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

    // The web control actor is a peer producer on the event bus. Binding happens
    // synchronously here so a port conflict fails fast, before the pipeline
    // starts running.
    let web = WebControlActor::new(
        bind,
        Arc::clone(&status),
        system_event_tx.clone(),
        Arc::clone(&shutdown),
    );
    let web_handle = web.spawn().expect("failed to bind web control server");
    println!("Web control listening on {}", web_handle.addr());

    println!("Starting Wing-Tape pipeline...");
    // `main` owns the lifecycle. The orchestrator runs on its own thread so
    // `main` can react to signals and close the event channel when it is time to
    // exit; otherwise the orchestrator would wait forever on a sender `main`
    // itself still holds.
    let orchestrator_handle = thread::spawn(move || {
        let mut orchestrator = orchestrator;
        orchestrator.run();
    });

    // A finite run (`--duration`) is expected to end on its own; otherwise a
    // terminal state keeps the process alive so status can still be served. A
    // signal always exits.
    let exit_on_terminal = args.exit_on_failure || args.duration > 0;
    wait_for_shutdown(&shutdown, &status, exit_on_terminal);

    println!("Shutting down...");
    // Ask the orchestrator to stop both actors, then close every event sender.
    // std::mpsc delivers queued messages before reporting disconnect, so the
    // Stop is always observed, after which `run()` returns.
    let _ = system_event_tx.send(SystemEvent::User(UserCommand::Stop));

    // The web actor holds an event_tx clone and must exit (dropping it) before
    // the orchestrator can observe channel closure. Set the flag even when the
    // shutdown was triggered by a terminal state rather than a signal.
    shutdown.store(true, Ordering::SeqCst);
    let _ = web_handle.join();

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

/// Blocks until a signal arrives, or until the pipeline reaches a terminal state
/// when `exit_on_terminal` is set.
///
/// A terminal state (`Failed`/`Stopped`) does not end the process by default: the
/// status server keeps running so the UI can report the error or confirm the stop.
/// Only a finite `--duration` run or an explicit `--exit-on-failure` opts into
/// exiting automatically.
fn wait_for_shutdown(shutdown: &AtomicBool, status: &SharedStatus, exit_on_terminal: bool) {
    let mut announced_terminal = false;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }

        let state = status.lock().ok().map(|status| status.state);
        if let Some(state) = state {
            if matches!(state, PipelineState::Stopped | PipelineState::Failed) {
                if exit_on_terminal {
                    return;
                }

                if !announced_terminal {
                    eprintln!(
                        "Pipeline is {state:?}; keeping the process alive to serve status. \
                         Send SIGINT/SIGTERM to exit, or pass --exit-on-failure."
                    );
                    announced_terminal = true;
                }
            }
        }

        thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Instant;
    use wing_tape::pipeline::PipelineStatus;

    fn failed_status() -> SharedStatus {
        Arc::new(Mutex::new(PipelineStatus {
            state: PipelineState::Failed,
            ..PipelineStatus::new()
        }))
    }

    fn join_with_timeout(handle: thread::JoinHandle<()>, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !handle.is_finished() {
            if Instant::now() > deadline {
                panic!("wait_for_shutdown did not return within {timeout:?}");
            }
            thread::sleep(Duration::from_millis(5));
        }
        handle.join().unwrap();
    }

    #[test]
    fn keeps_serving_after_failure_by_default() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&shutdown);
        let status = failed_status();
        let probe = Arc::clone(&status);

        let handle = thread::spawn(move || wait_for_shutdown(&flag, &probe, false));

        // A failure must not tear the process down while status is still useful.
        thread::sleep(Duration::from_millis(250));
        assert!(
            !handle.is_finished(),
            "process must keep serving status after a failure by default"
        );

        shutdown.store(true, Ordering::Relaxed);
        join_with_timeout(handle, Duration::from_secs(1));
    }

    #[test]
    fn exits_on_failure_when_opted_in() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let status = failed_status();

        let handle = thread::spawn(move || wait_for_shutdown(&shutdown, &status, true));
        join_with_timeout(handle, Duration::from_secs(1));
    }
}
