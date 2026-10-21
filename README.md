<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.7.1

**A fast, modern download manager written in Rust.**  
HTTP/HTTPS · FTP/SFTP · WebDAV · BitTorrent · Metalink · yt-dlp · CLI · GUI · Library

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

[![CI](https://github.com/davimf721/KGet/actions/workflows/ci.yml/badge.svg)](https://github.com/davimf721/KGet/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/Kget.svg)](https://crates.io/crates/Kget)
[![docs.rs](https://docs.rs/Kget/badge.svg)](https://docs.rs/Kget)

> **1.7.1** is a reliability release: turbo downloads can no longer finish with a
> corrupted file, every download resumes after Ctrl+C, CLI flags no longer leak
> into `config.json`, proxies cover HTTPS/SOCKS5, and the macOS app finds yt-dlp
> installed with Homebrew. See the [CHANGELOG](CHANGELOG.md).

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
| HTTP / HTTPS | *(auto)* | Multi-connection, resume, HTTP/HTTPS/SOCKS5 proxy, byte-exact files |
| FTP | `ftp://` *(auto)* or `--ftp` | Authenticated or anonymous, resume (`REST`) |
| SFTP | `sftp://` *(auto)* or `--sftp` | Password or key-based; host-key verification; resume |
| WebDAV | `webdav://` *(auto)* or `--webdav` | HTTP Basic auth embedded in URL |
| Magnet / BitTorrent | *(auto-detected)* | Magnet links and `.torrent` files/URLs (`torrent-native` feature) |
| Metalink `.meta4` / `.metalink` | *(auto)* or `--metalink` | v3 + v4 (RFC 5854), multi-mirror, SHA-256/512/MD5 verified |
| Video sites | `--ytdlp` or *(auto-detected)* | YouTube, Vimeo, Twitch, TikTok, Instagram… via yt-dlp |

### Download Engine
- **Turbo mode** (`-a`) — parallel byte-range connections; every chunk is validated and retried from its exact offset
- **Safe resume** — data goes to `<file>.part` (plus a `<file>.kget` state for turbo mode); press Ctrl+C at any time and run the same command again (`-c` for simple downloads)
- **Overwrite control** — `--no-clobber` skips existing files; `--retries N` and `--timeout SECS` tune the network behaviour
- **Batch download** (`--batch urls.txt`, or `--batch -` for stdin) — `-j/--jobs N` parallel downloads, unique file names
- **Download scheduling** (`--at "HH:MM"`) — sleep until a specific local wall-clock time
- **Speed limiting** (`-l <bytes/s>`) — global token-bucket throttle across all parallel threads
- **Custom HTTP headers** (`-H "Name: Value"`) — inject arbitrary headers into any request
- **Auto-extract archives** (`--extract`) — unzip/tar/7z after download (`.zip`, `.tar.gz`, `.7z`, …)
- **SHA-256 verification** (`--sha256 <hash>`) — hard-error on mismatch; never silently accepts corrupt files
- **Any checksum** (`--checksum sha512:<hex>`) and **sidecar files** (`--checksum-url …/SHA256SUMS`) — GNU, BSD and hash-only formats
- **Content-Disposition** — uses server-suggested filenames automatically
- **Filename safety** — server-provided names are sanitized (no traversal, control or reserved characters); `%20` is decoded

### Integrity & Security
- **Multi-algorithm checksums** — SHA-256, SHA-512, SHA-1, MD5, BLAKE3
- **SFTP host-key verification** — checks `~/.ssh/known_hosts`; hard-errors on mismatch
- **Retry policy** — retries 408/429/5xx and network errors with backoff (honours `Retry-After`); fails immediately on other 4xx
- **JSONL events** (`--jsonl`) — versioned (`"v":1`) machine-readable events for every protocol
- **Diagnostics** (`--doctor`) — config path, yt-dlp location, extract tools, compiled features

### History & Persistence
- **Download history** — every download recorded in a SQLite `history.db` (multi-process safe, credentials stripped; `history.json` is migrated); `--history` with filters (`--status`, `--domain`, `--type`, `--protocol`, `--since`, `--until`, `--search`, `--sort`, `--json`) / `--history-clear`
- **Configuration** — flags apply only to the current run; `--save-config` persists them (proxy passwords are never written). `KGET_CONFIG_DIR` overrides the config directory
- **Interactive REPL** (`--interactive`) — every CLI download option, batch, retry, checksums, Tab completion, persistent history, Ctrl+C to pause

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

| macOS App | CLI | Interactive |
|-----------|-----|-------------|
| <img src="https://github.com/user-attachments/assets/d5603a9c-f1f7-46b9-bdc5-0072d21f96cb" width="400"/> | <img src="https://github.com/user-attachments/assets/084ed2b1-9663-429b-8719-890a16bd545c" width="400"/> |<img src="https://github.com/user-attachments/assets/b49bb298-fd53-41f0-99fc-f02ae811f348" width="400"/>|

---
<img width="909" height="618" alt="image" src="https://github.com/user-attachments/assets/084ed2b1-9663-429b-8719-890a16bd545c" />


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
- **macOS** — `KGet-1.7.1-macOS-Native.dmg` (native SwiftUI app, no Rust needed)
- **macOS CLI** — `kget-1.7.1-macos-arm64` (Apple Silicon) / `kget-1.7.1-macos-x64` (Intel)
- **Linux** — `kget-1.7.1-linux-x64`, `kget-1.7.1-linux-x64-static` (musl), `kget-1.7.1-linux-arm64`
- **Windows** — `kget-1.7.1-windows-x64.exe` / `.zip`
- Verify with `SHA256SUMS-1.7.1.txt`

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

# Interrupted? Run it again — turbo resumes automatically, -c continues a simple download
kget -c https://example.com/big.iso

# Don't overwrite, more retries, shorter timeout
kget --no-clobber --retries 5 --timeout 20 https://example.com/file.zip

# Verify with a checksum file published next to the download
kget --checksum-url https://releases.ubuntu.com/24.04/SHA256SUMS https://releases.ubuntu.com/24.04/ubuntu-24.04-desktop-amd64.iso

# Check your environment (paths, yt-dlp, tools, features)
kget --doctor
```

### Protocols

```bash
# FTP (anonymous or authenticated) — detected from the scheme
kget ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz
kget ftp://user:pass@server/file.zip

# SFTP (password or key-based); /~/ is relative to your home directory
kget sftp://user:pass@server/path/to/file.dat
kget sftp://user@server/~/backups/file.dat

# WebDAV (auto-detected from scheme)
kget webdav://files.myserver.com/share/report.pdf
kget webdavs://user:pass@nas.local/backups/db.tar.gz

# Magnet link or .torrent file/URL (auto-detected)
kget "magnet:?xt=urn:btih:HASH&dn=filename"
kget ~/Downloads/debian.iso.torrent

# Metalink (v3 or v4) — tries mirrors in priority order, verifies size and hash
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

yt-dlp is looked up in `PATH`, then in `/opt/homebrew/bin`, `/usr/local/bin`,
`/opt/local/bin` and `~/.local/bin` (so the macOS app finds Homebrew installs
when opened from the Dock). Set `KGET_YTDLP_PATH=/path/to/yt-dlp` to use a
specific binary.

### Batch & scheduling

```bash
# Batch — one URL per line, # = comment; 8 at a time
kget --batch urls.txt -O ~/Downloads/ -j 8
cat urls.txt | kget --batch - -O ~/Downloads/

# Schedule for tonight at 11pm
kget --at "23:00" -a https://example.com/large-file.iso
```

### Checksums & verification

```bash
# Verify against expected SHA-256 (or any algorithm)
kget --sha256 abc123def456... https://example.com/file.iso
kget --checksum sha512:9f86d08… https://example.com/file.iso
kget --checksum-url https://example.com/file.iso.sha256 https://example.com/file.iso

# Auto-extract after download
kget --extract https://example.com/archive.tar.gz

# Custom headers (useful for authenticated APIs)
kget -H "Authorization: Bearer token123" -H "Accept: application/json" https://api.example.com/export
```

### History

```bash
kget --history                    # list last 50 downloads
kget --history-clear              # remove all entries
kget --history-clear completed    # keep only failures
kget --history-clear failed       # remove only failures
kget --history --status failed --domain example.com --since 7d
kget --history --type video --sort largest --history-limit 10
kget --history --json             # JSON array (with --jsonl: one object per line)
```

### Interactive REPL

```bash
kget --interactive
```

```
kget> https://example.com/file.zip          # pasting a URL downloads it
kget> cd ~/Downloads                        # where this session saves files
kget> set turbo on                          # session defaults: turbo, quality, retries, timeout
kget> download -a -o "ubuntu.iso" --checksum-url https://host/SHA256SUMS https://releases.ubuntu.com/...
kget> download -l 1048576 -p socks5://127.0.0.1:1080 https://example.com/big.iso   # this download only
kget> download --ytdlp --quality 720p https://youtube.com/watch?v=...
kget> batch urls.txt -j 8                   # parallel downloads from a file
kget> info https://example.com/file.zip     # size, type, resumable? (no download)
kget> history failed                        # also: history 20 | completed | search <text> | clear
kget> retry 2                               # re-run an entry; resumes its .part file
kget> open last                             # also: reveal, extract, checksum, verify
kget> config set connections 8              # saved to config.json (config show | path | reset)
kget> doctor
kget> help download
```

Tab completes commands, flags, config keys and paths; ↑/↓ browse previous commands (saved across
sessions). Ctrl+C stops the running download and keeps partial data — `retry last` continues it.

### JSONL events (for scripts and agents)

```bash
kget --jsonl -a https://example.com/file.iso | jq -c 'select(.event=="progress") | .percent'
```

Every line is one JSON object with `"v": 1` and an `event` field:

```jsonc
{"v":1,"event":"started","url":"…","protocol":"http","advanced":true,"output":"…"}
{"v":1,"event":"progress","bytes":524288,"total":1048576,"percent":50.0,"speed_bps":3145728,"eta_secs":12}
{"v":1,"event":"status","message":"Calculating SHA256 hash..."}           // human text, may change
{"v":1,"event":"files","files":[{"name":"a.mkv","size":123}]}            // torrents
{"v":1,"event":"file_progress","files":[{"idx":0,"downloaded":64,"size":123,"pct":52.0}]}
{"v":1,"event":"phase","phase":"verifying","algorithm":"sha256"}         // hashing started
{"v":1,"event":"verified","algorithm":"sha256","hash":"…","ok":true}
{"v":1,"event":"phase","phase":"extracting"}                             // --extract
{"v":1,"event":"saved","path":"/abs/path/file.iso","bytes":1048576}
{"v":1,"event":"completed"}
{"v":1,"event":"error","code":"http_404","message":"…","retryable":false}
```

Match on `event` and the structured fields, never on `status` messages. New event types and fields
are added without changing `"v"`, so ignore the ones you don't know; `"v"` is bumped only when an
existing event or field is removed or changes meaning. In `--batch` mode every event also carries
the `url` it belongs to.

Exit codes: `0` success, `1` error, `130` cancelled (Ctrl+C — partial data is kept for resume).

### All CLI flags

| Flag | Description |
|------|-------------|
| `-a, --advanced` | Turbo mode — parallel connections, resumable |
| `-O <path>` | Output file or directory |
| `-q, --quiet` | Minimal output |
| `-c, --continue` | Continue a partial download (`<file>.part`) |
| `--no-clobber` | Skip the download if the file already exists |
| `--retries <n>` | Attempts per request (default 3) |
| `--timeout <secs>` | Connect / per-read timeout (default 60) |
| `-p <proxy>` | HTTP, HTTPS or SOCKS5 proxy (`socks5://host:1080`); used for HTTP and HTTPS targets |
| `--proxy-type <t>` | `http`, `https`, `socks5` (inferred from the proxy URL when omitted) |
| `-l <bytes/s>` | Speed limit in bytes per second (`0` = unlimited) |
| `--save-config` | Persist proxy/limit/torrent flags to `config.json` (never the proxy password) |
| `-H "Name: Value"` | Extra HTTP header (repeatable) |
| `--sha256 <hash>` | Verify SHA-256 after download |
| `--checksum <alg:hex>` | Verify with sha256, sha512, sha1, md5 or blake3 |
| `--checksum-url <url>` | Verify against a checksum file (GNU, BSD or hash-only) |
| `--extract` | Auto-extract archives after download |
| `--at "HH:MM"` | Schedule download for a specific local time |
| `--batch <file\|->` | Download all URLs from a file (or stdin) |
| `-j, --jobs <n>` | Parallel downloads in batch mode (default 4) |
| `--doctor` | Print diagnostics and exit |
| `--ftp` | Use FTP protocol |
| `--sftp` | Use SFTP protocol |
| `--webdav` | Use WebDAV protocol |
| `--ytdlp` | Route through yt-dlp (auto-detected for video sites) |
| `--quality <q>` | yt-dlp quality: `best`, `1080p`, `720p`, `480p`, `360p`, `audio` |
| `--metalink` | Download from a Metalink manifest |
| `--history` | Show download history (filters: `--status`, `--domain`, `--type`, `--protocol`, `--since`, `--until`, `--search`, `--sort`, `--history-limit`, `--history-offset`, `--json`) |
| `--history-clear [completed\|failed]` | Clear history |
| `--jsonl` | Emit JSON Lines events to stdout |
| `--gui` | Launch egui graphical interface |
| `-i, --interactive` | Interactive REPL mode |

---

## Library Usage

KGet is also a reusable Rust library. Add it to your project:

```toml
[dependencies]
Kget = "1.7.1"

# Optional: torrent client
Kget = { version = "1.7.1", features = ["torrent-native"] }

# Optional: async API
Kget = { version = "1.7.1", features = ["async"] }
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
    .spawn();

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

### Releases

Releases are built by GitHub Actions (`.github/workflows/release.yml`): pushing a
`vX.Y.Z` tag (or running the workflow manually) builds Linux, macOS and Windows
binaries plus the macOS DMG, publishes the GitHub release with `SHA256SUMS`,
publishes the crate to crates.io and updates the Homebrew tap.

## Testing

```bash
cargo test                          # all tests
cargo test --lib --test unit_tests  # unit tests only
cargo test --test mock_server_tests # HTTP mock server tests
cargo test --test resume_tests      # resume / truncation / retry regressions
./run-tests.sh                      # full suite with lint and format check
```

Tests use a temporary `KGET_CONFIG_DIR`, so they never change your real configuration.

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


## License

MIT — see [LICENSE](LICENSE)
