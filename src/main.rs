#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(feature = "gui"))]
use clap::CommandFactory;
use clap::Parser;
use std::error::Error;

#[cfg(feature = "gui")]
use std::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};

#[cfg(all(windows, not(debug_assertions)))]
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

mod interactive;

#[cfg(feature = "gui")]
mod gui;

#[cfg(feature = "gui")]
use crate::gui::KGetGui;
use kget::advanced_download::AdvancedDownloader;
#[cfg(feature = "gui")]
use kget::app::{DownloadCommand, WorkerToGuiMessage, spawn_download_worker};
use kget::config::{Config, ProxyType};
use kget::download::download as cli_download;
use kget::ftp::FtpDownloader;
use kget::optimization::Optimizer;
use kget::sftp::SftpDownloader;
use kget::utils;
use kget::{DownloadOptions, verify_iso_integrity};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(disable_version_flag = true)]
struct Args {
    /// Show version information
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    /// URL file for the download
    #[arg(default_value_t = String::new())]
    url: String,

    /// Output file name
    #[arg(short = 'O', long = "output")]
    output: Option<String>,

    /// Silent mode (no progress bar)
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// Use advanced download (parallel and resumable)
    #[arg(short = 'a', long = "advanced")]
    advanced: bool,

    /// Use torrent download (magnet links)
    #[arg(short = 't', long = "torrent")]
    torrent: bool,

    /// Proxy URL (ex: http://proxy:8080)
    #[arg(short = 'p', long = "proxy")]
    proxy: Option<String>,

    /// Proxy user
    #[arg(long = "proxy-user")]
    proxy_user: Option<String>,

    /// Proxy password
    #[arg(long = "proxy-pass")]
    proxy_pass: Option<String>,

    /// Proxy type (http, https, socks5)
    #[arg(long = "proxy-type", default_value = "http")]
    proxy_type: String,

    /// Speed limit in bytes/second
    #[arg(short = 'l', long = "limit")]
    speed_limit: Option<u64>,

    /// Disable cache
    #[arg(long = "no-cache")]
    no_cache: bool,

    /// Maximum number of peers for torrent download
    #[arg(long = "max-peers")]
    max_peers: Option<usize>,

    /// Maximum number of seeds for torrent download
    #[arg(long = "max-seeds")]
    max_seeds: Option<usize>,

    /// Port for torrent connections
    #[arg(long = "torrent-port")]
    torrent_port: Option<u16>,

    /// Disable DHT for torrents
    #[arg(long = "no-dht")]
    no_dht: bool,

    /// Expected SHA256 hash for automatic verification after download
    #[arg(long = "sha256")]
    sha256: Option<String>,

    /// Use GUI mode
    #[arg(long = "gui")]
    gui: bool,

    /// Use FTP download
    #[arg(long = "ftp")]
    ftp: bool,

    /// Use SFTP download
    #[arg(long = "sftp")]
    sftp: bool,

    /// Use interactive mode
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    #[cfg(all(windows, not(debug_assertions)))]
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let args = Args::parse();
    let mut config = Config::load()?;

    if args.interactive {
        interactive::interactive_mode();
        return Ok(());
    }

    let should_start_gui =
        args.gui || (args.url.is_empty() && !args.ftp && !args.sftp && !args.torrent);

    if !should_start_gui {
        if let Some(proxy_url) = args.proxy.clone() {
            config.proxy.enabled = true;
            config.proxy.url = Some(proxy_url);
        }
        if let Some(user) = args.proxy_user.clone() {
            config.proxy.username = Some(user);
        }
        if let Some(pass) = args.proxy_pass.clone() {
            config.proxy.password = Some(pass);
        }
        config.proxy.proxy_type = match args.proxy_type.to_lowercase().as_str() {
            "https" => ProxyType::Https,
            "socks5" => ProxyType::Socks5,
            _ => ProxyType::Http,
        };
        if let Some(limit) = args.speed_limit {
            config.optimization.speed_limit = Some(limit);
        }
        if args.no_cache {
            config.optimization.cache_enabled = false;
        }
        if args.torrent {
            config.torrent.enabled = true;
        }
        if let Some(max_peers) = args.max_peers {
            config.torrent.max_peers = max_peers;
        }
        if let Some(max_seeds) = args.max_seeds {
            config.torrent.max_seeds = max_seeds;
        }
        if let Some(port) = args.torrent_port {
            config.torrent.port = Some(port);
        }
        if args.no_dht {
            config.torrent.dht_enabled = false;
        }
        config.save()?;
    }

