//! Bounded, immediately flushed diagnostic files. Never persist authentication bodies.
use crate::events::LogLine;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub struct DiagnosticLog {
    directory: PathBuf,
    file: Option<File>,
    size: u64,
    max_bytes: u64,
    archives: usize,
}

impl DiagnosticLog {
    pub fn open(directory: &Path) -> io::Result<Self> {
        Self::with_limits(directory, 2 * 1024 * 1024, 3)
    }
    fn with_limits(directory: &Path, max_bytes: u64, archives: usize) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("rommfs.log"))?;
        let size = file.metadata()?.len();
        Ok(Self {
            directory: directory.into(),
            file: Some(file),
            size,
            max_bytes,
            archives,
        })
    }
    pub fn append(&mut self, line: &LogLine) -> io::Result<()> {
        // Single-line records prevent remote filenames from forging log entries.
        let message: String = line
            .message
            .chars()
            .take(8192)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let text = format!(
            "{} {:?} {} {}\n",
            line.unix_secs, line.level, line.op, message
        );
        if self.size.saturating_add(text.len() as u64) > self.max_bytes {
            self.file.take(); // Windows cannot rename an open file.
            for i in (1..=self.archives).rev() {
                let to = self.directory.join(format!("rommfs.{i}.log"));
                if to.exists() {
                    fs::remove_file(&to)?;
                }
                let from = self.directory.join(if i == 1 {
                    "rommfs.log".into()
                } else {
                    format!("rommfs.{}.log", i - 1)
                });
                if from.exists() {
                    fs::rename(from, to)?;
                }
            }
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.directory.join("rommfs.log"))?,
            );
            self.size = 0;
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("diagnostic log unavailable after rotation failure"))?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        self.size += text.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Level;
    #[test]
    fn rotates_and_retains_recent_errors_with_bounded_disk_usage() {
        let tmp = tempfile::tempdir().unwrap();
        let mut log = DiagnosticLog::with_limits(tmp.path(), 100, 2).unwrap();
        for i in 0..20 {
            log.append(&LogLine {
                unix_secs: i,
                level: Level::Error,
                op: "mount",
                message: "failed\nretry".into(),
            })
            .unwrap();
        }
        drop(log);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 3);
        assert!(fs::read_to_string(tmp.path().join("rommfs.log"))
            .unwrap()
            .contains("19 Error mount failed retry"));
        assert!(fs::read_dir(tmp.path())
            .unwrap()
            .all(|p| p.unwrap().metadata().unwrap().len() <= 100));
    }
}
