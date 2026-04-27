# KGet Architecture

KGet should evolve as a shared Rust download engine with thin platform-specific
frontends. The Rust crate owns protocols, queue orchestration, configuration,
progress events, cancellation, integrity checks, and persistence. macOS, iOS,
iPadOS, Windows, and Linux clients should consume that engine through stable
commands and events instead of reimplementing download behavior.

## Current Shape

- `src/lib.rs` exposes the reusable Rust library.
- `src/main.rs` is the CLI and egui launcher.
- `src/app.rs` contains the first shared application command/event worker.
- `src/download.rs` handles simple HTTP/HTTPS downloads.
- `src/advanced_download.rs` handles resumable multi-connection downloads.
- `src/ftp`, `src/sftp`, and `src/torrent` are protocol adapters.
- `src/config.rs` owns persisted cross-platform configuration.
- `src/gui.rs` is the egui desktop UI.
- `macos-app` is a native SwiftUI shell that launches the Rust binary.

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
- CLI output intended for app parsing should be machine-stable, preferably JSON
  lines, not localized human text.
- Platform shells should own platform UX only: menus, notifications, sharing,
  file pickers, permissions, and window/navigation behavior.

## Cross-Platform Plan

For iPhone and iPad, prefer a shared SwiftUI app that calls the Rust engine via
UniFFI or a small C ABI. Keep background downloads in mind early: iOS requires
careful use of background sessions, app groups, file providers, and share sheet
handoff. If full torrent support is restricted on iOS distribution channels,
design the UI to hide unsupported capabilities cleanly.

For Windows and Linux, the fastest path is a shared Rust/Tauri or egui desktop
client backed by the same `kget-app` API. A more native path can come later if
the app needs platform-specific integration beyond tray, notifications, and file
association.

## Near-Term Refactor Order

1. Keep shrinking `src/main.rs` until it only parses CLI args and launches modes.
2. Move queue state from individual UIs into an app service.
3. Replace ad hoc text parsing between SwiftUI and Rust with JSONL events.
4. Split protocol traits from concrete implementations.
5. Add persistence for download history and resumable queue state.
6. Introduce FFI only after the app command/event API is stable.

