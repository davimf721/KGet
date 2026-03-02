//! Utility functions for KGet.
//!
//! This module provides helper functions used throughout the library:
//! - Console output management
//! - URL filename extraction
//! - Path resolution
//!
//! # Example
//!
//! ```rust
//! use kget::get_filename_from_url_or_default;
//!
//! let name = get_filename_from_url_or_default(
//!     "https://example.com/downloads/file.zip",
//!     "download"
//! );
//! assert_eq!(name, "file.zip");
//! ```

/// Print a message to the console if not in quiet mode.
/// 
/// # Arguments
/// 
/// * `msg` - The message to print
/// * `quiet_mode` - If true, suppress printing the message
///
/// # Example
///
/// ```rust
/// use kget::print;
///
/// print("Starting download...", false); // Prints to stdout
/// print("Starting download...", true);  // Suppressed
/// ```
pub fn print(msg: &str, quiet_mode: bool) {
    if !quiet_mode {
        println!("{}", msg);
    }
}

/// Extract the filename from a URL or return a default.
///
/// Parses the URL and returns the last path segment as the filename.
/// If parsing fails or the path is empty, returns the default filename.
///
/// # Arguments
///
/// * `url_str` - URL to extract filename from
/// * `default_filename` - Fallback filename if extraction fails
///
/// # Returns
///
/// The extracted filename or the default.
///
/// # Example
///
/// ```rust
/// use kget::get_filename_from_url_or_default;
///
/// // Successful extraction
/// assert_eq!(
///     get_filename_from_url_or_default("https://example.com/file.zip", "default"),
///     "file.zip"
/// );
///
/// // Fallback to default
/// assert_eq!(
///     get_filename_from_url_or_default("https://example.com/", "download.bin"),
///     "download.bin"
/// );
/// ```
pub fn get_filename_from_url_or_default(url_str: &str, default_filename: &str) -> String {
    // Tries to parse the URL
    if let Ok(parsed_url) = url::Url::parse(url_str) {
        // Tries to get the last segment of the path
        if let Some(segments) = parsed_url.path_segments() {
            if let Some(last_segment) = segments.last() {
                if !last_segment.is_empty() {
                    return last_segment.to_string();
                }
            }
        }
    }
    // Returns the default filename if parsing fails or the path is empty/invalid
    default_filename.to_string()
}

/// Resolve the final output path for a download.
///
/// Handles three cases:
/// 1. `output_arg` is `None`: Extract filename from URL
/// 2. `output_arg` is a directory: Append filename from URL to directory
/// 3. `output_arg` is a file path: Use it directly
///
/// # Arguments
///
/// * `output_arg` - User-provided output path (can be file or directory)
/// * `url` - Source URL for filename extraction
/// * `default_name` - Fallback filename if URL doesn't contain one
///
/// # Returns
///
/// The resolved output file path.
///
/// # Example
///
/// ```rust
/// use kget::resolve_output_path;
///
/// // No output specified - use filename from URL
/// let path = resolve_output_path(None, "https://example.com/file.zip", "download");
/// assert_eq!(path, "file.zip");
///
/// // Custom filename
/// let path = resolve_output_path(Some("myfile.zip".to_string()), "https://example.com/file.zip", "download");
/// assert_eq!(path, "myfile.zip");
/// ```
pub fn resolve_output_path(output_arg: Option<String>, url: &str, default_name: &str) -> String {
    if let Some(path_str) = output_arg {
        let path = std::path::Path::new(&path_str);
        if path.is_dir() {
            let filename = get_filename_from_url_or_default(url, default_name);
            path.join(filename).to_string_lossy().to_string()
        } else {
            path_str
        }
    } else {
        get_filename_from_url_or_default(url, default_name)
    }
}

