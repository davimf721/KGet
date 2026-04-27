//! # KGet Library Usage Examples
//!
//! This example demonstrates all the ways to use KGet as a library in your Rust projects.
//!
//! ## Running Examples
//!
//! ```bash
//! cargo run --example lib_usage -- <command> [args]
//! ```
//!
//! ## Available Commands
//!
//! | Command | Description |
//! |---------|-------------|
//! | `simple <url> [output]` | Standard HTTP/HTTPS download |
//! | `advanced <url> <output>` | Parallel download with progress callbacks |
//! | `auto-verify <url> <output>` | Download ISO and verify SHA256 |
//! | `progress-only` | Standalone progress bar demo |
//! | `verify-only <path>` | SHA256 file integrity checker |
//! | `config-custom` | Custom configuration example |
//! | `torrent <magnet> [output_dir]` | Download via magnet link |

use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kget::{
    AdvancedDownloader, Config, DownloadOptions, Optimizer, ProxyConfig, ProxyType,
    create_progress_bar, download,
    torrent::{TorrentCallbacks, download_magnet},
    verify_iso_integrity,
};

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "simple" => run_simple(args)?,
        "advanced" => run_advanced(args)?,
        "auto-verify" => run_auto_verify(args)?,
        "progress-only" => run_progress_demo(),
        "verify-only" => run_verify_only(args)?,
        "config-custom" => run_config_custom()?,
        "torrent" => run_torrent(args)?,
        _ => print_usage(),
    }

    Ok(())
}

fn print_usage() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              KGet Library Usage Examples                      ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ simple <url> [output]      │ Basic HTTP/HTTPS download        ║");
    println!("║ advanced <url> <output>    │ Parallel chunks + callbacks      ║");
    println!("║ auto-verify <url> <output> │ ISO download with SHA256 check   ║");
    println!("║ progress-only              │ Standalone progress bar demo     ║");
    println!("║ verify-only <path>         │ Standalone SHA256 file checker   ║");
    println!("║ config-custom              │ Programmatic config setup        ║");
    println!("║ torrent <magnet> [dir]     │ Download via magnet link         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}

// ============================================================================
// Example 1: Simple Download
// ============================================================================

/// Basic download using the simple `download()` function.
///
/// This is the easiest way to download a file - just provide a URL.
fn run_simple(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let url = args.next().expect("Usage: simple <url> [output]");
    let output = args.next();

    let options = DownloadOptions {
        output_path: output,
        quiet_mode: false,
        verify_iso: false,
        expected_sha256: None,
    };

    println!("📥 Starting simple download: {}", url);
    download(
        &url,
        ProxyConfig::default(),
        Optimizer::new(),
        options,
        None,
    )?;

    println!("✅ Download complete!");
    Ok(())
}

// ============================================================================
// Example 2: Advanced Download with Progress Callbacks
// ============================================================================

/// Advanced download with parallel connections and progress tracking.
///
/// Use this for large files or when you need progress callbacks.
fn run_advanced(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let url = args.next().expect("Usage: advanced <url> <output>");
    let output = args.next().expect("Usage: advanced <url> <output>");

    // Create downloader with custom settings
    let mut downloader = AdvancedDownloader::new(
        url.clone(),
        output.clone(),
        false, // quiet_mode = false (show output)
        ProxyConfig::default(),
        Optimizer::new(),
    );

    // Set progress callback (receives 0.0 to 1.0)
    downloader.set_progress_callback(|progress| {
        let percent = progress * 100.0;
        let filled = (progress * 30.0) as usize;
        let empty = 30 - filled;
        print!(
            "\r[{}{}] {:.1}%",
            "█".repeat(filled),
            "░".repeat(empty),
            percent
        );
        use std::io::Write;
        std::io::stdout().flush().ok();
    });

    // Set status callback for human-readable messages
    downloader.set_status_callback(|msg| {
        println!("\n📊 {}", msg);
    });

    println!("🚀 Starting advanced parallel download...");
    println!("   URL: {}", url);
    println!("   Output: {}", output);

    downloader.download()?;

    println!("\n✅ Download complete!");
    Ok(())
}

// ============================================================================
// Example 3: ISO Download with Automatic Verification
// ============================================================================

/// Download an ISO file and automatically verify its SHA256 hash.
fn run_auto_verify(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let url = args.next().expect("Usage: auto-verify <url> <output>");
    let output = args.next().expect("Usage: auto-verify <url> <output>");

    let options = DownloadOptions {
        output_path: Some(output.clone()),
        verify_iso: true, // Automatically verify after download
        quiet_mode: false,
        expected_sha256: None,
    };

    println!("💿 Downloading ISO with automatic integrity check...");
    println!("   URL: {}", url);
    println!("   Output: {}", output);

    download(
        &url,
        ProxyConfig::default(),
        Optimizer::new(),
        options,
        Some(&|status| println!("📊 {}", status)), // Status callback
    )?;

    println!("✅ Download and verification complete!");
    Ok(())
}

// ============================================================================
// Example 4: Standalone Progress Bar
// ============================================================================

/// Use KGet's progress bar for your own tasks.
fn run_progress_demo() {
    println!("🎨 KGet Progress Bar Demo\n");

    let total_steps: u64 = 100;

    // Create progress bar with KGet's styling
    let bar = create_progress_bar(
        false,                          // quiet_mode
        "Processing files".to_string(), // message
        Some(total_steps),              // total (None for spinner)
        false,                          // is_parallel
    );

    for i in 0..total_steps {
        std::thread::sleep(Duration::from_millis(30));
        bar.set_position(i + 1);
    }

    bar.finish_with_message("✅ All files processed!");

    // Demo: Indeterminate spinner
    println!("\n🔄 Spinner demo (unknown duration)...");
    let spinner = create_progress_bar(
        false,
        "Connecting to server".to_string(),
        None, // No total = spinner mode
        false,
    );

    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(50));
        spinner.tick();
    }

    spinner.finish_with_message("✅ Connected!");
}

