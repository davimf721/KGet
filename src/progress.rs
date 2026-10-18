//! Progress bar utilities for download visualization.
//!
//! This module provides a customizable progress bar for tracking downloads
//! in the terminal.
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::create_progress_bar;
//!
//! // Create a progress bar for a 100MB file
//! let pb = create_progress_bar(false, "Downloading file.zip".into(), Some(100_000_000), false);
//! pb.set_position(50_000_000); // 50% complete
//! pb.finish_with_message("Download complete!");
//! ```

use indicatif::{ProgressBar, ProgressStyle};

/// Create a progress bar for download tracking.
///
/// # Arguments
///
/// * `quiet_mode` - If true, returns a hidden progress bar (no output)
/// * `msg` - Message to display above the progress bar
/// * `length` - Total size in bytes (None for indeterminate spinner)
/// * `is_parallel` - Kept for API compatibility (no visual difference)
///
/// # Returns
///
/// A configured [`ProgressBar`] ready for use.
///
/// # Example
///
/// ```rust,no_run
/// use kget::create_progress_bar;
///
/// // Determinate progress bar
/// let pb = create_progress_bar(false, "file.zip".into(), Some(1000), false);
///
/// // Indeterminate spinner (unknown size)
/// let spinner = create_progress_bar(false, "Connecting...".into(), None, false);
///
/// // Parallel download progress
/// let parallel = create_progress_bar(false, "file.iso".into(), Some(4_000_000_000), true);
/// ```
pub fn create_progress_bar(
    quiet_mode: bool,
    msg: String,
    length: Option<u64>,
    is_parallel: bool,
) -> ProgressBar {
    let bar = if quiet_mode {
        ProgressBar::hidden()
    } else {
        match length {
            Some(len) => ProgressBar::new(len),
            None => ProgressBar::new_spinner(),
        }
    };

    bar.set_message(msg);

    if length.is_some() {
        // `is_parallel` used to add a "Chunks:" line built from a key indicatif
        // does not provide; both modes now share the same template.
        let _ = is_parallel;
        let template = "{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%) eta: {eta} speed: {binary_bytes_per_sec}";

        bar.set_style(
            ProgressStyle::default_bar()
                .template(template)
                .unwrap()
                .progress_chars("=>-"),
        );
    } else {
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg} {elapsed}")
                .unwrap(),
        );
    }

    bar
}

/// Rate limiter for progress notifications.
///
/// Use it so callbacks, `PROGRESS:` lines and JSONL events fire at most every
/// `ms` milliseconds (and always at 100%), instead of once per buffer.
#[derive(Debug, Clone)]
pub struct Throttle {
    last: Option<std::time::Instant>,
    every: std::time::Duration,
    last_pct: f64,
}

impl Throttle {
    /// Create a limiter that fires at most every `ms` milliseconds.
    pub fn new(ms: u64) -> Self {
        Self {
            last: None,
            every: std::time::Duration::from_millis(ms),
            last_pct: -1.0,
        }
    }

    /// `true` when the interval has elapsed and the percentage moved at least
    /// 0.1 point, or when 100% is reached for the first time.
    pub fn ready(&mut self, pct: f64) -> bool {
        let elapsed = self.last.is_none_or(|t| t.elapsed() >= self.every);
        let finished = pct >= 100.0 && self.last_pct < 100.0;
        let ok = finished || (elapsed && (pct - self.last_pct).abs() >= 0.1);
        if ok {
            self.last = Some(std::time::Instant::now());
            self.last_pct = pct;
        }
        ok
    }

    /// Time-only variant for transfers whose total size is unknown.
    pub fn tick(&mut self) -> bool {
        let ok = self.last.is_none_or(|t| t.elapsed() >= self.every);
        if ok {
            self.last = Some(std::time::Instant::now());
        }
        ok
    }
}
