# KGet Improvement Roadmap

## Shipped

### v1.8.0
- ✅ Persistent daemon and local MCP server for agents, plus a browser native-messaging extension.
- ✅ `hf://` / `ollama://` model sources, async turbo downloads with HTTP/2 and experimental HTTP/3,
  parallel Metalink mirrors, and a ratatui terminal UI.
- ✅ SQLite history, smart organization rules, duplicate handling, OS keychain credentials and UniFFI bindings.
- ✅ Hardened archive extraction, FTP input validation, fuzz targets and panic containment in the library.

### v1.7.1
- ✅ **Safe resume** — `.part` + `.kget` state for turbo mode, validated chunks, retry from exact offset, Ctrl+C keeps progress.
- ✅ **CLI flags no longer persisted** — `--save-config`, never stores proxy passwords; `KGET_CONFIG_DIR`.
- ✅ **Unified HTTP client** — HTTP/HTTPS/SOCKS5 proxies for all targets, no transparent decompression.
- ✅ **yt-dlp** — found outside `PATH` (Homebrew/MacPorts/pipx), `KGET_YTDLP_PATH`, real progress and output path.
- ✅ **New flags** — `-c`, `--no-clobber`, `--retries`, `--timeout`, `-j`, `--checksum`, `--checksum-url`, `--doctor`.
- ✅ **Versioned JSONL protocol** — used by the macOS app (pause/resume, real file names).
- ✅ **`.torrent` files**, Metalink v3, FTP/SFTP resume and timeouts, atomic multi-process history.
- ✅ **CI + automated releases** — GitHub Actions for Linux/macOS/Windows, crates.io and Homebrew.

### v1.7.0
- ✅ **Batch download (`--batch urls.txt`)** — parallel, one URL per line, `#` = comment.
- ✅ **Download scheduling (`--at "HH:MM"`)** — sleep until local wall-clock time.
- ✅ **Custom HTTP headers (`-H "Name: Value"`)** — injected into all download modes.
- ✅ **Auto-extract archives (`--extract`)** — unzip/tar/7z after download.
- ✅ **yt-dlp integration (`--ytdlp`, auto-detected)** — YouTube, Vimeo, Twitch, TikTok…, `--quality` preset.
- ✅ **WebDAV support (`webdav://`, `webdavs://`)** — RFC 4918; Basic auth from URL.
- ✅ **Share Extension** — macOS Share sheet → KGet, `kget://download?url=` scheme.
- ✅ **Public library API overhaul** — `kget::builder()`, `kget::batch()`, `KgetError`, `DownloadEvent` channel, typed checksums, `RetryConfig`, async API.
- ✅ **macOS app — complete redesign** — `NavigationSplitView`, sidebar filter nav, shimmer progress bar, drag-and-drop, clipboard monitor, speed sparkline, history tab.
- ✅ **egui GUI — complete redesign** — Apple-inspired colors, sidebar, shimmer bar, system-adaptive dark/light.
- ✅ **Security/bug fixes** — `AdvancedDownloader::new()` → `Result`; global `TokenBucket` throttle; `ResumePolicy`; correct ISO MIME; `validate_filename` security hardening; 4xx no retry; SFTP host-key hard-error.

### v1.6.3
- ✅ **Persistent download history** — `history.json`; `--history` / `--history-clear`. _(v1.6.3)_
- ✅ **Metalink / mirror fallback** — `.meta4` / `.metalink`, priority-ordered mirrors, SHA-256 verification. _(v1.6.3)_
- ✅ **SFTP SSH host-key verification** — checks `~/.ssh/known_hosts`. _(v1.6.3)_
- ✅ **JSONL events** — `--jsonl` for scripts and agents. _(v1.6.3)_
- ✅ **Content-Disposition filename** — uses server-suggested names. _(v1.6.3)_
- ✅ **Speed throttling** — token-bucket rate limiter for HTTP downloads. _(v1.6.3)_

## Product Ideas

- Persistent download queue with pause, resume, retry, and scheduled downloads.
- Browser extension for one-click capture (Chrome/Firefox/Safari).
- Download categories, tags, smart folders, and duplicate detection.
- Per-download speed limits, global bandwidth profiles, and metered-network mode.
- Download history search with filters by domain, status, type, and date.
- Better torrent UX: file selection, priority, peer/seeder stats, tracker status, and ratio controls.
- Notification center integration (macOS/Linux/Windows).

## Engineering Ideas

- Add a central job model: `DownloadJob`, `DownloadId`, `DownloadState`.
- Store queue/history in SQLite with schema migrations.
- Add a protocol trait for HTTP, FTP, SFTP, WebDAV, and torrent adapters.
- Add integration tests around resume, cancellation, range fallback, and disk-full behavior.
- Add CI matrix for macOS, Linux, and Windows with CLI-only and GUI-feature builds.
- Add release automation that builds signed macOS artifacts and cross-platform binaries.
- UniFFI or C ABI bindings for future iOS/iPadOS/Windows native shells.

## Platform Expansion

- **iPhone/iPad:** shared SwiftUI app, Rust engine via UniFFI/C ABI, share sheet, Files integration, background-safe downloads.
- **macOS:** add menu bar queue controls (the app already speaks only the JSONL protocol).
- **Windows:** Tauri or egui AppImage/installer with tray integration and file associations.
- **Linux:** egui/Tauri AppImage/deb/rpm; xdg-open, desktop files, tray notifications.
