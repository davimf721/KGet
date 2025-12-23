<img width="1000" height="500" alt="image" src="https://github.com/user-attachments/assets/d0888e3f-90a2-42d6-a9aa-b216dc36f1f4" />

# KGet!  v1.5.3 (Latest Release)

A modern, lightweight, and versatile downloader written in Rust for fast and reliable file downloads via command line (CLI) and graphical user interface (GUI).

[English](README.md) | [Português](translations/README.pt-BR.md) | [Español](translations/README.es.md)

## Screenshots
- GUI:
 <img src="https://github.com/user-attachments/assets/6da6dbd4-b3ae-4669-b64b-ffe3b961beb2"  width="600"/>
 
- Torrent on `localhost:9091/transmission/web/`:
 <img src="https://github.com/user-attachments/assets/d80b60d7-f53e-4198-8e11-1cacf0e78958"  width="600"/>

- CLI:
 <img src="https://github.com/user-attachments/assets/a835c4df-5424-4aaa-b687-2445a99ba067"  width="600"/>

- Interactive:
<img src="https://github.com/user-attachments/assets/c8d03a5c-6459-4f3d-a581-5180797f8b1c"  width="600"/>

## How It Works (Summary)
1.  **Progress Bar (CLI):** Shows speed, ETA, and transferred bytes.
2.  **Smart File Naming:**
    *   Uses the filename from the URL.
    *   Defaults to `index.html` if the URL ends with `/`.
3.  **Error Handling:** Exits with code 1 on HTTP errors (e.g., 404).
4.  **Space Check:** Verifies available disk space.
5.  **Automatic Retry:** Retries download on network failure.
6.  **ISO Smart Detection:** Detects `.iso` files to ensure raw binary transfer and prevent corruption.
7.  **Integrity Check:** Optional SHA256 verification for disk images after download.
8.  **Memory Efficient:** Parallel downloads use streaming buffers to maintain a low RAM footprint regardless of file size.
9.  **Disk Optimization:** Uses buffered I/O to prevent high disk active time and system freezes during fast transfers.
10. **Advanced Download Mode (HTTP/HTTPS):** Downloads in parallel chunks, supports resume.
11. **Proxy Support:** HTTP, HTTPS, SOCKS5 with authentication.
12. **Optimization Features:** Compression (for cache), file caching, speed limiting.
13. **Torrent Downloads (Magnet Links):**
    * **Default:** opens the magnet link using your system's default BitTorrent client (qBittorrent/Transmission/etc).
    * **Optional (feature):** can download via Transmission RPC (`torrent-transmission` feature).
14. **FTP/SFTP Downloads:** Connects to FTP/SFTP servers to transfer files.

## Features
See the full list of features and recent changes in the [CHANGELOG](CHANGELOG.md).

## KGet now is a Library too!
If you want to use KGet as a library you can click [here](LIB.md).

## Optional Cargo features

### GUI (`gui`)
Build/run with GUI support:

```bash
cargo build --features gui
cargo run --features gui -- --gui
```

### Transmission RPC torrent backend (`torrent-transmission`)
If you want KGet to add magnet links to a Transmission daemon (RPC), build with:

```bash
cargo build --features torrent-transmission
# or with GUI:
cargo build --features "gui torrent-transmission"
```

Select the backend at runtime:

- Default (no env var): uses the system torrent client (`xdg-open`/`open`/`start`)
- Transmission RPC:

```bash
# Linux/macOS
export KGET_TORRENT_BACKEND=transmission

# Windows PowerShell (current session)
$env:KGET_TORRENT_BACKEND="transmission"
```

Transmission settings (env vars):
- `KGET_TRANSMISSION_HOST` (default: `localhost`)
- `KGET_TRANSMISSION_PORT` (default: `9091`)
- `KGET_TRANSMISSION_RPC_PATH` (default: `/transmission/rpc`)
- `KGET_TRANSMISSION_WEB_PATH` (default: `/transmission/web/`)
- Optional auth: `KGET_TRANSMISSION_USER`, `KGET_TRANSMISSION_PASS`

Compatibility:
- You can also use `KGET_TRANSMISSION_URL` and `KGET_TRANSMISSION_WEB` (full URLs).

## Installation

### Option 1: Compile from source
You will need Rust installed. If you don't have it, install it from [rustup.rs](https://rustup.rs/).

Install some dependencies:
For Debian/Ubuntu based systems:
```bash
sudo apt update
sudo apt install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev pkg-config
```
For Fedora:
```bash
sudo dnf install -y libxcb-devel libxkbcommon-devel openssl-devel pkg-config
```

