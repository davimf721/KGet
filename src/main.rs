use clap::Parser;
use std::error::Error;

use crate::download::download;
use crate::advanced_download::AdvancedDownloader;
use crate::torrent_downloader::TorrentDownloader;
use crate::config::Config;
use crate::optimization::Optimizer;
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL do arquivo para download
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

    /// Desabilitar compressão
    #[arg(long = "no-compress")]
    no_compress: bool,

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
}

mod download;
mod progress;
mod utils;
mod advanced_download;
mod config;
mod optimization;
mod torrent;
mod torrent_downloader;

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let mut config = Config::load()?;

    // Atualizar configuração com argumentos da linha de comando
    if let Some(proxy_url) = args.proxy {
        config.proxy.enabled = true;
        config.proxy.url = Some(proxy_url);
    }

    if let Some(user) = args.proxy_user {
        config.proxy.username = Some(user);
    }

    if let Some(pass) = args.proxy_pass {
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

    if args.no_compress {
        config.optimization.compression = false;
    }

    if args.no_cache {
        config.optimization.cache_enabled = false;
    }

    // Configurações de torrent
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

    // Salvar configuração atualizada
    config.save()?;

    let optimizer = Optimizer::new(config.optimization.clone());

    // Verificar se é um magnetic link para usar o downloader de torrent
    if args.torrent || args.url.starts_with("magnet:?") {
        let downloader = TorrentDownloader::new(
            args.url,
            args.output.unwrap_or_else(|| "output".to_string()),
            args.quiet,
            config.proxy,
            optimizer,
            config.torrent,
        );
        downloader.download()?;
    } else if args.advanced {
        let downloader = AdvancedDownloader::new(
            args.url,
            args.output.unwrap_or_else(|| "output".to_string()),
            args.quiet,
            config.proxy,
            optimizer,
        );
        downloader.download()?;
    } else {
        download(&args.url, args.quiet, args.output, config.proxy, optimizer)?;
    }

    Ok(())
}
