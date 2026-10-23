<img width="1000" height="500" alt="KGet Banner" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet v1.8.0

**A fast, modern download manager written in Rust.**  
HTTP/HTTPS · FTP/SFTP · WebDAV · BitTorrent · Metalink · yt-dlp · Hugging Face · Ollama · CLI · TUI · GUI · MCP · Library

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

[![CI](https://github.com/davimf721/KGet/actions/workflows/ci.yml/badge.svg)](https://github.com/davimf721/KGet/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/Kget.svg)](https://crates.io/crates/Kget)
[![docs.rs](https://docs.rs/Kget/badge.svg)](https://docs.rs/Kget)

> **1.8.0** brings KGet to the agent era: a persistent daemon, MCP server,
> browser extension, terminal UI, AI-model sources, HTTP/2 turbo downloads and
> SQLite history — with hardened archive extraction and OS-backed credentials.
> See the [CHANGELOG](CHANGELOG.md).

---

## Why KGet?

Most download tools are either too simple (single-connection wget-clones) or too bloated (full Electron apps). KGet is a **Rust-native download engine** that gives you:

- **Turbo multi-connection downloads** — splits files into parallel byte ranges (up to 32× speed on fast connections)
- **Full protocol coverage** — HTTP/HTTPS (HTTP/2), FTP/SFTP, WebDAV, magnet links, yt-dlp, and AI models from Hugging Face and Ollama in one binary
- **Many frontends, one engine** — native macOS SwiftUI app, cross-platform egui GUI, terminal UI, a polished CLI, a browser extension and an MCP server for AI agents, all sharing one job queue (optionally a background daemon)
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
| Hugging Face | `hf://` or `huggingface.co/…` *(auto)* | Models, datasets, spaces; `--include`/`--exclude`, `--revision`, `--hf-cache`; LFS SHA-256 verified |
| Ollama | `ollama://` *(auto)* | Pulls into the Ollama models directory (or plain files with `-O`); digests verified |

### Download Engine
- **Turbo mode** (`-a`) — parallel byte-range connections on an async engine with work stealing; every chunk is validated and retried from its exact offset
- **HTTP/2 and HTTP/3** (`--http-version`) — HTTP/2 negotiated automatically; experimental HTTP/3 behind a build flag
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
- **Saved passwords** — FTP/SFTP/WebDAV/proxy passwords and HF tokens in the OS keychain (`--save-credentials`, `--credentials`); never in `config.json`
- **JSONL events** (`--jsonl`) — versioned (`"v":1`) machine-readable events for every protocol
- **Diagnostics** (`--doctor`) — config path, yt-dlp location, extract tools, compiled features

### History & Persistence
- **Download history** — every download recorded in a SQLite `history.db` (multi-process safe, credentials stripped; `history.json` is migrated); searchable with filters, tags and `--json`
- **Smart rules** — auto-organize into folders, tag downloads, skip or hard-link duplicates
- **Background daemon** — `kget daemon` + `kget queue …`: a persistent queue that survives restarts
- **Configuration** — flags apply only to the current run; `--save-config` persists them (proxy passwords are never written). `KGET_CONFIG_DIR` overrides the config directory
- **Interactive REPL** (`--interactive`) — every CLI download option, batch, retry, checksums, Tab completion, persistent history, Ctrl+C to pause

### GUIs
- **Native macOS app** — SwiftUI, `NavigationSplitView` sidebar, clipboard monitor, drag-and-drop URLs, speed sparkline, history tab, Share Extension, menu bar
- **Cross-platform egui GUI** (`--gui`) — Apple-inspired design, system-adaptive dark/light, sidebar navigation, shimmer progress bar
- **Terminal UI** (`kget tui`) — full-screen queue, details and history in any terminal
- **Browser extension** — Chrome, Edge, Brave, Firefox: send links (or every download) to KGet

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
- **macOS** — `KGet-1.8.0-macOS-Native.dmg` (native SwiftUI app, no Rust needed)
- **macOS CLI** — `kget-1.8.0-macos-arm64` (Apple Silicon) / `kget-1.8.0-macos-x64` (Intel)
- **Linux** — `kget-1.8.0-linux-x64`, `kget-1.8.0-linux-x64-static` (musl), `kget-1.8.0-linux-arm64`
- **Windows** — `kget-1.8.0-windows-x64.exe` / `.zip`
- Verify with `SHA256SUMS-1.8.0.txt`

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

### AI models (Hugging Face, Ollama)

```bash
kget hf://Qwen/Qwen2.5-0.5B-Instruct                          # whole repo into ./Qwen2.5-0.5B-Instruct/
kget hf://TheBloke/Llama-2-7B-GGUF --include "*Q4_K_M.gguf"    # only matching files
kget hf://org/repo --revision v1.0 --exclude "*fp32*" -j 8     # tag/branch/commit, 8 files at a time
kget hf://datasets/org/name/data/train.parquet                 # one file of a dataset
kget hf://org/repo --hf-cache                                  # into ~/.cache/huggingface/hub for transformers
kget ollama://llama3.2:3b                                      # into ~/.ollama/models, ready for `ollama run`
kget ollama://llama3.2 -O ./models/                            # plain files instead
```

Gated or private repos use `HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, the `hf auth login` token, or a token saved
with `kget --credentials add hf://token@huggingface.co`. Interrupted model downloads skip completed files on the
next run.

### Torrent file selection and seeding

```bash
kget --list-files "magnet:?xt=urn:btih:HASH"                   # numbered file list, then exit
kget --select 1,3-5 "magnet:?xt=urn:btih:HASH"                 # only these files
kget --select-glob "*.mkv" ~/Downloads/show.torrent            # only matching paths
kget --seed-ratio 1.5 --seed-time 2h "magnet:?xt=urn:btih:HASH"  # keep seeding afterwards
```

Selection, `--list-files` and `--seed-time` need the native client (`--features torrent-native`); the
Transmission backend supports `--select` and `--seed-ratio`. With `--jsonl`, `torrent_stats` events report peers,
seeders, speeds, ratio and trackers.

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

### History search

```bash
kget --history                    # list last 50 downloads
kget --history --status failed --domain example.com --since 7d
kget --history --type video --sort largest --history-limit 10
kget --history --search ubuntu --tag linux --until 2026-10-01
kget --history --json             # JSON array (with --jsonl: one object per line)
kget --history-clear              # remove all entries
kget --history-clear completed    # keep only failures
kget --history-clear failed       # remove only failures
```

Filters: `--status` (completed, failed, cancelled, handed_off), `--domain` (includes subdomains), `--type`
(video, audio, image, archive, document, disk, package, or extensions), `--protocol`, `--since`/`--until`
(`today`, `7d`, `12h`, `YYYY-MM-DD`; UTC), `--search`, `--tag`; `--sort newest|oldest|largest|smallest|name|fastest`,
`--history-limit`, `--history-offset`. The REPL `history` command takes the same filters.

### Smart rules, tags and duplicates

```bash
kget --rules add name=ISOs ext=iso domain=ubuntu.com,debian.org folder="ISOs/{domain}" tags=linux
kget --rules test https://releases.ubuntu.com/24.04/ubuntu.iso   # which rule matches, and where it would go
kget --rules enable                     # organize every download (or --organize for one run)
kget --rules enable builtins            # also use the built-in Videos/Music/Images/… folders
kget --rules list                       # also: remove <name>, disable [name]
kget --rules suggest                    # rules suggested from your history (--ai asks Claude, --apply adds them)
kget --tag work --tag q4 https://example.com/report.pdf
kget --duplicates skip https://example.com/file.zip   # already downloaded? skip | ask | download | link
kget --dedupe https://example.com/copy.iso            # identical content already saved? hard-link it
```

Rules live in `config.json` and are tried in order (first match wins). They match on domain, URL regex/glob,
extension, category, MIME type, size or protocol, and can set a folder (`{category}`, `{domain}`, `{ext}`,
`{protocol}`, `{date:%Y-%m}`), tags, connections, speed limit, extraction or a required checksum. `-O` always wins
over a rule's folder. `--rules suggest --ai` needs `ANTHROPIC_API_KEY` and sends only aggregated counts, after you
confirm. Without `--duplicates`, KGet asks on a terminal and downloads again otherwise.

### Saved passwords (keychain)

```bash
kget --save-credentials sftp://alice@server/backups/db.tar.gz   # prompts once, saves in the keychain
kget sftp://alice@server/backups/db.tar.gz                      # next time: no prompt
kget --credentials add ftp://bob@files.example.com              # save ahead of time
kget --credentials list                                         # targets only, never passwords
kget --credentials delete ftp://bob@files.example.com
kget --credentials status                                       # which keychain is used
```

Passwords go to the macOS Keychain, Windows Credential Manager or Secret Service (Linux/BSD). Targets are
`scheme://user@host[:port]` (sftp, ftp, webdav, webdavs, or a proxy URL), `ssh-key:///path/to/key` for a key
passphrase, or `hf://token@huggingface.co`. `--save-config` also puts the proxy password there. Set
`KGET_KEYCHAIN=off` to disable it.

### HTTP/2 and HTTP/3

```bash
kget -a https://example.com/big.iso                       # auto: HTTP/2 via ALPN when offered, else HTTP/1.1
kget -a --http-version 1.1 https://example.com/big.iso    # never HTTP/2
kget -a --http-version 2 http://h2c.example.com/big.iso   # HTTP/2 prior knowledge (also cleartext)
```

Set a default with `optimization.http_version` in `config.json`. `--http-version 3` (QUIC) is experimental and
needs `RUSTFLAGS="--cfg reqwest_unstable" cargo build --release --features http3`; other builds reject it with a
clear message. Metalink downloads fetch chunks from every mirror at once and drop mirrors that fail.

### Daemon and queue

```bash
kget daemon                          # start the background daemon (detaches; --foreground to stay attached)
kget queue add -a https://example.com/big.iso --wait   # queue a download and follow it
kget queue list                      # also: status <id>, pause, resume, cancel, rm, watch [ids]
kget https://example.com/file.zip --via-daemon         # any download through the daemon
kget daemon status                   # exit code 3 if not running
kget daemon stop                     # unfinished jobs resume on the next start
```

The daemon keeps a persistent queue (`jobs.db`), runs `daemon.max_concurrent` jobs at a time (`-j`), and saves into
`daemon.download_dir` (default `~/Downloads`); clients cannot write outside it unless
`daemon.allow_outside_download_dir` is set. It listens on a `0600` Unix socket (loopback TCP with a token on Windows).
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#daemon-and-mcp) for the protocol and security model.

### Use from AI agents (MCP)

```bash
claude mcp add kget -- kget mcp                           # Claude Code
claude mcp add kget -- kget mcp --root ~/Downloads/agent  # restrict the agent to one folder
```

`kget mcp` speaks the Model Context Protocol on stdin/stdout. Tools: `download`, `download_status`,
`list_downloads`, `cancel_download`, `pause_download`, `resume_download`, `search_history`, `verify_checksum`,
`get_config`. Every write (and every file `verify_checksum` reads) stays inside `--root` (default: the daemon's
download directory). It uses a running daemon, so downloads outlive the agent session (`--start-daemon` starts
one; `--no-daemon` keeps everything in-process).

### Terminal UI

```bash
kget tui            # uses the daemon if it is running, else runs downloads in this process
kget tui --daemon   # start/use the daemon: downloads continue after you quit
kget tui --local    # always in-process
```

Keys: `a` add a URL, `p`/Space pause or resume, `c` cancel, `r` retry, `d` remove, `o` open, `1`/`2` queue or
history, `/` search history, `f` filter by status, `?` help, `q` quit. Included in default builds (`tui` feature).

### Browser extension

```bash
kget native-host install                    # register with every installed browser
kget native-host install --browser firefox  # or chrome, chromium, edge, brave
kget native-host uninstall
```

Then load `browser-extension/` as an unpacked extension (Chrome, Edge, Brave, Chromium) or a temporary add-on
(Firefox 128+). It adds "Download with KGet" to the context menu, a popup with live progress, and optional
interception of browser downloads. Downloads go through the daemon (started automatically). See
[browser-extension/README.md](browser-extension/README.md).

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
{"v":1,"event":"rule","name":"ISOs","output":"ISOs/ubuntu.com/","tags":["linux"]}  // smart rule matched
{"v":1,"event":"started","url":"…","protocol":"http","advanced":true,"output":"…"}
{"v":1,"event":"progress","bytes":524288,"total":1048576,"percent":50.0,"speed_bps":3145728,"eta_secs":12}
{"v":1,"event":"status","message":"Calculating SHA256 hash..."}           // human text, may change
{"v":1,"event":"files","files":[{"name":"a.mkv","size":123}]}            // torrents
{"v":1,"event":"file_progress","files":[{"idx":0,"downloaded":64,"size":123,"pct":52.0}]}
{"v":1,"event":"torrent_stats","peers":12,"upload_bps":40960,"ratio":0.4,"seeding":false,…} // torrents
{"v":1,"event":"phase","phase":"verifying","algorithm":"sha256"}         // hashing started
{"v":1,"event":"verified","algorithm":"sha256","hash":"…","ok":true}
{"v":1,"event":"phase","phase":"extracting"}                             // --extract
{"v":1,"event":"saved","path":"/abs/path/file.iso","bytes":1048576}
{"v":1,"event":"duplicate","kind":"content","action":"linked","path":"/abs/older/copy.iso"} // --dedupe
{"v":1,"event":"completed"}
{"v":1,"event":"error","code":"http_404","message":"…","retryable":false}
```

Match on `event` and the structured fields, never on `status` messages. New event types and fields
are added without changing `"v"`, so ignore the ones you don't know; `"v"` is bumped only when an
existing event or field is removed or changes meaning. In `--batch` mode every event also carries
the `url` it belongs to.

Exit codes: `0` success, `1` error, `130` cancelled (Ctrl+C — partial data is kept for resume).

### All CLI flags

Run `kget --help` for the full list.

| Flag | Description |
|------|-------------|
| `-a, --advanced` | Turbo mode — parallel connections, resumable |
| `--http-version <v>` | Turbo HTTP version: `auto` (HTTP/2 via ALPN), `1.1`, `2`, `3` (needs an `http3` build); config: `optimization.http_version` |
| `-O, --output <path>` | Output file or directory |
| `-q, --quiet` | Minimal output |
| `-c, --continue` | Continue a partial download (`<file>.part`) |
| `--no-clobber` | Skip the download if the file already exists |
| `--retries <n>` | Attempts per request (default 3) |
| `--timeout <secs>` | Connect / per-read timeout (default 60) |
| `-p, --proxy <url>` | HTTP, HTTPS or SOCKS5 proxy (`socks5://host:1080`); used for HTTP and HTTPS targets |
| `--proxy-user`, `--proxy-pass` | Proxy credentials (the password is never written to `config.json`) |
| `--proxy-type <t>` | `http`, `https`, `socks5` (inferred from the proxy URL when omitted) |
| `-l, --limit <bytes/s>` | Speed limit in bytes per second (`0` = unlimited) |
| `--save-config` | Persist proxy/limit/torrent flags to `config.json` (proxy password → keychain) |
| `--save-credentials` | Save the passwords this download uses in the system keychain |
| `--credentials <action> [target]` | `list`, `status`, `add <target>`, `delete <target>` |
| `-H, --header "Name: Value"` | Extra HTTP header (repeatable) |
| `--sha256 <hash>` | Verify SHA-256 after download |
| `--checksum <alg:hex>` | Verify with sha256, sha512, sha1, md5 or blake3 |
| `--checksum-url <url>` | Verify against a checksum file (GNU, BSD or hash-only) |
| `--extract` | Auto-extract archives after download |
| `--at "HH:MM"` | Schedule download for a specific local time |
| `--batch <file\|->` | Download all URLs from a file (or stdin) |
| `-j, --jobs <n>` | Parallel downloads in batch mode, files at a time for `hf://`/`ollama://` (default 4) |
| `--include`, `--exclude <glob>` | `hf://`: only / skip matching files (repeatable) |
| `--revision <rev>` | `hf://`: branch, tag or commit |
| `--hf-cache` | `hf://`: write into the Hugging Face hub cache |
| `-t, --torrent` | Force torrent |
| `--list-files` | Torrent: print the numbered file list and exit (`torrent-native`) |
| `--select <list>`, `--select-glob <pattern>` | Torrent: download only some files (repeatable) |
| `--seed-ratio <r>`, `--seed-time <d>` | Torrent: keep seeding until a ratio / for a duration (`30m`, `2h`) |
| `--max-peers`, `--max-seeds`, `--torrent-port`, `--no-dht` | Torrent client settings |
| `--organize`, `--no-organize` | Apply (or skip) the smart rules for this download |
| `--rules <action> …` | `list`, `test <url>`, `add key=value…`, `remove <name>`, `enable\|disable [name\|builtins]`, `suggest` (`--ai`, `--apply`) |
| `--tag <tag>` | Tag recorded in the history (repeatable); with `--history`: filter by tag |
| `--duplicates <policy>` | Already downloaded: `skip`, `ask`, `download`, `link` |
| `--dedupe` | Hard-link identical content already in the history |
| `--via-daemon` | Run the download through the background daemon |
| `--doctor` | Print diagnostics and exit |
| `--ftp`, `--sftp`, `--webdav` | Force FTP / SFTP / WebDAV |
| `--ytdlp` | Route through yt-dlp (auto-detected for video sites) |
| `--quality <q>` | yt-dlp quality: `best`, `1080p`, `720p`, `480p`, `360p`, `audio` |
| `--metalink` | Download from a Metalink manifest |
| `--history` | Show download history (filters: `--status`, `--domain`, `--type`, `--protocol`, `--since`, `--until`, `--search`, `--tag`, `--sort`, `--history-limit`, `--history-offset`, `--json`) |
| `--history-clear [completed\|failed]` | Clear history |
| `--jsonl` | Emit JSON Lines events to stdout |
| `--gui` | Launch egui graphical interface |
| `-i, --interactive` | Interactive REPL mode |

| Subcommand | Description |
|------------|-------------|
| `kget daemon [--foreground] [status\|stop]` | Background download daemon |
| `kget queue add\|list\|status\|cancel\|pause\|resume\|rm\|watch` | Manage the daemon's queue |
| `kget mcp [--root DIR]` | MCP server for AI agents (stdio) |
| `kget tui [--daemon\|--local]` | Terminal UI |
| `kget native-host install\|uninstall` | Register the browser extension's native host |

---

## Library Usage

KGet is also a reusable Rust library. Add it to your project:

```toml
[dependencies]
Kget = "1.8.0"

# Optional: torrent client
Kget = { version = "1.8.0", features = ["torrent-native"] }

# Optional: async API
Kget = { version = "1.8.0", features = ["async"] }
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

Beyond the builder, the library exposes the same job queue the daemon and GUIs use (`kget::JobManager`),
the shared dispatcher (`kget::runner`), history search (`kget::queue::HistoryQuery`), the keychain
(`kget::credentials`), Hugging Face/Ollama downloads (`kget::hf`, `kget::ollama`) and a daemon client.
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

# Everything except HTTP/3
cargo build --release --features gui,torrent-native,torrent-transmission

# Minimal CLI (no keychain, no terminal UI)
cargo build --release --no-default-features

# Experimental HTTP/3 (QUIC)
RUSTFLAGS="--cfg reqwest_unstable" cargo build --release --features http3

# UniFFI bindings (Swift/Kotlin) for native apps
./scripts/gen-bindings.sh            # into bindings/
./scripts/build-xcframework.sh       # macOS/iOS XCFramework

# Native macOS app + DMG (requires Xcode)
./build-native-macos.sh

# Cross-compile Linux/Windows from macOS (requires zig)
brew install zig && cargo install cargo-zigbuild
./build-cross.sh
```

### Feature flags

| Flag | Default | Adds |
|------|---------|------|
| `keychain` | ✅ | Saved passwords in the OS credential store (pure Rust) |
| `tui` | ✅ | `kget tui` terminal UI (ratatui, pure Rust) |
| `gui` | | egui desktop UI, `torrent-native`, desktop notifications |
| `torrent-native` | | Built-in BitTorrent client (librqbit): file selection, seeding, swarm stats |
| `torrent-transmission` | | Transmission RPC client |
| `http3` | | Experimental HTTP/3; needs `RUSTFLAGS="--cfg reqwest_unstable"` |
| `async` | | `download_async()` / `download_all_async()` builder helpers |

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
cargo test --test robustness_tests  # disk full, cancel mid-chunk, changing files, Range ignored, stalls
cargo test --test daemon_tests --test mcp_tests --test native_host_tests
cargo test -p kget-ffi              # UniFFI engine tests
node --test browser-extension/tests/
cargo fuzz run url                  # fuzzing (cargo-fuzz, nightly; see fuzz/)
./run-tests.sh                      # full suite with lint and format check
```

Tests use a temporary `KGET_CONFIG_DIR`, so they never change your real configuration.

---

## Platform Support

| Platform | CLI / TUI / daemon | egui GUI | Native App | Browser extension |
|----------|--------------------|----------|------------|-------------------|
| macOS | ✅ | ✅ | ✅ SwiftUI DMG | ✅ Chromium browsers, Firefox |
| Linux | ✅ | ✅ | — | ✅ Chromium browsers, Firefox |
| Windows | ✅ | ✅ | — | ✅ Chromium browsers, Firefox |

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
