#[cfg(not(feature = "gui"))]
use clap::CommandFactory;
use clap::Parser;
use std::error::Error;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(feature = "gui")]
use std::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};

mod interactive;

#[cfg(feature = "gui")]
mod gui;

#[cfg(feature = "gui")]
use crate::gui::KGetGui;
use kget::advanced_download::{AdvancedDownloader, ResumePolicy};
#[cfg(feature = "gui")]
use kget::app::{DownloadCommand, WorkerToGuiMessage, spawn_download_worker};
use kget::checksum::{ChecksumAlgorithm, compute_checksum, parse_sidecar};
use kget::config::{Config, ProxyType};
use kget::events::{FileEntry, FileProgressEntry, JsonEvent, SpeedMeter};
use kget::ftp::FtpDownloader;
use kget::metalink;
use kget::optimization::Optimizer;
use kget::protocol::{self, Protocol};
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};
use kget::sftp::SftpDownloader;
use kget::torrent::{TorrentCallbacks, TorrentOptions, TorrentOutcome};
use kget::utils;
use kget::webdav::WebDavDownloader;
use kget::ytdlp::{VideoEvent, VideoQuality, download_video_with_cancel, ytdlp_binary};
use kget::{ByteProgressCallback, DownloadOptions, KgetError, OverwritePolicy};

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(disable_version_flag = true)]
struct Args {
    /// Show version information
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    /// URL file for the download
    #[arg(default_value_t = String::new())]
    url: String,

    /// Output file name or directory
    #[arg(short = 'O', long = "output")]
    output: Option<String>,

    /// Silent mode (no progress bar)
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// Use advanced download (parallel and resumable)
    #[arg(short = 'a', long = "advanced")]
    advanced: bool,

    /// Use torrent download (magnet links, .torrent files)
    #[arg(short = 't', long = "torrent")]
    torrent: bool,

    /// Proxy URL (ex: http://proxy:8080, socks5://127.0.0.1:1080)
    #[arg(short = 'p', long = "proxy")]
    proxy: Option<String>,

    /// Proxy user
    #[arg(long = "proxy-user")]
    proxy_user: Option<String>,

    /// Proxy password (never written to config.json)
    #[arg(long = "proxy-pass")]
    proxy_pass: Option<String>,

    /// Proxy type (http, https, socks5); inferred from the proxy URL when omitted
    #[arg(long = "proxy-type")]
    proxy_type: Option<String>,

    /// Speed limit in bytes/second (0 = unlimited)
    #[arg(short = 'l', long = "limit")]
    speed_limit: Option<u64>,

    /// Deprecated: the download cache was removed (no effect)
    #[arg(long = "no-cache", hide = true)]
    no_cache: bool,

    /// Maximum number of peers for torrent download
    #[arg(long = "max-peers")]
    max_peers: Option<usize>,

    /// Maximum number of seeds for torrent download
    #[arg(long = "max-seeds")]
    max_seeds: Option<usize>,

    /// Port for torrent connections
    #[arg(long = "torrent-port")]
    torrent_port: Option<u16>,

    /// Disable DHT for torrents
    #[arg(long = "no-dht")]
    no_dht: bool,

    /// Expected SHA256 hash for automatic verification after download
    #[arg(long = "sha256")]
    sha256: Option<String>,

    /// Expected checksum as <algorithm>:<hex> (sha256, sha512, sha1, md5, blake3)
    #[arg(long = "checksum")]
    checksum: Option<String>,

    /// URL of a checksum file (SHA256SUMS, file.iso.sha256…) used to verify the download
    #[arg(long = "checksum-url")]
    checksum_url: Option<String>,

    /// Continue a partially downloaded file (<file>.part)
    #[arg(short = 'c', long = "continue")]
    continue_download: bool,

    /// Do not overwrite an existing file; skip the download instead
    #[arg(long = "no-clobber", conflicts_with = "continue_download")]
    no_clobber: bool,

    /// Maximum attempts per request
    #[arg(long = "retries", default_value_t = kget::http::DEFAULT_RETRIES)]
    retries: u32,

