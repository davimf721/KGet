# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Project Is

KGet is a Rust download manager published both as a CLI binary and a reusable library (`crate name: Kget`). It supports HTTP/HTTPS, FTP, SFTP, and magnet/torrent links. There is also a native macOS SwiftUI shell in `macos-app/` that wraps the Rust binary.

## Build Commands

```bash
# CLI only (no GUI)
cargo build --release

# With egui desktop GUI
cargo build --release --features gui

# With native torrent client only
cargo build --release --features torrent-native

# Native macOS app + DMG (requires Xcode/swift)
./build-native-macos.sh

# Cross-compile Linux/Windows from macOS (requires cross+Docker or cargo-zigbuild+zig)
./build-cross.sh
```

The Rust binary is `target/release/kget`. The macOS `.app` bundle and `.dmg` go to `release/`.

## Running

```bash
./target/release/kget --gui                    # egui GUI (requires --features gui)
./target/release/kget <url>                    # basic HTTP download
./target/release/kget -a <url>                 # turbo mode (parallel, resumable)
./target/release/kget --ftp ftp://...          # FTP
./target/release/kget --sftp sftp://...        # SFTP
./target/release/kget "magnet:?xt=urn:btih:…" # torrent
./target/release/kget --interactive            # REPL mode
./target/release/kget --jsonl <url>            # machine-readable JSONL events
```

## Testing

```bash
cargo test                                      # all tests
cargo test --lib --test unit_tests             # unit tests only
cargo test --test cli_tests                    # CLI integration tests
cargo test --test mock_server_tests            # wiremock HTTP tests
cargo test --test torrent_tests                # torrent tests
cargo test --test torrent_tests --features torrent-native
cargo check --features gui                     # verify GUI compiles
cargo clippy --all-targets -- -D warnings      # lint
cargo fmt -- --check                           # format check
./run-tests.sh                                 # full automated suite (all of the above)
```

## Architecture

The repository is a single Rust crate with both a `lib` and a `bin` target.

**Core library** (`src/lib.rs` re-exports everything):
- `src/download.rs` — single-stream HTTP/HTTPS download with retry, gzip/brotli/lz4 decompression, and optional SHA256 verification
- `src/advanced_download.rs` — resumable multi-connection HTTP downloader (`AdvancedDownloader`); splits file into byte ranges, parallelizes via rayon
- `src/ftp/` and `src/sftp/` — protocol adapters for FTP (suppaftp) and SFTP (ssh2)
- `src/torrent/` — torrent support: `native.rs` (librqbit, behind `torrent-native` feature), `transmission.rs` (Transmission RPC, behind `torrent-transmission` feature), `external.rs`, and `mod.rs` which dispatches
- `src/config.rs` — JSON config persisted to the OS config dir; owns proxy, optimization, torrent settings
- `src/optimization.rs` — `Optimizer` selects connection count/strategy based on file type/size
- `src/progress.rs` — indicatif progress bar factory
- `src/utils.rs` — filename extraction, output path resolution, print helpers
- `src/app.rs` — `DownloadCommand`/`WorkerToGuiMessage` channel contract + `spawn_download_worker`; this is the shared orchestration layer that all frontends should use

**Binary** (`src/main.rs`):
- Parses CLI args with clap
- Routes to: interactive REPL (`src/interactive.rs`), egui GUI (feature-gated, `src/gui.rs`), or CLI mode
- JSONL event emission for machine-readable output (`--jsonl` flag)

**macOS native app** (`macos-app/`):
- SwiftUI shell that spawns `kget-bin` (the Rust binary renamed)
- Communicates via subprocess stdio; parses text output (being migrated to JSONL)
- `DownloadManager.swift` drives the Rust process; `ContentView.swift` is the main UI; `MenuBarView.swift` is the menu bar extra

## Feature Flags

| Flag | Adds |
|------|------|
| `gui` | egui desktop UI, `torrent-native`, desktop notifications |
| `torrent-native` | librqbit BitTorrent client |
| `torrent-transmission` | Transmission RPC client |

Default build has none of these — CLI + FTP/SFTP + torrent stubs only.

## Architecture Direction

Per `docs/ARCHITECTURE.md`: the goal is a thin `main.rs` that only parses args and launches modes, with all download logic in the library. Frontends should communicate through `DownloadCommand`/`WorkerToGuiMessage` (defined in `src/app.rs`), not by parsing text output. CLI output for machine consumers should be JSONL (`--jsonl`), not localized text. Core download code must not import GUI crates.

## Config File Location

- macOS: `~/Library/Application Support/kget/config.json`
- Linux: `~/.config/kget/config.json`
- Windows: `%APPDATA%\kget\config.json`

## Crate Name vs Package Name

The crate name (used in `use` statements) is `kget` (lowercase). The package name on crates.io is `Kget` (capital K). The binary is also `kget`.
