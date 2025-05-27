# Using KGet as a Crate

KGet can be used as a Rust library in your own projects to add powerful download capabilities (HTTP, HTTPS, FTP, SFTP, torrents, progress, proxy, etc).

[English](README.md) | [Português](translations/LIB.pt-BR.md) | [Español](translations/LIB.es.md)

## Add to Your `Cargo.toml`

```toml
[dependencies]
kget = "1.5.0"
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