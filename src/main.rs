//! `kget` binary: parses the command line and launches a mode (CLI download,
//! batch, interactive REPL, terminal UI, GUI, history, doctor).
//!
//! All download logic lives in the library: [`kget::runner`] (dispatch,
//! verification, history, JSONL), [`kget::cli`] (terminal helpers) and
//! [`kget::app`] / [`kget::jobs`] (GUI worker and job manager).

// Same lint policy as the library: a panic on bad input is a bug here too.
#![cfg_attr(
    not(test),
    warn(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

#[cfg(not(feature = "gui"))]
use clap::CommandFactory;
use clap::Parser;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod interactive;

#[cfg(feature = "gui")]
mod gui;

#[cfg(feature = "tui")]
mod tui;

use kget::cli::{self, ConfigOverrides, HistoryFilter};
use kget::config::Config;
use kget::daemon::cli as daemon_cli;
use kget::daemon::protocol::SubmitParams;
use kget::protocol::Protocol;
use kget::runner::{self, DownloadJob, Reporter, RunContext};
use kget::utils;
use kget::ytdlp::VideoQuality;
use kget::{KgetError, OverwritePolicy};

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(disable_version_flag = true)]
struct Args {
    /// Show version information
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    #[command(subcommand)]
    command: Option<Command>,

    /// Run the download through the background daemon (started if needed)
    #[arg(long = "via-daemon")]
    via_daemon: bool,

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

    /// HTTP version for turbo downloads: auto, 1.1, 2 or 3 (3 needs a build with --features http3)
    #[arg(long = "http-version", value_name = "VERSION", value_parser = parse_http_version)]
    http_version: Option<kget::http::HttpVersion>,

    /// Torrent: download only these files, by number as shown by --list-files (e.g. 1,3-5; repeatable)
    #[arg(long = "select", value_name = "LIST")]
    select: Vec<String>,

    /// Torrent: download only files whose path matches this pattern (e.g. "*.mkv"; repeatable)
    #[arg(long = "select-glob", value_name = "PATTERN")]
    select_glob: Vec<String>,

    /// Torrent: print the files of a magnet link or .torrent and exit (needs torrent-native)
    #[arg(long = "list-files")]
    list_files: bool,

    /// Torrent: keep seeding after the download until this share ratio (e.g. 1.5)
    #[arg(long = "seed-ratio", value_name = "RATIO")]
    seed_ratio: Option<String>,

    /// Torrent: keep seeding after the download for this long (e.g. 30m, 2h; bare number = minutes)
    #[arg(long = "seed-time", value_name = "DURATION")]
    seed_time: Option<String>,

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

    /// Persist the given options (proxy, limit, torrent settings) to config.json;
    /// the proxy password goes to the system keychain
    #[arg(long = "save-config")]
    save_config: bool,

    /// Save passwords used by this download (URL or prompt) in the system keychain
    #[arg(long = "save-credentials")]
    save_credentials: bool,

    /// Manage saved passwords: list | status | add <target> | delete <target>
    #[arg(long = "credentials", num_args = 1..=2, value_names = ["ACTION", "TARGET"])]
    credentials: Option<Vec<String>>,

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

    /// Number of simultaneous downloads in batch mode, and of files at a time for hf:// and ollama://
    #[arg(short = 'j', long = "jobs", default_value_t = 4)]
    jobs: usize,

    /// hf://: only download files matching this glob (repeatable: --include "*.gguf")
    #[arg(long = "include", value_name = "GLOB")]
    include: Vec<String>,

    /// hf://: skip files matching this glob (repeatable: --exclude "*fp32*")
    #[arg(long = "exclude", value_name = "GLOB")]
    exclude: Vec<String>,

    /// hf://: branch, tag or commit to download (default: main, or the URL's @revision)
    #[arg(long = "revision", value_name = "REV")]
    revision: Option<String>,

    /// hf://: write into the Hugging Face hub cache (HF_HUB_CACHE / HF_HOME) so transformers reuses it
    #[arg(long = "hf-cache")]
    hf_cache: bool,

    /// Extra HTTP header sent with the request (repeatable: -H "Referer: https://…")
    #[arg(short = 'H', long = "header")]
    header: Vec<String>,

    /// Auto-extract archive after a successful download (.zip, .tar.gz, .7z …)
    #[arg(long = "extract")]
    extract: bool,

    /// Remux/transmux downloaded media or local file to target format (e.g. mp4, mkv) without re-encoding
    #[arg(long = "remux", value_name = "FORMAT")]
    remux: Option<String>,

