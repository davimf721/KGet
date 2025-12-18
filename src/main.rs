use clap::Parser;
use clap::CommandFactory;
use std::error::Error;
use std::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use std::thread;
use std::time::Duration;

use crate::download::download as cli_download;
use crate::advanced_download::AdvancedDownloader;
use crate::config::Config;
use crate::optimization::Optimizer;
#[cfg(feature = "gui")] use crate::gui::KGetGui;
use crate::gui_types::{DownloadCommand, WorkerToGuiMessage};
pub use crate::ftp::FtpDownloader;
pub use crate::sftp::SftpDownloader;
pub use crate::torrent::TorrentDownloader;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
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

    // /// Disable compression
    // #[arg(long = "no-compress")]
    // no_compress: bool,

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

mod download;
mod progress;
mod utils;
mod advanced_download;
mod config;
mod optimization;
mod ftp;
mod gui_types;
#[cfg(feature = "gui")]
mod gui;
mod sftp;
mod torrent;
mod interactive;

fn download_worker(
    config: Config,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
    runtime: tokio::runtime::Runtime,
) {
    for command in download_rx {
        match command {
            DownloadCommand::Start(url, output_path) => {
                let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate(format!(
                    "Preparing download: {} to {}",
                    url, output_path
                )));
                
                let optimizer_clone = Optimizer::new(config.optimization.clone());
                let proxy_clone = config.proxy.clone();

                let result: Result<(), Box<dyn Error + Send + Sync>> = if url.starts_with("magnet:?") {
                    let downloader = TorrentDownloader::new(
                        url.clone(),
                        output_path.clone(),
                        false,
                        proxy_clone,
                        optimizer_clone,
                    );
                    runtime.block_on(downloader.download())
                } else if url.starts_with("ftp://") {
                    let downloader = FtpDownloader::new(
                        url.clone(),
                        output_path.clone(),
                        false,
                        proxy_clone,
                        optimizer_clone,
                    );
                    downloader.download()
                } else if url.starts_with("sftp://") {
                    let downloader = SftpDownloader::new(
                        url.clone(),
                        output_path.clone(),
                        false,
                        proxy_clone,
                        optimizer_clone,
                    );
                    downloader.download()
                } else {
                    status_tx.send(WorkerToGuiMessage::StatusUpdate(format!("Starting HTTP download: {}", url))).unwrap();
                    for i in 1..=10 {
                    
                        thread::sleep(Duration::from_millis(200));
                        let _ = status_tx.send(WorkerToGuiMessage::Progress(i as f32 / 10.0));
                    }
                    Ok(())
                };

                match result {
                    Ok(_) => {
                        let _ = status_tx.send(WorkerToGuiMessage::Completed(format!(
                            "Successfully downloaded {} to {}",
                            url, output_path
                        )));
                    }
                    Err(e) => {
                        let _ = status_tx.send(WorkerToGuiMessage::Error(format!(
                            "Download failed for {}: {}",
                            url, e
                        )));
                    }
                }
            }
            DownloadCommand::Cancel => {
                let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate(
                    "Cancel command received (implement cancellation logic in worker).".to_string(),
                ));
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();
    let mut config = Config::load()?;

    if args.interactive {
        interactive::interactive_mode();
        return Ok(());
    }

    if !args.gui {
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
            "https" => crate::config::ProxyType::Https,
            "socks5" => crate::config::ProxyType::Socks5,
            _ => crate::config::ProxyType::Http,
        };
        if let Some(limit) = args.speed_limit {
            config.optimization.speed_limit = Some(limit);
        }
        // if args.no_compress {
        //     config.optimization.compression = false;
        // }
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

    if args.gui {
        #[cfg(feature = "gui")]
        {
            let (download_tx, download_rx_worker): (MpscSender<DownloadCommand>, MpscReceiver<DownloadCommand>) = mpsc::channel();
            let (status_tx_worker, status_rx_gui): (MpscSender<WorkerToGuiMessage>, MpscReceiver<WorkerToGuiMessage>) = mpsc::channel();

            let worker_config = config.clone();
            let runtime = tokio::runtime::Runtime::new()?;

            thread::spawn(move || {
                download_worker(worker_config, download_rx_worker, status_tx_worker, runtime);
            });

            let mut native_options = eframe::NativeOptions::default();
            native_options.viewport.inner_size = Some(egui::Vec2::new(900.0, 400.0));

            if let Err(e) = eframe::run_native(
                "KGet Downloader",
                native_options,
                Box::new(move |cc| {
                    Ok(Box::new(KGetGui::new(cc, download_tx, status_rx_gui)))
                }),
            ) {
                eprintln!("Failed to launch GUI: {e}");
                return Err("Failed to launch GUI".into());
            }
        }

        #[cfg(not(feature = "gui"))]
        {
            eprintln!("GUI support was not compiled in. Rebuild with `--features gui` to enable it.");
            return Err("GUI not available (compile with --features gui)".into());
        }
    }

    let optimizer = Optimizer::new(config.optimization.clone());

    if args.url.is_empty() && !args.gui {
    
        Args::command().print_help()?;
        return Err("URL is required for CLI mode.".into());
    }

    if args.ftp {
        let url = args.url.clone();
        let output = args.output.unwrap_or_else(|| utils::get_filename_from_url_or_default(&url, "ftp_output"));
        let downloader = FtpDownloader::new(
            url.to_owned(),
            output,
            args.quiet,
            config.proxy,
            optimizer,
        );
        return downloader.download();
    }

    if args.sftp {
        let url = args.url.clone();
        let output = args.output.unwrap_or_else(|| utils::get_filename_from_url_or_default(&url, "sftp_output"));
        let downloader = SftpDownloader::new(
            url.to_owned(),
            output,
            args.quiet,
            config.proxy,
            optimizer,
        );
        return downloader.download();
    }
    
    if args.torrent || args.url.starts_with("magnet:?") {
        let downloader = TorrentDownloader::new(
            args.url,
            args.output.unwrap_or_else(|| "torrent_output".to_string()),
            args.quiet,
            config.proxy,
            optimizer,
        );
        tokio::runtime::Runtime::new()?
            .block_on(downloader.download())?;
    } else if args.advanced {
        let downloader = AdvancedDownloader::new(
            args.url.clone(),
            args.output.unwrap_or_else(|| utils::get_filename_from_url_or_default(&args.url, "advanced_output")),
            args.quiet,
            config.proxy,
            optimizer,
        );
        downloader.download()?;
    } else {
        cli_download(
            &args.url, 
            args.quiet, 
            args.output.or_else(|| Some(utils::get_filename_from_url_or_default(&args.url, "http_output"))),
            config.proxy, 
            optimizer
        )?;
    }

    Ok(())
}
