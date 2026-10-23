//! Shared download dispatcher used by every frontend.
//!
//! The CLI (`kget <url>`, `--batch`), the interactive REPL, the egui GUI
//! worker and the [`JobManager`](crate::jobs::JobManager) all run downloads
//! through [`run_job`], so protocol dispatch, resume, checksum verification,
//! history recording, auto-extraction and JSONL events behave the same
//! everywhere.
//!
//! A download is described by a [`DownloadJob`]; process-wide state (config,
//! cancel token) lives in a [`RunContext`] and output is controlled by a
//! [`Reporter`]: human-readable text unless `quiet`, structured [`JsonEvent`]s
//! when an event sink is set.
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::runner::{DownloadJob, Reporter, RunContext, run_job};
//!
//! let mut job = DownloadJob::new("https://example.com/file.iso");
//! job.output = Some("./downloads/".into());
//! job.advanced = true;
//!
//! let ctx = RunContext::new(kget::Config::default());
//! let outcome = run_job(&job, &ctx, &Reporter::quiet())?;
//! # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
//! ```

use crate::advanced_download::{AdvancedDownloader, ResumePolicy};
use crate::checksum::{ChecksumAlgorithm, compute_checksum, parse_sidecar};
use crate::config::Config;
use crate::credentials::{CredentialResolver, SavePolicy};
use crate::dedupe::DuplicatePolicy;
use crate::events::{FileEntry, FileProgressEntry, JsonEvent, Phase, SpeedMeter};
use crate::ftp::FtpDownloader;
use crate::metalink;
use crate::optimization::Optimizer;
use crate::organize::{Preempted, apply_rules, check_duplicate, dedupe_after};
use crate::protocol::{self, Protocol};
use crate::queue::{DownloadHistory, EntryStatus, HistoryEntry};
use crate::sftp::SftpDownloader;
use crate::torrent::{TorrentCallbacks, TorrentJobOptions, TorrentOptions, TorrentOutcome};
use crate::utils;
use crate::webdav::WebDavDownloader;
use crate::ytdlp::{VideoEvent, VideoQuality, download_video_with_cancel, ytdlp_binary};
use crate::{ByteProgressCallback, DownloadOptions, KgetError, OverwritePolicy};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type BoxError = Box<dyn Error + Send + Sync>;

// ============================================================================
// Job description
// ============================================================================

/// Everything needed to run one download.
///
/// Build it with [`DownloadJob::new`] (or `..Default::default()`) so fields
/// added in future versions do not break your code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadJob {
    /// URL, magnet link or path of a `.torrent` / `.meta4` file
    pub url: String,
    /// Output file or directory (`None` = file name from the URL in the
    /// current directory; torrents use the configured torrent directory)
    pub output: Option<String>,
    /// Protocol forced by a flag; `None` = detect from the URL
    pub protocol: Option<Protocol>,
    /// Parallel, resumable HTTP download (turbo mode)
    pub advanced: bool,
    /// Compute and report the SHA-256 of ISO files (single-stream HTTP)
    pub verify_iso: bool,
    /// Expected SHA-256 hash
    pub sha256: Option<String>,
    /// Expected checksum (`--checksum <algorithm>:<hex>`)
    pub checksum: Option<(ChecksumAlgorithm, String)>,
    /// URL of a checksum file (`SHA256SUMS`, `file.iso.sha256`…)
    pub checksum_url: Option<String>,
    /// Extra HTTP headers
    pub headers: Vec<(String, String)>,
    /// yt-dlp quality preset
    pub quality: VideoQuality,
    /// Continue an existing `<file>.part`
    pub resume: bool,
    /// Behaviour when the destination already exists
    pub overwrite: OverwritePolicy,
    /// Maximum attempts per request
    pub retries: u32,
    /// Connect / per-read timeout in seconds (0 = no read timeout)
    pub timeout: u64,
    /// Extract the archive after a successful download
    pub extract: bool,
    /// Save the passwords this download used (from the URL or a prompt) in
    /// the system keychain (`--save-credentials`)
    pub save_credentials: bool,
    /// Only download files matching one of these globs (`hf://`; `--include`)
    pub include: Vec<String>,
    /// Skip files matching any of these globs (`hf://`; `--exclude`)
    pub exclude: Vec<String>,
    /// Branch, tag or commit (`hf://`; `--revision`)
    pub revision: Option<String>,
    /// Write into the Hugging Face hub cache layout (`--hf-cache`)
    pub hf_cache: bool,
    /// Files downloaded at the same time by multi-file sources
    /// (`hf://`, `ollama://`; `-j`)
    pub parallel_files: usize,
    /// Tags recorded in the history (`--tag`), merged with the tags of the
    /// matching smart rule
    pub tags: Vec<String>,
    /// Apply the smart rules ([`crate::rules`]): `Some(true)` = `--organize`,
    /// `Some(false)` = `--no-organize`, `None` = `config.rules_enabled`
    pub organize: Option<bool>,
    /// Directory used when `output` is `None` (e.g. the interactive `cd`
    /// directory); rule folders are created inside it
    pub base_dir: Option<String>,
    /// What to do when the URL was already downloaded and the file is still
    /// there (`--duplicates`); `None` = ask on a terminal (CLI/REPL with
    /// [`RunContext::credential_prompts`]), download again otherwise
    pub duplicates: Option<DuplicatePolicy>,
    /// Replace the downloaded file by a hard link when the history already
    /// has a file with the same SHA-256 (`--dedupe`)
    pub dedupe: bool,
    /// Torrent file selection and seeding limits
    #[serde(skip_serializing_if = "TorrentJobOptions::is_default")]
    pub torrent: TorrentJobOptions,
    /// HTTP version of a turbo download (`--http-version`); `None` = the
    /// configuration's `optimization.http_version`, else auto
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_version: Option<crate::http::HttpVersion>,
}

impl Default for DownloadJob {
    fn default() -> Self {
        Self::new(String::new())
    }
}

