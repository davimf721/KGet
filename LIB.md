# Using KGet as a Rust Library

KGet is both a desktop download manager and a reusable Rust download engine.
The library API is intended for apps, CLIs, automations, and future native
frontends that need HTTP/HTTPS, FTP, SFTP, magnet links, progress callbacks,
resume support, proxy support, and SHA256 verification.

[English](LIB.md) | [Português](translations/LIB.pt-br.md) | [Español](translations/LIB.es.md)

## Installation

```toml
[dependencies]
Kget = "1.6.1"
```

Optional features:

```toml
[dependencies]
Kget = { version = "1.6.1", features = ["torrent-native"] }
Kget = { version = "1.6.1", features = ["gui"] }
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

See [examples/lib_usage.rs](examples/lib_usage.rs) for a larger cookbook.
