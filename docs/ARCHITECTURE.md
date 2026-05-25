# KGet Architecture

KGet should evolve as a shared Rust download engine with thin platform-specific
frontends. The Rust crate owns protocols, queue orchestration, configuration,
progress events, cancellation, integrity checks, and persistence. macOS, iOS,
iPadOS, Windows, and Linux clients should consume that engine through stable
commands and events instead of reimplementing download behavior.

## Current Shape

**Core library** (`src/lib.rs` re-exports everything):

| Module | Responsibility |
|--------|---------------|
| `src/download.rs` | Single-stream HTTP/HTTPS download with retry, gzip/brotli/lz4 decompression, SHA-256/multi-algorithm verification, `Content-Disposition` support, 4xx-vs-5xx retry policy |
| `src/advanced_download.rs` | Resumable multi-connection HTTP downloader (`AdvancedDownloader`); splits file into byte ranges, parallelises via rayon; global `TokenBucket` throttle; `ResumePolicy` enum |
| `src/builder.rs` | `DownloadBuilder` + `BatchBuilder` — fluent entry points (`kget::builder()`, `kget::batch()`); `.download()`, `.spawn()`, `.download_to_bytes()`, `.download_async()` |
| `src/error.rs` | `KgetError` typed enum with `From` impls for `reqwest::Error`, `io::Error`, `Box<dyn Error>` |
| `src/events.rs` | `DownloadEvent` channel variants: `Progress`, `Status`, `Completed`, `Error` |
| `src/checksum.rs` | `ChecksumAlgorithm` enum + `compute_checksum()` — SHA-256, SHA-512, SHA-1, MD5, BLAKE3 |
| `src/ftp/` | FTP protocol adapter (suppaftp) |
| `src/sftp/mod.rs` | SFTP protocol adapter (ssh2); SSH host-key verification against `~/.ssh/known_hosts`; `CheckResult::Failure` hard-errors |
| `src/webdav/mod.rs` | WebDAV adapter — rewrites `webdav(s)://` to `http(s)://`, extracts Basic auth credentials, re-exported `is_webdav_url()` |
| `src/ytdlp/mod.rs` | yt-dlp integration — `is_video_url()`, `VideoQuality` enum, `download_video()`, `ytdlp_binary()` |
| `src/torrent/` | Torrent support: `native.rs` (librqbit, `torrent-native` feature), `transmission.rs` (Transmission RPC, `torrent-transmission` feature), `external.rs`, `mod.rs` dispatcher |
| `src/metalink/mod.rs` | Metalink RFC 5854 parser + `download_metalink()` — tries mirrors in priority order, SHA-256 verification |
| `src/queue.rs` | Persistent download history (`DownloadHistory`, `HistoryEntry`, `EntryStatus`) backed by `history.json` |
| `src/config.rs` | JSON config persisted to the OS config dir; owns proxy, optimization, torrent, yt-dlp settings |
| `src/optimization.rs` | `Optimizer` selects connection count/strategy based on file type/size |
| `src/progress.rs` | indicatif progress bar factory |
| `src/utils.rs` | Filename extraction, output path resolution, `validate_filename` (null bytes, path traversal, >255 bytes, Windows reserved names), `auto_extract` |
| `src/app.rs` | `DownloadCommand`/`WorkerToGuiMessage` channel contract + `spawn_download_worker`; shared orchestration layer for all frontends |

**Binary** (`src/main.rs`):
- Parses CLI args with clap
- Routes to: interactive REPL (`src/interactive.rs`), egui GUI (feature-gated, `src/gui.rs`), or CLI mode
- JSONL event emission (`--jsonl` flag)
- Batch download (`--batch`), scheduling (`--at "HH:MM"`), header injection (`-H`)

**macOS native app** (`macos-app/`):
- SwiftUI shell that spawns `kget-bin` (the Rust binary renamed)
- Communicates via subprocess stdio (being migrated to JSONL)
- `DownloadManager.swift` drives the Rust process
- `ContentView.swift` — `NavigationSplitView` main UI (v1.7.0 redesign)
- `MenuBarView.swift` — menu bar extra with active download count
- `SettingsView.swift` — settings panel with yt-dlp quality, auto-extract, advanced-by-default
- `ShareExtension/ShareViewController.swift` — Share Extension encoding `kget://download?url=`

**egui GUI** (`src/gui.rs`):
- Feature-gated behind `gui`
- Apple-inspired system-adaptive design: `Colors` struct with `light()`/`dark()` palettes
- Left `SidePanel` (180px): Library nav + per-category count badges + dark/light toggle
- Download cards: 10px radius, status dot, type badges, 3px shimmer progress bar
- Status bar: live counts + Clear Completed action

## Target Architecture

```text
clients/
  macos-swiftui/        native macOS shell
  apple-shared/         Swift package for iOS/iPadOS/macOS UI code
  desktop-egui/         optional Rust-native desktop UI
  windows/              future native shell or Tauri shell
  linux/                future native shell or Tauri shell

crates/
  kget-core/            download domain, jobs, protocols, storage-free logic
  kget-app/             queue, commands, events, cancellation, settings
  kget-cli/             command line parsing and terminal progress
  kget-ffi/             C ABI or UniFFI bindings for Swift/Kotlin/C#/Tauri
```

The repository does not need to jump to this layout in one commit. Move toward
it by extracting stable modules when a feature needs them.

## Boundaries

- Core download code must not depend on GUI crates.
- Frontends should speak in `DownloadCommand` and progress/status events.
- Protocol modules should return structured errors where possible.
- CLI output intended for app parsing should be machine-stable JSONL, not localized human text.
- Platform shells should own platform UX only: menus, notifications, sharing, file pickers, permissions, and window/navigation behavior.

## Feature Flags

| Flag | Adds |
|------|------|
| `gui` | egui desktop UI, `torrent-native`, desktop notifications |
| `torrent-native` | librqbit BitTorrent client |
| `torrent-transmission` | Transmission RPC client |
| `async` | `download_async()` / `download_all_async()` via tokio spawn_blocking |

Default build has none — CLI + FTP/SFTP + torrent stubs only.

## Cross-Platform Plan

For iPhone and iPad, prefer a shared SwiftUI app that calls the Rust engine via
UniFFI or a small C ABI. Keep background downloads in mind early: iOS requires
careful use of background sessions, app groups, file providers, and share sheet
handoff.

For Windows and Linux, the fastest path is a shared Rust/Tauri or egui desktop
client backed by the same `kget-app` API.

## Near-Term Refactor Order

1. Keep shrinking `src/main.rs` until it only parses CLI args and launches modes.
2. Move queue state from individual UIs into an app service.
3. Replace ad hoc text parsing between SwiftUI and Rust with JSONL events.
4. Split protocol traits from concrete implementations.
5. Store queue/history in SQLite with schema migrations.
6. Introduce FFI only after the app command/event API is stable.
