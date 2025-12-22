//! KGet Library Usage Showcase
//!
//! This example file demonstrates every possible way to use KGet as a library.
//!
//! Usage:
//!   cargo run --example lib_usage -- <command> [args]
//!
//! Commands:
//!   simple <url> [output]      - Standard HTTP download.
//!   advanced <url> <output>    - Parallel, resumable download with streaming.
//!   auto-verify <url> <output> - Download an ISO and verify SHA256 without prompting.
//!   progress-only              - Use KGet's styled progress bar for a custom task.
//!   verify-only <path>         - Use the standalone SHA256 integrity checker.
//!   config-custom              - Tweak Proxy and Optimizer settings programmatically.

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use kget::{
    create_progress_bar, download, verify_iso_integrity, AdvancedDownloader, 
    Config, DownloadOptions, Optimizer, ProxyType,
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
        _ => print_usage(),
    }

    Ok(())
}

fn print_usage() {
    println!("KGet Library Examples - Usage Guide");
    println!("  simple <url> [output]      | Basic HTTP/HTTPS download");
    println!("  advanced <url> <output>    | Parallel chunks + RAM/Disk optimization");
    println!("  auto-verify <url> <output> | ISO download with automatic SHA256 check");
    println!("  progress-only              | Standalone usage of KGet progress bars");
    println!("  verify-only <path>         | Standalone SHA256 file checker");
    println!("  config-custom              | Programmatic config/proxy setup");
}

/// 1. Simple Download API
fn run_simple(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let url = args.next().expect("URL required");
    let output = args.next();

    let config = Config::default();
    let options = DownloadOptions {
        output_path: output,
        ..Default::default()
    };

    println!("Starting simple download...");
    download(&url, config.proxy, Optimizer::new(config.optimization), options, None)?;
    Ok(())
}

/// 2. Advanced Downloader API (Parallel + Optimized)
fn run_advanced(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let url = args.next().expect("URL required");
    let output = args.next().expect("Output path required");

    let config = Config::default();
    let optimizer = Optimizer::new(config.optimization);

    // The AdvancedDownloader uses BufWriter (2MB) and 16KB streaming
    // to ensure high speed with low RAM usage and low Disk IOPS.
    let downloader = AdvancedDownloader::new(
        url, 
        output, 
        false, // quiet_mode
        config.proxy, 
        optimizer
    );

    println!("Starting advanced parallel download...");
    downloader.download()?;
    Ok(())
}

/// 3. Automatic ISO Verification (No stdin prompt)
fn run_auto_verify(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let url = args.next().expect("URL required");
    let output = args.next().expect("Output path required");

    let config = Config::default();
    let options = DownloadOptions {
        output_path: Some(output),
        verify_iso: true, // The lib will verify automatically without asking
        quiet_mode: false,
    };

    println!("Downloading ISO with automatic integrity check enabled...");
    download(&url, config.proxy, Optimizer::new(config.optimization), options, None)?;
    Ok(())
}

/// 4. Custom Progress Bar Usage
fn run_progress_demo() {
    let total_steps = 100;
    // Create a bar with KGet's green style, smooth increments, and ETA
    let bar = create_progress_bar(
        false, 
        "External Library Task".to_string(), 
        Some(total_steps), 
        false
    );

    for _ in 0..total_steps {
        std::thread::sleep(Duration::from_millis(50));
        bar.inc(1);
    }

    bar.finish_with_message("Task completed successfully!");
}

/// 5. Standalone Integrity Checker
fn run_verify_only(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = args.next().expect("File path required");
    println!("Running SHA256 check for: {}", path);
    verify_iso_integrity(Path::new(&path), None)?;
    Ok(())
}

/// 6. Tweak Configuration programmatically
fn run_config_custom() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut config = Config::load().unwrap_or_default();

    // Setup a proxy programmatically
    config.proxy.enabled = true;
    config.proxy.url = Some("http://my-proxy:8080".to_string());
    config.proxy.proxy_type = ProxyType::Http;

    // Adjust optimization limits
    config.optimization.max_connections = 8;
    
    println!("Custom config initialized. Proxy enabled: {}", config.proxy.enabled);
    println!("Ready to pass this 'config.proxy' to any download function.");
    Ok(())
}
