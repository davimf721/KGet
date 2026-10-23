# Using KGet as a Rust Library

KGet is both a desktop download manager and a reusable Rust download engine.
The library API is intended for apps, CLIs, automations, and future native
frontends that need HTTP/HTTPS, FTP, SFTP, WebDAV, magnet links, progress
callbacks, resume support, proxy support, and multi-algorithm checksum
verification.

[English](LIB.md) | [Português](translations/LIB.pt-br.md) | [Español](translations/LIB.es.md)

## Installation

```toml
[dependencies]
Kget = "1.7.1"

# Optional: built-in torrent client
Kget = { version = "1.7.1", features = ["torrent-native"] }

# Optional: async API
Kget = { version = "1.7.1", features = ["async"] }

# Library only: skip the terminal UI (the `tui` default feature), keep the keychain
Kget = { version = "1.7.1", default-features = false, features = ["keychain"] }
```

Default features are `keychain` (OS credential store) and `tui` (the `kget tui`
binary only). `http3` enables experimental HTTP/3 and needs
`RUSTFLAGS="--cfg reqwest_unstable"`.

Inside this repository, examples can use:

```toml
[dependencies]
Kget = { path = "." }
```

## Quick Start — Builder API (recommended)

```rust,no_run
use kget::KgetError;

fn main() -> Result<(), KgetError> {
    // Simple download
    kget::builder("https://example.com/file.zip")
        .output("./downloads/")
        .connections(8)
        .sha256("abc123def456...")
        .download()?;

    Ok(())
}
```

## Builder Methods

`kget::builder(url)` returns a `DownloadBuilder`. All methods are chainable:

| Method | Description |
|--------|-------------|
| `.output(path)` | Save to file or directory |
| `.connections(n)` | Parallel connections (turbo mode) |
| `.speed_limit(bps)` | Max bytes/sec (global token bucket) |
| `.proxy(url)` | HTTP or SOCKS5 proxy URL |
| `.proxy_auth(user, pass)` | Credentials for the proxy |
| `.quiet(bool)` | Suppress progress output |
| `.sha256(hash)` | Verify SHA-256 after download |
| `.sha512(hash)` | Verify SHA-512 after download |
| `.sha1(hash)` | Verify SHA-1 after download |
| `.md5(hash)` | Verify MD5 after download |
| `.blake3(hash)` | Verify BLAKE3 after download |
| `.verify_from(url)` | Download and parse a GNU/BSD sidecar checksum file |
| `.header(name, value)` | Add an HTTP header |
| `.retry(config)` | Custom retry policy (see `RetryConfig`) |
| `.range(start, end)` | Request a specific byte range |

Terminal methods:

| Method | Returns | Description |
|--------|---------|-------------|
| `.download()` | `Result<DownloadResult, KgetError>` | Download to disk |
| `.download_to_bytes()` | `Result<Vec<u8>, KgetError>` | Download into memory |
| `.download_to_reader()` | `Result<impl Read, KgetError>` | Streaming reader |
| `.spawn()` | `(JoinHandle<Result<DownloadResult, KgetError>>, Receiver<DownloadEvent>)` | Background thread with event channel |
| `.download_async()` | `impl Future<…>` | Async API (feature `async`) |

### DownloadResult

```rust
pub struct DownloadResult {
    pub path: String,
    pub bytes_downloaded: u64,
    pub avg_speed_bps: u64,
    pub duration: std::time::Duration,
    pub connections_used: usize,
    pub checksums: ComputedChecksums,
}
```

## Batch Downloads

```rust,no_run
use kget::KgetError;

let results = kget::batch([
    "https://mirror1.example.com/file.iso",
    "https://mirror2.example.com/other.tar.gz",
])
.concurrency(4)
.output_dir("./downloads/")
.download_all();

for r in results {
    match r.result {
        Ok(d) => println!("✓ {} — {:.1} MB/s avg", r.url, d.avg_speed_bps as f64 / 1e6),
        Err(e) => eprintln!("✗ {}: {}", r.url, e),
    }
}
```

