# Changelog

[English](CHANGELOG.md) | [Português](translations/CHANGELOG.pt-BR.md) | [Español](translations/CHANGELOG.es.md)

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0.html),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.7.0] - 2026-05-24

### Added
- **Batch download (`--batch urls.txt`):** download multiple files in parallel from a plain-text file — one URL per line, lines starting with `#` are skipped as comments. All URLs run concurrently via OS threads. `--output` is treated as the destination directory. Per-URL `[OK]`/`[FAIL]` status printed; summary at the end.
- **History tab in macOS app:** new "History" sidebar item reads the persistent `history.json` produced by the Rust CLI. Shows all-time downloads with date, file size, and status badge. Hover a row to reveal a re-download button that re-queues the URL and switches to the "All Downloads" view.
- **Drag-and-drop URL into the macOS window:** drag any HTTP/HTTPS/FTP/magnet link from Safari, Chrome, or any other app and drop it onto the KGet window. A translucent blue overlay appears while hovering; on drop the URL lands in the input bar ready to start.
- **Clipboard monitor in macOS app:** the app silently watches the clipboard every 1.5 s. When a new HTTP, HTTPS, FTP, SFTP, or magnet URL is detected, a dismissable banner slides in below the URL bar with a one-click "Download" button. The banner is suppressed if the URL is already in the current download list.
- **Custom HTTP headers (`-H "Name: Value"`):** pass one or more `-H` flags to inject arbitrary request headers into both simple and turbo (advanced) downloads. Multiple headers can be stacked. Works in single-URL, batch, and interactive modes.
- **Speed sparkline in macOS app:** each active download row now shows a 44×16 pt miniature real-time speed graph that accumulates the last 30 speed readings. Built with SwiftUI `Path` + gradient fill via the new `SparklineView` component.
- **Auto-extract archives:** `kget --extract` automatically runs `unzip`, `tar`, or `7z` on the downloaded file when the extension is `.zip`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tar.xz`, or `.7z`. Controllable via the new "Auto-extract archives" toggle in the macOS Settings → Downloads tab.
- **Download scheduling (`--at "HH:MM"`):** defer a CLI download to a specific local wall-clock time. The process sleeps until the target minute is reached, then starts the download. Works with single-URL, advanced, and batch modes.
- **yt-dlp integration (`--ytdlp`, auto-detected):** if `yt-dlp` (or `youtube-dl`) is installed, URLs from YouTube, Vimeo, Twitch, TikTok, Instagram, Bilibili, and 10+ other platforms are automatically routed through it. Quality preset selectable via `--quality best|1080p|720p|480p|360p|audio`. macOS app detects video URLs and adds a global default quality picker in Settings → Downloads.
- **WebDAV support (`webdav://`, `webdavs://`):** new `WebDavDownloader` adapter in `src/webdav/mod.rs` converts `webdav://` → `http://` and `webdavs://` → `https://`, extracts embedded credentials (`webdav://user:pass@host/path`), and injects an HTTP Basic `Authorization` header. Detected automatically from URL scheme; explicit `--webdav` flag also available. Compatible with Synology, Nextcloud, Nextcloud, Apache WebDAV, and any RFC 4918 server.
- **Share Extension (`Share > KGet`):** the macOS Share Extension (`macos-app/ShareExtension/`) is now complete and functional. `ShareViewController` encodes the shared URL as `kget://download?url=<encoded>` and opens the main app via `NSWorkspace`. `KGetApp.swift` parses the new format and starts the download. The extension is compiled and embedded into `KGet.app/Contents/PlugIns/KGetShareExtension.appex` by `build-native-macos.sh`.
- **Public library API overhaul (`src/builder.rs`, `src/error.rs`, `src/events.rs`, `src/checksum.rs`):**
  - **Builder pattern** — `kget::builder(url)` and `kget::batch([...])` entry points replace positional args. Fluent methods: `.output()`, `.connections()`, `.speed_limit()`, `.proxy()`, `.proxy_auth()`, `.sha256/sha512/sha1/md5/blake3()`, `.verify_from()`, `.header()`, `.retry()`, `.range()`, `.quiet()`.
  - **Typed errors** — `KgetError` enum (`Network`, `Io`, `ChecksumMismatch`, `Protocol`, `Cancelled`, `NotFound`, `SidecarError`, `Other`) with `From` impls for `reqwest::Error`, `std::io::Error`, and `Box<dyn Error>`.
  - **Event channel** — `.spawn()` returns `(JoinHandle, Receiver<DownloadEvent>)` with `Progress { percent, speed_bps, eta_secs }`, `Status`, `Completed`, `Error` variants.
  - **Download metrics** — `DownloadResult` struct returned by `.download()` with `path`, `bytes_downloaded`, `avg_speed_bps`, `duration`, `connections_used`, `checksums`.
  - **In-memory download** — `.download_to_bytes() -> Vec<u8>` and `.download_to_reader() -> impl Read` (no filesystem writes).
  - **HTTP range** — `.range(start, end)` sends `Range: bytes=start-end`; works with in-memory and on-disk paths.
  - **Sidecar checksum verification** — `.verify_from(url)` downloads and parses GNU (`<hash>  <file>`) and BSD (`SHA256 (file) = hash`) sidecar files, auto-selects algorithm by hash length.
  - **Multi-algorithm checksums** — SHA-256, SHA-512, SHA-1 (via `sha1` crate), MD5 (via `md-5` crate), BLAKE3 (via `blake3` crate). `ChecksumAlgorithm` enum + `compute_checksum()` in `src/checksum.rs`.
  - **Configurable retry** — `RetryConfig { max_attempts, backoff: Backoff::Exponential { base_ms, max_ms }, retry_on_status }`. Permanent errors (`Cancelled`, `NotFound`, `ChecksumMismatch`) never retry.
  - **Batch with concurrency control** — `BatchBuilder::concurrency(n)` uses a Rayon thread pool; returns `Vec<BatchResult>` (one per URL).
  - **Async API** — `DownloadBuilder::download_async()` and `BatchBuilder::download_all_async()` behind `--features async`. Both use `tokio::task::spawn_blocking` so they never block the executor.