    /// Schedule the download to start at a specific local time (HH:MM, 24-hour)
    #[arg(long = "at")]
    at: Option<String>,

    /// Show download history
    #[arg(long = "history")]
    history: bool,

    /// Clear download history (all, or --history-clear completed|failed)
    #[arg(long = "history-clear")]
    history_clear: Option<String>,

    /// History filter: status (completed, failed, cancelled, handed_off; repeatable)
    #[arg(long = "status", requires = "history")]
    history_status: Vec<String>,

    /// History filter: domain (also matches subdomains)
    #[arg(long = "domain", requires = "history")]
    history_domain: Option<String>,

    /// History filter: file type (video, audio, image, archive, document, disk, package) or extensions (iso,zip)
    #[arg(long = "type", requires = "history")]
    history_type: Option<String>,

    /// History filter: protocol (http, ftp, sftp, webdav, torrent, video)
    #[arg(long = "protocol", requires = "history")]
    history_protocol: Option<String>,

    /// History filter: started on/after (YYYY-MM-DD, 'YYYY-MM-DD HH:MM', today, 7d, 12h; UTC)
    #[arg(long = "since", requires = "history")]
    history_since: Option<String>,

    /// History filter: started on/before (YYYY-MM-DD includes that day; UTC)
    #[arg(long = "until", requires = "history")]
    history_until: Option<String>,

    /// History filter: text in the URL or filename
    #[arg(long = "search", requires = "history")]
    history_search: Option<String>,

    /// History order: newest, oldest, largest, smallest, name, fastest
    #[arg(long = "sort", requires = "history", default_value = "newest")]
    history_sort: String,

    /// Maximum number of history entries shown (0 = all)
    #[arg(long = "history-limit", requires = "history", default_value_t = 50)]
    history_limit: usize,

    /// Skip this many history entries (paging)
    #[arg(long = "history-offset", requires = "history", default_value_t = 0)]
    history_offset: usize,

    /// Print the history as a JSON array (with --jsonl: one object per line)
    #[arg(long = "json", requires = "history")]
    history_json: bool,

    /// Download via WebDAV (webdav:// or webdavs:// URLs); auto-detected from scheme
    #[arg(long = "webdav")]
    webdav: bool,

    /// Route URL through yt-dlp (auto-detected for known video platforms)
    #[arg(long = "ytdlp")]
    ytdlp: bool,

    /// Video quality for yt-dlp downloads: best, 1080p, 720p, 480p, 360p, audio
    #[arg(long = "quality", default_value = "best")]
    quality: String,

    /// Sort this download into a folder with the smart rules (see --rules)
    #[arg(long = "organize", conflicts_with = "no_organize")]
    organize: bool,

    /// Do not apply the smart rules, even if enabled in the config
    #[arg(long = "no-organize")]
    no_organize: bool,

    /// Tag recorded in the history (repeatable); with --history: only entries with this tag
    #[arg(long = "tag")]
    tag: Vec<String>,

    /// Already-downloaded URL: skip, ask, download or link (default: ask on a terminal, else download)
    #[arg(long = "duplicates", value_name = "POLICY")]
    duplicates: Option<String>,

    /// Replace the download with a hard link when identical content is already in the history
    #[arg(long = "dedupe")]
    dedupe: bool,

    /// Manage smart rules: list | test <url> | add key=value… | remove <name> | enable|disable [name] | suggest
    #[arg(long = "rules", num_args = 1.., value_names = ["ACTION", "ARGS"])]
    rules: Option<Vec<String>>,

    /// With --rules suggest: also ask Claude (needs ANTHROPIC_API_KEY; sends only aggregated counts after confirmation)
    #[arg(long = "ai", requires = "rules")]
    ai: bool,

    /// With --rules suggest: add the suggested rules without asking
    #[arg(long = "apply", requires = "rules")]
    apply: bool,
}

/// `--http-version`: a known version that this build supports.
fn parse_http_version(s: &str) -> Result<kget::http::HttpVersion, String> {
    s.parse::<kget::http::HttpVersion>()?.ensure_available()
}

/// Subcommands (`kget <url>` keeps working without one).
#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Run the background download daemon (`daemon status`, `daemon stop`)
    Daemon(DaemonArgs),
    /// Manage the daemon's download queue
    Queue {
        #[command(subcommand)]
        action: QueueAction,
    },
    /// Serve the Model Context Protocol on stdin/stdout for AI agents
    Mcp(McpArgs),
    /// Full-screen terminal UI (uses the running daemon when there is one)
    Tui(TuiArgs),
    /// Native-messaging host for the browser extension (started by the browser)
    #[command(hide = true)]
    NativeHost(NativeHostArgs),
}

