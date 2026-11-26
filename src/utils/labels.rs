//! Zero-indexed species labels, loaded from a single-column text file.
//!
//! Each line of the file maps to one class id, starting at 0. A blank line is
//! treated as "no label" for that class rather than shifting subsequent labels,
//! so class ids always stay aligned with the model's output indices.

use std::fs;
use std::path::Path;

/// Species labels indexed by model class id.
#[derive(Debug, Clone, Default)]
pub struct Labels {
    labels: Vec<String>,
}

impl Labels {
    /// Loads labels from a UTF-8 text file with one label per line.
    ///
    /// Class ids are positional: the first line is class 0, the second is class 1,
    /// and so on. Trailing whitespace (including `\r` from CRLF files) is stripped.
    pub fn from_path<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let labels = contents
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect();
        Ok(Self { labels })
    }

    /// Builds labels directly from an iterator, useful for tests and embedding.
    pub fn from_iter<I, S>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            labels: iter.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns the label for `class_id`, or `None` if the id is out of range or the
    /// corresponding line was blank.
    pub fn get(&self, class_id: usize) -> Option<&str> {
        self.labels
            .get(class_id)
            .map(|label| label.as_str())
            .filter(|label| !label.is_empty())
    }

    /// Number of positional entries, including blank lines.
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_labels(contents: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn loads_zero_indexed_labels() {
        let file = write_labels("American Robin\nHouse Sparrow\nBlue Jay\n");
        let labels = Labels::from_path(file.path()).unwrap();

        assert_eq!(labels.len(), 3);
        assert_eq!(labels.get(0), Some("American Robin"));
        assert_eq!(labels.get(1), Some("House Sparrow"));
        assert_eq!(labels.get(2), Some("Blue Jay"));
        assert_eq!(labels.get(3), None);
    }

    #[test]
    fn blank_line_keeps_positions_aligned() {
        let labels = Labels::from_iter(["A", "", "C"]);
        assert_eq!(labels.get(0), Some("A"));
        assert_eq!(labels.get(1), None);
        assert_eq!(labels.get(2), Some("C"));
    }

    #[test]
    fn strips_crlf_and_trailing_whitespace() {
        let file = write_labels("First\r\nSecond  \r\n");
        let labels = Labels::from_path(file.path()).unwrap();

        assert_eq!(labels.get(0), Some("First"));
        assert_eq!(labels.get(1), Some("Second"));
    }
}