### Fixed
- **`AdvancedDownloader::new()` panicked on HTTP client build failure** — changed return type from `Self` to `Result<Self, …>` so the error propagates instead of crashing.
- **Parallel throttle was per-thread** — with N connections and a 1 MB/s limit, actual throughput was N×1 MB/s. Replaced per-thread busy-wait with a global `Arc<Mutex<TokenBucket>>` shared across all rayon workers; aggregate rate is now bounded correctly.
- **`file.set_len(total_size)` happened before confirming range support** — if the server returned 200 instead of 206, the file was preallocated and then overwritten by a full-stream download producing a corrupted result. Preallocation now only occurs when `supports_range` is confirmed.
- **ISO integrity prompt read from stdin in library/automation context** — `AdvancedDownloader` now has a `ResumePolicy` field (`Ask` / `AlwaysResume` / `AlwaysRestart`). Library callers set `AlwaysResume` to avoid blocking. `Ask` (default) preserves the existing interactive behavior.
- **Wrong ISO MIME type** — `"application/x-iso9001"` (ISO 9001 quality standard) corrected to `"application/x-iso9660-image"`.
- **`verify_file_sha256` printed to stdout unconditionally** — all `println!` calls removed; messages are now sent only via the optional callback.
- **Simple downloader retried on 404/403/410** — permanent 4xx errors now fail immediately; only transient 5xx responses and connection errors are retried.
- **`validate_filename` was insufficient** — now also rejects: null bytes (`\0`), path traversal sequences (`..`), filenames longer than 255 bytes, and Windows reserved device names (CON, PRN, AUX, NUL, COM1–COM9, LPT1–LPT9) — case-insensitive, with or without extension.
- **`sftp/mod.rs`: `CheckResult::Failure` silently continued** — the libssh2 internal error case now returns a hard error and aborts the connection instead of bypassing host-key verification.

