use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;
use dirs::config_dir;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub enabled: bool,
    pub url: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub proxy_type: ProxyType,
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub cache_dir: Option<String>,
    pub speed_limit: Option<u64>, // em bytes por segundo
    pub max_connections: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub proxy: ProxyConfig,
    pub optimization: OptimizationConfig,
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
                cache_dir: Some("~/.cache/kelpsget".to_string()),
                speed_limit: None,
                max_connections: 4,
            },
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = get_config_path()?;
        
        if !config_path.exists() {
            let config = Config::default();
            config.save()?;
            return Ok(config);
        }

        let config_str = fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = get_config_path()?;
        let config_str = serde_json::to_string_pretty(self)?;
        
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        fs::write(config_path, config_str)?;
        Ok(())
    }
}

fn get_config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut config_dir = config_dir()
        .ok_or("Não foi possível encontrar o diretório de configuração")?;
    
    config_dir.push("kelpsget");
    config_dir.push("config.json");
    
    Ok(config_dir)
} 