impl DownloadJob {
    /// A job for `url` with default options.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            output: None,
            protocol: None,
            advanced: false,
            verify_iso: false,
            sha256: None,
            checksum: None,
            checksum_url: None,
            headers: Vec::new(),
            quality: VideoQuality::Best,
            resume: false,
            overwrite: OverwritePolicy::Overwrite,
            retries: crate::http::DEFAULT_RETRIES,
            timeout: crate::http::DEFAULT_TIMEOUT_SECS,
            extract: false,
            save_credentials: false,
            include: Vec::new(),
            exclude: Vec::new(),
            revision: None,
            hf_cache: false,
            parallel_files: 4,
            tags: Vec::new(),
            organize: None,
            base_dir: None,
            duplicates: None,
            dedupe: false,
            torrent: TorrentJobOptions::default(),
            http_version: None,
        }
    }

    /// The protocol this job uses: the forced one, or detected from the URL.
    pub fn protocol(&self) -> Protocol {
        self.protocol.unwrap_or_else(|| protocol::detect(&self.url))
    }
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Data saved by KGet (path when known: single file downloads)
    Saved(Option<PathBuf>),
    /// Handed to an external application (e.g. the system torrent client)
    HandedOff,
}

// ============================================================================
// Context and reporting
// ============================================================================

/// Process-wide state for a download run.
#[derive(Clone)]
pub struct RunContext {
    /// Configuration (proxy, limits, torrent settings…) for this run
    pub config: Config,
    /// Set to `true` to stop the download; partial data is kept
    pub cancel: Arc<AtomicBool>,
    /// When set, receives [`is_cancel_aware`] for the protocol of each job as
    /// it starts (used by the CLI's Ctrl+C handler)
    pub cancel_aware: Option<Arc<AtomicBool>>,
    /// Append every job to the persistent download history (default `true`)
    pub record_history: bool,
    /// Ask on the terminal for passwords that are neither in the URL nor in
    /// the system keychain, and offer to save them; also ask whether to
    /// download an already-downloaded URL again (default `false`).
    ///
    /// Only the CLI and the interactive REPL set this; it has no effect when
    /// the reporter has an event sink (`--jsonl`, GUI, job manager) or when
    /// stdin/stderr are not terminals, so background jobs never block on a
    /// prompt.
    pub credential_prompts: bool,
}

impl RunContext {
    /// A context with a fresh cancel token that records history.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_aware: None,
            record_history: true,
            credential_prompts: false,
        }
    }

    /// Use `cancel` as the cancel token.
    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Receives structured events (`started`, `progress`, `verified`, …).
pub type EventSink = Arc<dyn Fn(&JsonEvent) + Send + Sync>;

/// Where the output of a job goes.
#[derive(Clone, Default)]
pub struct Reporter {
    /// Suppress human-readable output (progress bars, messages)
    pub quiet: bool,
    /// Structured events; `Some` means a machine consumer is listening (JSONL,
    /// GUI, job manager), which also implies non-interactive behaviour.
    ///
    /// Checksum verification and archive extraction are announced with a
    /// [`JsonEvent::Phase`] event, whether the hash is computed by the runner
    /// (`--checksum`) or inside the engine (`--sha256`, ISO verification).
    pub events: Option<EventSink>,
}

impl std::fmt::Debug for Reporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reporter")
            .field("quiet", &self.quiet)
            .field("events", &self.events.is_some())
            .finish()
    }
}

impl Reporter {
    /// Human-readable output on the terminal (`quiet = false`, no events).
    pub fn terminal() -> Self {
        Self::default()
    }

    /// No output at all.
    pub fn quiet() -> Self {
        Self {
            quiet: true,
            ..Self::default()
        }
    }

    /// Quiet, with every event sent to `sink`.
    pub fn events(sink: EventSink) -> Self {
        Self {
            quiet: true,
            events: Some(sink),
        }
    }

    /// `kget --jsonl` output: quiet, events printed to stdout as JSON Lines.
    ///
    /// With `tag_url`, every line gets an extra `"url"` field (batch mode).
    pub fn jsonl_stdout(tag_url: Option<String>) -> Self {
        Self::events(jsonl_stdout_sink(tag_url))
    }

    pub(crate) fn emit(&self, ev: &JsonEvent) {
        if let Some(sink) = &self.events {
            sink(ev);
        }
    }

    fn jsonl(&self) -> bool {
        self.events.is_some()
    }
}

/// Event sink printing each event to stdout as one JSON line (`kget --jsonl`).
///
/// With `tag_url`, every line gets an extra `"url"` field (batch mode).
pub fn jsonl_stdout_sink(tag_url: Option<String>) -> EventSink {
    Arc::new(move |ev: &JsonEvent| match &tag_url {
        Some(url) => {
            let mut v: serde_json::Value = serde_json::from_str(&ev.to_line()).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("url".into(), url.clone().into());
            }
            println!("{v}");
        }
        None => ev.emit(),
    })
}

/// Whether downloads over `proto` stop cleanly when cancelled (others can only
/// be interrupted by exiting the process).
pub fn is_cancel_aware(proto: Protocol) -> bool {
    matches!(
        proto,
        Protocol::Http
            | Protocol::WebDav
            | Protocol::Torrent
            | Protocol::Video
            | Protocol::HuggingFace
            | Protocol::Ollama
    )
}

// ============================================================================
// Running a job
// ============================================================================

