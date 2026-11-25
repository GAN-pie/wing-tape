use crate::pipeline::{AudioWindow, CaptureCommand, CaptureEvent, SystemEvent};
use std::sync::mpsc::{self, TryRecvError};
use std::time::Instant;

pub trait AudioSource: Send + 'static {
    fn next_chunk(&mut self) -> Option<Vec<f32>>;
    fn sample_rate(&self) -> u32;
}

pub struct CaptureActor {
    command_rx: mpsc::Receiver<CaptureCommand>,
    event_tx: mpsc::Sender<SystemEvent>,
    source: Box<dyn AudioSource>,
    sample_rate: u32,
}

impl CaptureActor {
    pub fn new(
        command_rx: mpsc::Receiver<CaptureCommand>,
        event_tx: mpsc::Sender<SystemEvent>,
        source: Box<dyn AudioSource>,
    ) -> Self {
        let sample_rate = source.sample_rate();
        Self {
            command_rx,
            event_tx,
            source,
            sample_rate,
        }
    }

    /// Runs the capture loop until a terminal [`CaptureCommand::Stop`] arrives, the
    /// command channel closes, the audio source is exhausted, or the event receiver
    /// goes away.
    ///
    /// `Stop` is terminal: the actor exits and the owning thread's `join` completes.
    /// Use the orchestrator's pause gate to temporarily stop consuming windows; do
    /// not send `Stop` for a pause, since the actor cannot be restarted.
    ///
    /// Shutdown latency:
    /// - While idle (before `Start`), the actor blocks on the command channel, so
    ///   `Stop` is handled immediately and no CPU is spent spinning.
    /// - While running, commands are polled between audio reads, so `Stop` latency is
    ///   bounded by one `next_chunk` call — i.e. one window step (~2 s for a 3 s
    ///   window with 1 s overlap at 48 kHz).
    pub fn run(mut self) {
        let mut running = false;

        loop {
            // Poll for commands before each audio read so that a terminal Stop is
            // observed as soon as the current read completes.
            match self.command_rx.try_recv() {
                Ok(CaptureCommand::Start) => running = true,
                Ok(CaptureCommand::Stop) => return,
                Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => {}
            }

            if !running {
                // Idle: block instead of spinning so the actor costs no CPU and can
                // react to Stop without a poll interval.
                match self.command_rx.recv() {
                    Ok(CaptureCommand::Start) => {
                        running = true;
                        continue;
                    }
                    Ok(CaptureCommand::Stop) | Err(_) => return,
                }
            }

            match self.source.next_chunk() {
                Some(data) => {
                    let window = AudioWindow {
                        data,
                        sample_rate: self.sample_rate,
                        timestamp: Instant::now(),
                    };
                    if self
                        .event_tx
                        .send(SystemEvent::Capture(CaptureEvent::WindowReady(window)))
                        .is_err()
                    {
                        // Nobody is listening; capturing is pointless.
                        return;
                    }
                }
                None => {
                    let _ = self.event_tx.send(SystemEvent::Capture(CaptureEvent::Error(
                        "Source exhausted".to_string(),
                    )));
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    /// A source that never runs out, so the actor keeps running until told to stop.
    struct EndlessSource;

    impl AudioSource for EndlessSource {
        fn next_chunk(&mut self) -> Option<Vec<f32>> {
            thread::sleep(Duration::from_millis(1));
            Some(vec![0.0; 8])
        }

        fn sample_rate(&self) -> u32 {
            48000
        }
    }

    /// Joins without risking an indefinitely hung test suite on regression.
    fn join_with_timeout(handle: thread::JoinHandle<()>, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !handle.is_finished() {
            if Instant::now() > deadline {
                panic!("capture actor did not exit within {timeout:?}");
            }
            thread::sleep(Duration::from_millis(5));
        }
        handle.join().expect("capture actor panicked");
    }

    #[test]
    fn stop_exits_an_idle_actor() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            CaptureActor::new(cmd_rx, event_tx, Box::new(EndlessSource)).run();
        });

        // The actor is idle: Stop must terminate it without ever starting capture.
        cmd_tx.send(CaptureCommand::Stop).unwrap();
        join_with_timeout(handle, Duration::from_secs(1));

        // No window should ever have been emitted: the actor was never started.
        // (The receiver reports `Disconnected` once the actor drops `event_tx`.)
        assert!(event_rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn stop_exits_a_running_actor() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            CaptureActor::new(cmd_rx, event_tx, Box::new(EndlessSource)).run();
        });

        cmd_tx.send(CaptureCommand::Start).unwrap();
        event_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("expected a window to prove the actor is running");

        cmd_tx.send(CaptureCommand::Stop).unwrap();
        // Before the fix, Stop only cleared `running` and this join would hang.
        join_with_timeout(handle, Duration::from_secs(2));
    }
}
