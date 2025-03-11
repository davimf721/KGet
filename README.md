# KelpsGet v0.1.3

A modern, lightweight wget clone written in Rust for fast and reliable file downloads from the command line.

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

## Features
✅ Simple CLI tool for downloading files via HTTP/HTTPS.<br>
✅ Progress bar with real-time speed and ETA tracking.<br>
✅ Custom output names (-O flag to rename downloaded files).<br>
✅ MIME type detection and proper file handling.<br>
✅ Cross-platform (Linux, macOS, Windows).<br>
✅ Silent mode for scripts.<br>
✅ Automatic space checking before download.<br>
✅ Automatic retry on connection failure.<br>
✅ File name validation.<br>
✅ Support for different MIME types.<br>
✅ Detailed download information display.<br>
✅ Advanced download mode with parallel chunks and resume capability.<br>
✅ Proxy support (HTTP, HTTPS, SOCKS5).<br>
✅ Automatic compression and caching.<br>
✅ Speed limiting and connection control.<br>

## Installation
### Option 1: Install via Cargo
```bash
cargo install kelpsget
```
### Option 2: Download Pre-built Binaries
Download the latest binary for your OS from [Release](https://github.com/davimf721/KelpsGet/releases)

### Linux/macOS:
```bash
chmod +x kelpsget  # Make executable  
./kelpsget [URL]    # Run directly
```
### Windows:
Run the .exe file directly.

## Usage Examples
Basic Download:
```bash
kelpsget https://example.com/file.txt
```
Rename the Output File:
```bash
kelpsget -O new_name.txt https://example.com/file.txt
```
Silent Mode:
```bash
kelpsget -q https://example.com/file.txt
```
Advanced Download Mode (Parallel and Resumable):
```bash
kelpsget -a https://example.com/large_file.zip
```
Using Proxy:
```bash
kelpsget -p http://proxy:8080 https://example.com/file.txt
```
With Proxy Authentication:
```bash
kelpsget -p http://proxy:8080 --proxy-user user --proxy-pass pass https://example.com/file.txt
```
Speed Limiting:
```bash
kelpsget -l 1048576 https://example.com/file.txt  # 1MB/s limit
```
Disable Compression:
```bash
kelpsget --no-compress https://example.com/file.txt
```
Disable Cache:
```bash
kelpsget --no-cache https://example.com/file.txt
```

## How It Works
1. Progress Bar: Shows download speed, ETA, and bytes transferred.
2. Smart File Naming:
  - Uses the filename from the URL (e.g., file.txt from https://example.com/file.txt).
  - Defaults to index.html if the URL ends with /.
3. Error Handling: Exits with code 1 on HTTP errors (e.g., 404).
4. Space Checking: Checks available disk space before downloading.
5. Automatic Retry: Retries download if connection fails.
6. Advanced Download Mode:
  - Downloads file in parallel chunks for better performance
  - Supports resuming interrupted downloads
  - Automatically handles large files efficiently
7. Proxy Support:
  - HTTP, HTTPS, and SOCKS5 proxy support
  - Proxy authentication
  - Configurable proxy settings
8. Optimization Features:
  - Automatic compression (gzip, brotli, lz4)
  - File caching for faster repeated downloads
  - Speed limiting
  - Connection control

## Configuration
KelpsGet uses a configuration file located at:
- Windows: `%APPDATA%\kelpsget\config.json`
- Linux/macOS: `~/.config/kelpsget/config.json`

Example configuration:
```json
{
  "proxy": {
    "enabled": false,
    "url": null,
    "username": null,
    "password": null,
    "proxy_type": "Http"
  },
  "optimization": {
    "compression": true,
    "compression_level": 6,
    "cache_enabled": true,
    "cache_dir": "~/.cache/kelpsget",
    "speed_limit": null,
    "max_connections": 4
  }
}
```

## Security Features
- Space Checking: Ensures enough disk space before downloading.
- File Name Validation: Prevents path injection.
- URL Handling: Safely handles URLs.
- Automatic Retry: Retries download if network fails.
- Secure Proxy Support: Encrypted proxy connections.

## Contributing
Found a bug or want to add a feature? Open an issue or submit a PR!

🚀 Download files effortlessly with Rust's speed and reliability. 🚀

## 🔗 Important Links
- 📚 [Documentation](https://davimf721.github.io/KelpsGet/)
- 📦 [crates.io](https://crates.io/crates/kelpsget)
- 💻 [GitHub](https://github.com/davimf721/KelpsGet)
- 📝 [Changelog](CHANGELOG.md)

## 🎯 Next Steps
We are working on the following improvements:

- [ ] FTP/SFTP download support
- [ ] Web interface for download monitoring
- [ ] Cloud storage service integration
- [ ] Custom plugin system
- [ ] Torrent download support
- [ ] Adaptive compression improvements
- [ ] Cache system optimization
- [ ] Additional proxy protocol support
- [ ] Desktop GUI interface
- [ ] Multi-language documentation

Want to contribute to any of these features? Check out our [contributing guide](CONTRIBUTING.md)!

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
