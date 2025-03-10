# KelpsGet v0.1.2

A modern, lightweight wget clone written in Rust for fast and reliable file downloads from the command line.

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
Silent Mode:
```bash
kelpsget -q https://example.com/file.txt
```
Advanced Download Mode (Parallel and Resumable):
```bash
kelpsget -a https://example.com/large_file.zip
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

## Security Features
- Space Checking: Ensures enough disk space before downloading.
- File Name Validation: Prevents path injection.
- URL Handling: Safely handles URLs.
- Automatic Retry: Retries download if network fails.

## Contributing
Found a bug or want to add a feature? Open an issue or submit a PR!

🚀 Download files effortlessly with Rust's speed and reliability. 🚀

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.


