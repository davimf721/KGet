//! Minimal append-only log file for the daemon.
//!
//! Callers are responsible for never passing secrets: URLs go through
//! [`redact_url`](crate::queue::redact_url), header values and tokens are
//! never logged.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Size at which `daemon.log` is moved to `daemon.log.1`.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Thread-safe log writer (`daemon.log`). Write errors are ignored: logging
/// must never stop the daemon.
#[derive(Debug)]
pub struct Logger {
    path: Option<PathBuf>,
    file: Mutex<Option<File>>,
    echo: bool,
}

impl Logger {
    /// Append to `path` (created `0600`). With `echo`, lines are also printed
    /// to stderr (`kget daemon --foreground`).
    pub fn open(path: &Path, echo: bool) -> Self {
        let file = open_append(path);
        Self {
            path: Some(path.to_path_buf()),
            file: Mutex::new(file),
            echo,
        }
    }

    /// A logger that discards everything.
    pub fn disabled() -> Self {
        Self {
            path: None,
            file: Mutex::new(None),
            echo: false,
        }
    }

    /// Path of the log file, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Write one line prefixed with a UTC timestamp and `level`.
    pub fn log(&self, level: &str, msg: &str) {
        let line = format!(
            "{} {level:<5} {}\n",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            msg.replace(['\n', '\r'], " ")
        );
        if self.echo {
            eprint!("{line}");
        }
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if let (Some(path), Some(file)) = (&self.path, guard.as_mut()) {
            let _ = file.write_all(line.as_bytes());
            if file.metadata().map(|m| m.len()).unwrap_or(0) > MAX_LOG_BYTES {
                let _ = std::fs::rename(path, path.with_extension("log.1"));
                *guard = open_append(path);
            }
        }
    }

    /// `INFO` line.
    pub fn info(&self, msg: &str) {
        self.log("INFO", msg);
    }

    /// `WARN` line.
    pub fn warn(&self, msg: &str) {
        self.log("WARN", msg);
    }
}

fn open_append(path: &Path) -> Option<File> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    let _ = super::restrict_to_owner(path);
    Some(file)
}