/// Run one job: download, verify, record it in the history, extract, and
/// emit events through `reporter`.
pub fn run_job(
    job: &DownloadJob,
    ctx: &RunContext,
    reporter: &Reporter,
) -> Result<Outcome, BoxError> {
    let proto = job.protocol();
    if let Some(flag) = &ctx.cancel_aware {
        flag.store(is_cancel_aware(proto), Ordering::SeqCst);
    }
    // Smart rules may change the destination, tags and per-download settings.
    let mut job = job.clone();
    let mut ruled_ctx = ctx.clone();
    let (tags, rule_check) = match apply_rules(&mut job, &mut ruled_ctx, reporter, proto) {
        Ok(tags) => (tags, Ok(())),
        Err(e) => (job.tags.clone(), Err(e)),
    };
    let job = &job;
    let ctx = &ruled_ctx;
    reporter.emit(&JsonEvent::Started {
        url: crate::queue::redact_url(&job.url),
        protocol: proto.as_str().to_string(),
        advanced: job.advanced,
        output: job.output.clone().unwrap_or_default(),
    });

    // A proxy password saved with `--save-config` lives in the keychain.
    let with_proxy_password;
    let ctx = {
        let mut proxy = ctx.config.proxy.clone();
        if crate::credentials::fill_proxy_password(&mut proxy) {
            let mut filled = ctx.clone();
            filled.config.proxy = proxy;
            with_proxy_password = filled;
            &with_proxy_password
        } else {
            ctx
        }
    };
    let creds = Arc::new(credential_resolver(job, ctx, reporter));
    let started = std::time::Instant::now();
    let mut content_sha: Option<String> = None;
    let mut skipped = false;
    let result = match rule_check.and_then(|()| check_duplicate(job, ctx, reporter, proto)) {
        Err(e) => Err(e),
        Ok(Some(Preempted::Skipped(path))) => {
            skipped = true;
            Ok(Outcome::Saved(Some(path)))
        }
        Ok(Some(Preempted::Linked(path, sha))) => {
            content_sha = sha;
            Ok(Outcome::Saved(Some(path)))
        }
        Ok(None) => dispatch(job, ctx, reporter, proto, &creds).and_then(|outcome| {
            // Checksums and dedupe apply to single files, not hf:// directories.
            if let Outcome::Saved(Some(path)) = &outcome
                && path.is_file()
            {
                let verified = post_verify(job, ctx, reporter, path)?;
                content_sha = dedupe_after(job, reporter, path, verified);
            }
            Ok(outcome)
        }),
    };
    // Offer to save / save accepted passwords once the progress output is done.
    creds.finish();

    let cancelled = ctx.cancelled();
    if ctx.record_history && !skipped && !matches!(proto, Protocol::Metalink) {
        let elapsed = started.elapsed();
        record_history(job, proto, &result, cancelled, elapsed, &tags, content_sha);
    }

    match &result {
        Ok(Outcome::Saved(path)) => {
            if let Some(p) = path {
                if job.extract && !skipped && utils::is_extractable(p) {
                    reporter.emit(&JsonEvent::Phase {
                        phase: Phase::Extracting,
                        algorithm: None,
                    });
                    if let Err(e) = crate::auto_extract(p, reporter.quiet) {
                        eprintln!("Warning: auto-extract: {e}");
                    }
                }
                let bytes = if p.is_dir() {
                    crate::fetch::dir_size(p)
                } else {
                    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
                };
                reporter.emit(&JsonEvent::Saved {
                    path: p.to_string_lossy().into_owned(),
                    bytes,
                });
            }
            reporter.emit(&JsonEvent::Completed);
        }
        Ok(Outcome::HandedOff) => {
            reporter.emit(&JsonEvent::Status {
                message: "Opened in the system torrent client; KGet did not download the data"
                    .into(),
            });
            reporter.emit(&JsonEvent::Completed);
        }
        Err(e) => {
            let (code, retryable) = error_code(e.as_ref(), cancelled);
            if let Some(KgetError::ChecksumMismatch { algorithm, got, .. }) =
                e.downcast_ref::<KgetError>()
            {
                reporter.emit(&JsonEvent::Verified {
                    algorithm: algorithm.clone(),
                    hash: got.clone(),
                    ok: false,
                });
            }
            reporter.emit(&JsonEvent::Error {
                code,
                message: e.to_string(),
                retryable,
            });
        }
    }
    result
}

/// Credentials for `user@host` URLs (FTP, SFTP, WebDAV): the URL, then the
/// system keychain, then — only with [`RunContext::credential_prompts`] and no
/// event sink — a terminal prompt.
fn credential_resolver(
    job: &DownloadJob,
    ctx: &RunContext,
    reporter: &Reporter,
) -> CredentialResolver {
    let machine = reporter.jsonl();
    let mut creds = CredentialResolver::system()
        .quiet(machine)
        .interactive(ctx.credential_prompts && !machine);
    if job.save_credentials {
        creds = creds.save_policy(SavePolicy::Always);
    }
    creds
}