Clone the repository and compile the project:
```bash
git clone https://github.com/davimf721/KGet.git
cd KGet
cargo build --release
```
The executable will be in `target/release/kget`. You can copy it to a directory in your `PATH`:
```bash
sudo cp target/release/kget /usr/local/bin/
```

### Option 2: Install via Cargo
You can install the published binary from crates.io (the GUI and Transmission backend are opt-in via features):

```bash
# Install the binary without GUI (default)
cargo install Kget

# Install with GUI support
cargo install Kget --features gui

# Install with Transmission RPC backend (optional)
cargo install Kget --features torrent-transmission

# Install with both
cargo install Kget --features "gui torrent-transmission"
```

If you encounter issues with the GUI when installing via `cargo install`, compiling from source is more reliable.

### Option 3: Download Pre-compiled Binaries
Check the [Releases](https://github.com/davimf721/KGet/releases) section for the latest binaries for your OS.

#### Linux/macOS:
```bash
chmod +x ksget  # Make executable
./kelpsget [URL]    # Run directly
```
#### Windows:
Run the `.exe` file directly.

## Usage

### Command Line (CLI)
```bash
kget [OPTIONS] <URL>
```
**Examples:**
*   **HTTP/HTTPS Download:**
    ```bash
    kget https://example.com/file.txt
    ````
*   **Rename Output File:**
    ```bash
    kget -O new_name.txt https://example.com/file.txt
    kget -O ~/MyDownloads/ https://example.com/video.mp4 # Saves as ~/MyDownloads/video.mp4
    ````
*   **FTP Download:**
    ```bash
    kget ftp://user:password@ftp.example.com/archive.zip
    kget --ftp ftp://ftp.example.com/pub/file.txt
    ````
*   **SFTP Download:**
    (Requires SSH key setup or password if the server allows it)
    ```bash
    kget sftp://user@sftp.example.com/path/file.dat
    kget --sftp sftp://user@sftp.example.com/path/file.dat -O local.dat
    ````
*   **Torrent Download (Magnet Link):**
    (Requires `transmission-daemon` configured and running)
    ```bash
    kget "magnet:?xt=urn:btih:YOUR_HASH_HERE&dn=TorrentName"
    kget --torrent "magnet:?xt=urn:btih:YOUR_HASH_HERE" -O ~/MyTorrents/
    ````
    KelpsGet will add the torrent to Transmission and attempt to open the web interface (`http://localhost:9091`) for management.

*   **Silent Mode:**
    ```bash
    kget -q https://example.com/file.txt
    ````
*   **Advanced Download Mode (HTTP/HTTPS):**
    ```bash
    kget -a https://example.com/large_file.zip
    ```
    *   **ISO Download with Verification:**
        KGet will automatically detect the ISO and ask if you want to verify the SHA256 hash once finished.
*   **Use Proxy:**
    ```bash
    kget -p http://proxy:8080 https://example.com/file.txt
    ````
*   **Proxy with Authentication:**
    ```bash
    kget -p http://proxy:8080 --proxy-user user --proxy-pass pass https://example.com/file.txt
    ````
*   **Speed Limit:**
    ```bash
    kget -l 1048576 https://example.com/file.txt  # Limit to 1MB/s
    ````
*   **Disable Compression (KGet-specific, not HTTP):**
    ```bash
    kget --no-compress https://example.com/file.txt
    ````
*   **Disable Cache (KGet-specific):**
    ```bash
    kget --no-cache https://example.com/file.txt
    ````


## 🔗 Important Links
- 📚 [Documentation](https://davimf721.github.io/KGet/)
- 📦 [crates.io](https://crates.io/crates/Kget)
- 💻 [GitHub](https://github.com/davimf721/KGet)
- 📝 [Changelog](CHANGELOG.md)

## You can see posts about the project in others communities:
- [Dev.to](https://dev.to/davimf7221/kelpsget-v014-modern-download-manager-in-rust-4b9f)
- [r/rust](https://www.reddit.com/r/rust/comments/1kt69vh/after_5_months_of_development_i_finally_released/)
- [PitchHut](https://www.pitchhut.com/project/kelpsget)
- [Hacker News](https://hn.algolia.com/?query=Show%20HN%3A%20KelpsGet%20%E2%80%93%20Modern%20download%20manager%20built%20in%20Rust&type=story&dateRange=all&sort=byDate&storyText=false&prefix&page=0)

## Contributing
Want to contribute? Check out our [contribution guide](CONTRIBUTING.md)!

Found a bug or want to add a feature? Open an issue or send a PR!

🚀 Download files effortlessly with the speed and reliability of Rust. 🚀

## License
This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
