//! Multi-file transfers shared by the AI model sources (`hf://`, `ollama://`).
//!
//! A model download is a list of files ([`FetchItem`]) fetched with the turbo
//! engine ([`AdvancedDownloader`]: parallel connections, `.part`/`.kget`
//! resume, SHA-256 verification), a few files at a time. Files already
//! complete on disk are skipped, so running the same command again resumes an
//! interrupted multi-file download where it stopped.
//!
//! Progress is aggregated over all files: one terminal progress bar, or —
//! with an event sink — the JSONL `files`, `progress` and `file_progress`
//! events also used by torrents.

use crate::advanced_download::{AdvancedDownloader, ResumePolicy};
use crate::config::ProxyConfig;
use crate::error::KgetError;
use crate::events::{FileEntry, FileProgressEntry, JsonEvent, SpeedMeter};
use crate::optimization::Optimizer;
use crate::runner::EventSink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Upper bound for files downloaded at the same time.
pub const MAX_PARALLEL_FILES: usize = 16;

/// Network and output settings for a multi-file transfer.
#[derive(Clone)]
pub struct TransferOptions {
    /// Proxy used for every request
    pub proxy: ProxyConfig,
    /// Connections per file and global speed limit (shared by the files)
    pub optimizer: Optimizer,
    /// Set to `true` to stop; partial files are kept for resume
    pub cancel: Arc<AtomicBool>,
    /// Maximum attempts per request
    pub retries: u32,
    /// Connect / per-read timeout in seconds (0 = no read timeout)
    pub timeout: u64,
    /// Extra HTTP headers sent with every file request
    pub headers: Vec<(String, String)>,
    /// Files downloaded at the same time (1–[`MAX_PARALLEL_FILES`])
    pub parallel_files: usize,
    /// No terminal output
    pub quiet: bool,
    /// Structured events (`files`, `progress`, `file_progress`, `status`)
    pub events: Option<EventSink>,
}

impl std::fmt::Debug for TransferOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Headers may carry tokens: only their names are shown.
        let names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("TransferOptions")
            .field("proxy_enabled", &self.proxy.enabled)
            .field("retries", &self.retries)
            .field("timeout", &self.timeout)
            .field("headers", &names)
            .field("parallel_files", &self.parallel_files)
            .field("quiet", &self.quiet)
            .field("events", &self.events.is_some())
            .finish()
    }
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            proxy: ProxyConfig::default(),
            optimizer: Optimizer::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            retries: crate::http::DEFAULT_RETRIES,
            timeout: crate::http::DEFAULT_TIMEOUT_SECS,
            headers: Vec::new(),
            parallel_files: 4,
            quiet: true,
            events: None,
        }
    }
}

impl TransferOptions {
    pub(crate) fn emit(&self, ev: &JsonEvent) {
        if let Some(sink) = &self.events {
            sink(ev);
        }
    }

    /// Status line: printed on the terminal, or sent as a `status` event.
    pub(crate) fn status(&self, message: impl Into<String>) {
        let message = message.into();
        if !self.quiet {
            println!("{message}");
        }
        self.emit(&JsonEvent::Status { message });
    }
}

/// One file of a multi-file transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchItem {
    /// Name shown in progress output and `files` events (relative path)
    pub name: String,
    /// URL to download
    pub url: String,
    /// Final location
    pub dest: PathBuf,
    /// Expected size in bytes, when known
    pub size: Option<u64>,
    /// Expected SHA-256 (lowercase hex), verified by the engine
    pub sha256: Option<String>,
}

/// What a multi-file transfer did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FetchStats {
    /// Files in the transfer
    pub files: usize,
    /// Files already complete on disk (not downloaded again)
    pub skipped: usize,
    /// Total size of all files in bytes
    pub bytes: u64,
}

/// Result of a model download (`hf://`, `ollama://`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDownload {
    /// The saved file (single-file download) or the directory holding the files
    pub path: PathBuf,
    /// Whether `path` is a single downloaded file
    pub single_file: bool,
    /// Files in the download
    pub files: usize,
    /// Files that were already complete on disk
    pub skipped: usize,
    /// Total size in bytes
    pub bytes: u64,
}

/// A server-provided relative path (`dir/file.bin`) as a safe local path:
/// `None` for absolute paths, `..`, empty components or drive prefixes, so a
/// listing can never write outside the destination directory.
pub fn safe_relative_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

/// Whether `item` is already complete at its destination.
fn is_complete(item: &FetchItem) -> bool {
    match std::fs::metadata(&item.dest) {
        Ok(m) if m.is_file() => item.size.is_none_or(|s| s == m.len()),
        _ => false,
    }
}

