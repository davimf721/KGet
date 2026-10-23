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
    url::Url::parse(url_str)
        .ok()
        .and_then(|u| u.path_segments()?.next_back().map(str::to_string))
        .filter(|s| !s.is_empty())
        .map(|s| urlencoding::decode(&s).map(|c| c.into_owned()).unwrap_or(s))
        .map(|s| sanitize_filename(&s))
        .unwrap_or_else(|| default_filename.to_string())
}

/// Turn an untrusted name (from a URL, `Content-Disposition`, a manifest…)
/// into a safe single path component.
///
/// Never fails: separators, control characters and characters forbidden on
/// Windows become `_`, leading/trailing dots and spaces are trimmed, and the
/// result is truncated to 255 bytes on a UTF-8 boundary. An empty result
/// becomes `"download"`.
///
/// ```rust
/// use kget::utils::sanitize_filename;
///
/// assert_eq!(sanitize_filename("../etc/passwd"), "_etc_passwd");
/// assert_eq!(sanitize_filename("v1..2.tar.gz"), "v1..2.tar.gz");
/// assert_eq!(sanitize_filename(""), "download");
/// ```
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Truncate first, then trim: truncating last could leave a trailing dot
    // or space, and trimming spaces and dots in one pass handles ". . a".
    let mut end = cleaned.len().min(255);
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    let cleaned = cleaned[..end].trim_matches(|c: char| c == '.' || c.is_whitespace());
    if cleaned.is_empty() {
        "download".to_string()
    } else {
        cleaned.to_string()
    }
}

/// Expand a leading `~` / `~/` to the user's home directory.
///
/// ```rust
/// use kget::utils::expand_tilde;
///
/// assert_eq!(expand_tilde("/tmp/x"), "/tmp/x");
/// assert!(!expand_tilde("~/Downloads").starts_with('~'));
/// ```
pub fn expand_tilde(p: &str) -> String {
    if (p == "~" || p.starts_with("~/") || p.starts_with("~\\"))
        && let Some(home) = dirs::home_dir()
    {
        let rest = p[1..].trim_start_matches(['/', '\\']);
        if rest.is_empty() {
            return home.to_string_lossy().into_owned();
        }
        return home.join(rest).to_string_lossy().into_owned();
    }
    p.to_string()
}

/// Reserve a unique path for `name` inside `dir`.
///
/// `file.zip` becomes `file (1).zip`, `file (2).zip`… when the file already
/// exists on disk or was already reserved in `taken`.
pub fn unique_path(
    dir: &std::path::Path,
    name: &str,
    taken: &mut std::collections::HashSet<std::path::PathBuf>,
) -> std::path::PathBuf {
    let p = std::path::Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let mut candidate = dir.join(name);
    let mut i = 1;
    while candidate.exists() || taken.contains(&candidate) {
        candidate = dir.join(format!("{stem} ({i}){ext}"));
        i += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

/// `true` if an executable named `tool` is found in `PATH`.
pub fn which_in_path(tool: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{tool}.exe"),
            format!("{tool}.cmd"),
            tool.to_string(),
        ]
    } else {
        vec![tool.to_string()]
    };
    std::env::split_paths(&path)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|c| c.is_file()))
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
        let path_str = expand_tilde(&path_str);
        let path = std::path::Path::new(&path_str);
        if path.is_dir() || path_str.ends_with('/') || path_str.ends_with(std::path::MAIN_SEPARATOR)
        {
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
    let output_dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let dir_str = output_dir.to_str().ok_or("Invalid UTF-8 in output path")?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Never extract directly into an existing download directory.  Archive
    // tools may follow a symlink created by an earlier archive entry, letting
    // a later entry overwrite a file outside that directory.  A fresh sibling
    // directory has no such pre-existing links; inspect its result before
    // publishing it to the requested destination.
    let staging = ExtractionDir::new(output_dir)?;
    let staging_str = staging
        .path()
        .to_str()
        .ok_or("Invalid UTF-8 in staging path")?;
    let (cmd, args): (&str, Vec<String>) = if name.ends_with(".zip") {
        (
            "unzip",
            vec![
                "-o".into(),
                path_str.into(),
                "-d".into(),
                staging_str.into(),
            ],
        )
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        (
            "tar",
            vec![
                "-xzf".into(),
                path_str.into(),
                "-C".into(),
                staging_str.into(),
            ],
        )
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        (
            "tar",
            vec![
                "-xjf".into(),
                path_str.into(),
                "-C".into(),
                staging_str.into(),
            ],
        )
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        (
            "tar",
            vec![
                "-xJf".into(),
                path_str.into(),
                "-C".into(),
                staging_str.into(),
            ],
        )
    } else if name.ends_with(".7z") {
        (
            "7z",
            vec!["x".into(), path_str.into(), format!("-o{staging_str}")],
        )
    } else {
        return Ok(());
    };

    if !quiet {
        println!("Extracting {} …", path.display());
    }

    let mut command = std::process::Command::new(cmd);
    command.args(&args);
    if quiet {
        // Never let the tool's listing leak into machine-readable output.
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }
    let status = command
        .status()
        .map_err(|e| format!("Failed to run `{cmd}`: {e} — is it installed?"))?;

    if !status.success() {
        return Err(format!("`{cmd}`: extraction failed (exit {:?})", status.code()).into());
    }

    publish_extraction(staging.path(), output_dir)?;

    if !quiet {
        println!("Extracted to: {dir_str}");
    }
    Ok(())
}

/// A uniquely-created, sibling staging directory for an archive extraction.
/// It is removed on every return path.
struct ExtractionDir(std::path::PathBuf);

impl ExtractionDir {
    fn new(parent: &std::path::Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(parent)?;
        let pid = std::process::id();
        for attempt in 0..128u32 {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = parent.join(format!(".kget-extract-{pid}-{nonce}-{attempt}"));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique extraction directory",
        ))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for ExtractionDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Refuse links in extracted content, then move only top-level entries into
/// the destination.  Refusing pre-existing targets makes publication unable to
/// traverse or overwrite an attacker-controlled link in that destination.
fn publish_extraction(
    staging: &std::path::Path,
    output_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    reject_symlinks(staging)?;
    let entries: Vec<std::path::PathBuf> = std::fs::read_dir(staging)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    for source in &entries {
        let name = source.file_name().ok_or("Invalid extracted file name")?;
        if output_dir.join(name).symlink_metadata().is_ok() {
            return Err(format!(
                "refusing to overwrite existing extracted path: {}",
                name.to_string_lossy()
            )
            .into());
        }
    }
    for source in entries {
        let name = source.file_name().ok_or("Invalid extracted file name")?;
        std::fs::rename(&source, output_dir.join(name))?;
    }
    Ok(())
}

fn reject_symlinks(dir: &std::path::Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ty = std::fs::symlink_metadata(&path)?.file_type();
        if ty.is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("archive contains a symbolic link: {}", path.display()),
            ));
        }
        if ty.is_dir() {
            reject_symlinks(&path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod extraction_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn extracted_symlinks_are_rejected_before_publish() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("staging");
        let out = root.path().join("out");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        symlink("/tmp", staging.join("escape")).unwrap();
        assert!(publish_extraction(&staging, &out).is_err());
        assert!(!out.join("escape").exists());
    }
}
