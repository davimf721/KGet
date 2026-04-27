---
name: kget-architecture
description: Use when working on KGet architecture, refactors, module boundaries, frontend/backend separation, cross-platform clients, or download-engine design.
---

# KGet Architecture Skill

KGet is a Rust download manager/library with CLI, egui GUI, and a native SwiftUI
macOS shell. Treat the Rust crate as the download engine and platform UIs as
thin clients.

## Workflow

1. Read `docs/ARCHITECTURE.md` first.
2. Inspect `src/lib.rs`, `src/app.rs`, and `src/main.rs` before refactoring.
3. Keep core modules independent from GUI crates.
4. Prefer command/event contracts over frontend-specific callbacks.
5. Keep refactors incremental and compile-check after each meaningful boundary move.

## Rules

- Put reusable orchestration in `src/app.rs` or a future app crate.
- Put protocol behavior in `src/download.rs`, `src/advanced_download.rs`, `src/ftp`, `src/sftp`, or `src/torrent`.
- Keep `src/main.rs` focused on CLI parsing and launching modes.
- Do not make SwiftUI, egui, or CLI parse fragile human text when a structured event can be added.
- Do not introduce a new cross-platform shell until the Rust command/event API is stable.

## Validation

Run focused checks first:

```bash
cargo check
cargo test --lib
```

If GUI behavior changes, also check:

```bash
cargo check --features gui
```