    /// I/O timeout in seconds (connect and each read; 0 = no read timeout)
    #[arg(long = "timeout", default_value_t = kget::http::DEFAULT_TIMEOUT_SECS)]
    timeout: u64,

    /// Persist the given options (proxy, limit, torrent settings) to config.json
    #[arg(long = "save-config")]
    save_config: bool,

    /// Print diagnostics (paths, yt-dlp, tools, features) and exit
    #[arg(long = "doctor")]
    doctor: bool,

    /// Use GUI mode
    #[arg(long = "gui")]
    gui: bool,

    /// Use FTP download (auto-detected for ftp:// URLs)
    #[arg(long = "ftp")]
    ftp: bool,

    /// Use SFTP download (auto-detected for sftp:// URLs)
    #[arg(long = "sftp")]
    sftp: bool,

    /// Use interactive mode
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,

    /// Emit versioned JSON Lines events (one JSON object per line) for machine consumers
    #[arg(long = "jsonl")]
    jsonl: bool,

    /// Download from a Metalink manifest (.meta4 or .metalink file/URL)
    #[arg(long = "metalink")]
    metalink: bool,

    /// Download all URLs listed in a file (one per line, # for comments; "-" = stdin)
    #[arg(long = "batch")]
    batch: Option<String>,

    /// Number of simultaneous downloads in batch mode
    #[arg(short = 'j', long = "jobs", default_value_t = 4)]
    jobs: usize,

    /// Extra HTTP header sent with the request (repeatable: -H "Referer: https://…")
    #[arg(short = 'H', long = "header")]
    header: Vec<String>,

    /// Auto-extract archive after a successful download (.zip, .tar.gz, .7z …)
    #[arg(long = "extract")]
    extract: bool,

    /// Schedule the download to start at a specific local time (HH:MM, 24-hour)
    #[arg(long = "at")]
    at: Option<String>,

    /// Show download history
    #[arg(long = "history")]
    history: bool,

    /// Clear download history (all, or --history-clear completed)
    #[arg(long = "history-clear")]
    history_clear: Option<String>,

    /// Download via WebDAV (webdav:// or webdavs:// URLs); auto-detected from scheme
    #[arg(long = "webdav")]
    webdav: bool,

    /// Route URL through yt-dlp (auto-detected for known video platforms)
    #[arg(long = "ytdlp")]
    ytdlp: bool,

    /// Video quality for yt-dlp downloads: best, 1080p, 720p, 480p, 360p, audio
    #[arg(long = "quality", default_value = "best")]
    quality: String,
}

// ============================================================================
// Shared download dispatcher (used by CLI, batch and interactive mode)
// ============================================================================

/// Process-wide state for a download run.
pub(crate) struct Ctx {
    pub config: Config,
    pub cancel: Arc<AtomicBool>,
}

/// Everything needed to run one download.
#[derive(Clone)]
pub(crate) struct Job {
    pub url: String,
    pub output: Option<String>,
    /// Protocol forced by a flag; `None` = detect from the URL
    pub forced: Option<Protocol>,
    pub advanced: bool,
    pub sha256: Option<String>,
    pub checksum: Option<(ChecksumAlgorithm, String)>,
    pub checksum_url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub quality: VideoQuality,
    pub quiet: bool,
    pub jsonl: bool,
    pub resume: bool,
    pub overwrite: OverwritePolicy,
    pub retries: u32,
    pub timeout: u64,
    pub extract: bool,
    /// Extra `"url"` field on JSONL events (batch mode)
    pub tag_events: bool,
}

impl Job {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            output: None,
            forced: None,
            advanced: false,
            sha256: None,
            checksum: None,
            checksum_url: None,
            headers: Vec::new(),
            quality: VideoQuality::Best,
            quiet: false,
            jsonl: false,
            resume: false,
            overwrite: OverwritePolicy::Overwrite,
            retries: kget::http::DEFAULT_RETRIES,
            timeout: kget::http::DEFAULT_TIMEOUT_SECS,
            extract: false,
            tag_events: false,
        }
    }

    pub fn protocol(&self) -> Protocol {
        self.forced.unwrap_or_else(|| protocol::detect(&self.url))
    }

    fn emit(&self, ev: &JsonEvent) {
        if !self.jsonl {
            return;
        }
        if self.tag_events {
            let mut v: serde_json::Value = serde_json::from_str(&ev.to_line()).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("url".into(), self.url.clone().into());
            }
            println!("{v}");
        } else {
            ev.emit();
        }
    }
}

