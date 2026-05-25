<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.7.0

**A fast, modern download manager written in Rust.**  
HTTP/HTTPS · FTP/SFTP · WebDAV · BitTorrent · Metalink · yt-dlp · CLI · GUI · Library

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

---

## Why KGet?

Most download tools are either too simple (single-connection wget-clones) or too bloated (full Electron apps). KGet is a **Rust-native download engine** that gives you:

- **Turbo multi-connection downloads** — splits files into parallel byte ranges (up to 32× speed on fast connections)
- **Full protocol coverage** — HTTP/HTTPS, FTP/SFTP, WebDAV, magnet links, and yt-dlp in one binary
- **Three frontends, one engine** — native macOS SwiftUI app, cross-platform egui GUI (Linux/Windows), and a polished CLI
- **A reusable Rust library** — embed the download engine in your own app with a fluent builder API, typed errors, and event channels

---

## Features

### Download Protocols
| Protocol | Flag | Notes |
|----------|------|-------|
| HTTP / HTTPS | *(auto)* | Multi-connection, resume, gzip/brotli/lz4, proxy |
| FTP | `--ftp` | Authenticated or anonymous |
| SFTP | `--sftp` | Password or key-based; host-key verification |
| WebDAV | `--webdav` or `webdav://` | HTTP Basic auth embedded in URL |
| Magnet / BitTorrent | *(auto-detected)* | Built-in torrent client (`torrent-native` feature) |
| Metalink `.meta4` | `--metalink` | Multi-mirror fallback, SHA-256 verified (RFC 5854) |
| Video sites | `--ytdlp` or *(auto-detected)* | YouTube, Vimeo, Twitch, TikTok, Instagram… via yt-dlp |

### Download Engine
- **Turbo mode** (`-a`) — parallel byte-range connections, resumable after interruption
- **Batch download** (`--batch urls.txt`) — one URL per line, `#` = comment, all run in parallel
- **Download scheduling** (`--at "HH:MM"`) — sleep until a specific local wall-clock time
- **Speed limiting** (`-l <bytes/s>`) — global token-bucket throttle across all parallel threads
- **Custom HTTP headers** (`-H "Name: Value"`) — inject arbitrary headers into any request
- **Auto-extract archives** (`--extract`) — unzip/tar/7z after download (`.zip`, `.tar.gz`, `.7z`, …)
- **SHA-256 verification** (`--sha256 <hash>`) — hard-error on mismatch; never silently accepts corrupt files
- **Sidecar checksum files** — verifies against GNU/BSD `.sha256sum` files
- **Content-Disposition** — uses server-suggested filenames automatically
- **Filename safety** — rejects null bytes, path traversal, Windows reserved names, and >255-byte filenames

### Integrity & Security
- **Multi-algorithm checksums** — SHA-256, SHA-512, SHA-1, MD5, BLAKE3
- **SFTP host-key verification** — checks `~/.ssh/known_hosts`; hard-errors on mismatch
- **Retry policy** — retries on 5xx and network errors only; fails immediately on 4xx
- **JSONL events** (`--jsonl`) — machine-readable progress for scripts and agents

### History & Persistence
- **Download history** — every download recorded to `history.json`; `--history` / `--history-clear`
- **Interactive REPL** (`--interactive`) — full command history, all protocols, live config editing

### GUIs
- **Native macOS app** — SwiftUI, `NavigationSplitView` sidebar, clipboard monitor, drag-and-drop URLs, speed sparkline, history tab, Share Extension, menu bar
- **Cross-platform egui GUI** (`--gui`) — Apple-inspired design, system-adaptive dark/light, sidebar navigation, shimmer progress bar

### Library
- **Fluent builder API** — `kget::builder(url).connections(8).sha256("…").download()?`
- **Typed errors** — `KgetError` enum with `From` impls for `reqwest::Error`, `io::Error`
- **Event channel** — `.spawn()` returns `(JoinHandle, Receiver<DownloadEvent>)`
- **Async API** — `.download_async()` / `.download_all_async()` behind `--features async`
- **In-memory download** — `.download_to_bytes()` and `.download_to_reader()`
- **Batch builder** — `kget::batch([…]).concurrency(4).download_all()`

