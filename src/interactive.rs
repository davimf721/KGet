//! Interactive REPL (`kget -i`).
//!
//! Every download goes through the same dispatcher as the CLI
//! (`crate::run_and_record`), so all protocols, resume, checksums, history and
//! extraction behave exactly like `kget <url>`.

use crate::{CANCEL_AWARE, Ctx, Job, Outcome};
use kget::checksum::{ChecksumAlgorithm, compute_checksum};
use kget::config::{Config, ProxyType};
use kget::protocol::{self, Protocol};
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};
use kget::utils;
use kget::ytdlp::VideoQuality;
use kget::{OverwritePolicy, http};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

type BoxError = Box<dyn Error + Send + Sync>;

// ─────────────────────────────────────────────────────────────────────────────
// ASCII Banner
// ─────────────────────────────────────────────────────────────────────────────

const BANNER: &str = r#"
  ██╗  ██╗ ██████╗ ███████╗████████╗
  ██║ ██╔╝██╔════╝ ██╔════╝╚══██╔══╝
  █████╔╝ ██║  ███╗█████╗     ██║
  ██╔═██╗ ██║   ██║██╔══╝     ██║
  ██║  ██╗╚██████╔╝███████╗   ██║
  ╚═╝  ╚═╝ ╚═════╝ ╚══════╝   ╚═╝
"#;

const COMMANDS: &[&str] = &[
    "download",
    "get",
    "dl",
    "batch",
    "info",
    "retry",
    "open",
    "reveal",
    "history",
    "config",
    "set",
    "cd",
    "pwd",
    "doctor",
    "checksum",
    "verify",
    "extract",
    "credentials",
    "clear",
    "version",
    "help",
    "exit",
    "quit",
];

const DOWNLOAD_FLAGS: &[&str] = &[
    "--advanced",
    "--turbo",
    "--no-turbo",
    "--output",
    "--quiet",
    "--continue",
    "--no-clobber",
    "--header",
    "--sha256",
    "--checksum",
    "--checksum-url",
    "--quality",
    "--retries",
    "--timeout",
    "--extract",
    "--limit",
    "--proxy",
    "--at",
    "--jobs",
    "--ftp",
    "--sftp",
    "--webdav",
    "--ytdlp",
    "--torrent",
    "--metalink",
    "--save-credentials",
];

const CONFIG_KEYS: &[&str] = &[
    "connections",
    "speed-limit",
    "ytdlp-path",
    "proxy",
    "proxy-type",
    "max-peers",
    "max-seeds",
    "torrent-port",
    "dht",
    "torrent-dir",
    "sftp-key",
];

const SET_KEYS: &[&str] = &["dir", "turbo", "quality", "retries", "timeout"];

// ─────────────────────────────────────────────────────────────────────────────
// Ctrl+C: cancel the running download, keep the REPL
// ─────────────────────────────────────────────────────────────────────────────

/// `true` while a download (or `--at` wait) is running.
static BUSY: AtomicBool = AtomicBool::new(false);
/// Ctrl+C presses during the current download.
static PRESSES: AtomicU8 = AtomicU8::new(0);