/// How a job ended.
pub(crate) enum Outcome {
    /// Data saved by KGet (path when known: single file downloads)
    Saved(Option<PathBuf>),
    /// Handed to an external application
    HandedOff,
}

/// Downloads that stop cleanly on Ctrl+C (others exit immediately).
static CANCEL_AWARE: AtomicBool = AtomicBool::new(true);

/// Run one job and record it in the history; emits JSONL events when asked.
pub(crate) fn run_and_record(job: &Job, ctx: &Ctx) -> Result<Outcome, BoxError> {
    let proto = job.protocol();
    CANCEL_AWARE.store(
        matches!(
            proto,
            Protocol::Http | Protocol::WebDav | Protocol::Torrent | Protocol::Video
        ),
        Ordering::SeqCst,
    );
    job.emit(&JsonEvent::Started {
        url: kget::queue::redact_url(&job.url),
        protocol: proto.as_str().to_string(),
        advanced: job.advanced,
        output: job.output.clone().unwrap_or_default(),
    });

    let result = run_job(job, ctx, proto).and_then(|outcome| {
        if let Outcome::Saved(Some(path)) = &outcome {
            post_verify(job, ctx, path)?;
        }
        Ok(outcome)
    });

    // History (best-effort; never fail the download over a history error)
    let output_dir = job
        .output
        .as_deref()
        .map(utils::expand_tilde)
        .unwrap_or_else(|| ".".into());
    let mut entry = HistoryEntry::new(&job.url, &output_dir, job.sha256.as_deref());
    let cancelled = ctx.cancel.load(Ordering::SeqCst);
    let (status, error_msg) = match &result {
        Ok(Outcome::Saved(path)) => {
            if let Some(p) = path {
                entry.set_saved_file(p);
            }
            (EntryStatus::Completed, None)
        }
        Ok(Outcome::HandedOff) => (EntryStatus::HandedOff, None),
        Err(_) if cancelled => (EntryStatus::Cancelled, None),
        Err(e) => (EntryStatus::Failed, Some(e.to_string())),
    };
    if !matches!(proto, Protocol::Metalink) {
        let _ = DownloadHistory::append(entry, status, error_msg);
    }

    match &result {
        Ok(Outcome::Saved(path)) => {
            if let Some(p) = path {
                if job.extract
                    && utils::is_extractable(p)
                    && let Err(e) = kget::auto_extract(p, job.quiet)
                {
                    eprintln!("Warning: auto-extract: {e}");
                }
                let bytes = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                job.emit(&JsonEvent::Saved {
                    path: p.to_string_lossy().into_owned(),
                    bytes,
                });
            }
            job.emit(&JsonEvent::Completed);
        }
        Ok(Outcome::HandedOff) => {
            job.emit(&JsonEvent::Status {
                message: "Opened in the system torrent client; KGet did not download the data"
                    .into(),
            });
            job.emit(&JsonEvent::Completed);
        }
        Err(e) => {
            let (code, retryable) = match e.downcast_ref::<KgetError>() {
                Some(k) => (k.code(), k.is_retryable(&[408, 429, 500, 502, 503, 504])),
                None if cancelled => ("cancelled".into(), false),
                None => ("error".into(), true),
            };
            if let Some(KgetError::ChecksumMismatch { algorithm, got, .. }) =
                e.downcast_ref::<KgetError>()
            {
                job.emit(&JsonEvent::Verified {
                    algorithm: algorithm.clone(),
                    hash: got.clone(),
                    ok: false,
                });
            }
            job.emit(&JsonEvent::Error {
                code,
                message: e.to_string(),
                retryable,
            });
        }
    }
    result
}

