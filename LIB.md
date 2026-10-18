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
```

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
| `.spawn()` | `Result<(JoinHandle, Receiver<DownloadEvent>), KgetError>` | Background thread with event channel |
| `.download_async()` | `impl Future<…>` | Async API (feature `async`) |

### DownloadResult

```rust
pub struct DownloadResult {
    pub path: String,
    pub bytes_downloaded: u64,
    pub avg_speed_bps: f64,
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
        Ok(d) => println!("✓ {} — {:.1} MB/s avg", r.url, d.avg_speed_bps / 1e6),
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
    .spawn()?;

for event in rx {
    match event {
        DownloadEvent::Progress { percent, speed_bps, eta_secs } => {
            print!("\r{:.1}%  {:.1} MB/s  eta {}s", percent, speed_bps / 1e6, eta_secs.unwrap_or(0));
        }
        DownloadEvent::Status(msg) => eprintln!("[status] {msg}"),
        DownloadEvent::Completed { path, bytes, avg_speed_bps, .. } => {
            println!("\nSaved {bytes} bytes to {path}  ({:.1} MB/s avg)", avg_speed_bps / 1e6);
        }
        DownloadEvent::Error(e) => eprintln!("Error: {e}"),
    }
}
handle.join().ok();
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

let hash = compute_checksum(ChecksumAlgorithm::Blake3, std::path::Path::new("file.bin"))?;
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

`AdvancedDownloader` splits the file into byte ranges and downloads them in
parallel via rayon. The global `TokenBucket` throttle (if configured) enforces
the aggregate speed limit across all threads.

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
(lowest first), tries each in order, and verifies SHA-256 after a successful
download. A corrupted mirror (hash mismatch) is deleted and the next mirror is
tried automatically.

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

```rust,no_run
use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};

let entry = HistoryEntry::new(
    "https://example.com/file.iso",
    "/home/user/Downloads",
    Some("expected_sha256_hex"),
);
// Atomic and safe with several kget processes writing at once
DownloadHistory::append(entry, EntryStatus::Completed, None)?;

for e in DownloadHistory::load().recent(10) {
    println!("{} {} {}", e.created_at_display(), e.status, e.filename);
}

// Read-modify-write under the history lock
DownloadHistory::update(|entries| entries.retain(|e| e.status == EntryStatus::Failed))?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

User names and passwords are stripped from stored URLs, the file keeps the last
1000 entries, and a corrupt file is moved to `history.json.corrupt`.
Set `KGET_CONFIG_DIR` to store `config.json` and `history.json` elsewhere.

History is stored at:
- macOS: `~/Library/Application Support/kget/history.json`
- Linux: `~/.config/kget/history.json`
- Windows: `%APPDATA%\kget\history.json`

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
`kget::JsonEvent` is the serializable type behind `kget --jsonl`.

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