### Added (continued)
- **Homebrew tap (`brew install kget`):** `Formula/kget.rb` added to the repository. Install with `brew tap davimf721/kget && brew install kget`. Optional features selectable at install time: `--with-gui` (egui graphical interface) and `--with-torrent` (native BitTorrent client). `release.sh` automatically patches the formula SHA256 after each tag push.
- **egui GUI — missing features parity with macOS app:**
  - **Speed sparkline** per active download (44×16pt, last 30 speed readings, gradient fill).
  - **Clipboard monitor** — polls every 1.5 s; shows dismissable banner with one-click download when a new downloadable URL is detected.
  - **Drag-and-drop URL** — detects `hovered_files` for visual overlay; reads URL from dropped bytes (browser link drags) or from `.url`/`.webloc` shortcut files.
  - **History tab** in sidebar — loads `history.json` from disk; shows all-time entries with date, size, status, error, re-download and Copy URL buttons.
  - **WebDAV URL validation** — `webdav://` and `webdavs://` now accepted by `validate_input()`.
  - **Updated protocol chips** in empty state — HTTP, FTP, WebDAV, Torrent, yt-dlp.

### Changed
- **macOS SwiftUI app — complete redesign:** `NavigationSplitView` layout with collapsible sidebar for filter navigation (All / Active / Completed / Failed with live count badges); clean URL input bar with inline Turbo toggle; simplified 3px thin progress bar with shimmer animation replacing the busy multi-segment bar; download rows with status dot, type badges (Turbo / ISO / Torrent), and compact action icons; `CleanProgressBar` component used throughout; `TypeBadge` component for download type labels; empty state with protocol chips; material-backed window background.
- **egui GUI (Linux/Windows) — complete redesign:** Apple-inspired system-adaptive color palette (light: `#F2F2F7` background, white cards / dark: pure black, `#1C1C1E` cards); left sidebar (180px) with Library navigation and per-category count badges; light/dark toggle button in sidebar; URL input card with Apple-style `↓` icon, inline Turbo/Verify toggles; 3px thin progress bar with shimmer; download cards with status dot, type badges, clean action buttons; proper card shadows; status bar with live stats.
- **egui theme is now system-adaptive:** reads OS dark/light preference at startup; manual override button in sidebar.

## [1.6.3] - 2026-05-21

### Added
- **Metalink support (`.meta4` / `.metalink`):** `kget --metalink file.meta4` (or any URL/path ending in `.meta4`/`.metalink`) parses the RFC 5854 manifest, tries mirrors in priority order, and verifies SHA-256 after download. Works in both CLI (`--metalink`) and interactive mode (`download --metalink`). Auto-detected by file extension — no flag needed for local files.
- **Persistent download history:** every CLI and interactive download is now recorded to `history.json` in the OS config dir. View with `kget --history`; clear with `kget --history-clear` (or `--history-clear completed`). Interactive mode gains `history`, `history clear`, and `history clear completed` commands.

### Security
- **SFTP: SSH host key verification against `~/.ssh/known_hosts`** — connections now check the server key and hard-error on mismatches (possible MITM), emitting a `ssh-keygen -R` hint. Unknown hosts emit a warning but continue (analogous to OpenSSH `StrictHostKeyChecking=accept-new`).

### Fixed
- **macOS native app now shows the correct version after every build.** `build-native-macos.sh` now updates the source `Info.plist` from `Cargo.toml` before compiling, so `Bundle.main` always reads the right version at runtime. Fallback strings in `ContentView` and `SettingsView` changed from hardcoded `"1.6.3"` to `"unknown"` so they never silently show a stale version.
- **`Optimizer::get_peer_limit()` returned bytes/second as peer count.** When a speed limit (e.g. 1 MB/s = 1048576) was set, the method returned that number as the torrent peer limit. Fixed to always return 50.
- **User-Agent was hardcoded as `KGet/1.0`.** Both the simple and advanced HTTP downloaders now send `KGet/1.6.3` (via `env!("CARGO_PKG_VERSION")`), matching the actual release version.