#[derive(clap::Args, Debug)]
struct TuiArgs {
    /// Use the background daemon, starting it if needed (downloads continue after quitting)
    #[arg(long = "daemon", conflicts_with = "local")]
    daemon: bool,
    /// Run downloads in this process even when a daemon is running
    #[arg(long = "local")]
    local: bool,
}

#[derive(clap::Args, Debug)]
struct DaemonArgs {
    #[command(subcommand)]
    action: Option<DaemonAction>,

    /// Stay in the foreground (log to stderr too) instead of detaching
    #[arg(long = "foreground")]
    foreground: bool,

    /// Directory downloads are saved in (default: daemon.download_dir, else ~/Downloads)
    #[arg(long = "download-dir")]
    download_dir: Option<PathBuf>,

    /// Let clients write outside the download directory
    #[arg(long = "allow-outside-download-dir")]
    allow_outside: bool,

    /// Number of downloads running at the same time
    #[arg(short = 'j', long = "max-concurrent")]
    max_concurrent: Option<usize>,
}

#[derive(clap::Subcommand, Debug)]
enum DaemonAction {
    /// Show whether the daemon is running (exit code 3 if not)
    Status {
        /// Print JSON
        #[arg(long = "json")]
        json: bool,
    },
    /// Stop the daemon; unfinished jobs resume on the next start
    Stop,
}

#[derive(clap::Subcommand, Debug)]
enum QueueAction {
    /// Add a download to the queue (starts the daemon if needed)
    Add(Box<QueueAddArgs>),
    /// List jobs
    List {
        /// Print JSON
        #[arg(long = "json")]
        json: bool,
    },
    /// Show one job
    Status {
        /// Job id
        id: u64,
        /// Print JSON
        #[arg(long = "json")]
        json: bool,
    },
    /// Cancel a job (partial data is kept)
    Cancel {
        /// Job id
        id: u64,
    },
    /// Pause a job
    Pause {
        /// Job id
        id: u64,
    },
    /// Resume a paused, cancelled or failed job
    Resume {
        /// Job id
        id: u64,
    },
    /// Remove a finished job from the queue
    #[command(alias = "remove")]
    Rm {
        /// Job id
        id: u64,
    },
    /// Print job events as they happen (until the given jobs finish)
    Watch {
        /// Job ids (none = every job, until Ctrl+C)
        ids: Vec<u64>,
        /// Print JSON lines
        #[arg(long = "json")]
        json: bool,
    },
}

#[derive(clap::Args, Debug)]
struct QueueAddArgs {
    /// URL, magnet link or .torrent/.meta4 file
    url: String,
    /// Output file or directory (must be inside the daemon's download directory)
    #[arg(short = 'O', long = "output")]
    output: Option<String>,
    /// Parallel, resumable HTTP download
    #[arg(short = 'a', long = "advanced")]
    advanced: bool,
    /// Expected SHA256 hash
    #[arg(long = "sha256")]
    sha256: Option<String>,
    /// Expected checksum as <algorithm>:<hex>
    #[arg(long = "checksum")]
    checksum: Option<String>,
    /// URL of a checksum file
    #[arg(long = "checksum-url")]
    checksum_url: Option<String>,
    /// Extra HTTP header (repeatable)
    #[arg(short = 'H', long = "header")]
    header: Vec<String>,
    /// Continue a partially downloaded file
    #[arg(short = 'c', long = "continue")]
    continue_download: bool,
    /// Skip the download when the file exists
    #[arg(long = "no-clobber", conflicts_with = "rename")]
    no_clobber: bool,
    /// Save under a free name ("file (1).zip") when the file exists
    #[arg(long = "rename")]
    rename: bool,
    /// Maximum attempts per request
    #[arg(long = "retries", default_value_t = kget::http::DEFAULT_RETRIES)]
    retries: u32,
    /// I/O timeout in seconds
    #[arg(long = "timeout", default_value_t = kget::http::DEFAULT_TIMEOUT_SECS)]
    timeout: u64,
    /// Extract the archive after downloading
    #[arg(long = "extract")]
    extract: bool,
    /// Video quality for yt-dlp downloads
    #[arg(long = "quality", default_value = "best")]
    quality: String,
    /// Force FTP
    #[arg(long = "ftp")]
    ftp: bool,
    /// Force SFTP
    #[arg(long = "sftp")]
    sftp: bool,
    /// Force torrent
    #[arg(short = 't', long = "torrent")]
    torrent: bool,
    /// Force WebDAV
    #[arg(long = "webdav")]
    webdav: bool,
    /// Force yt-dlp
    #[arg(long = "ytdlp")]
    ytdlp: bool,
    /// Force Metalink
    #[arg(long = "metalink")]
    metalink: bool,
    /// Wait for the download to finish, showing progress
    #[arg(short = 'w', long = "wait")]
    wait: bool,
    /// Print JSON
    #[arg(long = "json")]
    json: bool,
    /// Fail instead of starting the daemon when it is not running
    #[arg(long = "no-start")]
    no_start: bool,
    /// Tag recorded in the history (repeatable)
    #[arg(long = "tag")]
    tag: Vec<String>,
    /// Apply the smart rules (folders inside the download directory)
    #[arg(long = "organize", conflicts_with = "no_organize")]
    organize: bool,
    /// Do not apply the smart rules
    #[arg(long = "no-organize")]
    no_organize: bool,
    /// hf://: only files matching this glob (repeatable)
    #[arg(long = "include")]
    include: Vec<String>,
    /// hf://: skip files matching this glob (repeatable)
    #[arg(long = "exclude")]
    exclude: Vec<String>,
    /// hf://: branch, tag or commit
    #[arg(long = "revision")]
    revision: Option<String>,
    /// hf://: write into the Hugging Face cache (daemon needs --allow-outside-download-dir)
    #[arg(long = "hf-cache")]
    hf_cache: bool,
    /// Torrent: file numbers to download (e.g. 1,3-5; repeatable)
    #[arg(long = "select", value_name = "LIST")]
    select: Vec<String>,
    /// Torrent: download files matching this pattern (repeatable)
    #[arg(long = "select-glob", value_name = "PATTERN")]
    select_glob: Vec<String>,
    /// Torrent: seed until this share ratio
    #[arg(long = "seed-ratio", value_name = "RATIO")]
    seed_ratio: Option<String>,
    /// Torrent: seed for this long (30m, 2h; bare number = minutes)
    #[arg(long = "seed-time", value_name = "DURATION")]
    seed_time: Option<String>,
}