/// Machine-readable error code and retryability for a failed job (the
/// `code` / `retryable` fields of the JSONL `error` event).
pub fn error_code(e: &(dyn Error + Send + Sync + 'static), cancelled: bool) -> (String, bool) {
    match e.downcast_ref::<KgetError>() {
        Some(k) => (k.code(), k.is_retryable(&[408, 429, 500, 502, 503, 504])),
        None if cancelled => ("cancelled".into(), false),
        None => ("error".into(), true),
    }
}

/// `files` event entries for a torrent's file list; with a selection, each
/// entry says whether it will be downloaded.
pub fn torrent_file_entries(
    files: Vec<(String, u64)>,
    select: &crate::torrent::FileSelection,
) -> Vec<FileEntry> {
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    let selected = if select.is_empty() {
        None
    } else {
        Some(select.resolve(&names).ok().flatten().unwrap_or_default())
    };
    files
        .into_iter()
        .enumerate()
        .map(|(i, (name, size))| FileEntry {
            name,
            size,
            selected: selected.as_ref().map(|s| s.contains(&i)),
        })
        .collect()
}

/// `progress` event derived from a torrent's swarm statistics.
fn torrent_progress(stats: &crate::events::TorrentStats) -> JsonEvent {
    let total = (stats.size > 0).then_some(stats.size);
    let percent = total.map(|t| (stats.downloaded as f64 / t as f64 * 100.0).min(100.0));
    let eta_secs = match total {
        Some(t) if t <= stats.downloaded => Some(0),
        Some(t) if stats.download_bps > 0 => Some((t - stats.downloaded) / stats.download_bps),
        _ => None,
    };
    JsonEvent::Progress {
        bytes: stats.downloaded,
        total,
        percent,
        speed_bps: stats.download_bps,
        eta_secs,
        progress: percent.map(|p| p / 100.0),
    }
}

/// History entry for a finished job (best-effort; never fails the download).
fn record_history(
    job: &DownloadJob,
    proto: Protocol,
    result: &Result<Outcome, BoxError>,
    cancelled: bool,
    elapsed: std::time::Duration,
    tags: &[String],
    sha256: Option<String>,
) {
    let output_dir = job
        .output
        .as_deref()
        .map(utils::expand_tilde)
        .unwrap_or_else(|| ".".into());
    let mut entry = HistoryEntry::new(&job.url, &output_dir, job.sha256.as_deref());
    // `-o file.bin`: record that name, so a failed or cancelled entry can be
    // retried/resumed from the right `.part` file.
    if matches!(
        proto,
        Protocol::Http | Protocol::Ftp | Protocol::Sftp | Protocol::WebDav
    ) && let Some(o) = job.output.as_deref()
        && !o.ends_with('/')
        && !o.ends_with(std::path::MAIN_SEPARATOR)
        && !Path::new(&output_dir).is_dir()
    {
        let file = Path::new(&output_dir);
        entry.set_saved_file(file);
        if file.parent().is_none_or(|d| d.as_os_str().is_empty()) {
            entry.output_dir = ".".into();
        }
    }
    let (status, error_msg) = match result {
        Ok(Outcome::Saved(path)) => {
            if let Some(p) = path {
                entry.set_saved_file(p);
                // Multi-file downloads (hf://, ollama://) save a directory.
                if p.is_dir() {
                    entry.bytes_total = Some(crate::fetch::dir_size(p));
                }
            }
            (EntryStatus::Completed, None)
        }
        Ok(Outcome::HandedOff) => (EntryStatus::HandedOff, None),
        Err(_) if cancelled => (EntryStatus::Cancelled, None),
        Err(e) => (EntryStatus::Failed, Some(e.to_string())),
    };
    // Duration (and so the average speed) includes checksum verification.
    entry.set_elapsed(elapsed);
    entry.protocol = Some(proto.as_str().to_string());
    entry.checksum_ok = match result {
        Err(e) if matches!(e.downcast_ref(), Some(KgetError::ChecksumMismatch { .. })) => {
            Some(false)
        }
        Ok(Outcome::Saved(_))
            if job.sha256.is_some() || job.checksum.is_some() || job.checksum_url.is_some() =>
        {
            Some(true)
        }
        _ => None,
    };
    entry.tags = tags.to_vec();
    if status == EntryStatus::Completed {
        entry.sha256 = sha256;
    }
    let _ = DownloadHistory::append(entry, status, error_msg);
}

/// Optional checksum verification for `--checksum` / `--checksum-url`.
fn post_verify(
    job: &DownloadJob,
    ctx: &RunContext,
    reporter: &Reporter,
    path: &Path,
) -> Result<Option<String>, BoxError> {
    let mut expected = job.checksum.clone();
    if expected.is_none()
        && let Some(sidecar) = &job.checksum_url
    {
        let client = crate::http::build_client(&ctx.config.proxy, &Default::default())?;
        let resp = client.get(sidecar).send()?;
        if !resp.status().is_success() {
            return Err(Box::new(KgetError::SidecarError(format!(
                "HTTP {} fetching {sidecar}",
                resp.status()
            ))));
        }
        let text = resp.text()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        expected = parse_sidecar(&text, &name).or_else(|| {
            parse_sidecar(
                &text,
                &utils::get_filename_from_url_or_default(&job.url, ""),
            )
        });
        if expected.is_none() {
            return Err(Box::new(KgetError::SidecarError(format!(
                "No entry for '{name}' found in {sidecar}"
            ))));
        }
    }
    let Some((algo, want)) = expected else {
        if let Some(h) = &job.sha256 {
            reporter.emit(&JsonEvent::Verified {
                algorithm: "sha256".into(),
                hash: h.trim().to_ascii_lowercase(),
                ok: true,
            });
        }
        return Ok(job.sha256.as_ref().map(|h| h.trim().to_ascii_lowercase()));
    };
    if !reporter.quiet {
        println!("Verifying {}...", algo.name());
    }
    reporter.emit(&JsonEvent::Phase {
        phase: Phase::Verifying,
        algorithm: Some(algo.name().into()),
    });
    let got = compute_checksum(path, &algo)?;
    let want = want.trim().to_ascii_lowercase();
    let ok = got == want;
    reporter.emit(&JsonEvent::Verified {
        algorithm: algo.name().into(),
        hash: got.clone(),
        ok,
    });
    if !ok {
        let _ = std::fs::remove_file(path);
        return Err(Box::new(KgetError::ChecksumMismatch {
            algorithm: algo.name().into(),
            expected: want,
            got,
        }));
    }
    if !reporter.quiet {
        println!("{} OK: {}", algo.name().to_uppercase(), got);
    }
    Ok((algo == ChecksumAlgorithm::Sha256).then_some(got))
}

/// Progress callback that emits `progress` events.
fn event_progress(reporter: &Reporter) -> Option<ByteProgressCallback> {
    let sink = reporter.events.clone()?;
    let meter = Mutex::new(SpeedMeter::new());
    Some(Arc::new(move |bytes, total| {
        let ev = crate::guard::lock(&meter).progress(bytes, total);
        sink(&ev);
    }))
}

/// Status callback that emits `status` events, preceded by a `phase` event
/// when the engine announces a checksum computation.
fn event_status(reporter: &Reporter) -> impl Fn(String) + Send + Sync + 'static {
    let sink = reporter.events.clone();
    move |message: String| {
        // Progress is reported through structured progress events.
        if let Some(sink) = &sink
            && !message.starts_with("PROGRESS:")
        {
            if let Some(phase) = JsonEvent::phase_from_status(&message) {
                sink(&phase);
            }
            sink(&JsonEvent::Status { message });
        }
    }
}

