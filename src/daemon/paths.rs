//! Output path validation for jobs submitted by clients.
//!
//! Clients of the daemon and the MCP server (scripts, browser extensions,
//! LLM agents) must not be able to write anywhere the user can. A
//! [`PathPolicy`] resolves the directory and file name a client asks for and
//! rejects:
//!
//! - `..` components and NUL bytes, anywhere;
//! - file names that are not a single safe component (`a/b`, `.`, `x:y`…);
//! - unless `allow_outside` is set, any path outside the root directory —
//!   including through symbolic links, which are resolved before comparing.
//!
//! ```rust
//! use kget::daemon::PathPolicy;
//!
//! let root = tempfile::tempdir().unwrap();
//! let policy = PathPolicy::new(root.path(), false).unwrap();
//! assert!(policy.resolve_dir(Some("isos")).is_ok());
//! assert!(policy.resolve_dir(Some("../escape")).is_err());
//! assert!(policy.resolve_dir(Some("/etc")).is_err());
//! assert!(policy.check_file_name("../passwd").is_err());
//! ```

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Why a path was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// Contains `..`, a NUL byte, or is otherwise malformed
    Invalid(String),
    /// Resolves outside the allowed root
    OutsideRoot { path: PathBuf, root: PathBuf },
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::Invalid(msg) => write!(f, "Invalid path: {msg}"),
            PathError::OutsideRoot { path, root } => write!(
                f,
                "Path {} is outside the allowed directory {}",
                path.display(),
                root.display()
            ),
        }
    }
}

impl std::error::Error for PathError {}

/// Where clients may save files. Cheap to clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPolicy {
    root: PathBuf,
    allow_outside: bool,
}

impl PathPolicy {
    /// Allow writes inside `root` (created if missing), or anywhere with
    /// `allow_outside`. Relative client paths are always taken relative to
    /// `root`.
    pub fn new(root: impl AsRef<Path>, allow_outside: bool) -> io::Result<Self> {
        let root = PathBuf::from(crate::utils::expand_tilde(&root.as_ref().to_string_lossy()));
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root: root.canonicalize()?,
            allow_outside,
        })
    }

    /// The root directory (canonical).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether paths outside the root are accepted.
    pub fn allows_outside(&self) -> bool {
        self.allow_outside
    }

    /// Resolve a client directory (`None` = the root; relative = inside the
    /// root) to an absolute path, without creating it.
    pub fn resolve_dir(&self, dir: Option<&str>) -> Result<PathBuf, PathError> {
        let Some(dir) = dir.map(str::trim).filter(|d| !d.is_empty()) else {
            return Ok(self.root.clone());
        };
        let expanded = crate::utils::expand_tilde(dir);
        let path = Path::new(&expanded);
        check_components(path)?;
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        self.check_inside(&full)?;
        Ok(full)
    }

    /// Check that `name` is a single safe file name.
    pub fn check_file_name(&self, name: &str) -> Result<(), PathError> {
        if name.is_empty() || name == "." || name == ".." {
            return Err(PathError::Invalid(format!("'{name}' is not a file name")));
        }
        if crate::utils::sanitize_filename(name) != name {
            return Err(PathError::Invalid(format!(
                "'{name}' is not a single safe file name (no separators, control or reserved characters)"
            )));
        }
        Ok(())
    }

    /// Resolve `dir` and `name` to the absolute path of a file. The file
    /// itself, when it exists, must also resolve inside the root (a symlink
    /// pointing elsewhere is rejected).
    pub fn resolve_file(&self, dir: Option<&str>, name: &str) -> Result<PathBuf, PathError> {
        self.check_file_name(name)?;
        let full = self.resolve_dir(dir)?.join(name);
        self.check_inside(&full)?;
        Ok(full)
    }

    /// Resolve a client path to an existing file to read (`verify_checksum`):
    /// relative paths are inside the root.
    pub fn resolve_existing(&self, path: &str) -> Result<PathBuf, PathError> {
        let expanded = crate::utils::expand_tilde(path.trim());
        let p = Path::new(&expanded);
        check_components(p)?;
        let full = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        };
        let canonical = full
            .canonicalize()
            .map_err(|e| PathError::Invalid(format!("{}: {e}", full.display())))?;
        self.check_inside(&canonical)?;
        Ok(canonical)
    }

    /// Fail if `path` (absolute, without `..`) resolves outside the root.
    fn check_inside(&self, path: &Path) -> Result<(), PathError> {
        if self.allow_outside {
            return Ok(());
        }
        let Some(resolved) = resolve_existing_prefix(path) else {
            return Err(PathError::Invalid(format!(
                "'{}' goes through a broken symbolic link",
                path.display()
            )));
        };
        if resolved.starts_with(&self.root) {
            Ok(())
        } else {
            Err(PathError::OutsideRoot {
                path: path.to_path_buf(),
                root: self.root.clone(),
            })
        }
    }
}

