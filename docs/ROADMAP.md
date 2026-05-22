# KGet Improvement Roadmap

## Product Ideas

- ✅ **Persistent download history** — records every download in `history.json`; `--history` / `history` REPL command. _(v1.6.3)_
- ✅ **Metalink / mirror fallback** — `.meta4` / `.metalink` support with priority-ordered mirrors and SHA-256 verification. _(v1.6.3)_
- Persistent download queue with pause, resume, retry, and scheduled downloads.
- Browser integration and share extensions for one-click capture.
- Download categories, tags, smart folders, and duplicate detection.
- Per-download speed limits, global bandwidth profiles, and metered-network mode.
- Checksum verification beyond ISO files: SHA-256, SHA-512, MD5, and checksum URL discovery.
- Authenticated downloads: cookies, headers, bearer tokens, and basic auth profiles.
- Better torrent UX: file selection, priority, peer/seeder stats, tracker status, and ratio controls.
- Notification center with completed, failed, retrying, and verification events.
- Download history search with filters by domain, status, type, and date.

## Engineering Ideas

- Emit structured JSONL events from Rust for every frontend.
- Add a central job model: `DownloadJob`, `DownloadId`, `DownloadState`, and `DownloadEvent`.
- Store queue/history in SQLite with schema migrations.
- Add a protocol trait for HTTP, FTP, SFTP, and torrent adapters.
- Add integration tests around resume, cancellation, range fallback, and disk-full behavior.
- Add CI matrix for macOS, Linux, and Windows with CLI-only and GUI-feature builds.
- Add release automation that builds signed macOS artifacts and cross-platform binaries.
- Add observability logs with redaction for URLs, credentials, and proxy data.

## Platform Expansion

- **iPhone/iPad:** shared SwiftUI app, Rust engine via UniFFI/C ABI, share sheet, Files integration, background-safe downloads where possible.
- **macOS:** keep the native SwiftUI app, replace process-output parsing with structured events, add menu bar queue controls.
- **Windows:** start with Tauri or egui using the shared Rust app API; add installer, tray integration, and file associations.
- **Linux:** start with egui/Tauri AppImage/deb/rpm; support xdg-open, desktop files, and tray notifications.