/// Optional checksum verification for `--checksum` / `--checksum-url`.
fn post_verify(job: &Job, ctx: &Ctx, path: &Path) -> Result<(), BoxError> {
    let mut expected = job.checksum.clone();
    if expected.is_none()
        && let Some(sidecar) = &job.checksum_url
    {
        let client = kget::http::build_client(&ctx.config.proxy, &Default::default())?;
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
            job.emit(&JsonEvent::Verified {
                algorithm: "sha256".into(),
                hash: h.trim().to_ascii_lowercase(),
                ok: true,
            });
        }
        return Ok(());
    };
    if !job.quiet {
        println!("Verifying {}...", algo.name());
    }
    let got = compute_checksum(path, &algo)?;
    let want = want.trim().to_ascii_lowercase();
    let ok = got == want;
    job.emit(&JsonEvent::Verified {
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
    if !job.quiet {
        println!("{} OK: {}", algo.name().to_uppercase(), got);
    }
    Ok(())
}

/// Progress callback that emits JSONL `progress` events.
fn jsonl_progress(job: &Job) -> Option<ByteProgressCallback> {
    if !job.jsonl {
        return None;
    }
    let meter = Mutex::new(SpeedMeter::new());
    let job = job.clone();
    Some(Arc::new(move |bytes, total| {
        let ev = meter.lock().expect("meter poisoned").progress(bytes, total);
        job.emit(&ev);
    }))
}

fn jsonl_status(job: &Job) -> impl Fn(String) + Send + Sync + 'static {
    let job = job.clone();
    move |message: String| {
        // Progress is reported through structured progress events.
        if !message.starts_with("PROGRESS:") {
            job.emit(&JsonEvent::Status { message });
        }
    }
}