impl QueueAddArgs {
    fn params(&self) -> Result<SubmitParams, BoxError> {
        let mut job = DownloadJob::new(absolute_source(&self.url));
        job.output = self.output.clone();
        job.protocol = if self.ytdlp {
            Some(Protocol::Video)
        } else if self.webdav {
            Some(Protocol::WebDav)
        } else if self.metalink {
            Some(Protocol::Metalink)
        } else if self.ftp {
            Some(Protocol::Ftp)
        } else if self.sftp {
            Some(Protocol::Sftp)
        } else if self.torrent {
            Some(Protocol::Torrent)
        } else {
            None
        };
        job.advanced = self.advanced;
        job.sha256 = self.sha256.clone();
        job.checksum = match self.checksum.as_deref() {
            Some(spec) => Some(runner::parse_checksum_spec(spec)?),
            None => None,
        };
        job.checksum_url = self.checksum_url.clone();
        job.headers = runner::parse_extra_headers(&self.header);
        job.quality = VideoQuality::parse(&self.quality);
        job.resume = self.continue_download;
        job.overwrite = if self.no_clobber {
            OverwritePolicy::Skip
        } else if self.rename {
            OverwritePolicy::Rename
        } else {
            OverwritePolicy::Overwrite
        };
        job.retries = self.retries.max(1);
        job.timeout = self.timeout;
        job.extract = self.extract;
        job.tags = self.tag.clone();
        job.organize = match (self.organize, self.no_organize) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        };
        job.include = self.include.clone();
        job.exclude = self.exclude.clone();
        job.revision = self.revision.clone();
        job.hf_cache = self.hf_cache;
        job.torrent = cli::torrent_job_options(
            &self.select,
            &self.select_glob,
            self.seed_ratio.as_deref(),
            self.seed_time.as_deref(),
        )?;
        Ok(SubmitParams::from_job(&job, &std::env::current_dir()?))
    }
}

#[derive(clap::Args, Debug)]
struct McpArgs {
    /// Directory the agent may download into (default: daemon.download_dir, else ~/Downloads)
    #[arg(long = "root")]
    root: Option<PathBuf>,
    /// Never use the daemon; run downloads inside this process
    #[arg(long = "no-daemon", conflicts_with = "start_daemon")]
    no_daemon: bool,
    /// Start the daemon if it is not running, and always use it
    #[arg(long = "start-daemon")]
    start_daemon: bool,
}

#[derive(clap::Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
struct NativeHostArgs {
    #[command(subcommand)]
    action: Option<NativeHostAction>,
    /// Arguments the browser passes (the calling extension's origin)
    #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
    caller: Vec<String>,
}

