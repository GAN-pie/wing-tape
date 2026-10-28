use crate::pipeline::{AudioWindow, CaptureCommand, CaptureEvent, SystemEvent};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

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

    pub fn run(mut self) {
        let mut running = false;

        loop {
            // Check for commands without blocking too long
            if let Ok(cmd) = self.command_rx.try_recv() {
                match cmd {
                    CaptureCommand::Start => {
                        running = true;
                    }
                    CaptureCommand::Stop => {
                        running = false;
                    }
                }
            }

            if running {
                if let Some(data) = self.source.next_chunk() {
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
                        break;
                    }
                } else {
                    let _ = self.event_tx.send(SystemEvent::Capture(CaptureEvent::Error(
                        "Source exhausted".to_string(),
                    )));
                    break;
                }
            }

            // Small sleep to prevent tight loop if no data is available
            thread::sleep(Duration::from_millis(10));
        }
    }
}
