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
/// * `is_parallel` - If true, shows chunk information for parallel downloads
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

    if let Some(_) = length {
        let template = if is_parallel {
            "{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%) eta: {eta} speed: {binary_bytes_per_sec}\nChunks: {chunks} active"
        } else {
            "{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({percent}%) eta: {eta} speed: {binary_bytes_per_sec}"
        };

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