`kget::batch([...])` returns a `BatchBuilder`. `.concurrency(n)` uses a Rayon
thread pool. Returns `Vec<BatchResult>`.

Async batch: `.download_all_async()` (behind `--features async`).

## Event Channel

```rust,no_run
use kget::{DownloadEvent, KgetError};

let (handle, rx) = kget::builder("https://example.com/large.iso")
    .connections(4)
    .spawn();

for event in rx {
    match event {
        DownloadEvent::Progress { percent, speed_bps, eta_secs } => {
            print!("\r{:.1}%  {:.1} MB/s  eta {}s", percent, speed_bps as f64 / 1e6, eta_secs.unwrap_or(0));
        }
        DownloadEvent::Status(msg) => eprintln!("[status] {msg}"),
        DownloadEvent::Completed { path, sha256 } => {
            println!("\nSaved to {path} (sha256: {})", sha256.unwrap_or_default());
        }
        DownloadEvent::Error(e) => eprintln!("Error: {e}"),
    }
}
let result = handle.join().expect("download thread panicked")?;
println!("{:.1} MB/s avg", result.avg_speed_bps as f64 / 1e6);
# Ok::<(), KgetError>(())
```

## Typed Errors

```rust
pub enum KgetError {
    Network(reqwest::Error),
    Io(std::io::Error),
    ChecksumMismatch { algorithm: String, expected: String, got: String },
    Protocol(String),
    Cancelled,
    NotFound(String),
    SidecarError(String),
    Other(String),
}
```

`KgetError` implements `std::error::Error` + `Display`. It has `From` impls for
`reqwest::Error`, `std::io::Error`, and `Box<dyn Error + Send + Sync>`.

Permanent errors (`Cancelled`, `NotFound`, `ChecksumMismatch`) are never
retried regardless of the `RetryConfig`.

## Checksums

```rust,no_run
use kget::checksum::{compute_checksum, ChecksumAlgorithm};

let hash = compute_checksum(std::path::Path::new("file.bin"), &ChecksumAlgorithm::Blake3)?;
println!("BLAKE3: {hash}");
# Ok::<(), Box<dyn std::error::Error>>(())
```

Supported algorithms: `Sha256`, `Sha512`, `Sha1`, `Md5`, `Blake3`.

Sidecar verification (`.sha256sum`, `.md5`, etc.):

```rust,no_run
kget::builder("https://example.com/release.tar.gz")
    .verify_from("https://example.com/release.tar.gz.sha256sum")
    .download()?;
# Ok::<(), kget::KgetError>(())
```

`.verify_from()` downloads the sidecar file, detects the algorithm by hash
length, and verifies after download. Supports GNU (`<hash>  <file>`) and BSD
(`SHA256 (file) = hash`) formats.

## Retry Configuration

```rust,no_run
use kget::RetryConfig;

kget::builder("https://example.com/file.zip")
    .retry(RetryConfig {
        max_attempts: 5,
        backoff: kget::Backoff::Exponential { base_ms: 200, max_ms: 30_000 },
        retry_on_status: vec![503, 429],
    })
    .download()?;
# Ok::<(), kget::KgetError>(())
```

Default: 3 attempts, exponential backoff starting at 500 ms, retries the
statuses in `retry_on_status` (408, 429, 5xx) and connection errors. Other HTTP
errors (404, 403, …) fail immediately. Later attempts continue the partial file.

## ResumePolicy

`AdvancedDownloader` exposes a `set_resume_policy()` method to control whether
the interactive ISO-integrity prompt is shown:

```rust,no_run
use kget::{AdvancedDownloader, ResumePolicy, Optimizer, ProxyConfig};

let mut dl = AdvancedDownloader::new(
    "https://example.com/image.iso".to_string(),
    "image.iso".to_string(),
    true,
    ProxyConfig::default(),
    Optimizer::new(),
)?;
dl.set_resume_policy(ResumePolicy::AlwaysResume); // never block on stdin
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

| Variant | Behavior |
|---------|----------|
| `Ask` (default) | Prompts via stdin in interactive terminal sessions |
| `AlwaysResume` | Skips prompt, proceeds without asking |
| `AlwaysRestart` | Skips prompt, restarts from scratch |

Library callers should always set `AlwaysResume` or `AlwaysRestart` to avoid
blocking in non-interactive contexts.

## Advanced Parallel Download

```rust,no_run
use kget::{AdvancedDownloader, Optimizer, ProxyConfig};

