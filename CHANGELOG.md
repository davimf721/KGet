# Changelog

[English](CHANGELOG.md) | [Português](translations/CHANGELOG.pt-BR.md) | [Español](translations/CHANGELOG.es.md)

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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