/// Download every item, `opts.parallel_files` at a time.
///
/// Stops starting new files after the first failure (files in flight finish
/// or keep their `.part` for resume) and returns that failure.
pub fn fetch_all(items: &[FetchItem], opts: &TransferOptions) -> Result<FetchStats, KgetError> {
    let sizes: Vec<u64> = items.iter().map(|i| i.size.unwrap_or(0)).collect();
    let total: u64 = sizes.iter().sum();
    opts.emit(&JsonEvent::Files {
        files: items
            .iter()
            .map(|i| FileEntry {
                name: i.name.clone(),
                size: i.size.unwrap_or(0),
                // `--include` / `--exclude` already removed unwanted files.
                selected: None,
            })
            .collect(),
    });

    let done: Arc<Vec<AtomicU64>> = Arc::new(items.iter().map(|_| AtomicU64::new(0)).collect());
    let mut pending = Vec::new();
    let mut skipped = 0;
    for (idx, item) in items.iter().enumerate() {
        if is_complete(item) {
            let len = std::fs::metadata(&item.dest).map(|m| m.len()).unwrap_or(0);
            done[idx].store(len, Ordering::Relaxed);
            skipped += 1;
        } else {
            pending.push(idx);
        }
    }
    if skipped > 0 {
        opts.status(format!(
            "{skipped} of {} file(s) already complete, skipping",
            items.len()
        ));
    }

    let threads = opts
        .parallel_files
        .clamp(1, MAX_PARALLEL_FILES)
        .min(pending.len())
        .max(1);
    // The speed limit is global: split it between the files in flight.
    let mut optimizer = opts.optimizer.clone();
    if let Some(limit) = optimizer.speed_limit {
        optimizer.speed_limit = Some((limit / threads as u64).max(1));
    }

    let bar = if opts.quiet {
        indicatif::ProgressBar::hidden()
    } else {
        crate::progress::create_progress_bar(false, String::new(), Some(total), true)
    };
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(skipped);
    let failed = AtomicBool::new(false);
    let finished = AtomicBool::new(false);
    let first_error: Mutex<Option<KgetError>> = Mutex::new(None);
    let file_count = items.len();

    let aggregate =
        |done: &[AtomicU64]| -> u64 { done.iter().map(|d| d.load(Ordering::Relaxed)).sum() };

    std::thread::scope(|scope| {
        // Ticker: terminal bar and `progress` events every 200 ms,
        // `file_progress` about once per second.
        scope.spawn(|| {
            let mut meter = SpeedMeter::new();
            let mut last_files = Instant::now();
            loop {
                let stop = finished.load(Ordering::Relaxed);
                let bytes = aggregate(&done);
                bar.set_position(bytes);
                bar.set_message(format!(
                    "[{}/{}] files",
                    completed.load(Ordering::Relaxed),
                    file_count
                ));
                if opts.events.is_some() {
                    opts.emit(&meter.progress(bytes, (total > 0).then_some(total)));
                    if stop || last_files.elapsed() >= Duration::from_secs(1) {
                        opts.emit(&file_progress(&done, &sizes));
                        last_files = Instant::now();
                    }
                }
                if stop {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    loop {
                        if failed.load(Ordering::SeqCst) || opts.cancel.load(Ordering::SeqCst) {
                            break;
                        }
                        let n = next.fetch_add(1, Ordering::SeqCst);
                        let Some(&idx) = pending.get(n) else { break };
                        let item = &items[idx];
                        match fetch_one(item, idx, &done, &optimizer, opts) {
                            Ok(()) => {
                                completed.fetch_add(1, Ordering::Relaxed);
                                if !opts.quiet {
                                    bar.println(format!("✓ {}", item.name));
                                }
                            }
                            Err(e) => {
                                failed.store(true, Ordering::SeqCst);
                                let mut slot = crate::guard::lock(&first_error);
                                if slot.is_none() {
                                    *slot = Some(e);
                                }
                                break;
                            }
                        }
                    }
                })
            })
            .collect();
        for w in workers {
            // A panicking worker fails the fetch; ignoring it used to report
            // success with files missing.
            if let Err(p) = w.join() {
                failed.store(true, Ordering::SeqCst);
                crate::guard::lock(&first_error).get_or_insert_with(|| {
                    KgetError::Other(format!(
                        "Download worker panicked: {}",
                        crate::guard::panic_message(&*p)
                    ))
                });
            }
        }
        finished.store(true, Ordering::Relaxed);
    });

    if let Some(e) = first_error
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        bar.abandon();
        return Err(e);
    }
    if opts.cancel.load(Ordering::SeqCst) && completed.load(Ordering::Relaxed) < file_count {
        bar.abandon();
        return Err(KgetError::Cancelled);
    }
    bar.finish_and_clear();
    Ok(FetchStats {
        files: file_count,
        skipped,
        bytes: aggregate(&done).max(total),
    })
}

