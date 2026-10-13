//! Bounded, immediately flushed diagnostic files. Never persist authentication bodies.
use crate::events::LogLine;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Size at which `rommfs.log` rotates, and how many archives are kept.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const ARCHIVES: usize = 5;

/// A progress-bearing event reduced to what the throttle needs.
pub struct ProgressSample<'a> {
    pub running: bool,
    pub ready: bool,
    pub error: Option<&'a str>,
    pub detail: &'a str,
    /// Completed fraction in 0.0..=1.0 when the total is known.
    pub fraction: Option<f64>,
}

struct Seen {
    running: bool,
    ready: bool,
    stem: String,
    bucket: u32,
    logged_at_ms: u64,
}

/// Keeps per-tick progress events out of the durable log. Starts, finishes,
/// failures and changes of wording always pass; plain progress ticks pass
/// only after the fraction advanced a further 10% and at least 10 seconds
/// passed since the last logged line (whichever is less frequent).
#[derive(Default)]
pub struct ProgressThrottle {
    seen: std::collections::HashMap<&'static str, Seen>,
}

const PROGRESS_INTERVAL_MS: u64 = 10_000;

impl ProgressThrottle {
    pub fn admit(&mut self, kind: &'static str, sample: &ProgressSample, now_ms: u64) -> bool {
        // "Auxiliary files: 3 / 146" and "...: 4 / 146" share one stem.
        let stem: String = sample
            .detail
            .chars()
            .filter(|c| !c.is_ascii_digit())
            .collect();
        let bucket = sample
            .fraction
            .map_or(0, |f| (f.clamp(0.0, 1.0) * 10.0) as u32);
        // Baselines move only when a line is logged, so every comparison is
        // against the last line actually written.
        let pass = match self.seen.get(kind) {
            None => true,
            Some(last) => {
                sample.error.is_some()
                    || last.running != sample.running
                    || last.ready != sample.ready
                    || last.stem != stem
                    || (now_ms.saturating_sub(last.logged_at_ms) >= PROGRESS_INTERVAL_MS
                        && (sample.fraction.is_none() || bucket > last.bucket))
            }
        };
        if pass {
            self.seen.insert(
                kind,
                Seen {
                    running: sample.running,
                    ready: sample.ready,
                    stem,
                    bucket,
                    logged_at_ms: now_ms,
                },
            );
        }
        pass
    }
}

pub struct DiagnosticLog {
    directory: PathBuf,
    file: Option<File>,
    size: u64,
    max_bytes: u64,
    archives: usize,
}

impl DiagnosticLog {
    pub fn open(directory: &Path) -> io::Result<Self> {
        Self::with_limits(directory, MAX_FILE_BYTES, ARCHIVES)
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

    fn tick(detail: &str, fraction: Option<f64>) -> ProgressSample<'_> {
        ProgressSample {
            running: true,
            ready: false,
            error: None,
            detail,
            fraction,
        }
    }

    #[test]
    fn throttle_logs_start_finish_failure_and_sparse_ticks_only() {
        let mut t = ProgressThrottle::default();
        let d = "Downloading RetroBat 8.3.0";
        assert!(t.admit("i", &tick(d, Some(0.0)), 0));
        // Dense ticks inside the interval or without 10% progress are dropped.
        let mut passed = 0;
        for i in 1..=1000u64 {
            if t.admit("i", &tick(d, Some(i as f64 / 1000.0)), i * 20) {
                passed += 1;
            }
        }
        assert!(passed <= 10, "{passed} ticks logged");
        // Time elapsed but no further 10% advance: still dropped.
        let mut t = ProgressThrottle::default();
        assert!(t.admit("i", &tick(d, Some(0.05)), 0));
        assert!(!t.admit("i", &tick(d, Some(0.05)), 60_000));
        // Both conditions met: passes.
        assert!(t.admit("i", &tick(d, Some(0.25)), 60_000));
        // Without a known total only the time rule applies.
        assert!(t.admit("a", &tick("x", None), 0));
        assert!(!t.admit("a", &tick("x", None), 5_000));
        assert!(t.admit("a", &tick("x", None), 10_000));
        // Completion, failure and new wording always pass.
        let done = ProgressSample {
            running: false,
            ..tick(d, Some(1.0))
        };
        assert!(t.admit("i", &done, 60_001));
        assert!(t.admit(
            "i",
            &ProgressSample {
                error: Some("boom"),
                ..tick(d, None)
            },
            60_002
        ));
        assert!(t.admit("i", &tick("Extracting", None), 60_003));
        // Counters inside the same wording are ticks, not new wording.
        let mut t = ProgressThrottle::default();
        assert!(t.admit("x", &tick("Auxiliary files: 1 / 146", Some(1.0 / 146.0)), 0));
        assert!(!t.admit("x", &tick("Auxiliary files: 2 / 146", Some(2.0 / 146.0)), 1));
    }
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
