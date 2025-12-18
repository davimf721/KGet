# Using KGet as a Crate

KGet can be used as a Rust library in your own projects to add powerful download capabilities (HTTP, HTTPS, FTP, SFTP, torrents, progress, proxy, etc).

[English](README.md) | [Português](translations/LIB.pt-br.md) | [Español](translations/LIB.es.md)

## Add to Your `Cargo.toml`

Without GUI (recommended for servers/CI/minimal builds):

```toml
[dependencies]
kget = "1.5.1"
```

With GUI enabled (this pulls in optional GUI dependencies):

```toml
[dependencies]
kget = { version = "1.5.1", features = ["gui"] }
```

For local development (when working inside the repository), use a path dependency:

```toml
[dependencies]
kget = { path = "." }
```
## Basic Usage

```rust
use kget::KGet;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let kget = KGet::new()?;
    kget.download(
        "https://example.com/file.zip",
        Some("file.zip".to_string()),
        false, // quiet_mode
    )?;
    Ok(())
}
```

## Convenience top-level functions

The crate also exposes simple top-level functions so you can call them
directly without creating a `KGet` instance:

- `kget::download(url, output_path, quiet_mode)` — regular HTTP/HTTPS/FTP/SFTP download.
- `kget::advanced_download(url, output_path, quiet_mode)` — parallel/resumable download.

Example using the top-level `download` function:

```rust
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    kget::download("https://example.com/file.txt", Some("file.txt"), false)?;
    Ok(())
}
```

## Progress bar API

If you want to render a progress bar yourself (e.g., integrate with your own UI),
the crate exposes a progress bar factory:

```rust
let bar = kget::create_progress_bar_factory(false, "Downloading".to_string(), Some(1024u64), false);
// use `bar` as an `indicatif::ProgressBar`
```

The `examples/lib_usage.rs` file demonstrates a minimal usage scenario.

## GUI feature (optional)

The GUI is optional and provided behind a Cargo feature. Build or run with the GUI enabled using:

```bash
cargo build --features gui
cargo run --features gui -- --gui
```

When the `gui` feature is disabled the crate and binary compile without GUI-related dependencies.


## Advanced Download (Parallel Chunks, Resume)

```rust
use kget::KGet;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let kget = KGet::new()?;
    kget.advanced_download(
        "https://example.com/largefile.iso",
        Some("largefile.iso".to_string()),
        false,
    )?;
    Ok(())
}
```

## Custom Configuration

```rust
use kget::{KGet, Config};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut config = Config::load()?;
    config.optimization.speed_limit = Some(1024 * 1024); // 1 MB/s
    let kget = KGet::with_config(config);
    kget.download("https://example.com/file.zip", None, false)?;
    Ok(())
}
```

## Simple API

For quick downloads without creating a KGet instance:

```rust
use kget::simple;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    simple::download("https://example.com/file.txt", Some("file.txt"))?;
    Ok(())
}
```

## Progress Callback Example

```rust
use kget::{KGet, DownloadOptions};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options = DownloadOptions {
        quiet_mode: false,
        progress_callback: Some(Box::new(|current, total, _speed| {
            println!("Progress: {}/{}", current, total);
        })),
        ..Default::default()
    };
    kget::simple::download_with_options(
        "https://example.com/file.txt",
        Some("file.txt"),
        options,
    )?;
    Ok(())
}
```

## Supported Protocols

- HTTP/HTTPS
- FTP
- SFTP
- Magnet links (torrents, requires `transmission-daemon`)

## More

See [docs.rs/kget](https://docs.rs/kget) for full API documentation.

---