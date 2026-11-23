use crate::pipeline::capture::AudioSource;
use crate::utils::windowing::SlidingAudioWindow;
use hound;
use std::path::Path;

pub struct WAVFileSource {
    data: Vec<f32>,
    sample_rate: u32,
    window_processor: SlidingAudioWindow,
    index: usize,
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

        if !matches!(spec.sample_format, hound::SampleFormat::Int) {
            return Err(anyhow::anyhow!(
                "Only signed integer (Int) sample formats are supported for now. Found {:?}.",
                spec.sample_format
            ));
        }

        let sample_rate = spec.sample_rate;

        // Read all samples into memory to satisfy Send + 'static
        let mut data = Vec::with_capacity(reader.len() as usize);
        for sample_result in reader.samples::<i16>() {
            let s = sample_result.map_err(|e| anyhow::anyhow!("Error reading sample: {:?}", e))?;
            data.push(s as f32 / 32768.0);
        }

        let window_processor = SlidingAudioWindow::new(
            sample_rate as usize,
            window_duration_secs,
            overlap_duration_secs,
        );

        Ok(Self {
            data,
            sample_rate,
            window_processor,
            index: 0,
        })
    }
}

impl AudioSource for WAVFileSource {
    fn next_chunk(&mut self) -> Option<Vec<f32>> {
        loop {
            if self.index >= self.data.len() {
                return None;
            }

            // To keep it simple and match the behavior requested (one sample at a time
            // into the window processor), we'll take one sample.
            // However, feeding one sample at a time via process_chunk(vec![sample])
            // is what we did before and it worked.
            // NOTE

            let sample = self.data[self.index];
            self.index += 1;

            if let Some(window) = self.window_processor.process_chunk(vec![sample]) {
                return Some(window);
            }
            // If no window is ready, continue to the next sample in the loop
        }
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    /// Helper to create a dummy WAV file for testing.
    /// Creates a 16-bit mono WAV file with a known sequence of samples.
    fn create_test_wav(samples: Vec<i16>, sample_rate: u32) -> NamedTempFile {
        let mut temp_file = NamedTempFile::new().unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = hound::WavWriter::new(&mut temp_file, spec).unwrap();
        for s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        temp_file
    }

    #[test]
    fn test_wav_file_source_reading() {
        // Create a WAV file with 100 samples of 0
        let sample_rate = 1000;
        let samples = vec![0i16; 100];
        let temp_file = create_test_wav(samples, sample_rate);
        let path = temp_file.path();

        // Window: 0.1s (100 samples), Overlap: 0.05s (50 samples)
        // Step size = 100 - 50 = 50 samples
        let mut source = WAVFileSource::new(path, 0.1, 0.05).expect("Failed to create source");

        // First window should be ready after we've consumed enough samples.
        let window = source
            .next_chunk()
            .expect("Should have returned first window");
        assert_eq!(window.len(), 100);
        assert_eq!(window[0], 0.0);

        // Second window should be the next 50 samples (due to 50 sample overlap)
        let window2 = source.next_chunk();
        assert!(window2.is_none());
    }

    #[test]
    fn test_wav_file_source_sliding_windows() {
        let sample_rate = 1000;
        // Create 250 samples
        let samples = (0..250).map(|i| i as i16).collect::<Vec<_>>();
        let temp_file = create_test_wav(samples, sample_rate);
        let path = temp_file.path();

        // Window: 0.1s (100 samples), Overlap: 0.05s (50 samples)
        // Step: 50 samples.
        // Expected windows:
        // 1. [0..100]
        // 2. [50..150]
        // 3. [100..200]
        // 4. [150..250]
        let mut source = WAVFileSource::new(path, 0.1, 0.05).expect("Failed to create source");

        let w1 = source.next_chunk().expect("Window 1 missing");
        assert_eq!(w1.len(), 100);

        let w2 = source.next_chunk().expect("Window 2 missing");
        assert_eq!(w2.len(), 100);

        let w3 = source.next_chunk().expect("Window 3 missing");
        assert_eq!(w3.len(), 100);

        let w4 = source.next_chunk().expect("Window 4 missing");
        assert_eq!(w4.len(), 100);

        let w5 = source.next_chunk();
        assert!(w5.is_none(), "Window 5 should not exist");
    }
}
