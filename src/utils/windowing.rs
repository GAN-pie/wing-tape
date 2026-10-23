use std::collections::VecDeque;

pub struct SlidingAudioWindow {
    window_size_samples: usize,
    step_size_samples: usize,
    buffer: VecDeque<f32>,
}

impl SlidingAudioWindow {
    /// Creates a new `SlidingAudioWindow`.
    ///
    /// # Arguments
    /// * `sample_rate` - The audio sample rate in Hz.
    /// * `window_duration_secs` - The duration of each window in seconds.
    /// * `overlap_duration_secs` - The duration of overlap between consecutive windows in seconds.
    pub fn new(sample_rate: usize, window_duration_secs: f32, overlap_duration_secs: f32) -> Self {
        let window_size_samples = (sample_rate as f32 * window_duration_secs).round() as usize;
        let overlap_size_samples = (sample_rate as f32 * overlap_duration_secs).round() as usize;
        let step_size_samples = window_size_samples.saturating_sub(overlap_size_samples);

        if step_size_samples == 0 {
            panic!(
                "Step size must be greater than 0. Ensure overlap is less than window duration."
            );
        }

        Self {
            window_size_samples,
            step_size_samples,
            buffer: VecDeque::with_capacity(window_size_samples),
        }
    }

    /// Processes a chunk of audio and returns the next available window if one is ready.
    ///
    /// # Arguments
    /// * `chunk` - An owned vector of audio samples (transferred from the audio capture iterator).
    pub fn process_chunk(&mut self, chunk: Vec<f32>) -> Option<Vec<f32>> {
        self.buffer.extend(chunk);

        if self.buffer.len() >= self.window_size_samples {
            // Ensure the buffer is contiguous so we can take a slice
            let contiguous_slice = self.buffer.make_contiguous();

            // Create the window (this involves a copy, which is necessary to transfer ownership
            // to the inference thread via mpsc without requiring locks on the main buffer).
            let window = contiguous_slice[..self.window_size_samples].to_vec();

            // Advance the buffer by the step size to handle overlap
            self.buffer.drain(0..self.step_size_samples);

            return Some(window);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_panics_on_invalid_overlap() {
        // Overlap equal to window duration means step size is 0.
        let result = std::panic::catch_unwind(|| {
            SlidingAudioWindow::new(48000, 1.0, 1.0);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_sliding_window_logic() {
        // Setup: 10Hz sample rate, 0.3s window (3 samples), 0.1s overlap (1 sample).
        // Window size = 3. Overlap = 1. Step = 3 - 1 = 2.
        let mut window_processor = SlidingAudioWindow::new(10, 0.3, 0.1);

        // Chunk 1: [0.0, 1.0] -> Buffer: [0.0, 1.0] (len 2 < 3)
        assert_eq!(window_processor.process_chunk(vec![0.0, 1.0]), None);

        // Chunk 2: [2.0, 3.0] -> Buffer: [0.0, 1.0, 2.0, 3.0] (len 4 >= 3)
        // First window: [0.0, 1.0, 2.0]
        // Drain 2: Buffer becomes [2.0, 3.0]
        let window1 = window_processor.process_chunk(vec![2.0, 3.0]);
        assert_eq!(window1, Some(vec![0.0, 1.0, 2.0]));

        // Chunk 3: [4.0, 5.0] -> Buffer: [2.0, 3.0, 4.0, 5.0] (len 4 >= 3)
        // Second window: [2.0, 3.0, 4.0]
        // Drain 2: Buffer becomes [4.0, 5.0]
        let window2 = window_processor.process_chunk(vec![4.0, 5.0]);
        assert_eq!(window2, Some(vec![2.0, 3.0, 4.0]));

        // Chunk 4: [6.0] -> Buffer: [4.0, 5.0, 6.0] (len 3 >= 3)
        // Third window: [4.0, 5.0, 6.0]
        // Drain 2: Buffer becomes [6.0]
        let window3 = window_processor.process_chunk(vec![6.0]);
        assert_eq!(window3, Some(vec![4.0, 5.0, 6.0]));

        // Chunk 5: [7.0] -> Buffer: [6.0, 7.0] (len 2 < 3)
        assert_eq!(window_processor.process_chunk(vec![7.0]), None);
    }

    #[test]
    fn test_exact_window_count_and_content() {
        // Setup: 10Hz, 0.4s window (4 samples), 0.2s overlap (2 samples).
        // Window size = 4. Overlap = 2. Step = 2.
        let mut window_processor = SlidingAudioWindow::new(10, 0.4, 0.2);

        // Provide a sequence of chunks that sum to 10 samples.
        // Expected windows:
        // 1. [0, 1, 2, 3] -> Drain 2 -> Buffer [2, 3]
        // 2. [2, 3, 4, 5] -> Drain 2 -> Buffer [4, 5]
        // 3. [4, 5, 6, 7] -> Drain 2 -> Buffer [6, 7]
        // 4. [6, 7, 8, 9] -> Drain 2 -> Buffer [8, 9]

        let chunks = vec![
            vec![0.0, 1.0],
            vec![2.0, 3.0],
            vec![4.0, 5.0],
            vec![6.0, 7.0],
            vec![8.0, 9.0],
        ];

        let mut received_windows = Vec::new();
        for chunk in chunks {
            if let Some(window) = window_processor.process_chunk(chunk) {
                received_windows.push(window);
            }
        }

        assert_eq!(received_windows.len(), 4);
        assert_eq!(received_windows[0], vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(received_windows[1], vec![2.0, 3.0, 4.0, 5.0]);
        assert_eq!(received_windows[2], vec![4.0, 5.0, 6.0, 7.0]);
        assert_eq!(received_windows[3], vec![6.0, 7.0, 8.0, 9.0]);
    }
}
