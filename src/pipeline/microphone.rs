use crate::pipeline::AudioSource;
use crate::utils::windowing::SlidingAudioWindow;
use audio_capture::{AudioCapture, AudioCaptureConfig};

pub struct MicrophoneSource {
    iterator: AudioCapture,
    window_processor: SlidingAudioWindow,
    sample_rate: u32,
}

impl MicrophoneSource {
    pub fn new(
        config: AudioCaptureConfig,
        window_duration_secs: f32,
        overlap_duration_secs: f32,
    ) -> Result<Self, String> {
        let sample_rate = config.sample_rate as u32;
        let iterator = AudioCapture::new(config)
            .map_err(|e| format!("Failed to start audio capture: {:?}", e))?;

        let window_processor = SlidingAudioWindow::new(
            sample_rate as usize,
            window_duration_secs,
            overlap_duration_secs,
        );

        Ok(Self {
            iterator,
            window_processor,
            sample_rate,
        })
    }
}

impl AudioSource for MicrophoneSource {
    fn next_chunk(&mut self) -> Option<Vec<f32>> {
        // We keep pulling from the iterator until the window processor produces a window
        // or the iterator is exhausted.
        while let Some(frame) = self.iterator.next() {
            if let Some(window) = self.window_processor.process_chunk(frame) {
                return Some(window);
            }
        }
        None
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
