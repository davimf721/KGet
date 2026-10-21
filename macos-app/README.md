# KGet Native macOS App

A native macOS frontend for KGet built with SwiftUI, providing deep system integration. Requires macOS 13.0+.

## Features

### Modern SwiftUI Interface (v1.7.0 redesign)
- `NavigationSplitView` layout with collapsible sidebar
- Sidebar: All / Active / Completed / Failed nav items with live count badges
- Clean URL input bar with inline Turbo toggle
- 3px thin progress bar with shimmer animation
- Download cards: status dot, type badges (Turbo / ISO / Torrent), compact action icons
- Empty state with protocol chips

### Download Features
- **Drag-and-drop URLs** — drag any HTTP/HTTPS/FTP/magnet link into the window
- **Clipboard monitor** — detects new URLs every 1.5s, shows dismissable banner with one-click download
- **Speed sparkline** — 44×16pt real-time speed graph per active download (last 30 samples)
- **History tab** — full download history from `history.json`; hover to re-download
- **yt-dlp auto-detection** — video URLs routed through yt-dlp; quality picker in Settings
- **WebDAV support** — `webdav://` and `webdavs://` auto-detected from URL scheme
- **Auto-extract archives** — toggle in Settings → Downloads

### System Integration
- **Menu bar icon** — always-accessible, shows active download count
- **Share Extension** — share URLs from Safari (or any app) directly to KGet
- **URL scheme** — `kget://download?url=<encoded>` opens KGet and starts a download
- **Magnet links** — `.torrent` and magnet: links handled directly
- **Native notifications** — download completion and error notifications

## Building

```bash
# Build CLI backend + Swift app + DMG
./build-native-macos.sh
```

The script:
1. Builds the Rust binary with the native torrent client (`--features torrent-native`)
2. Compiles the Swift app via `swift build`
3. Assembles `KGet.app` bundle with the Rust binary renamed to `kget-bin`
4. Compiles and embeds `KGetShareExtension.appex` into `Contents/PlugIns/`
5. Creates `release/KGet-<version>-macOS-Native.dmg`

## Requirements

- macOS 13.0+
- Xcode 15+ (or Swift 5.9+ command line tools)
- Rust toolchain (`rustup.rs`)

## App Bundle Layout

```
KGet.app/
├── Contents/
│   ├── MacOS/
│   │   ├── KGet          # Swift frontend
│   │   └── kget-bin      # Rust backend
│   ├── Resources/
│   │   └── AppIcon.icns
│   ├── PlugIns/
│   │   └── KGetShareExtension.appex
│   └── Info.plist
```

The Swift frontend handles:
- UI rendering (SwiftUI + AppKit integration)
- System integration (URL schemes, Share Extension, notifications, drag-and-drop)
- Download queue management and history display
- Clipboard monitoring and sparkline charting

The Rust backend (`kget-bin`) handles:
- All actual file downloads (HTTP/HTTPS, FTP, SFTP, WebDAV, torrent)
- Progress, verification/extraction phases and errors via the versioned `--jsonl` protocol (`src/events.rs`); the app never parses human-readable text
- yt-dlp subprocess management
- History persistence

## Key Source Files

| File | Purpose |
|------|---------|
| `KGet/ContentView.swift` | Main `NavigationSplitView` UI, sidebar, download list |
| `KGet/DownloadManager.swift` | ObservableObject driving Rust subprocess, clipboard monitor, history |
| `KGet/MenuBarView.swift` | Menu bar extra with active download count |
| `KGet/SettingsView.swift` | Settings panel (connections, speed limit, yt-dlp quality, auto-extract) |
| `KGet/KGetApp.swift` | App entry point, URL scheme handler (`kget://download?url=`) |
| `ShareExtension/ShareViewController.swift` | Share Extension entry point |
| `Package.swift` | Swift package manifest |

## Development

1. **Swift-side changes**: edit files in `macos-app/KGet/`
2. **Rust-side changes**: edit files in `src/`
3. Rebuild with `./build-native-macos.sh`

Debugging:
```bash
# Test Rust backend directly
./target/release/kget --jsonl https://example.com/file.zip

# Watch JSONL events
./target/release/kget --jsonl -a https://example.com/file.zip | jq .
```

## License

MIT — see [LICENSE](../LICENSE)
