use std::error::Error;
use std::path::Path;
use std::time::Duration;
use std::sync::{Arc, Mutex};

use rqbit::prelude::*;
use rqbit::torrent_client::{TorrentClient, TorrentClientBuilder};
use rqbit::torrent_metainfo::TorrentMetainfo;
use rqbit::magnet::MagnetLink;

use crate::config::{ProxyConfig, TorrentConfig};
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::utils::print;

/// Função principal para download via torrent
/// 
/// Esta função recebe um magnetic link e realiza o download do arquivo
/// usando o protocolo BitTorrent.
pub fn download_torrent(
    url: &str,
    quiet: bool,
    output: Option<String>,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    torrent_config: TorrentConfig,
) -> Result<(), Box<dyn Error>> {
    if !url.starts_with("magnet:?") {
        return Err("URL não é um magnetic link válido".into());
    }

    // Configurar diretório de download
    let download_dir = torrent_config.download_dir.unwrap_or_else(|| ".".to_string());
    let output_path = output.unwrap_or_else(|| {
        // Extrair nome do arquivo do magnetic link ou usar um nome padrão
        if let Some(name) = extract_name_from_magnet(url) {
            name
        } else {
            "torrent_download".to_string()
        }
    });

    if !quiet {
        print(&format!("Iniciando download via torrent: {}", url), quiet);
        print(&format!("Salvando em: {}", output_path), quiet);
    }

    // Criar cliente torrent
    let magnet = MagnetLink::from_url(url).map_err(|e| format!("Erro ao analisar magnetic link: {}", e))?;
    
    // Configurar cliente torrent
    let mut builder = TorrentClientBuilder::new();
    
    // Configurar diretório de download
    builder = builder.download_dir(download_dir);
    
    // Configurar porta
    if let Some(port) = torrent_config.port {
        builder = builder.port(port);
    }
    
    // Configurar DHT
    if torrent_config.dht_enabled {
        builder = builder.enable_dht();
    }
    
    // Configurar limite de velocidade
    if let Some(limit) = optimizer.speed_limit {
        builder = builder.download_rate_limit(limit as usize);
    }
    
    // Configurar proxy
    if proxy.enabled {
        if let Some(proxy_url) = &proxy.url {
            builder = builder.proxy(proxy_url);
        }
    }
    
    // Criar cliente
    let client = builder.build().map_err(|e| format!("Erro ao criar cliente torrent: {}", e))?;
    
    // Iniciar download
    let torrent_handle = client.add_magnet(magnet).map_err(|e| format!("Erro ao adicionar magnet: {}", e))?;
    
    // Configurar barra de progresso
    let progress_bar = if !quiet {
        let pb = create_progress_bar(quiet, "Iniciando download via torrent...".to_string(), None, false);
        pb.set_message("Iniciando download via torrent...");
        Some(pb)
    } else {
        None
    };
    
    // Monitorar progresso
    let mut last_progress = 0;
    loop {
        // Obter estatísticas
        let stats = torrent_handle.stats();
        
        // Verificar se download foi concluído
        if stats.progress >= 100.0 {
            if let Some(pb) = &progress_bar {
                pb.finish_with_message("Download concluído!");
            }
            break;
        }
        
        // Atualizar barra de progresso
        if let Some(pb) = &progress_bar {
            let progress = stats.progress as u64;
            if progress > last_progress {
                pb.set_position(progress);
                pb.set_message(&format!(
                    "Baixando... {:.1}% | Seeds: {} | Peers: {} | Velocidade: {}/s",
                    stats.progress,
                    stats.connected_seeders,
                    stats.connected_leechers,
                    humansize::format_size(stats.download_rate, humansize::DECIMAL)
                ));
                last_progress = progress;
            }
        }
        
        // Aguardar um pouco antes de verificar novamente
        std::thread::sleep(Duration::from_millis(500));
    }
    
    // Finalizar cliente
    client.shutdown().map_err(|e| format!("Erro ao finalizar cliente torrent: {}", e))?;
    
    if !quiet {
        print("Download concluído com sucesso!", quiet);
    }

    Ok(())
}

/// Extrai o nome do arquivo de um magnetic link
fn extract_name_from_magnet(magnet_url: &str) -> Option<String> {
    // Procurar pelo parâmetro dn (display name) no magnetic link
    if let Some(dn_start) = magnet_url.find("&dn=") {
        let dn_value_start = dn_start + 4;
        if let Some(dn_end) = magnet_url[dn_value_start..].find('&') {
            let name = &magnet_url[dn_value_start..dn_value_start + dn_end];
            return Some(url_decode(name));
        } else {
            // Se não encontrar & depois do dn, pegar até o final da string
            let name = &magnet_url[dn_value_start..];
            return Some(url_decode(name));
        }
    }
    None
}

/// Decodifica uma string codificada em URL
fn url_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&input[i+1..i+3], 16) {
                result.push(hex as char);
                i += 3;
            } else {
                result.push('%');
                i += 1;
            }
        } else if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}
