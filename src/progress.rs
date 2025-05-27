use indicatif::{ProgressBar, ProgressStyle};

/// Make a custom progress bar for file downloads
/// 
/// # Arguments
///
/// * `quiet_mode` - If true, the progress bar will be hidden
/// * `msg` - Message to be displayed on the bar
/// * `length` - Total file size in bytes (optional)
/// * `is_parallel` - If true, shows parallel download information
///
/// # Returns
///
/// A progress bar configured with a custom style
pub fn create_progress_bar(quiet_mode: bool, msg: String, length: Option<u64>, is_parallel: bool) -> ProgressBar {
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
                .progress_chars("=>-")
        );
    } else {
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg} {elapsed}")
                .unwrap()
        );
    }

    bar
}