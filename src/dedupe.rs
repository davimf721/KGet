//! Duplicate detection: downloads already in the history, and identical
//! content saved under another name.
//!
//! Before a download, [`find_previous`] looks for a completed history entry
//! with the same URL (normalized with [`normalize_url`]: credentials and
//! fragment removed) whose file still exists with the recorded size and,
//! when known, SHA-256. What happens then is a [`DuplicatePolicy`]
//! (`--duplicates skip|ask|download|link`).
//!
//! After a download, [`find_same_content`] looks for another completed entry
//! with the same SHA-256; with `--dedupe` the new file is replaced by a hard
//! link to the existing one ([`replace_with_hardlink`]).

use crate::queue::{EntryStatus, HistoryEntry, HistoryQuery, HistoryStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// What to do when the URL was already downloaded and the file is still there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DuplicatePolicy {
    /// Keep the existing file and do not download
    Skip,
    /// Ask on the terminal (falls back to `Download` without one)
    Ask,
    /// Download again (the behaviour before KGet 1.8)
    Download,
    /// Hard-link (or copy) the existing file to the new destination
    Link,
}

impl DuplicatePolicy {
    /// Stable lowercase name.
    pub fn as_str(&self) -> &'static str {
        match self {
            DuplicatePolicy::Skip => "skip",
            DuplicatePolicy::Ask => "ask",
            DuplicatePolicy::Download => "download",
            DuplicatePolicy::Link => "link",
        }
    }
}

impl FromStr for DuplicatePolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "skip" => Ok(DuplicatePolicy::Skip),
            "ask" | "prompt" => Ok(DuplicatePolicy::Ask),
            "download" | "redownload" | "always" => Ok(DuplicatePolicy::Download),
            "link" | "hardlink" => Ok(DuplicatePolicy::Link),
            other => Err(format!(
                "unknown duplicate policy '{other}' (expected skip, ask, download or link)"
            )),
        }
    }
}

/// A history entry whose file is still on disk.
#[derive(Debug, Clone)]
pub struct Duplicate {
    /// The history entry
    pub entry: HistoryEntry,
    /// Where its file is
    pub path: PathBuf,
}

/// Normalize a URL for duplicate detection: credentials and fragment
/// removed, scheme and host lowercased, default port dropped.
///
/// ```rust
/// use kget::dedupe::normalize_url;
///
/// assert_eq!(
///     normalize_url("HTTPS://me:pw@Example.com:443/a.iso#top"),
///     "https://example.com/a.iso"
/// );
/// ```
pub fn normalize_url(raw: &str) -> String {
    match url::Url::parse(raw.trim()) {
        Ok(mut u) => {
            let _ = u.set_password(None);
            let _ = u.set_username("");
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => raw.trim().to_string(),
    }
}

/// Path of the file recorded by a history entry.
pub fn entry_path(e: &HistoryEntry) -> PathBuf {
    Path::new(&crate::utils::expand_tilde(&e.output_dir)).join(&e.filename)
}

/// Whether the entry's file still exists with the recorded size (and
/// SHA-256, when one was recorded).
fn still_valid(e: &HistoryEntry) -> Option<PathBuf> {
    let path = entry_path(e);
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || e.bytes_total != Some(meta.len()) {
        return None;
    }
    if let Some(want) = &e.sha256 {
        let got =
            crate::checksum::compute_checksum(&path, &crate::ChecksumAlgorithm::Sha256).ok()?;
        if !got.eq_ignore_ascii_case(want) {
            return None;
        }
    }
    Some(path)
}

/// Newest completed download of `url` whose file is still intact, from
/// the history database `store`.
pub fn find_previous_in(store: &HistoryStore, url: &str) -> Option<Duplicate> {
    let want = normalize_url(url);
    let mut q = HistoryQuery::new()
        .status(EntryStatus::Completed)
        .limit(200);
    q = match url::Url::parse(&want)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    {
        Some(host) => q.domain(&host),
        None => q.search(&want),
    };
    store
        .query(&q)
        .ok()?
        .into_iter()
        .filter(|e| normalize_url(&e.url) == want)
        .find_map(|entry| still_valid(&entry).map(|path| Duplicate { entry, path }))
}

/// [`find_previous_in`] on the default history database.
pub fn find_previous(url: &str) -> Option<Duplicate> {
    find_previous_in(&HistoryStore::open_default().ok()?, url)
}

/// A completed entry with the same SHA-256 whose file still exists and is
/// not `exclude`.
pub fn find_same_content_in(
    store: &HistoryStore,
    sha256: &str,
    exclude: &Path,
) -> Option<Duplicate> {
    let exclude = std::fs::canonicalize(exclude).unwrap_or_else(|_| exclude.to_path_buf());
    store
        .query(
            &HistoryQuery::new()
                .status(EntryStatus::Completed)
                .sha256(sha256)
                .limit(50),
        )
        .ok()?
        .into_iter()
        .find_map(|entry| {
            let path = entry_path(&entry);
            let canon = std::fs::canonicalize(&path).ok()?;
            let meta = std::fs::metadata(&canon).ok()?;
            (canon != exclude
                && meta.is_file()
                && entry.bytes_total.is_none_or(|b| b == meta.len()))
            .then_some(Duplicate { entry, path: canon })
        })
}

/// [`find_same_content_in`] on the default history database.
pub fn find_same_content(sha256: &str, exclude: &Path) -> Option<Duplicate> {
    find_same_content_in(&HistoryStore::open_default().ok()?, sha256, exclude)
}

/// Make `dst` a hard link to `src` (a copy when hard links are not possible,
/// e.g. across file systems). Returns `true` for a hard link.
pub fn link_or_copy(src: &Path, dst: &Path) -> std::io::Result<bool> {
    if let Some(dir) = dst.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    if dst.exists() {
        std::fs::remove_file(dst)?;
    }
    match std::fs::hard_link(src, dst) {
        Ok(()) => Ok(true),
        Err(_) => std::fs::copy(src, dst).map(|_| false),
    }
}

/// Replace `target` with a hard link to `existing` (same content). The swap
/// goes through a temporary name, so `target` is never missing; on failure
/// (other file system, no hard-link support) `target` is left untouched.
pub fn replace_with_hardlink(existing: &Path, target: &Path) -> std::io::Result<()> {
    let mut tmp_name = target.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".kget-link-{}", std::process::id()));
    let tmp = target.with_file_name(tmp_name);
    let _ = std::fs::remove_file(&tmp);
    std::fs::hard_link(existing, &tmp)?;
    std::fs::rename(&tmp, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies_parse() {
        assert_eq!("SKIP".parse(), Ok(DuplicatePolicy::Skip));
        assert_eq!("hardlink".parse(), Ok(DuplicatePolicy::Link));
        assert!("maybe".parse::<DuplicatePolicy>().is_err());
    }

    #[test]
    fn urls_normalize() {
        assert_eq!(normalize_url("ftp://u@h/x#y"), "ftp://h/x");
        assert_eq!(normalize_url("not a url"), "not a url");
    }
}
