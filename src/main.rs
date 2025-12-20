// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Silencia warnings chatos para focar no que importa
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(dead_code)]

use clap::{Parser, CommandFactory};
use std::error::Error;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use std::thread;
use std::time::Duration;

// Módulos locais
mod download;
mod progress;
mod utils;
mod advanced_download;
mod config;
mod optimization;
mod ftp;
mod gui_types;
mod sftp;
mod torrent;
mod interactive;

#[cfg(feature = "gui")]
mod gui;

// Imports do Crate
use crate::download::download as cli_download;
use crate::advanced_download::AdvancedDownloader;
use crate::config::Config;
use crate::optimization::Optimizer;
#[cfg(feature = "gui")]
use crate::gui::KGetGui;
use crate::gui_types::{DownloadCommand, WorkerToGuiMessage};
pub use crate::ftp::FtpDownloader;
pub use crate::sftp::SftpDownloader;
pub use crate::torrent::TorrentDownloader;

// Importante: Usando tipos da Lib para compatibilidade
use kget::{DownloadOptions, verify_iso_integrity};

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

fn download_worker(
    config: Config,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
    _runtime: tokio::runtime::Runtime,
) {
    for command in download_rx {
        match command {
            DownloadCommand::Start { url, output_path, is_advanced, verify_iso } => {
                let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate(format!("Initializing: {}", url)));

                let optimizer = Optimizer::new(config.optimization.clone());
                let proxy = config.proxy.clone();

                let result: Result<(), Box<dyn Error + Send + Sync>> = if is_advanced {
                    let downloader = AdvancedDownloader::new(
                        url.clone(),
                        output_path.clone(),
                        true, // quiet_mode para não poluir o stdout do worker
                        proxy,
                        optimizer,
                    );
                    downloader.download()
                } else {
                    let options = kget::DownloadOptions {
                        quiet_mode: true,
                        output_path: Some(output_path.clone()),
                        verify_iso,
                    };
                    cli_download(&url, proxy, optimizer, options)
                };

                match result {
                    Ok(_) => {
                        let _ = status_tx.send(WorkerToGuiMessage::Completed(output_path));
                    }
                    Err(e) => {
                        let _ = status_tx.send(WorkerToGuiMessage::Error(e.to_string()));
                    }
                }
            }
            DownloadCommand::Cancel => {
                let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate("Download cancelled.".into()));
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

    // NOVA LÓGICA: Determina se deve abrir a GUI (flag --gui OU nenhum argumento de download passado)
    let should_start_gui = args.gui || (args.url.is_empty() && !args.ftp && !args.sftp && !args.torrent);

    // Se NÃO for abrir a GUI, configuramos o ambiente CLI com base nos argumentos
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
            "https" => crate::config::ProxyType::Https,
            "socks5" => crate::config::ProxyType::Socks5,
            _ => crate::config::ProxyType::Http,
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

    // Se deve abrir a GUI, executa o bloco gráfico e encerra o programa depois
    if should_start_gui {
        #[cfg(feature = "gui")]
        {
            let (download_tx, download_rx_worker): (MpscSender<DownloadCommand>, MpscReceiver<DownloadCommand>) = mpsc::channel();
            let (status_tx_worker, status_rx_gui): (MpscSender<WorkerToGuiMessage>, MpscReceiver<WorkerToGuiMessage>) = mpsc::channel();

            let worker_config = config.clone();
            let runtime = tokio::runtime::Runtime::new()?;

            thread::spawn(move || {
                download_worker(worker_config, download_rx_worker, status_tx_worker, runtime);
            });

            // Configurações da janela atualizadas conforme seu pedido
            let mut native_options = eframe::NativeOptions::default();
            native_options.viewport.inner_size = Some(egui::Vec2::new(800.0, 550.0));
            native_options.viewport.resizable = Some(true);

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

            // Encerra o programa após fechar a GUI para não tentar rodar o modo CLI sem URL
            return Ok(());
        }

        #[cfg(not(feature = "gui"))]
        {
            eprintln!("GUI support was not compiled in. Rebuild with `--features gui` to enable it.");
            // Se tentou abrir GUI sem suporte, mas tem URL, deixa cair pro modo CLI?
            // Não, melhor avisar e sair se foi intencional.
            if args.gui {
                return Err("GUI not available (compile with --features gui)".into());
            }
            // Se caiu aqui porque não tinha URL, mostra o help
            if args.url.is_empty() {
                Args::command().print_help()?;
                return Ok(());
            }
        }
    }

    // ==========================================================
    // MODO CLI (Só executa se não entrou no bloco da GUI acima)
    // ==========================================================

    let optimizer = Optimizer::new(config.optimization.clone());

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
        // Criamos as opções uma única vez
        let options = DownloadOptions {
            quiet_mode: args.quiet,
            output_path: args.output.clone(),
            verify_iso: false, // O CLI gerencia a pergunta manualmente abaixo
        };

        cli_download(&args.url, config.proxy, optimizer, options)?;

        // COMPORTAMENTO CLI: Pergunta apenas se for um arquivo ISO e não estiver em modo quiet
        if !args.quiet && args.url.to_lowercase().ends_with(".iso") {
            println!("\nThis is an ISO file. Would you like to verify its integrity? (y/N)");
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() && input.trim().to_lowercase() == "y" {
                // Descobrimos o nome do arquivo para passar para a função de verificação
                let filename = utils::get_filename_from_url_or_default(&args.url, "download.iso");
                let path = std::path::Path::new(&filename);
                verify_iso_integrity(path)?;
            }
        }
    }

    Ok(())
}