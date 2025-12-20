# Using KGet as a Library

KGet is a versatile Rust crate that provides a high-performance download engine. It is designed to be integrated into other projects while maintaining the same efficiency (RAM/Disk optimization) as the CLI app.

[English](LIB.md) | [Português](translations/LIB.pt-br.md) | [Español](translations/LIB.es.md)

## Installation

Add KGet to your `Cargo.toml`:

```toml
[dependencies]
kget = { version = "1.5.1", features = ["gui"] }
```

For local development (when working inside the repository), use a path dependency:

```toml
[dependencies]
kget = { path = "." }
```
## Key Components
The library exposes the following building blocks:
- download: The standard function for single-stream HTTP/HTTPS/FTP/SFTP downloads.
- AdvancedDownloader: A struct for multi-threaded, parallel chunk downloads with automatic RAM/Disk I/O optimization.
- DownloadOptions: A struct to control library behavior (Quiet mode, Output path, Auto-verification).
- create_progress_bar: A factory function to create KGet-styled progress bars (green, smooth, with ETA).
- verify_iso_integrity: A standalone SHA256 checksum calculator.

## Complete Usage Guide
For a live demonstration of all features, see the [Cookbook Example](src/lib_usage.rs).

## Quick Example: Standard Download

```rust
use kget::{download, DownloadOptions, Config, Optimizer};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::default();
    let options = DownloadOptions {
        output_path: Some("file.zip".to_string()),
        ..Default::default()
    };

    download("https://example.com/file.zip", config.proxy, Optimizer::new(config.optimization), options)?;
    Ok(())
}
```
## Quick Example: Parallel Download
```rust
use kget::{AdvancedDownloader, Config, Optimizer};

let config = Config::default();
let downloader = AdvancedDownloader::new(
    "https://example.com/large.iso".into(),
    "large.iso".into(),
    false, // quiet_mode
    config.proxy,
    Optimizer::new(config.optimization)
);

downloader.download()?;
```
## Interactivity vs Library Behavior
The core library functions never use `stdin` or prompt the user. All decisions are made via the `DownloadOptions` struct:
- In the CLI App: We prompt the user "Do you want to verify?"
- In your Library code: You decide programmatically by setting `verify_iso`: `true` or `false`.

## Efficiency by Design
When using KGet as a library, you automatically benefit from:
1. Streaming I/O: 16KB read-write cycles to keep RAM usage low (~30MB).
2. Buffered Writes: 2MB BufWriter per thread to protect disk health and prevent system freezes.
3. Smart Detection: ISO files are automatically handled as raw binary to prevent corruption.


For more details, check the source of `src/lib.rs`.
## Supported Protocols

- HTTP/HTTPS
- FTP
- SFTP
- Magnet links (torrents, requires `transmission-daemon`)

## More

See [docs.rs/kget](https://docs.rs/kget) for full API documentation.

---