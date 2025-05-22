use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use std::io;
use dirs::config_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub enabled: bool,
    pub url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub proxy_type: ProxyType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyType {
    Http,
    Https,
    Socks5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConfig {
    pub compression: bool,
    pub compression_level: u8,
    pub cache_enabled: bool,
    pub cache_dir: String,
    pub speed_limit: Option<u64>,
    pub max_connections: usize,
}

// Função para fornecer o valor padrão para max_peer_connections
fn default_torrent_max_peer_connections() -> u32 {
     50
}

// Função para fornecer o valor padrão para max_upload_slots
fn default_torrent_max_upload_slots() -> u32 {
    4 // Este é o valor que você já usa no seu Default para Config
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    pub enabled: bool,
    pub download_dir: Option<String>,
    pub max_peers: usize,
    pub max_seeds: usize,
    pub port: Option<u16>,
    pub dht_enabled: bool,
    #[serde(default = "default_torrent_max_peer_connections")]
    pub max_peer_connections: u32,
    #[serde(default = "default_torrent_max_upload_slots")]
    pub max_upload_slots: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpConfig {
    pub passive_mode: bool,
    pub default_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpConfig {
    pub default_port: u16,
    pub key_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub proxy: ProxyConfig,
    pub optimization: OptimizationConfig,
    pub torrent: TorrentConfig,
    pub ftp: FtpConfig,
    pub sftp: SftpConfig,
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path()?;
        
        if !config_path.exists() {
            // Se o arquivo não existe, retorna a configuração padrão, que já tem o campo.
            return Ok(Self::default());
        }
        
        let config_str = fs::read_to_string(config_path)?;
        // O erro ocorre aqui se o arquivo JSON existente não tiver o campo.
        let config: Config = serde_json::from_str(&config_str)?; 
        
        Ok(config)
    }
    
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path()?;
        
        // Criar diretório de configuração se não existir
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let config_str = serde_json::to_string_pretty(self)?;
        fs::write(config_path, config_str)?;
        
        Ok(())
    }
    
    fn get_config_path() -> Result<PathBuf, io::Error> {
        let mut path = config_dir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Não foi possível encontrar o diretório de configuração")
        })?;
        
        path.push("kelpsget");
        path.push("config.json");
        
        Ok(path)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            proxy: ProxyConfig {
                enabled: false,
                url: None,
                username: None,
                password: None,
                proxy_type: ProxyType::Http,
            },
            optimization: OptimizationConfig {
                compression: true,
                compression_level: 6,
                cache_enabled: true,
                cache_dir: "~/.cache/kelpsget".to_string(),
                speed_limit: None,
                max_connections: 4,
            },
            torrent: TorrentConfig {
                enabled: false,
                download_dir: None,
                max_peers: 50,
                max_seeds: 25,
                port: None,
                dht_enabled: true,
                max_peer_connections: default_torrent_max_peer_connections(),
                max_upload_slots: default_torrent_max_upload_slots(),
            },
            ftp: FtpConfig {
                passive_mode: true,
                default_port: 21,
            },
            sftp: SftpConfig {
                default_port: 22,
                key_path: None,
            },
        }
    }
}