fn run_job(job: &Job, ctx: &Ctx, proto: Protocol) -> Result<Outcome, BoxError> {
    let config = &ctx.config;
    let optimizer = Optimizer::from_config(config.optimization.clone());
    let quiet = job.quiet || job.jsonl;
    let status = jsonl_status(job);

    match proto {
        Protocol::Video => {
            if ytdlp_binary().is_none() {
                let msg = kget::ytdlp::ytdlp_not_found_message();
                if !job.jsonl {
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
                    VideoEvent::Progress(p) if job.jsonl => {
                        let mut e = meter.lock().expect("meter poisoned").progress(0, None);
                        if let JsonEvent::Progress {
                            percent, progress, ..
                        } = &mut e
                        {
                            *percent = Some(p.percent);
                            *progress = Some(p.percent / 100.0);
                        }
                        job.emit(&e);
                    }
                    VideoEvent::Progress(p) if !quiet => {
                        println!(
                            "PROGRESS: {:.1}% SPEED: {} ETA: {}",
                            p.percent, p.speed, p.eta
                        )
                    }
                    VideoEvent::Saved(path) if !job.jsonl => println!("SAVED: {path}"),
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
                progress_callback: jsonl_progress(job),
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
            .download_to_path(Some(&status))?;
            Ok(Outcome::Saved(Some(path)))
        }
        Protocol::Metalink => {
            let output_dir = job.output.as_deref().unwrap_or(".");
            metalink::download_metalink(
                &job.url,
                output_dir,
                quiet,
                config.proxy.clone(),
                optimizer,
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
            .with_resume(job.resume || job.overwrite != OverwritePolicy::Overwrite);
            if let Some(cb) = jsonl_progress(job) {
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
            .with_resume(job.resume || job.overwrite != OverwritePolicy::Overwrite);
            if let Some(cb) = jsonl_progress(job) {
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
            let callbacks = if job.jsonl {
                let j = job.clone();
                TorrentCallbacks {
                    status: Some(Arc::new(jsonl_status(job))),
                    progress: Some(Arc::new(move |p| {
                        let pct = p as f64 * 100.0;
                        j.emit(&JsonEvent::Progress {
                            bytes: 0,
                            total: None,
                            percent: Some(pct),
                            speed_bps: 0,
                            eta_secs: None,
                            progress: Some(p as f64),
                        });
                    })),
                }
            } else {
                TorrentCallbacks::default()
            };
            let sizes: Arc<Mutex<Vec<u64>>> = Arc::default();
            let on_file_progress: Option<kget::torrent::FileProgressCb> = job.jsonl.then(|| {
                let j = job.clone();
                let sizes = sizes.clone();
                Arc::new(move |done: Vec<u64>| {
                    let sizes = sizes.lock().expect("sizes poisoned");
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
                    j.emit(&JsonEvent::FileProgress { files });
                }) as kget::torrent::FileProgressCb
            });
            let on_files: Option<kget::torrent::FilesCb> = job.jsonl.then(|| {
                let j = job.clone();
                let sizes = sizes.clone();
                Arc::new(move |files: Vec<(String, u64)>| {
                    *sizes.lock().expect("sizes poisoned") =
                        files.iter().map(|(_, size)| *size).collect();
                    j.emit(&JsonEvent::Files {
                        files: files
                            .into_iter()
                            .map(|(name, size)| FileEntry { name, size })
                            .collect(),
                    });
                }) as kget::torrent::FilesCb
            });
            let outcome = kget::torrent::download_torrent(
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
            if !std::io::stdin().is_terminal() || job.jsonl {
                downloader.set_resume_policy(ResumePolicy::AlwaysResume);
            }
            if let Some(cb) = jsonl_progress(job) {
                downloader.set_byte_progress_callback(move |b, t| cb(b, t));
            }
            if job.jsonl {
                downloader.set_status_callback(jsonl_status(job));
            }
            downloader.download()?;
            Ok(Outcome::Saved(Some(output_abs)))
        }
        Protocol::Http => {
            let options = DownloadOptions {
                quiet_mode: quiet,
                output_path: job.output.clone(),
                verify_iso: job.sha256.is_some(),
                expected_sha256: job.sha256.clone(),
                extra_headers: job.headers.clone(),
                resume: job.resume,
                overwrite: job.overwrite,
                retries: job.retries,
                timeout_secs: job.timeout,
                cancel: Some(ctx.cancel.clone()),
                progress_callback: jsonl_progress(job),
            };
            let path = if job.jsonl {
                kget::download_to_path(
                    &job.url,
                    config.proxy.clone(),
                    optimizer,
                    options,
                    Some(&status),
                )?
            } else {
                kget::download_to_path(&job.url, config.proxy.clone(), optimizer, options, None)?
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
                    kget::verify_file_sha256(&path, None, Some(&cb))?;
                }
            }
            Ok(Outcome::Saved(Some(path)))
        }
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
// Entry point
// ============================================================================

fn main() {
    let code = match real_main() {
        Ok(code) => code,
        Err(e) => {
            if matches!(e.downcast_ref::<KgetError>(), Some(KgetError::Cancelled)) {
                eprintln!("Download cancelled; run the same command again to resume.");
                130
            } else {
                eprintln!("Error: {e}");
                1
            }
        }
    };
    std::process::exit(code);
}

fn real_main() -> Result<i32, BoxError> {
    let args = Args::parse();
    let mut config = Config::load()?;

    if args.doctor {
        print_doctor(&config);
        return Ok(0);
    }

    if args.interactive {
        interactive::interactive_mode();
        return Ok(0);
    }

    // History commands (don't require a URL)
    if args.history {
        let history = DownloadHistory::load();
        let entries = history.recent(50);
        if entries.is_empty() {
            println!("No download history.");
        } else {
            println!("{:<16} {:<22} {:<12} File", "ID", "Date (UTC)", "Status");
            println!("{}", "-".repeat(86));
            for e in entries {
                println!(
                    "{:<16} {:<22} {:<12} {}",
                    e.id,
                    e.created_at_display(),
                    e.status,
                    e.filename
                );
            }
        }
        return Ok(0);
    }

    if let Some(ref scope) = args.history_clear {
        let mut removed = 0usize;
        DownloadHistory::update(|entries| {
            let before = entries.len();
            match scope.as_str() {
                "completed" | "done" => entries.retain(|e| e.status == EntryStatus::Failed),
                _ => entries.clear(),
            }
            removed = before - entries.len();
        })?;
        println!("Removed {} history entries.", removed);
        return Ok(0);
    }

    let wants_cli = !args.url.is_empty()
        || args.batch.is_some()
        || args.ftp
        || args.sftp
        || args.torrent
        || args.metalink
        || args.webdav
        || args.ytdlp;
    let should_start_gui = args.gui || (!wants_cli && !args.save_config);

    if !should_start_gui {
        // Flags only override the configuration for this run.
        apply_cli_overrides(&mut config, &args);
        if args.save_config {
            let mut to_save = config.clone();
            // Never persist the proxy password.
            to_save.proxy.password = None;
            to_save.save()?;
            println!("Configuration saved to {}", Config::path()?.display());
            if !wants_cli {
                return Ok(0);
            }
        }
    }

    if should_start_gui {
        #[cfg(feature = "gui")]
        {
            #[cfg(windows)]
            detach_owned_console();

            let (download_tx, download_rx_worker): (
                MpscSender<DownloadCommand>,
                MpscReceiver<DownloadCommand>,
            ) = mpsc::channel();
            let (status_tx_worker, status_rx_gui): (
                MpscSender<WorkerToGuiMessage>,
                MpscReceiver<WorkerToGuiMessage>,
            ) = mpsc::channel();

            spawn_download_worker(config.clone(), download_rx_worker, status_tx_worker);

            let mut native_options = eframe::NativeOptions::default();
            native_options.viewport.inner_size = Some(egui::Vec2::new(980.0, 680.0));
            native_options.viewport.min_inner_size = Some(egui::Vec2::new(820.0, 560.0));
            native_options.viewport.resizable = Some(true);

            // Embed logo in binary so it works in .app bundle
            let logo_bytes = include_bytes!("../logo.png");
            if let Ok(img) = image::load_from_memory(logo_bytes) {
                let rgba_img = img.into_rgba8();
                let (width, height) = rgba_img.dimensions();
                let rgba = rgba_img.into_raw();
                let icon = egui::IconData {
                    rgba,
                    width,
                    height,
                };
                native_options.viewport.icon = Some(std::sync::Arc::new(icon));
            }

            if let Err(e) = eframe::run_native(
                "KGet Downloader",
                native_options,
                Box::new(move |cc| {
                    egui_extras::install_image_loaders(&cc.egui_ctx);
                    Ok(Box::new(KGetGui::new(cc, download_tx, status_rx_gui)))
                }),
            ) {
                eprintln!("Failed to launch GUI: {e}");
                return Err("Failed to launch GUI".into());
            }

            return Ok(0);
        }

        #[cfg(not(feature = "gui"))]
        {
            if args.gui {
                eprintln!(
                    "GUI support was not compiled in. Rebuild with `--features gui` to enable it."
                );
                return Err("GUI not available (compile with --features gui)".into());
            }
            Args::command().print_help()?;
            return Ok(0);
        }
    }

    // ==========================================================
    //                         CLI MODE
    // ==========================================================

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let c = cancel.clone();
        let _ = ctrlc::set_handler(move || {
            // Second Ctrl+C, or a protocol that cannot stop cleanly: exit now.
            if c.swap(true, Ordering::SeqCst) || !CANCEL_AWARE.load(Ordering::SeqCst) {
                std::process::exit(130);
            }
        });
    }
    let ctx = Ctx { config, cancel };

    let checksum = match args.checksum.as_deref() {
        Some(spec) => Some(parse_checksum_spec(spec)?),
        None => None,
    };

    let mut template = Job::new(args.url.clone());
    template.output = args.output.clone();
    template.forced = forced_protocol(&args);
    template.advanced = args.advanced;
    template.sha256 = args.sha256.clone();
    template.checksum = checksum;
    template.checksum_url = args.checksum_url.clone();
    template.headers = parse_extra_headers(&args.header);
    template.quality = VideoQuality::parse(&args.quality);
    template.quiet = args.quiet || args.jsonl;
    template.jsonl = args.jsonl;
    template.resume = args.continue_download;
    template.overwrite = if args.no_clobber {
        OverwritePolicy::Skip
    } else {
        OverwritePolicy::Overwrite
    };
    template.retries = args.retries.max(1);
    template.timeout = args.timeout;
    template.extract = args.extract;

    // Scheduled start (applies to both batch and single-URL)
    if let Some(ref at_time) = args.at {
        wait_until(at_time, template.quiet)?;
    }

    if let Some(ref batch_file) = args.batch {
        return run_batch(batch_file, &args, &template, &ctx);
    }

    match run_and_record(&template, &ctx) {
        Ok(_) => Ok(0),
        Err(e) => Err(e),
    }
}

/// Protocol explicitly requested with a flag.
fn forced_protocol(args: &Args) -> Option<Protocol> {
    if args.ytdlp {
        Some(Protocol::Video)
    } else if args.webdav {
        Some(Protocol::WebDav)
    } else if args.metalink {
        Some(Protocol::Metalink)
    } else if args.ftp {
        Some(Protocol::Ftp)
    } else if args.sftp {
        Some(Protocol::Sftp)
    } else if args.torrent {
        Some(Protocol::Torrent)
    } else {
        None
    }
}

/// Apply CLI flags to the in-memory configuration (never persisted unless
/// `--save-config` is given).
fn apply_cli_overrides(config: &mut Config, args: &Args) {
    if let Some(url) = &args.proxy {
        config.proxy.enabled = true;
        config.proxy.url = Some(url.clone());
        config.proxy.proxy_type = match args.proxy_type.as_deref() {
            Some(t) => ProxyType::parse(t),
            None => ProxyType::from_url(url),
        };
    } else if let Some(t) = args.proxy_type.as_deref() {
        config.proxy.proxy_type = ProxyType::parse(t);
    }
    if let Some(u) = &args.proxy_user {
        config.proxy.username = Some(u.clone());
    }
    if let Some(p) = &args.proxy_pass {
        config.proxy.password = Some(p.clone());
    }
    if let Some(l) = args.speed_limit {
        config.optimization.speed_limit = (l > 0).then_some(l);
    }
    if args.torrent {
        config.torrent.enabled = true;
    }
    if let Some(v) = args.max_peers {
        config.torrent.max_peers = v;
    }
    if let Some(v) = args.max_seeds {
        config.torrent.max_seeds = v;
    }
    if let Some(v) = args.torrent_port {
        config.torrent.port = Some(v);
    }
    if args.no_dht {
        config.torrent.dht_enabled = false;
    }
}

fn parse_checksum_spec(spec: &str) -> Result<(ChecksumAlgorithm, String), BoxError> {
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

// ============================================================================
// Batch mode
// ============================================================================

fn run_batch(batch_file: &str, args: &Args, template: &Job, ctx: &Ctx) -> Result<i32, BoxError> {
    use std::io::Read;

    let content = if batch_file == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s
    } else {
        std::fs::read_to_string(batch_file)
            .map_err(|e| format!("Cannot read batch file '{}': {}", batch_file, e))?
    };

    let urls: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();

    if urls.is_empty() {
        if !args.jsonl {
            println!("Batch file contains no URLs.");
        }
        return Ok(0);
    }

    let output_dir = PathBuf::from(utils::expand_tilde(args.output.as_deref().unwrap_or(".")));
    std::fs::create_dir_all(&output_dir)?;
    let jobs = args.jobs.clamp(1, 64);
    if !args.jsonl {
        println!("Batch: {} URL(s) queued, {} at a time", urls.len(), jobs);
    }

    // Reserve one unique destination per URL so two "…/file.zip" never collide.
    let mut taken = std::collections::HashSet::new();
    let batch_jobs: Vec<Job> = urls
        .iter()
        .map(|url| {
            let mut job = template.clone();
            job.url = url.clone();
            job.quiet = true;
            job.tag_events = args.jsonl;
            job.forced = template.forced;
            let proto = job.protocol();
            job.output = Some(match proto {
                Protocol::Http | Protocol::WebDav | Protocol::Ftp | Protocol::Sftp => {
                    let name = utils::get_filename_from_url_or_default(url, "download");
                    let path = if template.overwrite == OverwritePolicy::Overwrite {
                        utils::unique_path(&output_dir, &name, &mut taken)
                    } else {
                        // -c / --no-clobber must find the existing file under its own name
                        let p = output_dir.join(&name);
                        if taken.insert(p.clone()) {
                            p
                        } else {
                            utils::unique_path(&output_dir, &name, &mut taken)
                        }
                    };
                    path.to_string_lossy().into_owned()
                }
                _ => output_dir.to_string_lossy().into_owned(),
            });
            job
        })
        .collect();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .thread_name(|i| format!("kget-batch-{i}"))
        .build()?;
    let results: Vec<(String, Result<(), String>)> = pool.install(|| {
        use rayon::prelude::*;
        batch_jobs
            .par_iter()
            .map(|job| {
                let r = run_and_record(job, ctx)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                if !args.jsonl {
                    match &r {
                        Ok(()) => println!("[OK]   {}", job.url),
                        Err(e) => eprintln!("[FAIL] {} — {}", job.url, e),
                    }
                }
                (job.url.clone(), r)
            })
            .collect()
    });

    let failed = results.iter().filter(|(_, r)| r.is_err()).count();
    let succeeded = results.len() - failed;
    if !args.jsonl {
        println!(
            "Batch complete: {} succeeded, {} failed.",
            succeeded, failed
        );
    }
    if ctx.cancel.load(Ordering::SeqCst) {
        return Ok(130);
    }
    Ok(if failed > 0 { 1 } else { 0 })
}

// ============================================================================
// Helpers
// ============================================================================

/// Parse `-H "Name: value"` (also `Name:value`); invalid headers are reported.
pub(crate) fn parse_extra_headers(raw: &[String]) -> Vec<(String, String)> {
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

fn print_doctor(config: &Config) {
    println!("KGet {}", env!("CARGO_PKG_VERSION"));
    println!(
        "OS:       {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    match kget::config::config_dir() {
        Ok(d) => println!("Config:   {}", d.display()),
        Err(e) => println!("Config:   unavailable ({e})"),
    }
    println!("PATH:     {}", std::env::var("PATH").unwrap_or_default());
    match ytdlp_binary() {
        Some(p) => println!("yt-dlp:   {p}"),
        None => println!(
            "yt-dlp:   NOT FOUND\n{}",
            kget::ytdlp::ytdlp_not_found_message()
        ),
    }
    for tool in ["unzip", "tar", "7z"] {
        match utils::which_in_path(tool) {
            Some(p) => println!("{tool:<9} {}", p.display()),
            None => println!("{tool:<9} missing (needed for --extract)"),
        }
    }
    println!(
        "Features: gui={} torrent-native={} torrent-transmission={}",
        cfg!(feature = "gui"),
        cfg!(feature = "torrent-native"),
        cfg!(feature = "torrent-transmission")
    );
    println!(
        "Proxy:    {}",
        if config.proxy.enabled {
            config.proxy.url.as_deref().unwrap_or("enabled, but no URL")
        } else {
            "disabled"
        }
    );
    println!(
        "Limit:    {}",
        config
            .optimization
            .speed_limit
            .map(|l| format!("{l} B/s"))
            .unwrap_or_else(|| "unlimited".into())
    );
    println!("Connections: {}", config.optimization.max_connections);
}

/// Hide the console window when the GUI was started by double-clicking
/// (i.e. this process is the only one attached to the console).
#[cfg(all(feature = "gui", windows))]
fn detach_owned_console() {
    use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleProcessList};
    let mut pids = [0u32; 2];
    // SAFETY: the buffer is valid for 2 entries.
    let n = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
    if n == 1 {
        // SAFETY: detaching from our own console has no preconditions.
        unsafe {
            FreeConsole();
        }
    }
}

/// Sleep until the next occurrence of `time` (format `HH:MM`, local time).
///
/// Uses the system time zone (including on Windows) and re-checks the clock
/// every 30 seconds, so DST changes and system sleep are handled.
fn wait_until(time: &str, quiet: bool) -> Result<(), BoxError> {
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