let mut downloader = AdvancedDownloader::new(
    "https://example.com/large.iso".to_string(),
    "large.iso".to_string(),
    false,           // quiet
    ProxyConfig::default(),
    Optimizer::new(),
)?;  // new() returns Result — propagate with ?

downloader.set_resume_policy(kget::ResumePolicy::AlwaysResume);
downloader.set_progress_callback(|progress| {
    print!("\r{:.1}%", progress * 100.0);
});
downloader.set_expected_sha256("expected_lowercase_sha256_hash");
downloader.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`AdvancedDownloader` splits the file into byte ranges and downloads them as
async tasks on a shared tokio runtime (the blocking API above is unchanged).
Idle connections split the range that would finish last (work stealing), and
HTTP/2 multiplexes ranges over a few connections. The global `TokenBucket`
throttle (if configured) enforces the aggregate speed limit across all
connections.

### HTTP version and mirrors

```rust,no_run
use kget::http::HttpVersion;
use kget::{AdvancedDownloader, Optimizer, ProxyConfig};

let mut dl = AdvancedDownloader::new(
    "https://mirror1.example.com/big.iso".to_string(),
    "big.iso".to_string(),
    true,
    ProxyConfig::default(),
    Optimizer::new(),
)?;
dl.set_resume_policy(kget::ResumePolicy::AlwaysResume);
let version: HttpVersion = "auto".parse()?;     // "1.1", "2", "3" also accepted
dl.set_http_version(version.ensure_available()?)?;
dl.set_mirrors(vec!["https://mirror2.example.com/big.iso".to_string()]);
dl.set_expected_sha256("expected_lowercase_sha256_hash"); // catches a mirror with other content
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`HttpVersion::Auto` (default) negotiates HTTP/2 through TLS ALPN;
`Http1Only` never uses HTTP/2; `Http2PriorKnowledge` speaks HTTP/2 without
negotiation (also cleartext h2c); `Http3` needs the `http3` feature built with
`--cfg reqwest_unstable` (`HttpVersion::http3_available()`), otherwise
`ensure_available()` returns an error. Mirrors with the same size and range
support each download their share of the chunks; a failing mirror is dropped.
`kget::http::ClientOptions` has a matching `http_version` field (build it with
`..Default::default()`).

## Basic Single-Stream Download

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions {
    output_path: Some("file.zip".to_string()),
    ..Default::default()
};

download(
    "https://example.com/file.zip",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    None,
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Download with Expected SHA-256

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions {
    output_path: Some("image.iso".to_string()),
    expected_sha256: Some("expected_lowercase_sha256_hash".to_string()),
    ..Default::default()
};

download(
    "https://example.com/image.iso",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    Some(&|status| println!("{status}")),
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## In-Memory Download

```rust,no_run
let bytes: Vec<u8> = kget::builder("https://example.com/data.json")
    .download_to_bytes()?;
println!("Got {} bytes", bytes.len());
# Ok::<(), kget::KgetError>(())
```

## FTP Download

```rust,no_run
use kget::ftp::FtpDownloader;
use kget::{Optimizer, ProxyConfig};

let dl = FtpDownloader::new(
    "ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz".to_string(),
    "emacs-28.2.tar.gz".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
);
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## SFTP Download

