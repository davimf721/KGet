# KelpsGet
A modern, lightweight wget clone written in Rust for fast and reliable file downloads from the command line.

## Features
✅ Simple CLI tool for downloading files via HTTP/HTTPS.
✅ Progress bar with real-time speed and ETA tracking.
✅ Custom output names (-O flag to rename downloaded files).
✅ MIME type detection and proper file handling.
✅ Cross-platform (Linux, macOS, Windows).

## Installation
### Option 1: Install via Cargo
```bash
cargo install kelpsget
```
### Option 2: Download Pre-built Binaries
Download the latest binary for your OS from [Release](https://github.com/davimf721/KelpsGet/releases/tag/beta)
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
## How It Works
1. Progress Bar: Shows download speed, ETA, and bytes transferred.
2.  Smart File Naming:
  - Uses the filename from the URL (e.g., file.txt from https://example.com/file.txt).
  - Defaults to index.html if the URL ends with /.
3. Error Handling: Exits with code 1 on HTTP errors (e.g., 404).

## Contributing
Found a bug or want to add a feature? Open an issue or submit a PR!

🚀 Download files effortlessly with Rust's speed and reliability. 🚀


