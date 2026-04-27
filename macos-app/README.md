# KGet Native macOS App

A native macOS frontend for KGet built with SwiftUI, providing deep system integration.

## Features

### 🖥️ Native SwiftUI Interface
- Modern, native macOS look and feel
- Dark mode support
- Drag and drop support

### 📍 Menu Bar Integration
- Always-accessible menu bar icon
- Quick download from clipboard
- View active downloads at a glance

### 🔗 URL Scheme Handlers
- `kget://example.com/file.zip` - Opens KGet and starts download
- `magnet:?xt=...` - Handles magnet links directly

### 📁 File Associations
- `.torrent` files - Open with KGet
- `.metalink` files - Open with KGet

### 🔧 macOS Services Menu
- Select any URL in any app
- Right-click → Services → "Download with KGet"

### 📤 Share Extension
- Share URLs from Safari directly to KGet
- Works with any app that supports sharing

### 🔔 Native Notifications
- Download completion notifications
- Error notifications

## Building

### Quick Build (CLI)
```bash
./build-native-macos.sh
```

### With Xcode
1. Open `macos-app/` folder in Xcode
2. Build and run

## Requirements
- macOS 13.0+
- Xcode 15+
- Rust toolchain (for backend)

## Architecture

```
KGet.app/
├── Contents/
│   ├── MacOS/
│   │   ├── KGet          # Swift frontend
│   │   └── kget-bin      # Rust backend
│   ├── Resources/
│   │   └── AppIcon.icns
│   ├── PlugIns/
│   │   └── ShareExtension.appex
│   └── Info.plist
```

The Swift frontend handles:
- UI rendering (SwiftUI)
- System integration (URL schemes, Services, notifications)
- Download management

The Rust backend (`kget-bin`) handles:
- Actual file downloads
- FTP/SFTP/HTTP protocols
- Torrent integration
- Progress reporting

## Development

### Adding new features

1. **Swift-side changes**: Edit files in `macos-app/KGet/`
2. **Rust-side changes**: Edit files in `src/`
3. Run `./build-native-macos.sh` to rebuild

### Debugging
```bash
# Run Swift app directly
swift run -c release

# Test Rust backend
cargo run -- https://example.com/file.zip
```

## License
MIT License - see [LICENSE](../LICENSE)
