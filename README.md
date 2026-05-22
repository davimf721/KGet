<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.6.3

A fast, modern download manager written in Rust. Supports HTTP/HTTPS, FTP/SFTP, and **magnet links** with a built-in torrent client.

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

## Features

- **Multi-protocol:** HTTP, HTTPS, FTP, SFTP, Magnet links, and **Metalink** (`.meta4`/`.metalink`)
- **Native Torrent Client:** Downloads torrents directly — no external apps needed
- **Turbo Mode:** Parallel connections for faster downloads
- **Metalink:** Multi-mirror downloads with automatic mirror fallback and SHA-256 verification (RFC 5854)
- **Download History:** Every download is recorded; browse with `--history`, clear with `--history-clear`
- **Interactive REPL:** Full `kget --interactive` mode with history, all protocols, and live config editing
- **GUI & CLI:** Use whichever you prefer
- **Cross-platform:** macOS, Linux, Windows
- **ISO Verification:** Optional SHA256 checksum for disk images
- **JSONL Events:** Experimental machine-readable progress for scripts and agents
- **Native Notifications:** Completion/error notifications in the Rust GUI on Linux/Windows

## Screenshots

| GUI | CLI |
|-----|-----|
| <img src="https://github.com/user-attachments/assets/f2862307-b524-4527-bee0-ce837c056e7d" width="400"/> | <img src="https://github.com/user-attachments/assets/a835c4df-5424-4aaa-b687-2445a99ba067" width="400"/> |

## Installation

### From Source

```bash
# Install Rust from https://rustup.rs if needed

# Linux dependencies (Debian/Ubuntu)
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config

# Clone and build
git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release --features gui

# Run
./target/release/kget --gui
```

### From crates.io

```bash
cargo install Kget --features gui
```

### Pre-built Binaries for macOS/Linux/Windows GUI

Download from [Releases](https://github.com/davimf721/KGet/releases).

## Usage

### GUI Mode
```bash
kget --gui
```

### CLI Mode
```bash
# Basic download
kget https://example.com/file.zip

# Turbo mode (parallel connections, resumable)
kget -a https://example.com/large.iso

# Save to specific location
kget -O ~/Downloads/myfile.zip https://example.com/file.zip

# Torrent download (auto-detected)
kget "magnet:?xt=urn:btih:HASH..."

# FTP – anonymous (no credentials needed)
kget --ftp ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz

# FTP – authenticated
kget --ftp ftp://user:pass@server/file.zip

# SFTP – password in URL
kget --sftp sftp://user:pass@server/path/to/file.dat

# SFTP – key-based (SSH agent or ~/.ssh/id_*)
kget --sftp sftp://user@server/path/to/file.dat
```

### Interactive Mode

```bash
kget --interactive
```

Launches a REPL with an ASCII art banner, command history, and full protocol support:

```
kget> download -a -o ~/Downloads/ubuntu.iso https://releases.ubuntu.com/...
kget> download --sftp sftp://user@server/backups/db.sql.gz
kget> download magnet:?xt=urn:btih:...
kget> config set connections 8
kget> config set speed-limit 1048576
kget> help
```

### Metalink downloads

```bash
# From a local manifest
kget --metalink ubuntu-24.04.meta4

# From a remote manifest (auto-detected by extension)
kget https://releases.ubuntu.com/ubuntu.meta4

# Tries each mirror in priority order; verifies SHA-256 automatically
```

### Download History

```bash
kget --history                    # list last 50 downloads
kget --history-clear              # remove all entries
kget --history-clear completed    # remove only completed/cancelled
```

Interactive mode:

```
kget> history
kget> history clear completed
```

### Options

| Flag | Description |
|------|-------------|
| `-a, --advanced` | Turbo mode with parallel connections (resumable) |
| `-O <path>` | Output file or directory |
| `-q, --quiet` | Minimal output |
| `-p <proxy>` | Use HTTP/SOCKS5 proxy |
| `-l <bytes>` | Speed limit in bytes/sec |
| `--sha256 <hash>` | Verify the completed file against an expected SHA256 hash |
| `--metalink` | Download from a Metalink manifest (`.meta4` / `.metalink`) |
| `--history` | Show download history (last 50 entries) |
| `--history-clear [completed]` | Clear history (all, or only completed/cancelled) |
| `--jsonl` | Emit experimental JSON Lines events for scripts and agents |
| `--ftp` | Use FTP protocol |
| `--sftp` | Use SFTP protocol (password or key-based auth) |
| `--gui` | Launch graphical interface |
| `-i, --interactive` | Interactive REPL mode |

## Library Usage

KGet can be used as a Rust library. See [LIB.md](LIB.md) for details.

```rust
use kget::{download, DownloadOptions, Optimizer, ProxyConfig};

let options = DownloadOptions::default();
download(
    "https://example.com/file.zip",
    ProxyConfig::default(),
    Optimizer::new(),
    options,
    None,
)?;
```

## Building

```bash
# CLI only
cargo build --release

# With GUI
cargo build --release --features gui

# Cross-compile for Linux/Windows (from macOS)
./build-cross.sh
```

## Testing

```bash
cargo test              # All tests
./run-tests.sh          # Full test suite with linting
```

## Links

- [Documentation](https://davimf721.github.io/KGet/)
- [Changelog](CHANGELOG.md)
- [crates.io](https://crates.io/crates/Kget)
- [Contributing](CONTRIBUTING.md)

## You can see posts about the project in others communities:
- [Dev.to](https://dev.to/davimf7221/kelpsget-v014-modern-download-manager-in-rust-4b9f)
- [r/rust](https://www.reddit.com/r/rust/comments/1kt69vh/after_5_months_of_development_i_finally_released/)
- [PitchHut](https://www.pitchhut.com/project/kelpsget)
- [Hacker News](https://hn.algolia.com/?query=Show%20HN%3A%20KelpsGet%20%E2%80%93%20Modern%20download%20manager%20built%20in%20Rust&type=story&dateRange=all&sort=byDate&storyText=false&prefix&page=0)


## License

MIT License - see [LICENSE](LICENSE)
