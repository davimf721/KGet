# Using KGet as a Rust Library

KGet is both a desktop download manager and a reusable Rust download engine.
The library API is intended for apps, CLIs, automations, and future native
frontends that need HTTP/HTTPS, FTP, SFTP, magnet links, progress callbacks,
resume support, proxy support, and SHA256 verification.

[English](LIB.md) | [Português](translations/LIB.pt-br.md) | [Español](translations/LIB.es.md)

## Installation

```toml
[dependencies]
Kget = "1.6.3"
```

Optional features:

```toml
[dependencies]
Kget = { version = "1.6.3", features = ["torrent-native"] }
Kget = { version = "1.6.3", features = ["gui"] }
```

Inside this repository, examples can use:

```toml
[dependencies]
Kget = { path = "." }
```

## Main API

- `download`: single-stream HTTP/HTTPS download with retry, streaming I/O, proxy support, safe output path handling, and optional SHA256 verification.
- `AdvancedDownloader`: resumable HTTP/HTTPS downloader using parallel byte ranges.
- `download_magnet`: magnet link downloader with native torrent support when built with `torrent-native`.
- `DownloadOptions`: quiet mode, output path, ISO verification, and expected SHA256 hash.
- `Config`, `ProxyConfig`, `Optimizer`: reusable configuration objects.
- `verify_file_sha256` and `verify_iso_integrity`: standalone checksum helpers.

## Basic Download

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    Ok(())
}
```

## Download with Expected SHA256

```rust,no_run
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions {
    output_path: Some("image.iso".to_string()),
    verify_iso: true,
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

If the calculated SHA256 does not match, KGet returns an error instead of
silently accepting the file.

## Advanced Parallel Download

```rust,no_run
use kget::{AdvancedDownloader, Optimizer, ProxyConfig};

let mut downloader = AdvancedDownloader::new(
    "https://example.com/large.iso".to_string(),
    "large.iso".to_string(),
    true,
    ProxyConfig::default(),
    Optimizer::new(),
);

downloader.set_progress_callback(|progress| {
    println!("{:.1}%", progress * 100.0);
});
downloader.set_expected_sha256("expected_lowercase_sha256_hash");
downloader.download()?;
# Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
```

`AdvancedDownloader` uses server byte ranges and refuses to write mismatched
data when a server claims range support but responds with a full `200 OK`.

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

## Library Behavior

- Library calls never prompt through `stdin`.
- Progress and status are exposed through callbacks.
- Files are streamed to disk instead of loaded fully into memory.
- Output filenames are validated against path separators.
- SHA256 helpers return errors on expected-hash mismatches.

## FTP Download

```rust,no_run
use kget::ftp::FtpDownloader;
use kget::{Optimizer, ProxyConfig};

// Anonymous FTP (no credentials required)
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

// Key-based authentication — uses SSH agent or ~/.ssh/id_ed25519, id_rsa, id_ecdsa automatically
let dl = SftpDownloader::new(
    "sftp://user@server.example.com/path/to/file.tar.gz".to_string(),
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

See [examples/lib_usage.rs](examples/lib_usage.rs) for a larger cookbook.