---

## Screenshots

| macOS App | CLI |
|-----------|-----|
| <img src="https://github.com/user-attachments/assets/d5603a9c-f1f7-46b9-bdc5-0072d21f96cb" width="400"/> | <img src="https://github.com/user-attachments/assets/a835c4df-5424-4aaa-b687-2445a99ba067" width="400"/> |

---

## Installation

### Homebrew (macOS / Linux)

```bash
brew tap davimf721/kget
brew install kget                           # CLI only
brew install kget --with-gui                # with egui graphical interface
brew install kget --with-torrent            # with native BitTorrent client
brew install kget --with-gui --with-torrent # all optional features
```

### Pre-built binaries

Download the latest release from [Releases](https://github.com/davimf721/KGet/releases):
- **macOS** — `KGet-1.7.0-macOS-Native.dmg` (native SwiftUI app, no Rust needed)
- **Linux/Windows** — CLI binary or GUI binary (see release assets)

### From crates.io

```bash
cargo install Kget --features gui   # with egui GUI
cargo install Kget                  # CLI only
```

### From source

```bash
# Rust toolchain: https://rustup.rs

# Linux dependencies (Debian/Ubuntu)
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
                    libxkbcommon-dev libssl-dev pkg-config

git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release --features gui
./target/release/kget --gui
```

---

## Usage

### Basic downloads

```bash
# HTTP/HTTPS
kget https://example.com/file.zip

# Save to a specific location
kget -O ~/Downloads/myfile.zip https://example.com/file.zip

# Turbo mode — parallel connections, resumable
kget -a https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso

# Quiet mode
kget -q https://example.com/file.zip
```

### Protocols

```bash
# FTP (anonymous or authenticated)
kget --ftp ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz
kget --ftp ftp://user:pass@server/file.zip

# SFTP (password or key-based)
kget --sftp sftp://user:pass@server/path/to/file.dat
kget --sftp sftp://user@server/path/to/file.dat

# WebDAV (auto-detected from scheme)
kget webdav://files.myserver.com/share/report.pdf
kget webdavs://user:pass@nas.local/backups/db.tar.gz

# Magnet link (auto-detected)
kget "magnet:?xt=urn:btih:HASH&dn=filename"

# Metalink — tries mirrors in priority order, verifies SHA-256
kget --metalink ubuntu-24.04.meta4
kget https://releases.ubuntu.com/ubuntu.meta4
```

### Video downloads

```bash
# Auto-detected from URL host
kget https://www.youtube.com/watch?v=dQw4w9WgXcQ

# Explicit flag with quality
kget --ytdlp --quality 1080p https://vimeo.com/123456
kget --ytdlp --quality audio https://www.youtube.com/watch?v=dQw4w9WgXcQ

# Qualities: best, 1080p, 720p, 480p, 360p, audio
```

### Batch & scheduling

```bash
# Batch — one URL per line, # = comment
kget --batch urls.txt -O ~/Downloads/

# Schedule for tonight at 11pm
kget --at "23:00" -a https://example.com/large-file.iso
```

### Checksums & verification

```bash
# Verify against expected SHA-256
kget --sha256 abc123def456... https://example.com/file.iso

# Auto-extract after download
kget --extract https://example.com/archive.tar.gz

# Custom headers (useful for authenticated APIs)
kget -H "Authorization: Bearer token123" -H "Accept: application/json" https://api.example.com/export
```

### History

```bash
kget --history                    # list last 50 downloads
kget --history-clear              # remove all entries
kget --history-clear completed    # remove only completed/cancelled
```

### Interactive REPL

```bash
kget --interactive
```

```
kget> download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/...
kget> download --sftp sftp://user@server/backups/db.sql.gz
kget> download --ytdlp --quality 720p https://youtube.com/watch?v=...
kget> config set connections 8
kget> config set speed-limit 1048576
kget> history
kget> help
```

### JSONL events (for scripts and agents)

```bash
kget --jsonl -a https://example.com/file.iso | jq '.percent'
```

### All CLI flags

| Flag | Description |
|------|-------------|
| `-a, --advanced` | Turbo mode — parallel connections, resumable |
| `-O <path>` | Output file or directory |
| `-q, --quiet` | Minimal output |
| `-p <proxy>` | HTTP/SOCKS5 proxy |
| `-l <bytes/s>` | Speed limit in bytes per second |
| `-H "Name: Value"` | Extra HTTP header (repeatable) |
| `--sha256 <hash>` | Verify SHA-256 after download |
| `--extract` | Auto-extract archives after download |
| `--at "HH:MM"` | Schedule download for a specific local time |
| `--batch <file>` | Download all URLs from a file |
| `--ftp` | Use FTP protocol |
| `--sftp` | Use SFTP protocol |
| `--webdav` | Use WebDAV protocol |
| `--ytdlp` | Route through yt-dlp (auto-detected for video sites) |
| `--quality <q>` | yt-dlp quality: `best`, `1080p`, `720p`, `480p`, `360p`, `audio` |
| `--metalink` | Download from a Metalink manifest |
| `--history` | Show download history |
| `--history-clear [completed]` | Clear history |
| `--jsonl` | Emit JSON Lines events to stdout |
| `--gui` | Launch egui graphical interface |
| `-i, --interactive` | Interactive REPL mode |

---

## Library Usage

KGet is also a reusable Rust library. Add it to your project:

```toml
[dependencies]
Kget = "1.7.0"

# Optional: torrent client
Kget = { version = "1.7.0", features = ["torrent-native"] }

# Optional: async API
Kget = { version = "1.7.0", features = ["async"] }
```

### Builder API (recommended)

```rust
use kget::KgetError;

// Simple download
kget::builder("https://example.com/file.zip")
    .output("./downloads/")
    .connections(8)
    .sha256("abc123...")
    .download()?;

// Parallel batch with event channel
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

// Event channel
let (handle, rx) = kget::builder("https://example.com/large.iso")
    .connections(4)
    .spawn()?;

for event in rx {
    match event {
        kget::DownloadEvent::Progress { percent, .. } => print!("\r{:.1}%", percent),
        kget::DownloadEvent::Completed { path, .. } => println!("\nSaved to {}", path),
        kget::DownloadEvent::Error(e) => eprintln!("Error: {}", e),
        _ => {}
    }
}
handle.join().ok();
```

See [LIB.md](LIB.md) for the complete library reference.

---

## Building

```bash
# CLI only (no GUI)
cargo build --release

# With egui GUI (Linux/Windows/macOS)
cargo build --release --features gui

# With native torrent client
cargo build --release --features torrent-native

# All features
cargo build --release --features gui,torrent-native,torrent-transmission

# Native macOS app + DMG (requires Xcode)
./build-native-macos.sh

# Cross-compile Linux/Windows from macOS (requires zig)
brew install zig && cargo install cargo-zigbuild
./build-cross.sh
```

## Testing

```bash
cargo test                          # all tests
cargo test --lib --test unit_tests  # unit tests only
cargo test --test mock_server_tests # HTTP mock server tests
./run-tests.sh                      # full suite with lint and format check
```

---

## Platform Support

| Platform | CLI | egui GUI | Native App |
|----------|-----|----------|------------|
| macOS | ✅ | ✅ | ✅ SwiftUI DMG |
| Linux | ✅ | ✅ | — |
| Windows | ✅ | ✅ | — |

---

## Links

- [Changelog](CHANGELOG.md)
- [Library Reference (LIB.md)](LIB.md)
- [Architecture](docs/ARCHITECTURE.md)
- [crates.io](https://crates.io/crates/Kget)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)

## Community

- [Dev.to](https://dev.to/davimf7221/kelpsget-v014-modern-download-manager-in-rust-4b9f)
- [r/rust](https://www.reddit.com/r/rust/comments/1kt69vh/after_5_months_of_development_i_finally_released/)
- [PitchHut](https://www.pitchhut.com/project/kelpsget)
- [Hacker News](https://hn.algolia.com/?query=Show%20HN%3A%20KelpsGet%20%E2%80%93%20Modern%20download%20manager%20built%20in%20Rust&type=story&dateRange=all&sort=byDate&storyText=false&prefix&page=0)

## License

MIT — see [LICENSE](LICENSE)
