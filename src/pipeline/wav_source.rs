use crate::pipeline::capture::AudioSource;
use crate::utils::windowing::SlidingAudioWindow;
use hound;
use std::path::Path;

pub struct WAVFileSource {
    samples: hound::Samples<std::fs::File, i16>,
    sample_rate: u32,
    window_processor: SlidingAudioWindow,
}

impl WAVFileSource {
    pub fn new<P: AsRef<Path>>(
        path: P,
        window_duration_secs: f32,
        overlap_duration_secs: f32,
    ) -> anyhow::Result<Self> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();

        if spec.bits_per_sample != 16 {
            return Err(anyhow::anyhow!(
                "Only 16-bit sample depth is supported for now. Found {} bits.",
                spec.bits_per_sample
            ));
        }

        if !matches!(spec.sample_type, hound::SampleType::Int) {
            return Err(anyhow::anyhow!(
                "Only signed integer (Int) sample types are supported for now. Found {:?}.",
                spec.sample_type
            ));
        }

        let sample_rate = spec.sample_rate;
        let samples = reader.samples::<i16>();
        let window_processor = SlidingAudioWindow::new(
            sample_rate as usize,
            window_duration_secs,
            overlap_duration_secs,
        );

        Ok(Self {
            samples,
            sample_rate,
            window_processor,
        })
    }
}

impl AudioSource for WAVFileSource {
    fn next_chunk(&mut self) -> Option<Vec<f32>> {
        loop {
            let sample = match self.samples.next() {
                Some(Ok(s)) => s as f32 / 32768.0,
                Some(Err(_)) => return None,
                None => return None,
            };

            if let Some(window) = self.window_processor.process_chunk(vec![sample]) {
                return Some(window);
            }
        }
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
