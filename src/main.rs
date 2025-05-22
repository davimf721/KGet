use clap::Parser;
// Adicione esta linha
use clap::CommandFactory;
use std::error::Error;
use std::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use std::thread;
use std::time::Duration;

use crate::download::download as cli_download;
use crate::advanced_download::AdvancedDownloader;
use crate::config::Config;
use crate::optimization::Optimizer;
use crate::gui::{KelpsGetGui, DownloadCommand, WorkerToGuiMessage};
pub use crate::ftp::FtpDownloader;
pub use crate::sftp::SftpDownloader;
pub use crate::torrent::TorrentDownloader;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL do arquivo para download
    #[arg(default_value_t = String::new())]
    url: String,

    /// Nome do arquivo de saída
    #[arg(short = 'O', long = "output")]
    output: Option<String>,

    /// Modo silencioso (sem barra de progresso)
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// Usar download avançado (paralelo e resumível)
    #[arg(short = 'a', long = "advanced")]
    advanced: bool,

    /// Usar download via torrent (magnetic links)
    #[arg(short = 't', long = "torrent")]
    torrent: bool,

    /// URL do proxy (ex: http://proxy:8080)
    #[arg(short = 'p', long = "proxy")]
    proxy: Option<String>,

    /// Usuário do proxy
    #[arg(long = "proxy-user")]
    proxy_user: Option<String>,

    /// Senha do proxy
    #[arg(long = "proxy-pass")]
    proxy_pass: Option<String>,

    /// Tipo de proxy (http, https, socks5)
    #[arg(long = "proxy-type", default_value = "http")]
    proxy_type: String,

    /// Limite de velocidade em bytes/segundo
    #[arg(short = 'l', long = "limit")]
    speed_limit: Option<u64>,

    // /// Desabilitar compressão
    // #[arg(long = "no-compress")]
    // no_compress: bool,

    /// Desabilitar cache
    #[arg(long = "no-cache")]
    no_cache: bool,

    /// Número máximo de peers para download via torrent
    #[arg(long = "max-peers")]
    max_peers: Option<usize>,

    /// Número máximo de seeds para download via torrent
    #[arg(long = "max-seeds")]
    max_seeds: Option<usize>,

    /// Porta para conexões torrent
    #[arg(long = "torrent-port")]
    torrent_port: Option<u16>,

    /// Desabilitar DHT para torrents
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
}

mod download;
mod progress;
mod utils;
mod advanced_download;
mod config;
mod optimization;
mod ftp;
mod gui;
mod sftp;
mod torrent;

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
                    downloader.download().map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn Error + Send + Sync>)
                } else if url.starts_with("sftp://") {
                    let downloader = SftpDownloader::new(
                        url.clone(),
                        output_path.clone(),
                        false,
                        proxy_clone,
                        optimizer_clone,
                    );
                    downloader.download().map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn Error + Send + Sync>)
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

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let mut config = Config::load()?;

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
        let (download_tx, download_rx_worker): (MpscSender<DownloadCommand>, MpscReceiver<DownloadCommand>) = mpsc::channel();
        let (status_tx_worker, status_rx_gui): (MpscSender<WorkerToGuiMessage>, MpscReceiver<WorkerToGuiMessage>) = mpsc::channel();

        let worker_config = config.clone();
        let runtime = tokio::runtime::Runtime::new()?;

        thread::spawn(move || {
            download_worker(worker_config, download_rx_worker, status_tx_worker, runtime);
        });

        let native_options = eframe::NativeOptions::default();
        eframe::run_native(
            "KelpsGet Downloader",
            native_options,
            Box::new(move |cc| {
                Ok(Box::new(KelpsGetGui::new(cc, download_tx, status_rx_gui)))
            }),
        )?;
        return Ok(());
    }

    let optimizer = Optimizer::new(config.optimization.clone());

    if args.url.is_empty() && !args.gui {
        // Agora Args::command() deve funcionar
        Args::command().print_help()?;
        return Err("URL is required for CLI mode.".into());
    }

    if args.ftp {
        let url = args.url.clone();
        let output = args.output.unwrap_or_else(|| utils::get_filename_from_url_or_default(&url, "ftp_output"));
        let downloader = FtpDownloader::new(
            url,
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
            url,
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
            .block_on(downloader.download())
            .map_err(|e| e as Box<dyn Error>)?;
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
        ).map_err(|e| e as Box<dyn Error>)?;
    }

    Ok(())
}
