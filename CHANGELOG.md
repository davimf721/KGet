# Changelog

[English](CHANGELOG.md) | [Português](translations/CHANGELOG.pt-BR.md) | [Español](translations/CHANGELOG.es.md)

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.5.2] - 2025-12-19

### Added
- **ISO Smart Handling**: Automatic detection of `.iso` files via URL and MIME type.
- **Corruption Prevention**: ISO files now bypass decompression/optimization layers to ensure binary 1:1 integrity.
- **Integrity Verification**: Added optional SHA256 checksum calculation at the end of ISO downloads.
- **Windows Dual Mode**: The application now runs without a console window when launched via Explorer, but automatically attaches to the terminal when run via CLI.

### Fixed
- **Memory & Disk Optimization**: Refactored `AdvancedDownloader` to use streaming writes with `BufWriter`, drastically reducing RAM usage and preventing 100% disk active time issues.
- **Verification Prompt**: Fixed bug where integrity check was running automatically in advanced mode; it now correctly asks the user for confirmation.
- **UI/UX**: Cleaned up terminal output during parallel downloads for a smoother progress bar experience.
- **GUI Icon**: Fixed issue with loading the application window icon.
- Fixed Rust compiler error `E0382` regarding `Mime` type ownership in `download.rs`.
- Improved parallel chunk writing safety for binary-heavy files.

## [1.5.1] - 2025-12-18

### Added
- Optional `gui` Cargo feature to make GUI dependencies opt-in; compile with `--features gui` to enable GUI support.
- Top-level convenience functions: `kget::download(...)` and `kget::advanced_download(...)` for easier library usage.
- `create_progress_bar_factory(...)` exported to let consumers create `indicatif` progress bars.
- `examples/lib_usage.rs` example demonstrating library usage.
- Docker development instructions and `docker-compose` integration to simplify building, testing and contributing.

### Changed
- Updated README and `LIB.md` with library usage instructions and examples.
- `CONTRIBUTING.md` and translations updated with Docker contributor workflow.
- GUI code split: `gui_types` module added so CLI builds without GUI feature.

### Fixed / Misc
- Minor documentation fixes and translation updates (PT-BR/ES).


## [1.5.0] - 2025-05-26

### Added
- New public Rust crate: KGet can now be used as a library in your own Rust projects, click [here](LIB.md) to see more.
- Improved GUI: larger fonts, better layout, and more intuitive controls.
- Clipboard integration for easy pasting of URLs.
- Download button and cancel button now always visible and functional in the GUI.
- **Interactive mode:** Run `kget --interactive` for a REPL-like experience with commands such as `download <url> [output]`, `help`, and `exit`.

### Changed
- Project renamed from "KelpsGet" to "KGet" for simplicity and consistency.
- Versioning scheme updated from 0.1.4 to 1.5.0 to allow for more frequent minor updates and clearer release tracking.
- Features list moved from README to CHANGELOG for easier maintenance and to keep the README concise.

### Removed
- Redundant or overly detailed features section from the README (now see CHANGELOG for all features).

## [0.1.4] - 2025-05-22

### Added
- Graphical User Interface (GUI) for easier downloads.
- FTP download support.
- SFTP download support (password and key-based authentication).
- Torrent download support via magnet links (integrates with Transmission daemon).
- Detailed instructions for Transmission daemon setup in README.

### Changed
- Refined output path determination to align comportamento with `wget`.
- Ensured `final_path` is always absolute to prevent "No such file or directory" errors in CWD.
- Updated README in English, Portuguese, and Spanish to reflect all new features and setup instructions.

### Fixed
- Resolved "No such file or directory" error when downloading without `-O` by ensuring absolute paths.
- Corrected `validate_filename` to only check the base filename, not the full path.
- Addressed potential issues with `map_err` in `main.rs` for torrent and HTTP downloads.

## [0.1.3] - 2025-03-11

### Added
- Advanced download mode with parallel chunks and resume capability
- Automatic compression support (gzip, brotli, lz4)
- Intelligent caching system for faster repeated downloads
- Speed limiting and connection control
- Multi-language documentation support

### Changed
- Improved error handling and user feedback
- Enhanced progress bar with more detailed information
- Optimized memory usage for large file downloads
- Updated proxy configuration system

### Fixed
- Fixed proxy authentication issues
- Resolved cache directory creation problems
- Fixed compression level handling
- Corrected file path handling on Windows

### Security
- Added secure proxy connection handling
- Improved URL validation
- Enhanced file name sanitization
- Added space checking before downloads

## [0.1.2] - 2025-03-10

### Added
- Proxy support (HTTP, HTTPS, SOCKS5)
- Proxy authentication
- Custom output file naming
- MIME type detection

### Changed
- Improved download speed calculation
- Enhanced progress bar display
- Better error messages
- Updated documentation

### Fixed
- Fixed connection timeout issues
- Resolved file permission problems
- Corrected URL parsing
- Fixed progress bar display on Windows

## [0.1.1] - 2025-03-09

### Added
- Silent mode for script integration
- Basic progress bar
- File size display
- Download speed tracking

### Changed
- Improved error handling
- Enhanced command-line interface
- Better file handling
- Updated installation instructions

### Fixed
- Fixed path handling issues
- Resolved permission problems
- Corrected progress display
- Fixed file overwrite behavior

## [0.1.0] - 2025-03-08

### Added
- Initial release
- Basic file download functionality
- Command-line interface
- Basic error handling
- Cross-platform support