```rust,no_run
use kget::sftp::SftpDownloader;
use kget::{Optimizer, ProxyConfig};

// Password embedded in the URL
let dl = SftpDownloader::new(
    "sftp://user:pass@server.example.com/path/to/file.tar.gz".to_string(),
    "file.tar.gz".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
);
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Authentication priority:
1. Password from URL (`sftp://user:pass@host/path`)
2. Running SSH agent
3. Default key files (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`)

Host keys are verified against `~/.ssh/known_hosts`; mismatches hard-error.

## WebDAV Download

```rust,no_run
use kget::webdav::WebDavDownloader;
use kget::{Optimizer, ProxyConfig};

let dl = WebDavDownloader::new(
    "webdavs://user:pass@nas.local/backups/db.tar.gz".to_string(),
    "db.tar.gz".to_string(),
    false,
    ProxyConfig::default(),
    Optimizer::new(),
);
dl.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`WebDavDownloader` rewrites `webdav://` → `http://` and `webdavs://` →
`https://`, extracts credentials, and injects HTTP Basic `Authorization`.
Compatible with Synology, Nextcloud, Apache WebDAV, and any RFC 4918 server.

Auto-detected from URL scheme — `kget::is_webdav_url(url)` is re-exported from
`lib.rs`.

## yt-dlp Integration

```rust,no_run
use kget::ytdlp::{download_video, is_video_url, VideoEvent, VideoQuality};

let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
if is_video_url(url) {
    let saved = download_video(url, "./downloads", &VideoQuality::P720, |ev| match ev {
        VideoEvent::Progress(p) => println!("{:.1}% {} ETA {}", p.percent, p.speed, p.eta),
        VideoEvent::Saved(path) => println!("saved {path}"),
        VideoEvent::Log(_) => {}
    })?;
    println!("final file: {saved}");
}
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`VideoQuality` variants: `Best`, `P1080`, `P720`, `P480`, `P360`, `AudioOnly`
(`-x --audio-format m4a`), `Custom(format)`; `VideoQuality::parse("720p")`.
yt-dlp is found via `KGET_YTDLP_PATH`, `ytdlp_path` in `config.json`, `PATH`,
then `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin`, `~/.local/bin`.
Use `download_video_with_cancel` to stop it with an `AtomicBool`.

## Magnet Links

```rust,no_run
use kget::{download_magnet, Optimizer, ProxyConfig, TorrentCallbacks};
use std::sync::Arc;

let callbacks = TorrentCallbacks {
    status: Some(Arc::new(|message| println!("{message}"))),
    progress: Some(Arc::new(|progress| println!("{:.1}%", progress * 100.0))),
};

download_magnet(
    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
    "./downloads",
    true,
    ProxyConfig::default(),
    Optimizer::new(),
    callbacks,
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Build with `torrent-native` for the built-in torrent client. Without it, KGet
falls back to the platform's default external magnet handler.

## Metalink Downloads

```rust,no_run
use kget::metalink::download_metalink;
use kget::{Optimizer, ProxyConfig};

download_metalink(
    "ubuntu-24.04.meta4",  // local file or https:// URL
    "~/Downloads",
    false,
    ProxyConfig::default(),
    Optimizer::new(),
)?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`download_metalink` parses the RFC 5854 manifest, sorts mirrors by `priority`
(lowest first), tries each in order, and verifies the strongest hash it lists
(SHA-256, then SHA-512, then MD5) after a successful download. A corrupted
mirror (hash mismatch) is deleted and the next mirror is tried automatically.
`download_metalink_with_status` takes an extra status callback that receives a
`kget::events::verifying_status` message before each hash is computed.

Parse the manifest yourself:

```rust,no_run
use kget::metalink::{parse, is_metalink};

if is_metalink("release.meta4") {
    let doc = parse(&std::fs::read_to_string("release.meta4")?)?;
    for file in &doc.files {
        println!("{}: {} mirror(s)", file.name, file.urls.len());
        if let Some((kind, hash)) = file.best_hash() {
            println!("  hash ({kind}): {hash}");
        }
    }
}
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

## Download History

The history is a SQLite database (`history.db`, WAL mode, safe with several
`kget` processes). Search it with `HistoryQuery`:

```rust,no_run
use kget::queue::{parse_date_bound, DownloadHistory, EntryStatus, HistoryQuery, HistorySort};

let query = HistoryQuery::new()
    .status(EntryStatus::Failed)
    .domain("example.com")            // also matches subdomains
    .file_type("video")               // category, or "iso,zip"
    .since(parse_date_bound("7d", false)?)
    .search("ubuntu")
    .tag("linux")
    .sort(HistorySort::Largest)
    .limit(20);

for e in DownloadHistory::query(&query)? {
    println!("{} {} {} {:?}", e.created_at_display(), e.status, e.filename, e.tags);
}
let failed = DownloadHistory::count(&HistoryQuery::new().status(EntryStatus::Failed))?;
let removed = DownloadHistory::delete(&HistoryQuery::new().status(EntryStatus::Completed))?;
println!("{failed} failed, removed {removed} completed");
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Record entries yourself (downloads run through `kget::runner` or `JobManager`
are recorded automatically):

```rust,no_run
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};

let entry = HistoryEntry::new(
    "https://example.com/file.iso",
    "/home/user/Downloads",
    Some("expected_sha256_hex"),
);
DownloadHistory::append(entry, EntryStatus::Completed, None)?;

for e in DownloadHistory::load().recent(10) {
    println!("{} {} {}", e.created_at_display(), e.status, e.filename);
}
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`HistoryStore::open(path)` gives direct access to a database file (`insert`,
`query`, `count`, `get`, `delete`). Entries record protocol, duration, average
speed, checksum result and tags. User names and passwords are stripped from
stored URLs, the database keeps the last 10,000 entries, and a corrupt database
is moved to `history.db.corrupt`. A `history.json` from KGet ≤ 1.7 is imported
once and kept as `history.json.bak`. Set `KGET_CONFIG_DIR` (or call
`kget::config::set_config_dir_override`) to store `config.json` and
`history.db` elsewhere.

History is stored at:
- macOS: `~/Library/Application Support/kget/history.db`
- Linux: `~/.config/kget/history.db`
- Windows: `%APPDATA%\kget\history.db`

## Job Manager

`JobManager` is the download queue every frontend uses (GUI worker, daemon,
MCP server, TUI, UniFFI bindings). It runs at most `max_concurrent` jobs,
assigns `JobId`s, and broadcasts `JobEvent`s to every subscriber.

```rust,no_run
use kget::jobs::{JobEvent, JobManager};
use kget::runner::DownloadJob;
use kget::JsonEvent;
use std::time::Duration;

let manager = JobManager::with_concurrency(kget::Config::load()?, 2);
let events = manager.subscribe();

let mut job = DownloadJob::new("https://example.com/big.iso");
job.output = Some("./downloads/".into());
job.advanced = true;
let id = manager.submit(job);

for event in events {
    match event {
        JobEvent::Download { id: j, event: JsonEvent::Progress { percent: Some(p), .. } } if j == id => {
            print!("\r{p:.1}%");
        }
        JobEvent::State { id: j, state, error } if j == id && state.is_finished() => {
            println!("\n{state} {}", error.unwrap_or_default());
            break;
        }
        _ => {}
    }
}

// Other operations
manager.pause(id).ok();                       // keeps partial data
manager.resume(id).ok();                      // back to Queued
let info = manager.wait(id, Some(Duration::from_secs(60)));
println!("{:?}", info.map(|i| i.state));
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

Also: `cancel`, `remove` (finished jobs), `cancel_all`, `list`, `get`,
`running`, `set_max_concurrent`, `set_record_history`, `wait_idle`.
`DownloadState` is `Queued → Running → (Verifying) → Completed | Failed`, plus
`Paused` and `Cancelled`. `JobEvent::Download` carries the same `JsonEvent`s
`kget --jsonl` prints; `JobInfo` and `JobEvent` are `Serialize`/`Deserialize`.

## Runner (one download, any protocol)

`kget::runner::run_job` is the dispatcher behind the CLI, the REPL and
`JobManager`: protocol detection, resume, checksum verification, smart rules,
history and extraction, with output controlled by a `Reporter`.

```rust,no_run
use kget::runner::{run_job, DownloadJob, Outcome, Reporter, RunContext};
use std::sync::Arc;

let mut job = DownloadJob::new("hf://org/repo");    // any URL kget accepts
job.include = vec!["*.safetensors".into()];
job.checksum = None;
job.tags = vec!["models".into()];

let ctx = RunContext::new(kget::Config::load()?);   // ctx.cancel stops it
let reporter = Reporter::events(Arc::new(|ev: &kget::JsonEvent| println!("{}", ev.to_line())));
match run_job(&job, &ctx, &reporter)? {
    Outcome::Saved(path) => println!("saved {path:?}"),
    Outcome::HandedOff => println!("opened in another application"),
}
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`Reporter::terminal()` prints progress bars, `Reporter::quiet()` prints
nothing, `Reporter::jsonl_stdout(None)` prints JSON Lines. Build
`DownloadJob` with `DownloadJob::new` (or `..Default::default()`): new fields
are added in minor versions. Helpers: `parse_checksum_spec("sha256:…")`,
`parse_extra_headers`, `parse_url_list`, `run_urls` (batch).

## AI Models (Hugging Face, Ollama)

The builder accepts `hf://` and `ollama://` URLs:

```rust,no_run
let result = kget::builder("hf://TheBloke/Llama-2-7B-GGUF")
    .include("*Q4_K_M.gguf")
    .revision("main")
    .parallel_files(4)
    .output("./models/")
    .download()?;
println!("saved into {}", result.path);
# Ok::<(), kget::KgetError>(())
```

Or call the modules directly:

```rust,no_run
use kget::fetch::TransferOptions;
use kget::hf::{self, HfOptions};
use kget::ollama::{self, OllamaOptions};

let transfer = TransferOptions { parallel_files: 4, ..Default::default() };

let opts = HfOptions { include: vec!["*.json".into()], hf_cache: true, ..Default::default() };
let (_listing, files) = hf::list("hf://Qwen/Qwen2.5-0.5B-Instruct", &opts, &transfer)?;
println!("{} file(s) would be downloaded", files.len());
let model = hf::download("hf://Qwen/Qwen2.5-0.5B-Instruct", &opts, &transfer)?;
println!("{} files, {} skipped, {} bytes in {}", model.files, model.skipped, model.bytes, model.path.display());

let pulled = ollama::download("ollama://llama3.2:3b", &OllamaOptions::default(), &transfer)?;
println!("Ollama model in {}", pulled.path.display());
# Ok::<(), kget::KgetError>(())
```

`HfRef::parse` / `OllamaRef::parse` parse references without network access.
Hugging Face tokens come from `HfAuth::Auto` (`HF_TOKEN`,
`HUGGING_FACE_HUB_TOKEN`, the `hf auth login` file, then the keychain entry
`hf://token@huggingface.co`), `HfAuth::Token(..)` or `HfAuth::Anonymous`.
LFS files and Ollama blobs are verified against their SHA-256. Ollama models
go to `OLLAMA_MODELS` (or `~/.ollama/models`) unless `OllamaOptions::output`
is set. `KGET_HF_ENDPOINT` and `KGET_OLLAMA_REGISTRY` point them at a mirror.

## Saved Credentials (Keychain)

```rust,no_run
use kget::credentials::{self, Target};

let target = Target::from_url("sftp://alice@example.com/data.bin").unwrap();
credentials::store(&target, "s3cret")?;
assert_eq!(credentials::get(&target)?.as_deref(), Some("s3cret"));
for t in credentials::list()? {
    println!("saved: {t}");
}
credentials::delete(&target)?;
# Ok::<(), kget::credentials::CredentialError>(())
```

Entries use the service `kget` and targets like `sftp://user@host[:port]`,
`ssh-key:///path/to/key` (`Target::ssh_key`) or `hf://token@huggingface.co`.
The FTP, SFTP, WebDAV downloaders and `run_job` look passwords up
automatically through a `CredentialResolver`. Without the `keychain` feature,
or with `KGET_KEYCHAIN=off`, the functions return
`CredentialError::Unavailable`; `KGET_KEYCHAIN=mock` (or `Keychain::mock()`)
uses an in-memory store for tests.

## Daemon Client

Talk to a running `kget daemon` (or start one) from your own program:

```rust,no_run
use kget::daemon::client::DaemonClient;
use kget::daemon::protocol::{SubmitParams, SubscribeParams};

let client = DaemonClient::ensure_running()?;    // connect_default() never starts it
let mut params = SubmitParams::new("https://example.com/file.iso");
params.advanced = true;
let job = client.submit(&params)?;               // JSON job snapshot
let id = job["id"].as_u64().unwrap_or_default();

let events = client.events(&SubscribeParams { ids: vec![id], ..Default::default() })?;
for event in events {
    println!("{event}");                          // JSON JobEvent
    if event["type"] == "state" && matches!(event["state"].as_str(), Some("completed" | "failed")) {
        break;
    }
}
println!("{} job(s) in the queue", client.list()?.len());
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`client.call(method, params)` reaches every JSON-RPC method (`get`, `wait`,
`cancel`, `pause`, `resume`, `remove`, `history.query`, `config.get`, …); see
`kget::daemon::protocol`. The daemon confines outputs to its download
directory, so `SubmitParams::dir`/`filename` must stay inside it.

## Async API

Behind `--features async`:

```rust,no_run
#[tokio::main]
async fn main() -> Result<(), kget::KgetError> {
    kget::builder("https://example.com/file.zip")
        .output("./downloads/")
        .connections(4)
        .download_async()
        .await?;
    Ok(())
}
```

Both `.download_async()` and `.download_all_async()` use
`tokio::task::spawn_blocking` internally and never block the executor.

## Resume, overwrite and cancellation (1.7.1)

Downloads are written to `<file>.part` and renamed only when complete (and
verified). Turbo downloads also keep a `<file>.kget` state and resume
automatically when the remote file is unchanged; simple downloads resume with
`resume: true`.

```rust,no_run
use kget::{download_to_path, DownloadOptions, Optimizer, OverwritePolicy, ProxyConfig};
use std::sync::{Arc, atomic::AtomicBool};

let cancel = Arc::new(AtomicBool::new(false));
let options = DownloadOptions {
    output_path: Some("./downloads/".to_string()),
    resume: true,                         // continue ./downloads/<name>.part
    overwrite: OverwritePolicy::Skip,     // like --no-clobber
    retries: 5,
    timeout_secs: 30,
    cancel: Some(cancel.clone()),         // set to true from another thread
    progress_callback: Some(Arc::new(|bytes, total| {
        println!("{bytes} / {total:?}");
    })),
    ..Default::default()
};
let saved = download_to_path("https://example.com/file.zip",
    ProxyConfig::default(), Optimizer::new(), options, None)?;
println!("saved to {}", saved.display());
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

HTTP errors are returned as boxed `KgetError::Http { status, url }` /
`KgetError::NotFound`, so `KgetError::from(err)` lets you match them.
`kget::protocol::detect(url)` tells which downloader a URL needs, and
`kget::JsonEvent` is the serializable type behind `kget --jsonl`
(`"v": 1`; new events and fields are additive and keep the version, so ignore
unknown ones). `JsonEvent::Phase` announces post-transfer work —
`{"event":"phase","phase":"verifying","algorithm":"sha256"}` or
`{"event":"phase","phase":"extracting"}` — and
`JsonEvent::phase_from_status` maps the engine's hashing status message
(`kget::events::verifying_status`) to it, for your own status callbacks.
Since 1.8 the protocol also has `rule` (a smart rule matched), `duplicate`
(already downloaded, or identical content) and `torrent_stats` (peers,
speeds, ratio, seeding state, trackers); a `match` on `JsonEvent` needs a
catch-all arm.

## Library Guarantees

- Library calls never prompt through `stdin` when `ResumePolicy` is set (and never without a terminal).
- Progress and status are exposed through callbacks and event channels.
- Files are streamed to disk instead of loaded fully into memory (except `.download_to_bytes()`).
- User-given output names are validated (no separators, null bytes, `.`/`..`, Windows reserved names); server-given names are sanitized.
- Downloaded bytes are never transparently decompressed.
- A partially downloaded file never carries the final name.
- SHA256 helpers return errors on expected-hash mismatches; never silently accept corrupt files.
- Only 408/429/5xx and connection errors are retried (honouring `Retry-After`); other 4xx fail immediately.

See [examples/lib_usage.rs](examples/lib_usage.rs) for a larger cookbook.
