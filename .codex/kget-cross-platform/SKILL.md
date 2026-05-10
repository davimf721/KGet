---
name: kget-cross-platform
description: Use when adding or planning KGet support for iPhone, iPad, macOS, Windows, Linux, packaging, app shells, or native platform integrations.
---

# KGet Cross-Platform Skill

Use the Rust engine as the common behavior layer. Add platform clients around it
without duplicating download logic.

Use `obsidian-project-memory` for this work: read the Segundo Cerebro Obsidian
context before implementing and update the relevant vault notes after important
platform, packaging, architecture, or roadmap changes.

## Platform Strategy

- Apple platforms: use SwiftUI and share code across macOS, iOS, and iPadOS.
- Rust to Apple: prefer UniFFI or a small C ABI after `src/app.rs` stabilizes.
- Windows/Linux: prefer egui or Tauri first for speed, then native shells only if needed.
- Keep unsupported platform capabilities explicit; iOS may restrict some torrent/background behaviors.

## Workflow

1. Read the Obsidian context via `obsidian-project-memory`.
2. Read `docs/ARCHITECTURE.md` and `docs/ROADMAP.md`.
3. Identify whether the feature belongs in Rust engine, app service, or platform UI.
4. Add structured Rust events before adding frontend parsing.
5. Keep platform-specific permissions, notifications, file pickers, and share flows in the platform client.
6. Add build/package notes when introducing a new platform target.
7. Update the vault when platform support, build steps, roadmap, or app boundaries change.

## Preferred Milestones

1. JSONL event stream from Rust.
2. Shared app command/event model.
3. Apple FFI package.
4. iPad layout.
5. iPhone layout.
6. Windows/Linux shell and packaging.
