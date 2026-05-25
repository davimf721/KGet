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

/// Returns `true` when the path's extension is a supported archive format.
pub fn is_extractable(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    name.ends_with(".zip")
        || name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar.bz2")
        || name.ends_with(".tbz2")
        || name.ends_with(".tar.xz")
        || name.ends_with(".txz")
        || name.ends_with(".7z")
}

/// Extract an archive at `path` into its parent directory using the system tool
/// (`unzip`, `tar`, or `7z`).  Returns `Ok(())` if the path is not a recognised
/// archive type (no-op) or if extraction succeeds.
///
/// # Errors
///
/// Returns an error if the tool is not found or exits with a non-zero status.
pub fn auto_extract(
    path: &std::path::Path,
    quiet: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path_str = path.to_str().ok_or("Invalid UTF-8 in path")?;
    let dir_str = path
        .parent()
        .and_then(|d| d.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(".");
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (cmd, args): (&str, Vec<String>) = if name.ends_with(".zip") {
        (
            "unzip",
            vec![
                "-o".into(),
                path_str.into(),
                "-d".into(),
                dir_str.into(),
            ],
        )
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        (
            "tar",
            vec![
                "-xzf".into(),
                path_str.into(),
                "-C".into(),
                dir_str.into(),
            ],
        )
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        (
            "tar",
            vec![
                "-xjf".into(),
                path_str.into(),
                "-C".into(),
                dir_str.into(),
            ],
        )
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        (
            "tar",
            vec![
                "-xJf".into(),
                path_str.into(),
                "-C".into(),
                dir_str.into(),
            ],
        )
    } else if name.ends_with(".7z") {
        (
            "7z",
            vec!["x".into(), path_str.into(), format!("-o{dir_str}")],
        )
    } else {
        return Ok(());
    };

    if !quiet {
        println!("Extracting {} …", path.display());
    }

    let status = std::process::Command::new(cmd)
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to run `{cmd}`: {e} — is it installed?"))?;

    if !status.success() {
        return Err(format!(
            "`{cmd}`: extraction failed (exit {:?})",
            status.code()
        )
        .into());
    }

    if !quiet {
        println!("Extracted to: {dir_str}");
    }
    Ok(())
}