fn dispatch(
    job: &DownloadJob,
    ctx: &RunContext,
    reporter: &Reporter,
    proto: Protocol,
    creds: &Arc<CredentialResolver>,
) -> Result<Outcome, BoxError> {
    let config = &ctx.config;
    let optimizer = Optimizer::from_config(config.optimization.clone());
    let jsonl = reporter.jsonl();
    let quiet = reporter.quiet || jsonl;
    let status = event_status(reporter);

    match proto {
        Protocol::HuggingFace => {
            let opts = crate::hf::HfOptions {
                revision: job.revision.clone(),
                include: job.include.clone(),
                exclude: job.exclude.clone(),
                output: job.output.clone(),
                hf_cache: job.hf_cache,
                expected_sha256: job.sha256.clone(),
                ..Default::default()
            };
            let done = crate::hf::download(&job.url, &opts, &transfer_options(job, ctx, reporter))?;
            report_model(&done, quiet);
            Ok(Outcome::Saved(Some(done.path)))
        }
        Protocol::Ollama => {
            let opts = crate::ollama::OllamaOptions {
                output: job.output.clone(),
                ..Default::default()
            };
            let done =
                crate::ollama::download(&job.url, &opts, &transfer_options(job, ctx, reporter))?;
            report_model(&done, quiet);
            Ok(Outcome::Saved(Some(done.path)))
        }
        Protocol::Video => {
            if ytdlp_binary().is_none() {
                let msg = crate::ytdlp::ytdlp_not_found_message();
                if !jsonl {
                    eprintln!("{msg}");
                }
                return Err(Box::new(KgetError::Protocol(msg)));
            }
            let output_dir = job.output.as_deref().unwrap_or(".");
            if !quiet {
                println!(
                    "Video detected — routing to {} (quality: {:?})",
                    ytdlp_binary().unwrap_or_default(),
                    job.quality
                );
            }
            let meter = Mutex::new(SpeedMeter::new());
            let cancel = Some(ctx.cancel.clone());
            let saved = download_video_with_cancel(
                &job.url,
                output_dir,
                &job.quality,
                cancel,
                |ev| match ev {
                    VideoEvent::Progress(p) if jsonl => {
                        let mut e = crate::guard::lock(&meter).progress(0, None);
                        if let JsonEvent::Progress {
                            percent, progress, ..
                        } = &mut e
                        {
                            *percent = Some(p.percent);
                            *progress = Some(p.percent / 100.0);
                        }
                        reporter.emit(&e);
                    }
                    VideoEvent::Progress(p) if !quiet => {
                        println!(
                            "PROGRESS: {:.1}% SPEED: {} ETA: {}",
                            p.percent, p.speed, p.eta
                        )
                    }
                    VideoEvent::Saved(path) if !jsonl => println!("SAVED: {path}"),
                    VideoEvent::Log(l) if !quiet => println!("{l}"),
                    _ => {}
                },
            )?;
            Ok(Outcome::Saved(Some(PathBuf::from(saved))))
        }
        Protocol::WebDav => {
            let output = utils::resolve_output_path(job.output.clone(), &job.url, "webdav_output");
            let options = DownloadOptions {
                expected_sha256: job.sha256.clone(),
                extra_headers: job.headers.clone(),
                resume: job.resume,
                overwrite: job.overwrite,
                retries: job.retries,
                timeout_secs: job.timeout,
                cancel: Some(ctx.cancel.clone()),
                progress_callback: event_progress(reporter),
                ..Default::default()
            };
            let path = WebDavDownloader::new(
                job.url.clone(),
                output,
                quiet,
                config.proxy.clone(),
                optimizer,
            )
            .with_options(options)
            .with_credentials(creds.clone())
            .download_to_path(Some(&status))?;
            Ok(Outcome::Saved(Some(path)))
        }
        Protocol::Metalink => {
            let output_dir = job.output.as_deref().unwrap_or(".");
            metalink::download_metalink_with_status(
                &job.url,
                output_dir,
                quiet,
                config.proxy.clone(),
                optimizer,
                jsonl.then_some(&status as &(dyn Fn(String) + Send + Sync)),
            )?;
            Ok(Outcome::Saved(None))
        }
        Protocol::Ftp => {
            let output = utils::resolve_output_path(job.output.clone(), &job.url, "ftp_output");
            let mut dl = FtpDownloader::new(
                job.url.clone(),
                output.clone(),
                quiet,
                config.proxy.clone(),
                optimizer,
            )
            .with_config(config.ftp.clone())
            .with_credentials(creds.clone())
            .with_resume(job.resume || job.overwrite != OverwritePolicy::Overwrite);
            if let Some(cb) = event_progress(reporter) {
                dl = dl.with_progress_callback(cb);
            }
            if job.overwrite == OverwritePolicy::Skip && Path::new(&output).exists() {
                return Ok(Outcome::Saved(Some(absolute(&output))));
            }
            dl.download()?;
            Ok(Outcome::Saved(Some(absolute(&output))))
        }
        Protocol::Sftp => {
            let output = utils::resolve_output_path(job.output.clone(), &job.url, "sftp_output");
            if job.overwrite == OverwritePolicy::Skip && Path::new(&output).exists() {
                return Ok(Outcome::Saved(Some(absolute(&output))));
            }
            let mut dl = SftpDownloader::new(
                job.url.clone(),
                output.clone(),
                quiet,
                config.proxy.clone(),
                optimizer,
            )
            .with_key_path(config.sftp.key_path.clone())
            .with_credentials(creds.clone())
            .with_resume(job.resume || job.overwrite != OverwritePolicy::Overwrite);
            if let Some(cb) = event_progress(reporter) {
                dl = dl.with_progress_callback(cb);
            }
            dl.download()?;
            Ok(Outcome::Saved(Some(absolute(&output))))
        }
        Protocol::Torrent => {
            let output_dir = job
                .output
                .clone()
                .or_else(|| config.torrent.download_dir.clone())
                .unwrap_or_else(|| "torrent_output".to_string());
            let output_dir = utils::expand_tilde(&output_dir);
            // Once the backend reports swarm stats, progress events carry
            // real byte counts and speeds; the bare fraction callback stops.
            let have_stats = Arc::new(AtomicBool::new(false));
            let callbacks = match &reporter.events {
                Some(sink) => {
                    let sink = sink.clone();
                    let have_stats = have_stats.clone();
                    TorrentCallbacks {
                        status: Some(Arc::new(event_status(reporter))),
                        progress: Some(Arc::new(move |p| {
                            if have_stats.load(Ordering::Relaxed) {
                                return;
                            }
                            let pct = p as f64 * 100.0;
                            sink(&JsonEvent::Progress {
                                bytes: 0,
                                total: None,
                                percent: Some(pct),
                                speed_bps: 0,
                                eta_secs: None,
                                progress: Some(p as f64),
                            });
                        })),
                    }
                }
                None => TorrentCallbacks::default(),
            };
            let sizes: Arc<Mutex<Vec<u64>>> = Arc::default();
            let on_file_progress: Option<crate::torrent::FileProgressCb> =
                reporter.events.clone().map(|sink| {
                    let sizes = sizes.clone();
                    Arc::new(move |done: Vec<u64>| {
                        let sizes = crate::guard::lock(&sizes);
                        let files = done
                            .iter()
                            .enumerate()
                            .map(|(idx, &downloaded)| {
                                let size = sizes.get(idx).copied().unwrap_or(0);
                                let pct = if size > 0 {
                                    (downloaded as f64 / size as f64 * 1000.0).round() / 10.0
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
                            .collect();
                        sink(&JsonEvent::FileProgress { files });
                    }) as crate::torrent::FileProgressCb
                });
            let on_files: Option<crate::torrent::FilesCb> = reporter.events.clone().map(|sink| {
                let sizes = sizes.clone();
                let select = job.torrent.select.clone();
                Arc::new(move |files: Vec<(String, u64)>| {
                    *crate::guard::lock(&sizes) = files.iter().map(|(_, size)| *size).collect();
                    sink(&JsonEvent::Files {
                        files: torrent_file_entries(files, &select),
                    });
                }) as crate::torrent::FilesCb
            });
            let on_stats: Option<crate::torrent::StatsCb> = reporter.events.clone().map(|sink| {
                let have_stats = have_stats.clone();
                Arc::new(move |stats: crate::events::TorrentStats| {
                    have_stats.store(true, Ordering::Relaxed);
                    if !stats.seeding {
                        sink(&torrent_progress(&stats));
                    }
                    sink(&JsonEvent::TorrentStats(Box::new(stats)));
                }) as crate::torrent::StatsCb
            });
            let outcome = crate::torrent::download_torrent(
                &job.url,
                &output_dir,
                quiet,
                config.proxy.clone(),
                optimizer,
                callbacks,
                TorrentOptions {
                    config: config.torrent.clone(),
                    cancel: Some(ctx.cancel.clone()),
                    on_files,
                    on_file_progress,
                    on_stats,
                    selection: job.torrent.select.clone(),
                    seed: job.torrent.seed_limits(&config.torrent),
                },
            )?;
            Ok(match outcome {
                TorrentOutcome::Downloaded => Outcome::Saved(None),
                TorrentOutcome::HandedOff => Outcome::HandedOff,
            })
        }
        Protocol::Http if job.advanced => {
            let output = utils::resolve_output_path(job.output.clone(), &job.url, "download");
            let output_abs = absolute(&output);
            if output_abs.exists() {
                match job.overwrite {
                    OverwritePolicy::Skip => {
                        if !quiet {
                            println!("File already exists, skipping: {}", output_abs.display());
                        }
                        return Ok(Outcome::Saved(Some(output_abs)));
                    }
                    OverwritePolicy::Rename | OverwritePolicy::Overwrite => {}
                }
            }
            let mut downloader = AdvancedDownloader::new(
                job.url.clone(),
                output_abs.to_string_lossy().into_owned(),
                quiet,
                config.proxy.clone(),
                optimizer,
            )?;
            if let Some(expected_sha256) = job.sha256.clone() {
                downloader.set_expected_sha256(expected_sha256);
            }
            downloader.set_extra_headers(job.headers.clone());
            downloader.set_cancel_token(ctx.cancel.clone());
            downloader.set_retries(job.retries);
            downloader.set_timeout_secs(job.timeout)?;
            if let Some(v) = job.http_version.or(config.optimization.http_version) {
                downloader.set_http_version(v.ensure_available()?)?;
            }
            if !std::io::stdin().is_terminal() || jsonl {
                downloader.set_resume_policy(ResumePolicy::AlwaysResume);
            }
            if let Some(cb) = event_progress(reporter) {
                downloader.set_byte_progress_callback(move |b, t| cb(b, t));
            }
            if jsonl {
                downloader.set_status_callback(event_status(reporter));
            }
            downloader.download()?;
            Ok(Outcome::Saved(Some(output_abs)))
        }
        Protocol::Http => {
            let options = DownloadOptions {
                quiet_mode: quiet,
                output_path: job.output.clone(),
                verify_iso: job.verify_iso || job.sha256.is_some(),
                expected_sha256: job.sha256.clone(),
                extra_headers: job.headers.clone(),
                resume: job.resume,
                overwrite: job.overwrite,
                retries: job.retries,
                timeout_secs: job.timeout,
                cancel: Some(ctx.cancel.clone()),
                progress_callback: event_progress(reporter),
            };
            let path = if jsonl {
                crate::download_to_path(
                    &job.url,
                    config.proxy.clone(),
                    optimizer,
                    options,
                    Some(&status),
                )?
            } else {
                crate::download_to_path(&job.url, config.proxy.clone(), optimizer, options, None)?
            };

            let is_iso = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("iso"));
            if is_iso
                && !quiet
                && job.sha256.is_none()
                && job.checksum.is_none()
                && std::io::stdin().is_terminal()
            {
                println!("\nThis is an ISO file. Would you like to verify its integrity? (y/N)");
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_ok()
                    && input.trim().eq_ignore_ascii_case("y")
                {
                    let cb = |m: String| println!("{m}");
                    crate::verify_file_sha256(&path, None, Some(&cb))?;
                }
            }
            Ok(Outcome::Saved(Some(path)))
        }
    }
}

/// Transfer settings of a multi-file job (`hf://`, `ollama://`).
fn transfer_options(
    job: &DownloadJob,
    ctx: &RunContext,
    reporter: &Reporter,
) -> crate::fetch::TransferOptions {
    crate::fetch::TransferOptions {
        proxy: ctx.config.proxy.clone(),
        optimizer: Optimizer::from_config(ctx.config.optimization.clone()),
        cancel: ctx.cancel.clone(),
        retries: job.retries,
        timeout: job.timeout,
        headers: job.headers.clone(),
        parallel_files: job.parallel_files,
        quiet: reporter.quiet || reporter.jsonl(),
        events: reporter.events.clone(),
    }
}

fn report_model(done: &crate::fetch::ModelDownload, quiet: bool) {
    if !quiet {
        println!(
            "Downloaded {} file(s) ({} already present), {} → {}",
            done.files,
            done.skipped,
            humansize::format_size(done.bytes, humansize::BINARY),
            done.path.display()
        );
    }
}

fn absolute(p: &str) -> PathBuf {
    let p = PathBuf::from(p);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
    }
}

// ============================================================================
// Batch
// ============================================================================

/// URLs listed in a batch file: one per line, blank lines and `#` comments
/// ignored.
pub fn parse_url_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// One job per URL, each with a unique destination inside `output_dir` so two
/// `…/file.zip` URLs never collide. Options are copied from `template`.
pub fn batch_jobs(urls: &[String], output_dir: &Path, template: &DownloadJob) -> Vec<DownloadJob> {
    batch_jobs_with(urls, output_dir, template, &Config::default())
}

/// [`batch_jobs`] with smart rules from `config`: without an explicit output
/// in `template`, each URL goes to its rule's folder inside `output_dir`.
fn batch_jobs_with(
    urls: &[String],
    output_dir: &Path,
    template: &DownloadJob,
    config: &Config,
) -> Vec<DownloadJob> {
    let mut taken = std::collections::HashSet::new();
    urls.iter()
        .map(|url| {
            let mut job = template.clone();
            job.url = url.clone();
            let proto = job.protocol();
            if proto == Protocol::Ollama || (proto == Protocol::HuggingFace && template.hf_cache) {
                // Into the Ollama models directory / hub cache, where tools find it.
                job.output = None;
                return job;
            }
            let dir = crate::organize::batch_dir(url, template, config, output_dir);
            let output_dir = dir.as_path();
            job.output = Some(match proto {
                // A whole repository or folder gets its own sub-directory.
                Protocol::HuggingFace => match crate::hf::HfRef::parse(url) {
                    Ok(r) if r.path.is_none() => {
                        output_dir.join(r.name()).to_string_lossy().into_owned()
                    }
                    _ => output_dir.to_string_lossy().into_owned(),
                },
                Protocol::Http | Protocol::WebDav | Protocol::Ftp | Protocol::Sftp => {
                    let name = utils::get_filename_from_url_or_default(url, "download");
                    let path = if template.overwrite == OverwritePolicy::Overwrite {
                        utils::unique_path(output_dir, &name, &mut taken)
                    } else {
                        // -c / --no-clobber must find the existing file under its own name
                        let p = output_dir.join(&name);
                        if taken.insert(p.clone()) {
                            p
                        } else {
                            utils::unique_path(output_dir, &name, &mut taken)
                        }
                    };
                    path.to_string_lossy().into_owned()
                }
                _ => output_dir.to_string_lossy().into_owned(),
            });
            job
        })
        .collect()
}

/// Download `urls` into `output_dir`, `jobs` at a time, and return
/// `(succeeded, failed)`.
///
/// Shared by `kget --batch` and the interactive `batch` command. Prints a
/// summary and one `[OK]` / `[FAIL]` line per URL, or — with `jsonl` — only
/// JSON Lines events tagged with their `"url"`.
pub fn run_urls(
    urls: Vec<String>,
    output_dir: &Path,
    jobs: usize,
    template: &DownloadJob,
    ctx: &RunContext,
    jsonl: bool,
) -> Result<(usize, usize), BoxError> {
    std::fs::create_dir_all(output_dir)?;
    let jobs = jobs.clamp(1, 64);
    if !jsonl {
        println!("Batch: {} URL(s) queued, {} at a time", urls.len(), jobs);
    }

    let batch = batch_jobs_with(&urls, output_dir, template, &ctx.config);
    for job in &batch {
        if let Some(dir) = job.output.as_deref().map(Path::new).and_then(Path::parent) {
            let _ = std::fs::create_dir_all(dir);
        }
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .thread_name(|i| format!("kget-batch-{i}"))
        .build()?;
    let results: Vec<Result<(), String>> = pool.install(|| {
        use rayon::prelude::*;
        batch
            .par_iter()
            .map(|job| {
                let reporter = if jsonl {
                    Reporter::jsonl_stdout(Some(job.url.clone()))
                } else {
                    Reporter::quiet()
                };
                let r = run_job(job, ctx, &reporter)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                if !jsonl {
                    match &r {
                        Ok(()) => println!("[OK]   {}", job.url),
                        Err(e) => eprintln!("[FAIL] {} — {}", job.url, e),
                    }
                }
                r
            })
            .collect()
    });

    let failed = results.iter().filter(|r| r.is_err()).count();
    let succeeded = results.len() - failed;
    if !jsonl {
        println!(
            "Batch complete: {} succeeded, {} failed.",
            succeeded, failed
        );
    }
    Ok((succeeded, failed))
}

// ============================================================================
// Option parsing and scheduling
// ============================================================================

/// Parse `<algorithm>:<hex>` (or a bare hex digest whose length identifies the
/// algorithm) as given to `--checksum`.
pub fn parse_checksum_spec(spec: &str) -> Result<(ChecksumAlgorithm, String), BoxError> {
    let (algo, hex) = match spec.split_once(':') {
        Some((a, h)) => (
            ChecksumAlgorithm::parse(a)
                .ok_or_else(|| format!("Unknown checksum algorithm '{a}'"))?,
            h,
        ),
        None => (
            ChecksumAlgorithm::from_hex_len(spec)
                .ok_or("Cannot infer checksum algorithm; use <algorithm>:<hex>")?,
            spec,
        ),
    };
    let hex = hex.trim().to_ascii_lowercase();
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Invalid checksum '{spec}'").into());
    }
    Ok((algo, hex))
}

/// Parse `-H "Name: value"` (also `Name:value`); invalid headers are reported
/// on stderr and skipped.
pub fn parse_extra_headers(raw: &[String]) -> Vec<(String, String)> {
    raw.iter()
        .filter_map(|h| match h.split_once(':') {
            Some((k, v)) if !k.trim().is_empty() => {
                Some((k.trim().to_string(), v.trim().to_string()))
            }
            _ => {
                eprintln!("Warning: ignoring invalid header '{h}' (expected \"Name: value\")");
                None
            }
        })
        .collect()
}

/// Sleep until the next occurrence of `time` (format `HH:MM`, local time).
///
/// Uses the system time zone (including on Windows) and re-checks the clock
/// every 30 seconds, so DST changes and system sleep are handled.
pub fn wait_until(time: &str, quiet: bool) -> Result<(), BoxError> {
    use chrono::{Local, NaiveTime, TimeZone};

    let target_time = NaiveTime::parse_from_str(time.trim(), "%H:%M")
        .map_err(|_| format!("Invalid time '{time}'. Expected HH:MM (24-hour)"))?;

    let now = Local::now();
    let mut date = now.date_naive();
    if now.time() >= target_time {
        date = date.succ_opt().ok_or("Invalid date")?;
    }
    let target_at = |d: chrono::NaiveDate| {
        let naive = d.and_time(target_time);
        Local
            .from_local_datetime(&naive)
            .earliest()
            // Skipped by a DST jump: start one hour later.
            .or_else(|| {
                Local
                    .from_local_datetime(&(naive + chrono::Duration::hours(1)))
                    .earliest()
            })
    };
    let first = target_at(date).ok_or("Cannot compute the scheduled time")?;

    if !quiet {
        let secs = (first - now).num_seconds().max(0);
        println!(
            "Scheduled for {} — waiting {:02}h {:02}m {:02}s …",
            first.format("%Y-%m-%d %H:%M"),
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        );
    }

    while let Some(target) = target_at(date) {
        let now = Local::now();
        if now >= target {
            break;
        }
        let remaining = (target - now).to_std().unwrap_or_default();
        std::thread::sleep(remaining.min(std::time::Duration::from_secs(30)));
    }

    if !quiet {
        println!("Starting scheduled download…");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_spec_parsing() {
        let (algo, hex) = parse_checksum_spec("md5:ABCDEF").unwrap();
        assert_eq!(algo, ChecksumAlgorithm::Md5);
        assert_eq!(hex, "abcdef");
        let sha256 = "a".repeat(64);
        assert_eq!(
            parse_checksum_spec(&sha256).unwrap().0,
            ChecksumAlgorithm::Sha256
        );
        assert!(parse_checksum_spec("nope:abc").is_err());
        assert!(parse_checksum_spec("sha256:xyz").is_err());
    }

    #[test]
    fn header_parsing() {
        let h = parse_extra_headers(&["Referer: https://x".into(), "bad".into(), "A:b".into()]);
        assert_eq!(
            h,
            vec![
                ("Referer".to_string(), "https://x".to_string()),
                ("A".to_string(), "b".to_string())
            ]
        );
    }

    #[test]
    fn url_list_and_unique_batch_outputs() {
        let urls = parse_url_list("# c\n\nhttp://a/f.zip\n  http://b/f.zip  \nmagnet:?xt=1\n");
        assert_eq!(urls.len(), 3);
        let dir = Path::new("out");
        let jobs = batch_jobs(&urls, dir, &DownloadJob::default());
        assert_ne!(jobs[0].output, jobs[1].output);
        assert_eq!(jobs[2].output.as_deref(), Some("out"));
    }

    #[test]
    fn machine_consumers_never_get_credential_prompts() {
        let job = DownloadJob::new("sftp://alice@example.com/f.bin");
        let mut ctx = RunContext::new(Config::default());
        // Library default: no prompts at all.
        assert!(!credential_resolver(&job, &ctx, &Reporter::terminal()).can_prompt());
        // Even with prompts enabled, an event sink (JSONL, GUI, job manager)
        // keeps the resolver non-interactive.
        ctx.credential_prompts = true;
        let events = Reporter::events(Arc::new(|_: &JsonEvent| {}));
        assert!(!credential_resolver(&job, &ctx, &events).can_prompt());
    }

    #[test]
    fn job_roundtrips_through_json() {
        let mut job = DownloadJob::new("https://example.com/a.iso");
        job.protocol = Some(Protocol::Http);
        job.checksum = Some((ChecksumAlgorithm::Sha256, "ab".into()));
        job.quality = VideoQuality::P720;
        job.overwrite = OverwritePolicy::Skip;
        let json = serde_json::to_string(&job).unwrap();
        let back: DownloadJob = serde_json::from_str(&json).unwrap();
        assert_eq!(back, job);
        let minimal: DownloadJob = serde_json::from_str(r#"{"url":"https://x/y"}"#).unwrap();
        assert_eq!(minimal, DownloadJob::new("https://x/y"));
    }
}