#[derive(clap::Subcommand, Debug)]
enum NativeHostAction {
    /// Register the host with browsers (writes the native messaging manifests)
    Install {
        /// chrome, chromium, edge, brave, firefox or all
        #[arg(long = "browser", default_value = "all")]
        browser: String,
        /// Extra extension ID to allow (Chromium ID or Firefox add-on ID); repeatable
        #[arg(long = "extension-id")]
        extension_id: Vec<String>,
    },
    /// Remove the host's manifests
    Uninstall {
        /// chrome, chromium, edge, brave, firefox or all
        #[arg(long = "browser", default_value = "all")]
        browser: String,
    },
}

/// A local `.torrent` / `.meta4` path made absolute (the daemon has its own
/// working directory); URLs are returned unchanged.
fn absolute_source(url: &str) -> String {
    let p = std::path::Path::new(url);
    if !url.contains("://")
        && !url.starts_with("magnet:")
        && p.is_relative()
        && p.exists()
        && let Ok(abs) = std::path::absolute(p)
    {
        return abs.to_string_lossy().into_owned();
    }
    url.to_string()
}

/// `kget daemon|queue|mcp …`.
fn run_command(cmd: &Command) -> Result<i32, BoxError> {
    match cmd {
        Command::Daemon(d) => match &d.action {
            Some(DaemonAction::Status { json }) => daemon_cli::status(*json),
            Some(DaemonAction::Stop) => daemon_cli::stop(),
            None if d.foreground => {
                let options = kget::daemon::DaemonOptions {
                    download_dir: d.download_dir.clone(),
                    allow_outside_download_dir: d.allow_outside.then_some(true),
                    max_concurrent: d.max_concurrent,
                    echo_log: true,
                    ..Default::default()
                };
                kget::daemon::run(options)?;
                Ok(0)
            }
            None => {
                let mut extra = Vec::new();
                if let Some(dir) = &d.download_dir {
                    extra.push("--download-dir".to_string());
                    extra.push(std::path::absolute(dir)?.to_string_lossy().into_owned());
                }
                if d.allow_outside {
                    extra.push("--allow-outside-download-dir".to_string());
                }
                if let Some(n) = d.max_concurrent {
                    extra.push(format!("--max-concurrent={n}"));
                }
                daemon_cli::start_background(&extra)
            }
        },
        Command::Queue { action } => match action {
            QueueAction::Add(a) => daemon_cli::add(&a.params()?, a.wait, a.json, !a.no_start),
            QueueAction::List { json } => daemon_cli::list(*json),
            QueueAction::Status { id, json } => daemon_cli::job_status(*id, *json),
            QueueAction::Cancel { id } => daemon_cli::job_action("cancel", *id, false),
            QueueAction::Pause { id } => daemon_cli::job_action("pause", *id, false),
            QueueAction::Resume { id } => daemon_cli::job_action("resume", *id, false),
            QueueAction::Rm { id } => daemon_cli::job_action("remove", *id, false),
            QueueAction::Watch { ids, json } => daemon_cli::watch(ids, *json),
        },
        // Handled in `real_main` (needs the global flags).
        Command::Tui(t) => launch_tui(Config::load()?, t),
        Command::Mcp(m) => {
            let mode = if m.no_daemon {
                kget::mcp::BackendMode::InProcess
            } else if m.start_daemon {
                kget::mcp::BackendMode::Daemon
            } else {
                kget::mcp::BackendMode::Auto
            };
            kget::mcp::serve_stdio(kget::mcp::McpOptions {
                root: m.root.clone(),
                mode,
            })?;
            Ok(0)
        }
        Command::NativeHost(n) => native_host_command(n),
    }
}

/// `kget native-host [install|uninstall]`.
fn native_host_command(n: &NativeHostArgs) -> Result<i32, BoxError> {
    use kget::native_host::install::{self, Browser, InstallOptions, Layout};
    let browsers = |name: &str| {
        Browser::parse_list(name).ok_or_else(|| {
            format!("unknown browser '{name}' (chrome, chromium, edge, brave, firefox or all)")
        })
    };
    let report = match &n.action {
        None => return kget::native_host::run(&n.caller),
        Some(NativeHostAction::Install {
            browser,
            extension_id,
        }) => install::install(
            &InstallOptions {
                browsers: browsers(browser)?,
                extension_ids: extension_id.clone(),
                exe: std::env::current_exe()?,
                only_present: browser.eq_ignore_ascii_case("all"),
            },
            &Layout::resolve()?,
        )?,
        Some(NativeHostAction::Uninstall { browser }) => {
            install::uninstall(&browsers(browser)?, &Layout::resolve()?)?
        }
    };
    for line in report {
        println!("{line}");
    }
    Ok(0)
}