fn file_progress(done: &[AtomicU64], sizes: &[u64]) -> JsonEvent {
    JsonEvent::FileProgress {
        files: done
            .iter()
            .zip(sizes)
            .enumerate()
            .map(|(idx, (d, &size))| {
                let downloaded = d.load(Ordering::Relaxed);
                let pct = if size > 0 {
                    ((downloaded as f64 / size as f64 * 1000.0).round() / 10.0).min(100.0)
                } else {
                    100.0
                };
                FileProgressEntry {
                    idx,
                    downloaded,
                    size,
                    pct,
                }
            })
            .collect(),
    }
}

/// Download one file with the turbo engine.
fn fetch_one(
    item: &FetchItem,
    idx: usize,
    done: &Arc<Vec<AtomicU64>>,
    optimizer: &Optimizer,
    opts: &TransferOptions,
) -> Result<(), KgetError> {
    if let Some(parent) = item.dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut dl = AdvancedDownloader::new(
        item.url.clone(),
        item.dest.to_string_lossy().into_owned(),
        true,
        opts.proxy.clone(),
        optimizer.clone(),
    )
    .map_err(KgetError::from)?;
    dl.set_extra_headers(opts.headers.clone());
    dl.set_cancel_token(opts.cancel.clone());
    dl.set_retries(opts.retries);
    dl.set_timeout_secs(opts.timeout).map_err(KgetError::from)?;
    // Never prompt: a multi-file transfer always resumes compatible state.
    dl.set_resume_policy(ResumePolicy::AlwaysResume);
    if let Some(h) = &item.sha256 {
        dl.set_expected_sha256(h.clone());
    }
    let done = done.clone();
    dl.set_byte_progress_callback(move |bytes, _| done[idx].store(bytes, Ordering::Relaxed));
    dl.download().map_err(KgetError::from)?;
    if let Some(size) = item.size {
        let len = std::fs::metadata(&item.dest)?.len();
        if len != size {
            return Err(KgetError::Other(format!(
                "{}: expected {size} bytes, got {len}",
                item.name
            )));
        }
    }
    Ok(())
}

/// Total size of the regular files under `dir` (symlinks are not followed).
pub fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Glob matching in the style of Python's `fnmatch` (used by
/// `huggingface_hub`'s `allow_patterns`): `*` matches any run of characters
/// including `/`, `?` one character, `[abc]` / `[a-z]` / `[!abc]` a class.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // Iterative matcher with backtracking to the last `*`.
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ti));
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                '[' => match match_class(&p[pi..], t[ti]) {
                    Some((true, len)) => {
                        pi += len;
                        ti += 1;
                        continue;
                    }
                    // Unterminated class: a literal '['.
                    None if t[ti] == '[' => {
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                    _ => {}
                },
                c if c == t[ti] => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, st)) => {
                pi = sp + 1;
                ti = st + 1;
                star = Some((sp, st + 1));
            }
            None => return false,
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Match `c` against the class starting at `p[0] == '['`; returns whether it
/// matched and the class length, or `None` when the class is unterminated.
fn match_class(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

/// Whether `path` passes the `--include` / `--exclude` filters: it must match
/// one include pattern (when any is given) and no exclude pattern.
pub fn filter_path(path: &str, include: &[String], exclude: &[String]) -> bool {
    (include.is_empty() || include.iter().any(|p| glob_match(p, path)))
        && !exclude.iter().any(|p| glob_match(p, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_follow_fnmatch() {
        assert!(glob_match("*.gguf", "model-q4.gguf"));
        assert!(glob_match("*.gguf", "sub/dir/model.gguf"));
        assert!(!glob_match("*.gguf", "model.gguf.json"));
        assert!(glob_match("*fp32*", "onnx/model_fp32.onnx"));
        assert!(glob_match(
            "model-?????-of-00002.safetensors",
            "model-00001-of-00002.safetensors"
        ));
        assert!(glob_match("onnx/*", "onnx/a/b.onnx"));
        assert!(glob_match("[ab]*.json", "a.json"));
        assert!(!glob_match("[!ab]*.json", "a.json"));
        assert!(glob_match("q[0-9]_k.gguf", "q4_k.gguf"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("a", ""));
        assert!(glob_match("[x", "[x"));
    }

    #[test]
    fn include_and_exclude() {
        let inc = vec!["*.gguf".to_string()];
        let exc = vec!["*fp32*".to_string()];
        assert!(filter_path("m-q4.gguf", &inc, &exc));
        assert!(!filter_path("m-fp32.gguf", &inc, &exc));
        assert!(!filter_path("README.md", &inc, &exc));
        assert!(filter_path("README.md", &[], &exc));
    }
}