// ============================================================================
// Example 5: Standalone File Verification
// ============================================================================

/// Verify SHA256 integrity of any file.
fn run_verify_only(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = args.next().expect("Usage: verify-only <file_path>");

    println!("🔐 Calculating SHA256 hash for: {}", path);
    println!("   This may take a while for large files...\n");

    verify_iso_integrity(Path::new(&path), Some(&|status| println!("   {}", status)))?;

    println!("\n💡 Compare this hash with the one provided by the source.");
    Ok(())
}

// ============================================================================
// Example 6: Custom Configuration
// ============================================================================

/// Programmatically configure proxy and optimization settings.
fn run_config_custom() -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("⚙️  Custom Configuration Demo\n");

    // Load existing config or use defaults
    let mut config = Config::load().unwrap_or_default();

    // ── Configure Proxy ──
    config.proxy.enabled = true;
    config.proxy.url = Some("http://proxy.example.com:8080".to_string());
    config.proxy.proxy_type = ProxyType::Http;
    // For authenticated proxy:
    // config.proxy.username = Some("user".to_string());
    // config.proxy.password = Some("pass".to_string());

    // ── Configure Optimization ──
    config.optimization.max_connections = 8; // Parallel connections
    config.optimization.compression = true; // Enable caching compression
    config.optimization.speed_limit = Some(5_000_000); // 5 MB/s limit

    // ── Configure Torrent ──
    config.torrent.enabled = true;
    config.torrent.dht_enabled = true;
    config.torrent.max_peers = 100;

    println!("📋 Configuration Summary:");
    println!(
        "   Proxy: {} ({})",
        config.proxy.url.as_deref().unwrap_or("none"),
        if config.proxy.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "   Max Connections: {}",
        config.optimization.max_connections
    );
    println!(
        "   Speed Limit: {:?} bytes/s",
        config.optimization.speed_limit
    );
    println!("   Torrent DHT: {}", config.torrent.dht_enabled);

    // Save config for future use
    // config.save()?;

    println!("\n✅ Config ready! Pass config.proxy to download functions.");

    // Example: Using custom config with AdvancedDownloader
    let _downloader = AdvancedDownloader::new(
        "https://example.com/file.zip".to_string(),
        "file.zip".to_string(),
        false,
        config.proxy.clone(),
        Optimizer::from_config(config.optimization),
    );

    Ok(())
}

// ============================================================================
// Example 7: Torrent Download
// ============================================================================

/// Download a file via magnet link using the native torrent client.
fn run_torrent(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let magnet = args
        .next()
        .expect("Usage: torrent <magnet_link> [output_dir]");
    let output_dir = args.next().unwrap_or_else(|| ".".to_string());

    println!("🧲 Starting torrent download...");
    println!("   Magnet: {}...", &magnet[..magnet.len().min(60)]);
    println!("   Output: {}\n", output_dir);

    // Create callbacks for progress and status
    let callbacks = TorrentCallbacks {
        status: Some(Arc::new(|msg| {
            println!("📊 {}", msg);
        })),
        progress: Some(Arc::new(|progress| {
            let percent = progress * 100.0;
            let filled = (progress * 30.0) as usize;
            let empty = 30 - filled;
            print!(
                "\r🧲 [{}{}] {:.1}%",
                "█".repeat(filled),
                "░".repeat(empty),
                percent
            );
            use std::io::Write;
            std::io::stdout().flush().ok();
        })),
    };

    download_magnet(
        &magnet,
        &output_dir,
        false, // quiet
        ProxyConfig::default(),
        Optimizer::new(),
        callbacks,
    )?;

    println!("\n✅ Torrent download complete!");
    Ok(())
}