impl Args {
    /// Protocol explicitly requested with a flag.
    fn forced_protocol(&self) -> Option<Protocol> {
        if self.ytdlp {
            Some(Protocol::Video)
        } else if self.webdav {
            Some(Protocol::WebDav)
        } else if self.metalink {
            Some(Protocol::Metalink)
        } else if self.ftp {
            Some(Protocol::Ftp)
        } else if self.sftp {
            Some(Protocol::Sftp)
        } else if self.torrent {
            Some(Protocol::Torrent)
        } else {
            None
        }
    }

    /// Flags that override the configuration for this run.
    fn overrides(&self) -> ConfigOverrides {
        ConfigOverrides {
            proxy: self.proxy.clone(),
            proxy_user: self.proxy_user.clone(),
            proxy_pass: self.proxy_pass.clone(),
            proxy_type: self.proxy_type.clone(),
            speed_limit: self.speed_limit,
            torrent: self.torrent,
            max_peers: self.max_peers,
            max_seeds: self.max_seeds,
            torrent_port: self.torrent_port,
            no_dht: self.no_dht,
        }
    }

    /// The `--history` filter flags.
    fn history_filter(&self) -> HistoryFilter {
        HistoryFilter {
            status: self.history_status.clone(),
            domain: self.history_domain.clone(),
            file_type: self.history_type.clone(),
            protocol: self.history_protocol.clone(),
            since: self.history_since.clone(),
            until: self.history_until.clone(),
            search: self.history_search.clone(),
            tags: self.tag.clone(),
            sort: self.history_sort.clone(),
            limit: self.history_limit,
            offset: self.history_offset,
        }
    }