### Added
- **`Content-Disposition` filename support.** When a server provides a `Content-Disposition: attachment; filename=…` header (plain or RFC 5987 `filename*=` form), KGet uses that name for the saved file instead of the URL path segment. Useful for redirected URLs or APIs that serve files under generic paths.

## [1.6.3] - 2026-05-10

### Added
- **Experimental JSONL CLI events:** `--jsonl` emits machine-readable `started`, `progress`, `status`, `completed`, and `error` events for agents, scripts, and future frontends.
- **GUI filtering:** macOS and the Rust GUI now support status filters for all, active, completed, and failed/cancelled downloads.
- **More download actions:** macOS and the Rust GUI now expose quick actions for Copy URL, Open File, Open Folder, and Copy SHA256 when a checksum is available.
- **Expected SHA256 in the Rust GUI:** the Linux/Windows GUI can pass an expected SHA256 hash into the download worker.

### Changed
- **macOS settings now affect real behavior:** advanced mode by default, completion notifications, and speed limit settings are persisted and applied to new downloads.
- **Version display cleanup:** macOS app and extension metadata now use 1.6.3, and visible version labels read from bundle metadata instead of hardcoded strings.
- **Speed limits now throttle HTTP downloads:** simple and advanced HTTP downloads honor the configured byte-per-second limit.

### Fixed
- **Advanced download metadata fallback:** when `HEAD` fails or omits `Content-Length`, KGet now probes with `Range: bytes=0-0` before giving up.
- **Resume progress accuracy:** advanced downloads now initialize progress from the existing partial file size instead of visually restarting from zero.
- **JSONL mode no longer mixes advanced human progress lines into machine output.**

## [1.6.2] - 2026-04-28

