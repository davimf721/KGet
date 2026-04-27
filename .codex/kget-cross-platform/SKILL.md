---
name: kget-cross-platform
description: Use when adding or planning KGet support for iPhone, iPad, macOS, Windows, Linux, packaging, app shells, or native platform integrations.
---

# KGet Cross-Platform Skill

Use the Rust engine as the common behavior layer. Add platform clients around it
without duplicating download logic.

## Platform Strategy

- Apple platforms: use SwiftUI and share code across macOS, iOS, and iPadOS.
- Rust to Apple: prefer UniFFI or a small C ABI after `src/app.rs` stabilizes.
- Windows/Linux: prefer egui or Tauri first for speed, then native shells only if needed.
- Keep unsupported platform capabilities explicit; iOS may restrict some torrent/background behaviors.

## Workflow

1. Read `docs/ARCHITECTURE.md` and `docs/ROADMAP.md`.
2. Identify whether the feature belongs in Rust engine, app service, or platform UI.
3. Add structured Rust events before adding frontend parsing.
4. Keep platform-specific permissions, notifications, file pickers, and share flows in the platform client.
5. Add build/package notes when introducing a new platform target.

## Preferred Milestones

1. JSONL event stream from Rust.
2. Shared app command/event model.
3. Apple FFI package.
4. iPad layout.
5. iPhone layout.
6. Windows/Linux shell and packaging.