    /// The download described by the flags.
    fn job(&self) -> Result<DownloadJob, BoxError> {
        let mut job = DownloadJob::new(self.url.clone());
        job.output = self.output.clone();
        job.protocol = self.forced_protocol();
        job.advanced = self.advanced;
        job.sha256 = self.sha256.clone();
        job.checksum = match self.checksum.as_deref() {
            Some(spec) => Some(runner::parse_checksum_spec(spec)?),
            None => None,
        };
        job.checksum_url = self.checksum_url.clone();
        job.headers = runner::parse_extra_headers(&self.header);
        job.quality = VideoQuality::parse(&self.quality);
        job.resume = self.continue_download;
        job.overwrite = if self.no_clobber {
            OverwritePolicy::Skip
        } else {
            OverwritePolicy::Overwrite
        };
        job.retries = self.retries.max(1);
        job.timeout = self.timeout;
        job.extract = self.extract;
        job.save_credentials = self.save_credentials;
        job.include = self.include.clone();
        job.exclude = self.exclude.clone();
        job.revision = self.revision.clone();
        job.hf_cache = self.hf_cache;
        job.parallel_files = self.jobs;
        job.tags = self.tag.clone();
        job.organize = match (self.organize, self.no_organize) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        };
        job.duplicates = match self.duplicates.as_deref() {
            Some(p) => Some(p.parse()?),
            None => None,
        };
        job.dedupe = self.dedupe;
        job.torrent = self.torrent_options()?;
        job.http_version = self.http_version;
        Ok(job)
    }

    /// `--select`, `--select-glob`, `--seed-ratio` and `--seed-time`.
    fn torrent_options(&self) -> Result<kget::torrent::TorrentJobOptions, BoxError> {
        cli::torrent_job_options(
            &self.select,
            &self.select_glob,
            self.seed_ratio.as_deref(),
            self.seed_time.as_deref(),
        )
    }

    /// The output for a single download.
    fn reporter(&self) -> Reporter {
        if self.jsonl {
            Reporter::jsonl_stdout(None)
        } else {
            Reporter {
                quiet: self.quiet,
                ..Reporter::default()
            }
        }
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
    if let Some(Command::Tui(t)) = &args.command {
        // Global flags (`--proxy`, `--limit`…) apply to in-process downloads.
        let mut config = Config::load()?;
        args.overrides().apply(&mut config);
        return launch_tui(config, t);
    }
    if let Some(cmd) = &args.command {
        return run_command(cmd);
    }
    let mut config = Config::load()?;

    if args.doctor {
        cli::print_doctor(&config);
        return Ok(0);
    }

    if let Some(cmd) = &args.credentials {
        let parts: Vec<&str> = cmd.iter().map(String::as_str).collect();
        println!("{}", kget::credentials::command(&parts)?);
        return Ok(0);
    }

    if let Some(cmd) = &args.rules {
        let parts: Vec<&str> = cmd.iter().map(String::as_str).collect();
        let opts = kget::rules::CommandOptions {
            ai: args.ai,
            apply: args.apply,
        };
        kget::rules::command(&parts, &mut config, opts)?;
        return Ok(0);
    }

    if args.interactive {
        interactive::interactive_mode();
        return Ok(0);
    }

    if args.list_files {
        if args.url.is_empty() {
            return Err("--list-files needs a magnet link or .torrent file".into());
        }
        args.overrides().apply(&mut config);
        let opts = args.torrent_options()?;
        cli::print_torrent_files(&args.url, &config, &opts.select, args.jsonl)?;
        return Ok(0);
    }

    if let Some(ref target_fmt) = args.remux {
        if args.url.is_empty() {
            return Err("--remux requires an input file path".into());
        }
        let input_path = std::path::Path::new(&args.url);
        let output_path = if let Some(ref out) = args.output {
            std::path::PathBuf::from(out)
        } else {
            input_path.with_extension(target_fmt.trim_start_matches('.'))
        };
        let opts = kget::remuxer::RemuxOptions {
            target_format: target_fmt.trim_start_matches('.').to_string(),
            faststart: true,
            overwrite: !args.no_clobber,
        };
        println!(
            "Remuxing {} -> {}...",
            input_path.display(),
            output_path.display()
        );
        kget::remuxer::remux_lossless(&[input_path], &output_path, &opts)?;
        println!("Remux completed: {}", output_path.display());
        return Ok(0);
    }

    // History commands (don't require a URL)
    if args.history {
        cli::print_history(
            &args.history_filter().query()?,
            args.history_json,
            args.jsonl,
        )?;
        return Ok(0);
    }

    if let Some(ref scope) = args.history_clear {
        let removed = cli::clear_history(scope)?;
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

    if should_start_gui {
        return launch_gui(&args, config);
    }

    // Flags only override the configuration for this run.
    args.overrides().apply(&mut config);
    if args.save_config {
        let path = cli::save_config(&config)?;
        println!("Configuration saved to {}", path.display());
        if !wants_cli {
            return Ok(0);
        }
    }

    run_cli(&args, config)
}

/// CLI mode: one download, or a batch with `--batch`.
fn run_cli(args: &Args, config: Config) -> Result<i32, BoxError> {
    let cancel = Arc::new(AtomicBool::new(false));
    // Whether the current download stops cleanly on Ctrl+C (others exit immediately).
    let cancel_aware = Arc::new(AtomicBool::new(true));
    {
        let c = cancel.clone();
        let aware = cancel_aware.clone();
        let _ = ctrlc::set_handler(move || {
            // Second Ctrl+C, or a protocol that cannot stop cleanly: exit now.
            if c.swap(true, Ordering::SeqCst) || !aware.load(Ordering::SeqCst) {
                std::process::exit(130);
            }
        });
    }
    let mut ctx = RunContext::new(config).with_cancel(cancel);
    ctx.cancel_aware = Some(cancel_aware);
    // Ask for missing passwords on the terminal (never in --jsonl mode).
    ctx.credential_prompts = true;

    let job = args.job()?;

    if args.via_daemon && args.batch.is_none() {
        if let Some(ref at_time) = args.at {
            runner::wait_until(at_time, args.quiet)?;
        }
        let mut job = job;
        job.url = absolute_source(&job.url);
        let params = SubmitParams::from_job(&job, &std::env::current_dir()?);
        return daemon_cli::download_via_daemon(&params, args.quiet || args.jsonl);
    }

    // Scheduled start (applies to both batch and single-URL)
    if let Some(ref at_time) = args.at {
        runner::wait_until(at_time, args.quiet || args.jsonl)?;
    }

    if let Some(ref batch_file) = args.batch {
        return run_batch(batch_file, args, &job, &ctx);
    }

    runner::run_job(&job, &ctx, &args.reporter())?;
    Ok(0)
}

fn run_batch(
    batch_file: &str,
    args: &Args,
    template: &DownloadJob,
    ctx: &RunContext,
) -> Result<i32, BoxError> {
    use std::io::Read;

    let content = if batch_file == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        s
    } else {
        std::fs::read_to_string(batch_file)
            .map_err(|e| format!("Cannot read batch file '{}': {}", batch_file, e))?
    };

    let urls = runner::parse_url_list(&content);
    if urls.is_empty() {
        if !args.jsonl {
            println!("Batch file contains no URLs.");
        }
        return Ok(0);
    }

    let output_dir = PathBuf::from(utils::expand_tilde(args.output.as_deref().unwrap_or(".")));
    let (_, failed) = runner::run_urls(urls, &output_dir, args.jobs, template, ctx, args.jsonl)?;
    if ctx.cancel.load(Ordering::SeqCst) {
        return Ok(130);
    }
    Ok(if failed > 0 { 1 } else { 0 })
}