fn install_ctrlc(cancel: Arc<AtomicBool>) {
    let _ = ctrlc::set_handler(move || {
        // At the prompt rustyline reads ^C itself; this only fires mid-download.
        if !BUSY.load(Ordering::SeqCst) {
            return;
        }
        let n = PRESSES.fetch_add(1, Ordering::SeqCst);
        if n == 0 && CANCEL_AWARE.load(Ordering::SeqCst) {
            cancel.store(true, Ordering::SeqCst);
            eprintln!("\nStopping… partial data is kept (press Ctrl+C again to quit KGet).");
        } else if n == 0 {
            eprintln!("\nThis transfer can't be paused cleanly. Press Ctrl+C again to quit KGet.");
        } else {
            std::process::exit(130);
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Session state
// ─────────────────────────────────────────────────────────────────────────────

struct Session {
    config: Config,
    cancel: Arc<AtomicBool>,
    /// Default output directory for this session (`cd`)
    dir: Option<String>,
    turbo: bool,
    quality: String,
    retries: u32,
    timeout: u64,
    last_path: Option<PathBuf>,
}

impl Session {
    fn new(config: Config) -> Self {
        Self {
            config,
            cancel: Arc::new(AtomicBool::new(false)),
            dir: None,
            turbo: false,
            quality: "best".into(),
            retries: http::DEFAULT_RETRIES,
            timeout: http::DEFAULT_TIMEOUT_SECS,
            last_path: None,
        }
    }

    fn prompt(&self) -> String {
        match &self.dir {
            Some(d) => format!("kget {}> ", shorten_home(d)),
            None => "kget> ".into(),
        }
    }
}

fn shorten_home(p: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home = home.to_string_lossy();
        if let Some(rest) = p.strip_prefix(home.as_ref()) {
            return format!("~{rest}");
        }
    }
    p.to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Download arguments
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct DownloadArgs {
    urls: Vec<String>,
    output: Option<String>,
    quiet: bool,
    advanced: Option<bool>,
    forced: Option<Protocol>,
    sha256: Option<String>,
    checksum: Option<String>,
    checksum_url: Option<String>,
    headers: Vec<String>,
    resume: bool,
    no_clobber: bool,
    quality: Option<String>,
    retries: Option<u32>,
    timeout: Option<u64>,
    extract: bool,
    limit: Option<u64>,
    proxy: Option<String>,
    at: Option<String>,
    jobs: usize,
    save_credentials: bool,
}

fn take_value<'a>(parts: &[&'a str], i: &mut usize, flag: &str) -> Result<&'a str, String> {
    *i += 1;
    parts
        .get(*i)
        .copied()
        .ok_or_else(|| format!("'{flag}' requires a value"))
}

fn parse_num<T: std::str::FromStr>(v: &str, flag: &str) -> Result<T, String> {
    v.parse()
        .map_err(|_| format!("'{flag}' expects a number, got '{v}'"))
}

/// Parse the flags of `download` / `batch` (see `help download`).
fn parse_download_args(parts: &[&str]) -> Result<DownloadArgs, String> {
    let mut args = DownloadArgs {
        jobs: 4,
        ..Default::default()
    };

    let mut i = 0;
    while i < parts.len() {
        let flag = parts[i];
        match flag {
            "-a" | "--advanced" | "--turbo" => args.advanced = Some(true),
            "--no-turbo" => args.advanced = Some(false),
            "-q" | "--quiet" => args.quiet = true,
            "-c" | "--continue" => args.resume = true,
            "--no-clobber" => args.no_clobber = true,
            "--extract" => args.extract = true,
            "--save-credentials" => args.save_credentials = true,
            "--ftp" => args.forced = Some(Protocol::Ftp),
            "--sftp" => args.forced = Some(Protocol::Sftp),
            "--webdav" => args.forced = Some(Protocol::WebDav),
            "--torrent" => args.forced = Some(Protocol::Torrent),
            "--metalink" => args.forced = Some(Protocol::Metalink),
            "--ytdlp" => args.forced = Some(Protocol::Video),
            "-o" | "-O" | "--output" => {
                args.output = Some(utils::expand_tilde(take_value(parts, &mut i, flag)?))
            }
            "-H" | "--header" => args.headers.push(take_value(parts, &mut i, flag)?.into()),
            "--sha256" => args.sha256 = Some(take_value(parts, &mut i, flag)?.into()),
            "--checksum" => args.checksum = Some(take_value(parts, &mut i, flag)?.into()),
            "--checksum-url" => args.checksum_url = Some(take_value(parts, &mut i, flag)?.into()),
            "--quality" => args.quality = Some(take_value(parts, &mut i, flag)?.into()),
            "--retries" => args.retries = Some(parse_num(take_value(parts, &mut i, flag)?, flag)?),
            "--timeout" => args.timeout = Some(parse_num(take_value(parts, &mut i, flag)?, flag)?),
            "-l" | "--limit" => {
                args.limit = Some(parse_num(take_value(parts, &mut i, flag)?, flag)?)
            }
            "-p" | "--proxy" => args.proxy = Some(take_value(parts, &mut i, flag)?.into()),
            "--at" => args.at = Some(take_value(parts, &mut i, flag)?.into()),
            "-j" | "--jobs" => args.jobs = parse_num(take_value(parts, &mut i, flag)?, flag)?,
            token if !token.starts_with('-') => args.urls.push(token.to_string()),
            unknown => {
                return Err(format!(
                    "Unknown flag: '{unknown}'. Run 'help download' for usage."
                ));
            }
        }
        i += 1;
    }

    if args.resume && args.no_clobber {
        return Err("'--continue' and '--no-clobber' cannot be used together".into());
    }
    Ok(args)
}

// ─────────────────────────────────────────────────────────────────────────────
// Running downloads
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for one run: saved config + per-download overrides.
fn effective_config(s: &Session, a: &DownloadArgs) -> Config {
    let mut config = s.config.clone();
    if let Some(l) = a.limit {
        config.optimization.speed_limit = (l > 0).then_some(l);
    }
    if let Some(p) = &a.proxy {
        config.proxy.enabled = true;
        config.proxy.url = Some(p.clone());
        config.proxy.proxy_type = ProxyType::from_url(p);
    }
    kget::credentials::fill_proxy_password(&mut config.proxy);
    config
}

/// Relative `-o` paths are resolved against the session's `cd` directory.
fn session_path(s: &Session, path: &str) -> String {
    match &s.dir {
        Some(dir) if Path::new(path).is_relative() => {
            let joined = Path::new(dir).join(path).to_string_lossy().into_owned();
            // Keep a trailing separator: it marks the path as a directory.
            if path.ends_with('/') || path.ends_with(std::path::MAIN_SEPARATOR) {
                format!("{joined}{}", std::path::MAIN_SEPARATOR)
            } else {
                joined
            }
        }
        _ => path.to_string(),
    }
}

/// A user-typed local path: `~` expanded, relative paths under the `cd` directory.
fn local_path(s: &Session, path: &str) -> PathBuf {
    PathBuf::from(session_path(s, &utils::expand_tilde(path)))
}

fn build_job(s: &Session, a: &DownloadArgs, url: &str) -> Result<Job, BoxError> {
    let mut job = Job::new(url);
    job.output = match &a.output {
        Some(o) => Some(session_path(s, o)),
        None => s.dir.clone().map(|d| format!("{d}/")),
    };
    job.quiet = a.quiet;
    job.advanced = a.advanced.unwrap_or(s.turbo);
    job.forced = a.forced;
    job.sha256 = a.sha256.clone();
    job.checksum = match &a.checksum {
        Some(spec) => Some(crate::parse_checksum_spec(spec)?),
        None => None,
    };
    job.checksum_url = a.checksum_url.clone();
    job.headers = crate::parse_extra_headers(&a.headers);
    job.quality = VideoQuality::parse(a.quality.as_deref().unwrap_or(&s.quality));
    job.resume = a.resume;
    job.overwrite = if a.no_clobber {
        OverwritePolicy::Skip
    } else {
        OverwritePolicy::Overwrite
    };
    job.retries = a.retries.unwrap_or(s.retries).max(1);
    job.timeout = a.timeout.unwrap_or(s.timeout);
    job.extract = a.extract;
    job.save_credentials = a.save_credentials;
    Ok(job)
}

/// Run `f` with Ctrl+C wired to the session's cancel token.
fn busy<T>(s: &Session, f: impl FnOnce() -> T) -> T {
    s.cancel.store(false, Ordering::SeqCst);
    PRESSES.store(0, Ordering::SeqCst);
    BUSY.store(true, Ordering::SeqCst);
    let r = f();
    BUSY.store(false, Ordering::SeqCst);
    r
}

fn run_download(s: &mut Session, a: DownloadArgs) -> Result<(), BoxError> {
    if a.urls.is_empty() {
        return Err("No URL provided. Usage: download [flags] <url> [url…]".into());
    }

    if let Some(at) = &a.at {
        CANCEL_AWARE.store(false, Ordering::SeqCst);
        busy(s, || crate::wait_until(at, a.quiet))?;
    }

    let ctx = Ctx {
        config: effective_config(s, &a),
        cancel: s.cancel.clone(),
    };

    if a.urls.len() > 1 {
        let dir = match &a.output {
            Some(o) => session_path(s, o),
            None => s.dir.clone().unwrap_or_else(|| ".".into()),
        };
        let template = build_job(s, &a, &a.urls[0])?;
        let (_, failed) = busy(s, || {
            crate::run_urls(
                a.urls.clone(),
                Path::new(&dir),
                a.jobs,
                &template,
                &ctx,
                false,
            )
        })?;
        if failed > 0 {
            return Err(format!("{failed} download(s) failed").into());
        }
        return Ok(());
    }

    let job = build_job(s, &a, &a.urls[0])?;
    let result = busy(s, || crate::run_and_record(&job, &ctx));
    match result {
        Ok(Outcome::Saved(Some(path))) => {
            println!("✓ Saved to {}", path.display());
            s.last_path = Some(path);
            Ok(())
        }
        Ok(Outcome::Saved(None)) => {
            println!("✓ Done");
            Ok(())
        }
        Ok(Outcome::HandedOff) => {
            println!("→ Opened in your torrent client");
            Ok(())
        }
        Err(e) if s.cancel.load(Ordering::SeqCst) => {
            let _ = e;
            println!(
                "⏸ Download stopped. Run 'retry last' (or the same command with -c) to resume."
            );
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn run_batch(s: &mut Session, parts: &[&str]) -> Result<(), BoxError> {
    let mut a = parse_download_args(parts)?;
    let mut urls = Vec::new();
    for item in std::mem::take(&mut a.urls) {
        if item.contains("://") || item.starts_with("magnet:?") {
            urls.push(item);
        } else {
            let path = local_path(s, &item);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("Cannot read batch file '{}': {e}", path.display()))?;
            urls.extend(
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_string),
            );
        }
    }
    if urls.is_empty() {
        return Err("No URLs. Usage: batch <file|url…> [-j N] [-o dir] [flags]".into());
    }
    a.urls = urls;
    let ctx = Ctx {
        config: effective_config(s, &a),
        cancel: s.cancel.clone(),
    };
    let dir = match &a.output {
        Some(o) => session_path(s, o),
        None => s.dir.clone().unwrap_or_else(|| ".".into()),
    };
    let template = build_job(s, &a, &a.urls[0])?;
    let (_, failed) = busy(s, || {
        crate::run_urls(
            a.urls.clone(),
            Path::new(&dir),
            a.jobs,
            &template,
            &ctx,
            false,
        )
    })?;
    if failed > 0 {
        return Err(format!("{failed} download(s) failed").into());
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// info
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_info(s: &Session, url: Option<&str>) -> Result<(), BoxError> {
    let url = url.ok_or("Usage: info <url>")?;
    let proto = protocol::detect(url);
    println!("Protocol      {proto}");
    match proto {
        Protocol::Http => {}
        Protocol::Video => {
            match kget::ytdlp_binary() {
                Some(p) => println!("yt-dlp        {p}"),
                None => println!("yt-dlp        NOT FOUND (brew install yt-dlp)"),
            }
            return Ok(());
        }
        _ => {
            println!(
                "File name     {}",
                utils::get_filename_from_url_or_default(url, "download")
            );
            return Ok(());
        }
    }

    let client = http::build_client(&s.config.proxy, &http::ClientOptions::with_timeout_secs(20))?;
    let mut resp = client.head(url).send()?;
    if !resp.status().is_success() {
        resp = client
            .get(url)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()?;
    }
    let h = |name: reqwest::header::HeaderName| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let size = h(reqwest::header::CONTENT_RANGE)
        .and_then(|r| r.rsplit_once('/').and_then(|(_, t)| t.parse::<u64>().ok()))
        .or_else(|| h(reqwest::header::CONTENT_LENGTH).and_then(|l| l.parse().ok()));
    let name = h(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|d| kget::download::parse_content_disposition_filename(&d))
        .map(|n| utils::sanitize_filename(&n))
        .unwrap_or_else(|| {
            utils::get_filename_from_url_or_default(resp.url().as_str(), "download")
        });
    let resumable = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT
        || h(reqwest::header::ACCEPT_RANGES).is_some_and(|v| v.eq_ignore_ascii_case("bytes"));

    println!("Status        {}", resp.status());
    if resp.url().as_str() != url {
        println!("Final URL     {}", resp.url());
    }
    println!("File name     {name}");
    println!(
        "Size          {}",
        size.map(|b| format!(
            "{} ({b} bytes)",
            humansize::format_size(b, humansize::BINARY)
        ))
        .unwrap_or_else(|| "unknown".into())
    );
    if let Some(t) = h(reqwest::header::CONTENT_TYPE) {
        println!("Type          {t}");
    }
    println!(
        "Resumable     {}",
        if resumable {
            "yes (turbo mode can use parallel connections)"
        } else {
            "no (single connection)"
        }
    );
    if let Some(e) = h(reqwest::header::ETAG) {
        println!("ETag          {e}");
    }
    if let Some(m) = h(reqwest::header::LAST_MODIFIED) {
        println!("Modified      {m}");
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// History, retry, open
// ─────────────────────────────────────────────────────────────────────────────

fn print_history(entries: &[&HistoryEntry]) {
    if entries.is_empty() {
        println!("No downloads in history.");
        return;
    }
    println!(
        "{:<4} {:<17} {:<12} {:>10}  File",
        "#", "Date (UTC)", "Status", "Size"
    );
    println!("{}", "─".repeat(78));
    for (i, e) in entries.iter().enumerate() {
        let size = e
            .bytes_total
            .map(|b| humansize::format_size(b, humansize::BINARY))
            .unwrap_or_default();
        let date = e.created_at_display();
        let date = date.trim_end_matches(" UTC");
        println!(
            "{:<4} {:<17} {:<12} {:>10}  {}",
            i + 1,
            date,
            e.status,
            size,
            e.filename
        );
        if let Some(err) = &e.error {
            println!("     └ {}", truncate(err, 70));
        }
    }
    println!("\nUse 'retry <#>' to download again, 'open <#>' to open a file.");
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn cmd_history(parts: &[&str]) -> Result<(), BoxError> {
    let history = DownloadHistory::load();
    match parts.first().copied() {
        Some("clear") => {
            let scope = parts.get(1).copied().unwrap_or("all");
            let mut removed = 0usize;
            DownloadHistory::update(|entries| {
                let before = entries.len();
                match scope {
                    "completed" | "done" => entries.retain(|e| e.status == EntryStatus::Failed),
                    "failed" => entries.retain(|e| e.status != EntryStatus::Failed),
                    _ => entries.clear(),
                }
                removed = before - entries.len();
            })?;
            println!("Removed {removed} history entries.");
        }
        Some("failed") => {
            let all = history.recent(usize::MAX);
            let v: Vec<_> = all
                .into_iter()
                .filter(|e| e.status == EntryStatus::Failed)
                .take(50)
                .collect();
            print_history(&v);
        }
        Some("completed") => {
            let all = history.recent(usize::MAX);
            let v: Vec<_> = all
                .into_iter()
                .filter(|e| e.status == EntryStatus::Completed)
                .take(50)
                .collect();
            print_history(&v);
        }
        Some("search") => {
            let q = parts[1..].join(" ").to_lowercase();
            if q.is_empty() {
                return Err("Usage: history search <text>".into());
            }
            let all = history.recent(usize::MAX);
            let v: Vec<_> = all
                .into_iter()
                .filter(|e| {
                    e.filename.to_lowercase().contains(&q) || e.url.to_lowercase().contains(&q)
                })
                .take(50)
                .collect();
            print_history(&v);
        }
        Some(n) if n.parse::<usize>().is_ok() => {
            print_history(&history.recent(n.parse().unwrap_or(20)));
        }
        Some(other) => {
            return Err(format!(
                "Unknown history option '{other}'. Try: history [N|failed|completed|search <text>|clear [completed|failed]]"
            )
            .into());
        }
        None => print_history(&history.recent(20)),
    }
    Ok(())
}

/// Resolve `last`, `#3` / `3` (position in `history`) or an id prefix.
fn find_entry(reference: &str) -> Option<HistoryEntry> {
    let history = DownloadHistory::load();
    let recent = history.recent(usize::MAX);
    let r = reference.trim_start_matches('#');
    if r == "last" {
        return recent.first().map(|e| (*e).clone());
    }
    if let Ok(n) = r.parse::<usize>()
        && n >= 1
    {
        return recent.get(n - 1).map(|e| (*e).clone());
    }
    recent.into_iter().find(|e| e.id.starts_with(r)).cloned()
}

fn cmd_retry(s: &mut Session, parts: &[&str]) -> Result<(), BoxError> {
    let reference = parts.first().copied().unwrap_or("last");
    let entry = find_entry(reference).ok_or_else(|| format!("No history entry '{reference}'"))?;
    println!("Retrying {} …", entry.url);
    let mut a = parse_download_args(&parts[parts.len().min(1)..])?;
    a.urls = vec![entry.url.clone()];
    if a.output.is_none() {
        let target = Path::new(&entry.output_dir).join(&entry.filename);
        a.output = Some(if entry.output_dir.is_empty() || entry.output_dir == "." {
            entry.filename.clone()
        } else {
            target.to_string_lossy().into_owned()
        });
    }
    if a.sha256.is_none() {
        a.sha256 = entry.expected_sha256.clone();
    }
    // Continue a partial download when there is one.
    a.resume = !a.no_clobber;
    run_download(s, a)
}

fn entry_path(e: &HistoryEntry) -> PathBuf {
    let dir = if e.output_dir.is_empty() {
        "."
    } else {
        &e.output_dir
    };
    Path::new(dir).join(&e.filename)
}

fn resolve_target(s: &Session, reference: Option<&str>) -> Result<PathBuf, BoxError> {
    match reference {
        None | Some("last") => s
            .last_path
            .clone()
            .or_else(|| find_entry("last").map(|e| entry_path(&e)))
            .ok_or_else(|| "Nothing downloaded yet".into()),
        Some(r) => {
            let p = local_path(s, r);
            if p.exists() {
                return Ok(p);
            }
            find_entry(r)
                .map(|e| entry_path(&e))
                .ok_or_else(|| format!("No file or history entry '{r}'").into())
        }
    }
}

fn open_with_system(path: &Path, reveal: bool) -> Result<(), BoxError> {
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()).into());
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if reveal {
            cmd.arg("-R");
        }
        cmd.arg(path).spawn()?;
    }
    #[cfg(windows)]
    {
        if reveal {
            use std::os::windows::process::CommandExt;
            // explorer parses its own command line; quote the path ourselves.
            std::process::Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", path.display()))
                .spawn()?;
        } else {
            std::process::Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(path)
                .spawn()?;
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = if reveal {
            path.parent().unwrap_or(path).to_path_buf()
        } else {
            path.to_path_buf()
        };
        std::process::Command::new("xdg-open").arg(target).spawn()?;
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Local file tools
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_checksum(s: &Session, parts: &[&str]) -> Result<(), BoxError> {
    let file = parts
        .first()
        .ok_or("Usage: checksum <file> [sha256|sha512|sha1|md5|blake3]")?;
    let path = local_path(s, file);
    let algos: Vec<ChecksumAlgorithm> = match parts.get(1) {
        Some(&"all") => vec![
            ChecksumAlgorithm::Sha256,
            ChecksumAlgorithm::Sha512,
            ChecksumAlgorithm::Sha1,
            ChecksumAlgorithm::Md5,
            ChecksumAlgorithm::Blake3,
        ],
        Some(name) => vec![
            ChecksumAlgorithm::parse(name).ok_or_else(|| format!("Unknown algorithm '{name}'"))?,
        ],
        None => vec![ChecksumAlgorithm::Sha256],
    };
    for algo in algos {
        println!("{:<7} {}", algo.name(), compute_checksum(&path, &algo)?);
    }
    Ok(())
}

fn cmd_verify(s: &Session, parts: &[&str]) -> Result<(), BoxError> {
    let (file, spec) = match parts {
        [f, s, ..] => (*f, *s),
        _ => return Err("Usage: verify <file> <alg:hex | hex>".into()),
    };
    let path = local_path(s, file);
    let (algo, want) = crate::parse_checksum_spec(spec)?;
    let got = compute_checksum(&path, &algo)?;
    if got == want {
        println!("✓ {} OK  {}", algo.name(), got);
        Ok(())
    } else {
        Err(format!(
            "✗ {} mismatch\n  expected {want}\n  got      {got}",
            algo.name()
        )
        .into())
    }
}

fn cmd_extract(s: &Session, parts: &[&str]) -> Result<(), BoxError> {
    let path = resolve_target(s, parts.first().copied())?;
    if !utils::is_extractable(&path) {
        return Err(format!("{} is not a supported archive", path.display()).into());
    }
    kget::auto_extract(&path, false)
}

// ─────────────────────────────────────────────────────────────────────────────
// set / cd (session only) and config (saved)
// ─────────────────────────────────────────────────────────────────────────────

fn on_off(v: &str) -> Result<bool, BoxError> {
    match v.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(format!("'{v}' is not on/off").into()),
    }
}

fn cmd_cd(s: &mut Session, target: Option<&str>) -> Result<(), BoxError> {
    match target {
        None | Some("-") => {
            s.dir = None;
            println!("Downloads go to the current directory again.");
        }
        Some(d) => {
            let path = local_path(s, d);
            std::fs::create_dir_all(&path)?;
            let abs = std::fs::canonicalize(&path).unwrap_or(path);
            s.dir = Some(abs.to_string_lossy().into_owned());
            println!("Downloads will be saved to {}", abs.display());
        }
    }
    Ok(())
}

fn cmd_set(s: &mut Session, parts: &[&str]) -> Result<(), BoxError> {
    let (key, value) = match parts {
        [] => {
            println!("Session settings (not saved; see 'config' for saved ones):");
            println!(
                "  dir       {}",
                s.dir
                    .as_deref()
                    .map(shorten_home)
                    .unwrap_or_else(|| "(current directory)".into())
            );
            println!("  turbo     {}", if s.turbo { "on" } else { "off" });
            println!("  quality   {}", s.quality);
            println!("  retries   {}", s.retries);
            println!("  timeout   {}s", s.timeout);
            return Ok(());
        }
        [k, v, ..] => (*k, *v),
        [k] => return Err(format!("Usage: set {k} <value>").into()),
    };
    match key {
        "dir" => cmd_cd(s, Some(value))?,
        "turbo" => {
            s.turbo = on_off(value)?;
            println!(
                "Turbo mode {} for this session",
                if s.turbo { "on" } else { "off" }
            );
        }
        "quality" => {
            s.quality = value.into();
            println!("Video quality: {value}");
        }
        "retries" => {
            s.retries = parse_num::<u32>(value, "retries")?.max(1);
            println!("Retries: {}", s.retries);
        }
        "timeout" => {
            s.timeout = parse_num(value, "timeout")?;
            println!("Timeout: {}s", s.timeout);
        }
        other => {
            return Err(format!("Unknown setting '{other}'. Keys: {}", SET_KEYS.join(", ")).into());
        }
    }
    Ok(())
}

fn show_config(config: &Config) {
    println!(
        "Saved configuration ({}):",
        kget::config::Config::path()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    );
    println!("  connections   {}", config.optimization.max_connections);
    println!(
        "  speed-limit   {}",
        config
            .optimization
            .speed_limit
            .map(|s| format!("{s} B/s"))
            .unwrap_or_else(|| "unlimited".into())
    );
    println!(
        "  proxy         {}",
        if config.proxy.enabled {
            format!(
                "{} ({:?})",
                config.proxy.url.as_deref().unwrap_or("(no URL)"),
                config.proxy.proxy_type
            )
        } else {
            "off".into()
        }
    );
    println!(
        "  ytdlp-path    {}",
        config.ytdlp_path.as_deref().unwrap_or("auto-detect")
    );
    println!("  max-peers     {}", config.torrent.max_peers);
    println!("  max-seeds     {}", config.torrent.max_seeds);
    println!(
        "  torrent-port  {}",
        config
            .torrent
            .port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "auto".into())
    );
    println!(
        "  dht           {}",
        if config.torrent.dht_enabled {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "  torrent-dir   {}",
        config
            .torrent
            .download_dir
            .as_deref()
            .unwrap_or("./torrent_output")
    );
    println!(
        "  sftp-key      {}",
        config
            .sftp
            .key_path
            .as_deref()
            .unwrap_or("~/.ssh/id_* (default)")
    );
}

fn cmd_config(parts: &[&str], config: &mut Config) -> Result<(), BoxError> {
    match parts.first().copied().unwrap_or("show") {
        "show" => show_config(config),
        "path" => println!("{}", Config::path()?.display()),
        "reset" => {
            if parts.get(1) != Some(&"--yes") {
                println!(
                    "This replaces {} with the defaults. Run 'config reset --yes' to confirm.",
                    Config::path()?.display()
                );
                return Ok(());
            }
            *config = Config::default();
            config.save()?;
            println!("Configuration reset to defaults.");
        }
        "set" => {
            let (key, value) = match parts {
                [_, k, v, ..] => (*k, *v),
                _ => {
                    println!("Usage: config set <key> <value>");
                    println!("  keys: {}", CONFIG_KEYS.join(", "));
                    return Ok(());
                }
            };
            match key {
                "connections" => {
                    config.optimization.max_connections =
                        parse_num::<usize>(value, key)?.clamp(1, 32)
                }
                "speed-limit" => {
                    config.optimization.speed_limit = match value {
                        "0" | "unlimited" | "off" => None,
                        v => Some(parse_num(v, key)?),
                    }
                }
                "ytdlp-path" => {
                    config.ytdlp_path =
                        (!matches!(value, "auto" | "")).then(|| utils::expand_tilde(value))
                }
                "proxy" => {
                    if matches!(value, "off" | "none") {
                        config.proxy.enabled = false;
                    } else {
                        config.proxy.enabled = true;
                        config.proxy.url = Some(value.into());
                        config.proxy.proxy_type = ProxyType::from_url(value);
                    }
                }
                "proxy-type" => config.proxy.proxy_type = ProxyType::parse(value),
                "max-peers" => config.torrent.max_peers = parse_num(value, key)?,
                "max-seeds" => config.torrent.max_seeds = parse_num(value, key)?,
                "torrent-port" => {
                    config.torrent.port = match value {
                        "auto" => None,
                        v => Some(parse_num(v, key)?),
                    }
                }
                "dht" => config.torrent.dht_enabled = on_off(value)?,
                "torrent-dir" => {
                    config.torrent.download_dir =
                        (value != "default").then(|| utils::expand_tilde(value))
                }
                "sftp-key" => {
                    config.sftp.key_path =
                        (!matches!(value, "none" | "default")).then(|| utils::expand_tilde(value))
                }
                "compression" | "cache" => {
                    println!("'{key}' was removed in 1.7.1 (it never affected downloads).");
                    return Ok(());
                }
                unknown => {
                    return Err(format!(
                        "Unknown config key '{unknown}'. Keys: {}",
                        CONFIG_KEYS.join(", ")
                    )
                    .into());
                }
            }
            config.save()?;
            println!("Saved: {key} = {value}");
        }
        other => {
            return Err(format!(
                "Unknown config command '{other}'. Try: config [show|set|path|reset]"
            )
            .into());
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Help
// ─────────────────────────────────────────────────────────────────────────────

fn print_help(topic: Option<&str>) {
    match topic {
        Some("download") | Some("get") | Some("dl") => println!(
            r#"
download [flags] <url> [url…]      (aliases: get, dl — or just paste a URL)

  Protocol is detected from the URL: http(s), ftp, sftp, webdav, magnet/.torrent,
  .meta4/.metalink and video sites (yt-dlp).

  -a, --turbo / --no-turbo  Parallel connections on/off (default: 'set turbo')
  -o, --output <path>       File or directory (default: 'cd' directory)
  -c, --continue            Resume <file>.part
      --no-clobber          Skip if the file already exists
  -q, --quiet               No progress output
  -H, --header "N: v"       Extra HTTP header (repeatable)
      --sha256 <hash>       Verify SHA-256
      --checksum <alg:hex>  Verify sha512/sha1/md5/blake3…
      --checksum-url <url>  Verify against SHA256SUMS / file.sha256
      --extract             Unzip / untar after download
  -l, --limit <bytes/s>     Speed limit for this download only
  -p, --proxy <url>         Proxy for this download only (http/https/socks5)
      --retries <n>         Attempts per request
      --timeout <secs>      Connect / read timeout
      --quality <q>         yt-dlp: best, 1080p, 720p, 480p, 360p, audio
      --at HH:MM            Start at a local time
      --save-credentials    Save the password used (URL or prompt) in the keychain
  -j, --jobs <n>            Parallel downloads when several URLs are given
      --ftp --sftp --webdav --ytdlp --torrent --metalink   Force a protocol

  Ctrl+C stops the download and keeps partial data ('retry last' resumes).

  sftp://user@host/…, ftp://user@host/… and webdav(s)://user@host/… use the
  password saved in the system keychain, or ask for it ('help credentials').
"#
        ),
        Some("credentials") | Some("keychain") => {
            println!("\n{}\n", kget::credentials::COMMAND_USAGE)
        }
        Some("history") | Some("retry") | Some("open") => println!(
            r#"
history                       Last 20 downloads (numbered #1 = newest)
history <N>                   Last N downloads
history failed | completed    Filter by status
history search <text>         Search file names and URLs
history clear [completed|failed]

retry [#|id|last] [flags]     Download a history entry again (resumes .part)
open [#|id|last|path]         Open a downloaded file
reveal [#|id|last|path]       Show it in Finder / Explorer / file manager
"#
        ),
        Some("config") | Some("set") => println!(
            r#"
config                        Show saved configuration
config set <key> <value>      Save a setting:
    connections <1-32>        speed-limit <bytes/s|0>     ytdlp-path <path|auto>
    proxy <url|off>           proxy-type <http|https|socks5>
    max-peers <n>             max-seeds <n>               torrent-port <n|auto>
    dht <on|off>              torrent-dir <path|default>  sftp-key <path|none>
config path | reset --yes

set                           Show session settings (not saved)
set turbo on|off  ·  set quality 720p  ·  set retries 5  ·  set timeout 30
cd <dir>   (= set dir)        Default download folder for this session; 'cd -' resets
pwd                           Show it
"#
        ),
        _ => println!(
            r#"
Downloads
  download [flags] <url…>   Download (or just paste a URL)       help download
  batch <file|url…> [-j N]  Download a list, N at a time
  info <url>                Size, type, file name, resume support
  retry [#|last]            Download a history entry again       help history
  open / reveal [#|last]    Open a downloaded file / show its folder

History & settings
  history [N|failed|search] Recent downloads
  config [set <k> <v>]      Saved settings                       help config
  credentials [list|delete] Passwords in the system keychain     help credentials
  set [key value]           Session settings (turbo, quality, retries, timeout)
  cd <dir> / pwd            Session download folder

Files
  checksum <file> [alg|all] Hash a file
  verify <file> <alg:hex>   Check a file against a hash
  extract [file|last]       Unpack an archive

Other
  doctor   clear   version   help [topic]   exit

Tips: Tab completes commands, flags and paths · ↑/↓ browse previous commands
      Ctrl+C stops a running download · quote paths with spaces: -o "~/My Files/x.iso"

Examples
  https://example.com/file.zip
  download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/…
  download --checksum-url https://host/SHA256SUMS https://host/image.iso
  batch urls.txt -j 8 -o ~/Downloads
  cd ~/Downloads
  set turbo on
  https://www.youtube.com/watch?v=…
  history failed
  retry 2
"#
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Line editor helper: completion
// ─────────────────────────────────────────────────────────────────────────────

struct ReplHelper {
    files: FilenameCompleter,
}

fn word_candidates(words: &[&str], prefix: &str) -> Vec<Pair> {
    words
        .iter()
        .filter(|w| w.starts_with(prefix))
        .map(|w| Pair {
            display: (*w).to_string(),
            replacement: format!("{w} "),
        })
        .collect()
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let before = &line[..pos];
        let start = before
            .rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &before[start..];
        let words: Vec<&str> = before[..start].split_whitespace().collect();

        let candidates = match words.as_slice() {
            [] => word_candidates(COMMANDS, word),
            ["config"] => word_candidates(&["show", "set", "path", "reset"], word),
            ["config", "set"] => word_candidates(CONFIG_KEYS, word),
            ["set"] => word_candidates(SET_KEYS, word),
            ["help"] => word_candidates(&["download", "history", "config", "credentials"], word),
            ["credentials"] => word_candidates(&["list", "add", "delete", "status"], word),
            ["history"] => word_candidates(&["failed", "completed", "search", "clear"], word),
            ["history", "clear"] => word_candidates(&["completed", "failed"], word),
            ["checksum", _] => {
                word_candidates(&["sha256", "sha512", "sha1", "md5", "blake3", "all"], word)
            }
            [cmd, ..]
                if word.starts_with('-')
                    && matches!(*cmd, "download" | "get" | "dl" | "batch" | "retry") =>
            {
                word_candidates(DOWNLOAD_FLAGS, word)
            }
            _ => return self.files.complete(line, pos, ctx),
        };
        Ok((start, candidates))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
}
impl Highlighter for ReplHelper {}
impl Validator for ReplHelper {}
impl Helper for ReplHelper {}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

fn looks_like_url(s: &str) -> bool {
    s.contains("://") || s.starts_with("magnet:?")
}

/// Execute one REPL line. Returns `false` to exit.
fn execute(s: &mut Session, parts: &[&str]) -> Result<bool, BoxError> {
    let cmd = parts[0];
    let rest = &parts[1..];

    // URLs pasted on their own (one or several) start a download.
    if looks_like_url(cmd) {
        run_download(s, parse_download_args(parts)?)?;
        return Ok(true);
    }

    match cmd {
        "exit" | "quit" | "q" => return Ok(false),
        "help" | "?" => print_help(rest.first().copied()),
        "version" => println!("KGet v{}", env!("CARGO_PKG_VERSION")),
        "clear" | "cls" => {
            print!("\x1B[2J\x1B[1;1H");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        "download" | "get" | "dl" => run_download(s, parse_download_args(rest)?)?,
        "batch" => run_batch(s, rest)?,
        "info" => cmd_info(s, rest.first().copied())?,
        "retry" => cmd_retry(s, rest)?,
        "open" => open_with_system(&resolve_target(s, rest.first().copied())?, false)?,
        "reveal" | "show" => open_with_system(&resolve_target(s, rest.first().copied())?, true)?,
        "history" => cmd_history(rest)?,
        "config" => cmd_config(rest, &mut s.config)?,
        "set" => cmd_set(s, rest)?,
        "cd" => cmd_cd(s, rest.first().copied())?,
        "pwd" => println!(
            "{}",
            s.dir.clone().unwrap_or_else(|| std::env::current_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_default())
        ),
        "doctor" => crate::print_doctor(&s.config),
        "checksum" | "hash" => cmd_checksum(s, rest)?,
        "verify" => cmd_verify(s, rest)?,
        "extract" => cmd_extract(s, rest)?,
        "credentials" | "keychain" => println!("{}", kget::credentials::command(rest)?),
        other => {
            let suggestion = COMMANDS
                .iter()
                .find(|c| c.starts_with(&other[..other.len().min(2)]))
                .map(|c| format!(" Did you mean '{c}'?"))
                .unwrap_or_default();
            return Err(format!("Unknown command '{other}'.{suggestion} Type 'help'.").into());
        }
    }
    Ok(true)
}

pub fn interactive_mode() {
    println!("{}", BANNER);
    println!("  Fast, Multi-Protocol Download Manager");
    println!("  v{}  —  Interactive Mode", env!("CARGO_PKG_VERSION"));
    println!("  Type 'help' for commands, Tab to complete, 'exit' to quit.\n");

    let config = Config::load().unwrap_or_else(|e| {
        eprintln!("Warning: could not load config ({}). Using defaults.", e);
        Config::default()
    });
    let mut session = Session::new(config);

    let mut rl: Editor<ReplHelper, DefaultHistory> = match Editor::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to initialise line editor: {}", e);
            return;
        }
    };
    rl.set_helper(Some(ReplHelper {
        files: FilenameCompleter::new(),
    }));
    // After Editor::new(): on a terminal rustyline installs its own SIGINT
    // handler, which would otherwise swallow Ctrl+C during downloads.
    install_ctrlc(session.cancel.clone());
    let history_file = kget::config::config_dir()
        .ok()
        .map(|d| d.join("repl_history.txt"));
    if let Some(f) = &history_file {
        let _ = rl.load_history(f);
    }

    loop {
        let prompt = session.prompt();
        match rl.readline(&prompt) {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(input);

                let owned: Vec<String> = match shlex::split(input) {
                    Some(p) if !p.is_empty() => p,
                    Some(_) => continue,
                    None => {
                        eprintln!("Error: unbalanced quotes");
                        continue;
                    }
                };
                let parts: Vec<&str> = owned.iter().map(String::as_str).collect();
                match execute(&mut session, &parts) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => eprintln!("✗ {e}"),
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("(Ctrl+C) Type 'exit' or press Ctrl+D to quit.");
            }
            Err(ReadlineError::Eof) => break,
            Err(err) => {
                eprintln!("Readline error: {:?}", err);
                break;
            }
        }
    }

    if let Some(f) = &history_file {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = rl.save_history(f);
    }
    println!("Goodbye!");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_download_flags() {
        let a = parse_download_args(&[
            "-a",
            "--no-clobber",
            "-l",
            "1000",
            "--checksum",
            "md5:abc",
            "-j",
            "2",
            "u1",
            "u2",
        ])
        .unwrap();
        assert_eq!(a.advanced, Some(true));
        assert!(a.no_clobber);
        assert_eq!(a.limit, Some(1000));
        assert_eq!(a.jobs, 2);
        assert_eq!(a.urls, vec!["u1", "u2"]);
        assert!(parse_download_args(&["-c", "--no-clobber", "u"]).is_err());
        assert!(parse_download_args(&["--bogus"]).is_err());
        assert!(parse_download_args(&["--retries", "x"]).is_err());
    }

    #[test]
    fn relative_output_uses_session_dir() {
        let mut s = Session::new(Config::default());
        assert_eq!(session_path(&s, "x.bin"), "x.bin");
        s.dir = Some("/data".into());
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(session_path(&s, "x.bin"), format!("/data{sep}x.bin"));
        assert!(session_path(&s, "sub/").ends_with(sep));
        let abs = std::env::temp_dir().join("y.bin");
        let abs = abs.to_string_lossy();
        assert_eq!(session_path(&s, &abs), abs);
    }

    #[test]
    fn completes_commands_and_keys() {
        let c = word_candidates(COMMANDS, "his");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].replacement, "history ");
        assert!(word_candidates(CONFIG_KEYS, "tor").len() >= 2);
    }
}