    if should_start_gui {
        #[cfg(feature = "gui")]
        {
            let (download_tx, download_rx_worker): (
                MpscSender<DownloadCommand>,
                MpscReceiver<DownloadCommand>,
            ) = mpsc::channel();
            let (status_tx_worker, status_rx_gui): (
                MpscSender<WorkerToGuiMessage>,
                MpscReceiver<WorkerToGuiMessage>,
            ) = mpsc::channel();

            spawn_download_worker(config.clone(), download_rx_worker, status_tx_worker);

            let mut native_options = eframe::NativeOptions::default();
            native_options.viewport.inner_size = Some(egui::Vec2::new(980.0, 680.0));
            native_options.viewport.min_inner_size = Some(egui::Vec2::new(820.0, 560.0));
            native_options.viewport.resizable = Some(true);

            // Embed logo in binary so it works in .app bundle
            let logo_bytes = include_bytes!("../logo.png");
            if let Ok(img) = image::load_from_memory(logo_bytes) {
                let rgba_img = img.into_rgba8();
                let (width, height) = rgba_img.dimensions();
                let rgba = rgba_img.into_raw();
                let icon = egui::IconData {
                    rgba,
                    width,
                    height,
                };
                native_options.viewport.icon = Some(std::sync::Arc::new(icon));
            }

            if let Err(e) = eframe::run_native(
                "KGet Downloader",
                native_options,
                Box::new(move |cc| {
                    egui_extras::install_image_loaders(&cc.egui_ctx);
                    Ok(Box::new(KGetGui::new(cc, download_tx, status_rx_gui)))
                }),
            ) {
                eprintln!("Failed to launch GUI: {e}");
                return Err("Failed to launch GUI".into());
            }

            return Ok(());
        }

        #[cfg(not(feature = "gui"))]
        {
            eprintln!(
                "GUI support was not compiled in. Rebuild with `--features gui` to enable it."
            );

            if args.gui {
                return Err("GUI not available (compile with --features gui)".into());
            }

            if args.url.is_empty() {
                Args::command().print_help()?;
                return Ok(());
            }
        }
    }

    // ==========================================================
    //                         CLI MODE
    // ==========================================================

    let optimizer = Optimizer::from_config(config.optimization.clone());

    if args.ftp {
        let url = args.url.clone();
        let output = utils::resolve_output_path(args.output, &url, "ftp_output");
        let downloader =
            FtpDownloader::new(url.to_owned(), output, args.quiet, config.proxy, optimizer);
        return downloader.download();
    }

    if args.sftp {
        let url = args.url.clone();
        let output = utils::resolve_output_path(args.output, &url, "sftp_output");
        let downloader =
            SftpDownloader::new(url.to_owned(), output, args.quiet, config.proxy, optimizer);
        return downloader.download();
    }

    if args.torrent || args.url.starts_with("magnet:?") {
        let output_dir = args.output.unwrap_or_else(|| "torrent_output".to_string());

        kget::torrent::download_magnet(
            &args.url,
            &output_dir,
            args.quiet,
            config.proxy,
            optimizer,
            kget::torrent::TorrentCallbacks::default(),
        )?;
    } else if args.advanced {
        let output = utils::resolve_output_path(args.output, &args.url, "advanced_output");
        let mut downloader = AdvancedDownloader::new(
            args.url.clone(),
            output,
            args.quiet,
            config.proxy,
            optimizer,
        );
        if let Some(expected_sha256) = args.sha256.clone() {
            downloader.set_expected_sha256(expected_sha256);
        }
        downloader.download()?;
    } else {
        let options = DownloadOptions {
            quiet_mode: args.quiet,
            output_path: args.output.clone(),
            verify_iso: args.sha256.is_some(),
            expected_sha256: args.sha256.clone(),
        };

        cli_download(&args.url, config.proxy, optimizer, options, None)?;

        if !args.quiet && args.url.to_lowercase().ends_with(".iso") {
            println!("\nThis is an ISO file. Would you like to verify its integrity? (y/N)");
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() && input.trim().to_lowercase() == "y"
            {
                let filename = utils::get_filename_from_url_or_default(&args.url, "download.iso");
                let path = std::path::Path::new(&filename);
                verify_iso_integrity(path, None)?;
            }
        }
    }

    Ok(())
}