// ============================================================================
// Terminal UI
// ============================================================================

#[cfg(feature = "tui")]
fn launch_tui(config: Config, t: &TuiArgs) -> Result<i32, BoxError> {
    let mode = if t.daemon {
        tui::Mode::Daemon
    } else if t.local {
        tui::Mode::Local
    } else {
        tui::Mode::Auto
    };
    tui::run(config, mode)?;
    Ok(0)
}

#[cfg(not(feature = "tui"))]
fn launch_tui(_config: Config, _t: &TuiArgs) -> Result<i32, BoxError> {
    Err("The terminal UI was not compiled in (rebuild with --features tui)".into())
}

// ============================================================================
// GUI
// ============================================================================

#[cfg(feature = "gui")]
fn launch_gui(_args: &Args, config: Config) -> Result<i32, BoxError> {
    use kget::app::{DownloadCommand, WorkerToGuiMessage, spawn_download_worker};
    use std::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};

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

    spawn_download_worker(config, download_rx_worker, status_tx_worker);

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
            Ok(Box::new(gui::KGetGui::new(cc, download_tx, status_rx_gui)))
        }),
    ) {
        eprintln!("Failed to launch GUI: {e}");
        return Err("Failed to launch GUI".into());
    }

    Ok(0)
}

#[cfg(not(feature = "gui"))]
fn launch_gui(args: &Args, _config: Config) -> Result<i32, BoxError> {
    if args.gui {
        eprintln!("GUI support was not compiled in. Rebuild with `--features gui` to enable it.");
        return Err("GUI not available (compile with --features gui)".into());
    }
    Args::command().print_help()?;
    Ok(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("kget").chain(args.iter().copied()))
            .unwrap_or_else(|e| panic!("{args:?}: {e}"))
    }

    #[test]
    fn subcommands_coexist_with_the_url_argument() {
        let a = parse(&["https://example.com/a.iso", "-a"]);
        assert!(a.command.is_none() && a.url == "https://example.com/a.iso" && a.advanced);
        assert!(matches!(parse(&["tui"]).command, Some(Command::Tui(_))));
        assert!(matches!(
            parse(&["tui", "--daemon"]).command,
            Some(Command::Tui(TuiArgs { daemon: true, .. }))
        ));
        assert!(matches!(
            parse(&["daemon", "status"]).command,
            Some(Command::Daemon(DaemonArgs {
                action: Some(DaemonAction::Status { .. }),
                ..
            }))
        ));
        assert!(matches!(parse(&["mcp"]).command, Some(Command::Mcp(_))));
        assert!(matches!(
            parse(&[
                "queue",
                "add",
                "hf://org/repo",
                "--include",
                "*.json",
                "--tag",
                "x"
            ])
            .command,
            Some(Command::Queue { .. })
        ));
        assert!(matches!(
            parse(&["native-host"]).command,
            Some(Command::NativeHost(NativeHostArgs { action: None, .. }))
        ));
        // Chrome passes the caller origin, Firefox the manifest path + id.
        assert!(matches!(
            parse(&["native-host", "chrome-extension://abc/"]).command,
            Some(Command::NativeHost(NativeHostArgs { action: None, ref caller })) if caller.len() == 1
        ));
        assert!(matches!(
            parse(&["native-host", "install", "--browser", "firefox"]).command,
            Some(Command::NativeHost(NativeHostArgs {
                action: Some(NativeHostAction::Install { ref browser, .. }),
                ..
            })) if browser == "firefox"
        ));
        assert_eq!(
            parse(&["https://x/y", "--http-version", "1.1"]).http_version,
            Some(kget::http::HttpVersion::Http1Only)
        );
        let h3 = Args::try_parse_from(["kget", "https://x/y", "--http-version", "3"]);
        assert_eq!(h3.is_ok(), kget::http::HttpVersion::http3_available());
        assert!(Args::try_parse_from(["kget", "u", "--http-version", "9"]).is_err());
        // A local file named like a subcommand is still downloadable as ./tui.
        assert_eq!(parse(&["./tui"]).url, "./tui");
    }
}
