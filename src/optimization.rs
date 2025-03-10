use std::error::Error;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use flate2::write::{GzEncoder, GzDecoder};
use lz4::block::{compress, CompressionMode};
use crate::config::OptimizationConfig;

/// Estrutura responsável por otimizar operações de download através de compressão e cache
pub struct Optimizer {
    config: OptimizationConfig,
}

impl Optimizer {
    /// Cria uma nova instância do otimizador com a configuração especificada
    pub fn new(config: OptimizationConfig) -> Self {
        Self { config }
    }

    /// Comprime os dados usando diferentes algoritmos baseado no nível de compressão configurado
    /// 
    /// Níveis 1-3: Gzip
    /// Níveis 4-6: LZ4
    /// Níveis 7-9: Brotli
    #[allow(dead_code)]
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        if !self.config.compression {
            return Ok(data.to_vec());
        }

        let compressed = match self.config.compression_level {
            1..=3 => {
                let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::fast());
                encoder.write_all(data)?;
                encoder.finish()?
            }
            4..=6 => {
                compress(data, Some(CompressionMode::FAST(0)), true)?
            }
            7..=9 => {
                let mut encoder = brotli::CompressorWriter::new(
                    Vec::new(),
                    self.config.compression_level as usize,
                    4096,
                    22,
                );
                encoder.write_all(data)?;
                encoder.into_inner()
            }
            _ => return Ok(data.to_vec()),
        };

        Ok(compressed)
    }

    /// Descomprime os dados usando o algoritmo apropriado baseado no cabeçalho do arquivo
    /// 
    /// Suporta Gzip, Brotli e LZ4
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        if !self.config.compression {
            return Ok(data.to_vec());
        }

        let mut decompressed = Vec::new();
        if data.starts_with(&[0x1f, 0x8b]) {
            let mut decoder = GzDecoder::new(Vec::new());
            decoder.write_all(data)?;
            decompressed = decoder.finish()?;
        } else if data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            let mut decoder = brotli::Decompressor::new(data, 4096);
            decoder.read_to_end(&mut decompressed)?;
        } else {
            let mut decoder = lz4::Decoder::new(data)?;
            decoder.read_to_end(&mut decompressed)?;
        }

        Ok(decompressed)
    }

    /// Recupera um arquivo do cache se existir
    /// 
    /// Retorna None se o cache estiver desabilitado ou o arquivo não existir
    #[allow(dead_code)]
    pub fn get_cached_file(&self, url: &str) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
        if !self.config.cache_enabled {
            return Ok(None);
        }

        let cache_path = self.get_cache_path(url)?;
        if cache_path.exists() {
            let mut file = File::open(cache_path)?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;
            return Ok(Some(contents));
        }

        Ok(None)
    }

    /// Armazena um arquivo no cache
    /// 
    /// Não faz nada se o cache estiver desabilitado
    #[allow(dead_code)]
    pub fn cache_file(&self, url: &str, data: &[u8]) -> Result<(), Box<dyn Error>> {
        if !self.config.cache_enabled {
            return Ok(());
        }

        let cache_path = self.get_cache_path(url)?;
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = File::create(cache_path)?;
        file.write_all(data)?;
        Ok(())
    }

    /// Gera o caminho do arquivo no cache baseado na URL
    /// 
    /// Usa MD5 para gerar um nome de arquivo único
    #[allow(dead_code)]
    fn get_cache_path(&self, url: &str) -> Result<PathBuf, Box<dyn Error>> {
        let mut cache_dir = PathBuf::from(
            self.config.cache_dir.as_deref().unwrap_or("~/.cache/kelpsget")
        );
        
        if cache_dir.starts_with("~") {
            if let Some(home) = dirs::home_dir() {
                cache_dir = home.join(cache_dir.strip_prefix("~").unwrap());
            }
        }

        let hash = md5::compute(url.as_bytes());
        cache_dir.push(format!("{:x}", hash));
        Ok(cache_dir)
    }
} 