/// Reject `..`, NUL bytes and (on Windows) drive-relative prefixes.
fn check_components(path: &Path) -> Result<(), PathError> {
    if path.to_string_lossy().contains('\0') {
        return Err(PathError::Invalid("contains a NUL byte".into()));
    }
    for c in path.components() {
        if matches!(c, Component::ParentDir) {
            return Err(PathError::Invalid(format!(
                "'{}' contains '..'",
                path.display()
            )));
        }
    }
    if !path.is_absolute() && path.has_root() {
        // `\dir` or `C:dir` on Windows: relative to an unknown drive or cwd
        return Err(PathError::Invalid(format!(
            "'{}' is neither absolute nor relative",
            path.display()
        )));
    }
    Ok(())
}

/// Canonicalize the longest existing prefix of `path` (resolving symlinks)
/// and append the rest. `path` has no `..` components. `None` when a
/// component is a broken symlink (writing through it could land anywhere).
fn resolve_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(c) = existing.canonicalize() {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return Some(out);
        }
        if existing
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return None;
        }
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return Some(path.to_path_buf()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_absolute_dirs() {
        let root = tempfile::tempdir().unwrap();
        let p = PathPolicy::new(root.path(), false).unwrap();
        let canon = root.path().canonicalize().unwrap();
        assert_eq!(p.resolve_dir(None).unwrap(), canon);
        assert_eq!(p.resolve_dir(Some("")).unwrap(), canon);
        assert_eq!(p.resolve_dir(Some("a/b")).unwrap(), canon.join("a/b"));
        assert_eq!(
            p.resolve_dir(Some(canon.join("x").to_str().unwrap()))
                .unwrap(),
            canon.join("x")
        );
        assert!(matches!(
            p.resolve_dir(Some("a/../../b")),
            Err(PathError::Invalid(_))
        ));
        let outside = tempfile::tempdir().unwrap();
        assert!(matches!(
            p.resolve_dir(Some(outside.path().to_str().unwrap())),
            Err(PathError::OutsideRoot { .. })
        ));
    }

    #[test]
    fn allow_outside_still_rejects_parent_components() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let p = PathPolicy::new(root.path(), true).unwrap();
        assert!(
            p.resolve_dir(Some(outside.path().to_str().unwrap()))
                .is_ok()
        );
        assert!(p.resolve_dir(Some("../x")).is_err());
    }

    #[test]
    fn file_names_are_single_components() {
        let root = tempfile::tempdir().unwrap();
        let p = PathPolicy::new(root.path(), false).unwrap();
        assert!(p.resolve_file(None, "file.iso").is_ok());
        for bad in ["", ".", "..", "a/b", "..\\x", "a\0b", "x:y"] {
            assert!(p.check_file_name(bad).is_err(), "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("target"), root.path().join("file-link"))
            .unwrap();
        std::fs::write(outside.path().join("target"), b"x").unwrap();
        let p = PathPolicy::new(root.path(), false).unwrap();
        assert!(p.resolve_dir(Some("link/sub")).is_err());
        assert!(p.resolve_file(None, "file-link").is_err());
        assert!(p.resolve_existing("file-link").is_err());
        std::os::unix::fs::symlink(outside.path().join("missing"), root.path().join("dangling"))
            .unwrap();
        assert!(p.resolve_file(None, "dangling").is_err());
    }
}