### Fixed
- **SFTP downloads were completely non-functional.** The previous implementation passed the full `sftp://…` URL string directly to `TcpStream::connect` and used it as the remote file path, causing an immediate connection error on every SFTP call. The module is now fully rewritten:
  - URL is parsed to extract `host`, `port`, `username`, and `remote_path` correctly.
  - Authentication tries in priority order: password embedded in the URL → running SSH agent → default key files (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`).
  - File is streamed in 32 KB chunks with a real-time progress bar.
  - Clear, actionable error messages at every failure point.
- **FTP anonymous login failed when the URL contained no username.** `url.username()` from the `url` crate returns an empty string `""` (not `None`) when the URL has no user segment. Passing `""` to `ftp.login()` caused anonymous FTP servers to reject the connection. The downloader now falls back to `"anonymous"` in that case.

### Added
- **Interactive mode is now fully implemented.** Previously `kget --interactive` opened a REPL that only printed `"Would download: …"` without performing any actual download. The mode is now feature-complete:
  - Unicode block-font ASCII art banner on entry.
  - `rustyline` line editor with persistent command history.
  - `download [flags] <url>` — dispatches to the correct downloader based on flags:
    - Default: simple HTTP/HTTPS with retry and progress bar.
    - `-a` / `--advanced` / `--turbo`: `AdvancedDownloader` (parallel byte-range, resumable).
    - `--ftp`: FTP downloader with anonymous fallback.
    - `--sftp`: SFTP downloader with multi-method SSH auth.
    - `--torrent` or auto-detected `magnet:?` prefix: native torrent engine.
    - `-o <path>`, `-q` (quiet), `--sha256 <hash>` flags supported.
  - `config [show | set <key> <value>]`: reads and persists settings (`connections`, `speed-limit`, `compression`, `cache`).
  - `clear`, `version`, `help` / `?` commands; `get`, `dl` as aliases for `download`.
  - Errors are printed and the REPL continues — a failed download never crashes the session.

### Changed
- **Mutex lock error handling in `AdvancedDownloader`:** all `.unwrap()` calls on `Mutex::lock()` replaced with `.expect("…")` and descriptive messages, making panics easier to diagnose if a lock is ever poisoned.
- **`Optimizer` public API cleanup:** removed `#[allow(dead_code)]` from the public methods `compress`, `get_cached_file`, and `cache_file` — these are valid library API surface and the suppression was masking legitimate lint signal.

## [1.6.1] - 2026-04-27

### Added
- macOS app now validates magnet links before creating a download card.
- Completed downloads include Open File and Open Folder actions.
- macOS app context menu on download cards: Copy URL, Open Folder, Restart, and Remove.
- Keyboard shortcuts in the macOS app: `Cmd+V`, `Cmd+L`, `Esc`, and `Delete`.
- Expected SHA256 verification through CLI `--sha256 <hash>` and library `DownloadOptions::expected_sha256`.
- Public `verify_file_sha256` helper for library users.
- Native completion and failure notifications for the Rust GUI on Linux and Windows through `notify-rust`.

### Changed
- Duplicate URL or magnet submissions now focus the existing macOS download card instead of adding another card.
- Advanced downloads respect optimizer connection limits and reject invalid byte-range responses.
- Library documentation was refreshed in English, Portuguese, and Spanish for the current API.

### Fixed
- Invalid magnet links are rejected before starting the torrent backend.
- SHA256 mismatch now fails the download instead of only printing the calculated hash.

## [1.6.0] - 2026-03-02

### Added
- **Native macOS App (SwiftUI):** Completely redesigned native macOS application with deep system integration.
  - URL scheme handlers (`kget://`, `magnet:`)
  - File associations (`.torrent` files)
  - Menu bar integration with quick actions
  - macOS Services menu support
  - Native notifications
  - Drag-and-drop DMG installer with visual guide (boxes, arrow, instruction text)
- **Improved Cross-Platform GUI:** Major visual overhaul for the egui-based GUI (Linux/Windows).
  - Download list with multiple concurrent downloads tracking
  - TURBO badge for parallel downloads mode
  - ISO badge for ISO files with automatic integrity verification
  - Multi-segment progress bar showing parallel connections (C1, C2, C3, C4)
  - Verification progress bar with purple theme and shield animation
  - Connections indicator (⚡ 4x) for turbo mode
  - Real-time speed and ETA display
  - Empty state with protocol icons
  - One-line URL input with integrated controls
  - Compact layout with truncated filenames and URLs
  - Proper button sizing and alignment
- **Visual Improvements:**
  - Enhanced dark theme with better contrast
  - Animated shimmer effects on progress bars
  - Status-colored badges and icons
  - Improved typography and spacing
  - DMG installer background with dark theme, rounded boxes, chevron arrow, and instruction text
- **Native Torrent Client (librqbit):** Built-in BitTorrent support - no external apps needed!
  - Magnet link downloads work directly in the GUI
  - DHT peer discovery, parallel piece downloading
  - Real-time progress with speed and ETA
  - Expandable file list showing individual file progress (macOS app)
  - Green TORRENT badge in download list
- **Refactored GUI (Linux/Windows):** Complete visual overhaul
  - Clean, modern dark theme with consistent color palette
  - Responsive layout that adapts to window size
  - Smart truncation for long filenames and paths
  - Inline folder selector with visual indicator
  - Turbo mode and SHA256 verification toggles
  - Shimmer animation on progress bars
  - Retry button for failed downloads
  - Status bar with active/completed counts
- **Cross-Platform Build Script:** `build-cross.sh` for compiling Linux and Windows binaries from macOS
- **Automated Test Suite:** 108+ tests covering torrents, CLI, config, and downloads
  - New `tests/torrent_tests.rs` with 42 torrent-specific tests
  - `run-tests.sh` script for full test execution
  
### Changed
- **GUI feature now includes torrent-native:** Building with `--features gui` automatically enables native torrent support
- **Progress calculation:** Fixed weighted progress for multi-file torrents
- **Code quality:** Refactored `gui.rs` from 975 to 780 lines following Clean Code principles
- **Dependencies:** Updated reqwest to 0.13.2 with hickory-dns

### Fixed
- Torrent downloads opening external app instead of downloading in GUI
- Progress bar "spasming" during downloads - now only increases
- DHT persistence lock conflict with `disable_dht_persistence: true`
- Layout overflow issues with long URLs and filenames

### Technical
- Uses `librqbit` 8.1.1 for native BitTorrent protocol
- Session management with `Arc<Session>` for proper cleanup
- JSON output format for torrent file lists and progress

## [1.5.4] - 2026-02-27

### Added
- **macOS App Bundle:** Native `.app` bundle for macOS users with easy drag-and-drop installation.
- **Easier version flag:** Use `kget -v` or `kget --version` to display version (changed from `-V`).
- **Comprehensive test suite:** Added 65+ tests covering unit tests, CLI integration tests, and mock server tests.
  - Unit tests for `utils`, `config`, `download`, `optimizer`, `progress`, and URL parsing.
  - CLI tests verifying all command-line flags and options.
  - Mock server tests using `wiremock` for HTTP download simulation without real network requests.
- **Testing infrastructure:** Added `wiremock`, `assert_cmd`, `predicates`, and `tokio-test` as dev-dependencies.

### Changed
- **Code cleanup:** Resolved all 25 compiler warnings for a cleaner build output.
- **Removed duplicate SFTP stub:** Consolidated SFTP implementation by removing unused `ftp/sftp.rs` file that conflicted with `sftp/mod.rs`.
- **Public API improvements:** Exported `get_filename_from_url_or_default`, `resolve_output_path`, and `print` functions from library root.

### Fixed
- Removed unused imports across multiple modules (`BufWriter`, `Path`, `Read`, `Write`, `Url`, `Session`, `Sftp`, etc.).
- Fixed unnecessary `mut` declarations in `download_whole()`, `download_chunks_parallel()`, and `download_worker()`.
- Prefixed intentionally unused function parameters with `_` (`output_dir`, `quiet`, `proxy`, `optimizer` in torrent module).
- Added `#[allow(dead_code)]` annotations for fields and structs reserved for future use (`TransmissionSettings`, `optimizer` field in downloaders).

## [1.5.3] - 2025-12-23

### Added
- **Torrent backend selection:** magnet links can now be handled by different backends.
- **Default torrent behavior (no extra features):** if the URL is a `magnet:?` link, KGet opens it using the system's default BitTorrent client (automatic detection via OS handler).
- **Optional Transmission RPC backend:** build with `--features torrent-transmission` and set `KGET_TORRENT_BACKEND=transmission` to download via Transmission RPC.
- **Transmission settings helper:** centralized settings for host/port/paths and optional auth (env-compatible).

### Changed
- **GUI footer:** app version is displayed in the bottom-right corner.
- **GUI window sizing:** improved default/min window sizing for a better first launch experience.

### Fixed
- Build fixes and feature-gating improvements for optional components (torrent backends / GUI split).

## [1.5.2] - 2025-12-19

### Added
- **ISO Smart Handling**: Automatic detection of `.iso` files via URL and MIME type.
- **Corruption Prevention**: ISO files now bypass decompression/optimization layers to ensure binary 1:1 integrity.
- **Integrity Verification**: Added optional SHA256 checksum calculation at the end of ISO downloads.
- **Windows Dual Mode**: The application now runs without a console window when launched via Explorer, but automatically attaches to the terminal when run via CLI.

### Fixed
- **Memory & Disk Optimization**: Refactored `AdvancedDownloader` to use streaming writes with `BufWriter`, drastically reducing RAM usage and preventing 100% disk active time issues.
- **Verification Prompt**: Fixed bug where integrity check was running automatically in advanced mode; it now correctly asks the user for confirmation.
- **UI/UX**: Cleaned up terminal output during parallel downloads for a smoother progress bar experience.
- **GUI Icon**: Fixed issue with loading the application window icon.
- Fixed Rust compiler error `E0382` regarding `Mime` type ownership in `download.rs`.
- Improved parallel chunk writing safety for binary-heavy files.

## [1.5.1] - 2025-12-18

### Added
- Optional `gui` Cargo feature to make GUI dependencies opt-in; compile with `--features gui` to enable GUI support.
- Top-level convenience functions: `kget::download(...)` and `kget::advanced_download(...)` for easier library usage.
- `create_progress_bar_factory(...)` exported to let consumers create `indicatif` progress bars.
- `examples/lib_usage.rs` example demonstrating library usage.
- Docker development instructions and `docker-compose` integration to simplify building, testing and contributing.

### Changed
- Updated README and `LIB.md` with library usage instructions and examples.
- `CONTRIBUTING.md` and translations updated with Docker contributor workflow.
- GUI code split: `gui_types` module added so CLI builds without GUI feature.

### Fixed / Misc
- Minor documentation fixes and translation updates (PT-BR/ES).


## [1.5.0] - 2025-05-26

### Added
- New public Rust crate: KGet can now be used as a library in your own Rust projects, click [here](LIB.md) to see more.
- Improved GUI: larger fonts, better layout, and more intuitive controls.
- Clipboard integration for easy pasting of URLs.
- Download button and cancel button now always visible and functional in the GUI.
- **Interactive mode:** Run `kget --interactive` for a REPL-like experience with commands such as `download <url> [output]`, `help`, and `exit`.

### Changed
- Project renamed from "KelpsGet" to "KGet" for simplicity and consistency.
- Versioning scheme updated from 0.1.4 to 1.5.0 to allow for more frequent minor updates and clearer release tracking.
- Features list moved from README to CHANGELOG for easier maintenance and to keep the README concise.

### Removed
- Redundant or overly detailed features section from the README (now see CHANGELOG for all features).

## [0.1.4] - 2025-05-22

### Added
- Graphical User Interface (GUI) for easier downloads.
- FTP download support.
- SFTP download support (password and key-based authentication).
- Torrent download support via magnet links (integrates with Transmission daemon).
- Detailed instructions for Transmission daemon setup in README.

### Changed
- Refined output path determination to align comportamento with `wget`.
- Ensured `final_path` is always absolute to prevent "No such file or directory" errors in CWD.
- Updated README in English, Portuguese, and Spanish to reflect all new features and setup instructions.

### Fixed
- Resolved "No such file or directory" error when downloading without `-O` by ensuring absolute paths.
- Corrected `validate_filename` to only check the base filename, not the full path.
- Addressed potential issues with `map_err` in `main.rs` for torrent and HTTP downloads.

## [0.1.3] - 2025-03-11

### Added
- Advanced download mode with parallel chunks and resume capability
- Automatic compression support (gzip, brotli, lz4)
- Intelligent caching system for faster repeated downloads
- Speed limiting and connection control
- Multi-language documentation support

### Changed
- Improved error handling and user feedback
- Enhanced progress bar with more detailed information
- Optimized memory usage for large file downloads
- Updated proxy configuration system

### Fixed
- Fixed proxy authentication issues
- Resolved cache directory creation problems
- Fixed compression level handling
- Corrected file path handling on Windows

### Security
- Added secure proxy connection handling
- Improved URL validation
- Enhanced file name sanitization
- Added space checking before downloads

## [0.1.2] - 2025-03-10

### Added
- Proxy support (HTTP, HTTPS, SOCKS5)
- Proxy authentication
- Custom output file naming
- MIME type detection

### Changed
- Improved download speed calculation
- Enhanced progress bar display
- Better error messages
- Updated documentation

### Fixed
- Fixed connection timeout issues
- Resolved file permission problems
- Corrected URL parsing
- Fixed progress bar display on Windows

## [0.1.1] - 2025-03-09

### Added
- Silent mode for script integration
- Basic progress bar
- File size display
- Download speed tracking

### Changed
- Improved error handling
- Enhanced command-line interface
- Better file handling
- Updated installation instructions

### Fixed
- Fixed path handling issues
- Resolved permission problems
- Corrected progress display
- Fixed file overwrite behavior

## [0.1.0] - 2025-03-08

### Added
- Initial release
- Basic file download functionality
- Command-line interface
- Basic error handling
- Cross-platform support
