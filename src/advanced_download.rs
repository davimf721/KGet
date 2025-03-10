use std::error::Error;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use rayon::prelude::*;
use reqwest::blocking::Client;
use crate::utils::print;

const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB chunks

pub struct AdvancedDownloader {
    client: Client,
    url: String,
    output_path: String,
    quiet_mode: bool,
}

impl AdvancedDownloader {
    pub fn new(url: String, output_path: String, quiet_mode: bool) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            url,
            output_path,
            quiet_mode,
        }
    }

    pub fn download(&self) -> Result<(), Box<dyn Error>> {
        // Verificar se o arquivo já existe e obter seu tamanho
        let existing_size = if Path::new(&self.output_path).exists() {
            Some(std::fs::metadata(&self.output_path)?.len())
        } else {
            None
        };

        // Obter o tamanho total do arquivo
        let total_size = self.get_file_size()?;
        
        // Criar ou abrir o arquivo de saída
        let file = if existing_size.is_some() {
            File::options().read(true).write(true).open(&self.output_path)?
        } else {
            File::create(&self.output_path)?
        };

        // Calcular chunks para download paralelo
        let chunks = self.calculate_chunks(total_size, existing_size)?;
        
        // Download paralelo dos chunks
        self.download_chunks_parallel(chunks, &file)?;

        Ok(())
    }

    fn get_file_size(&self) -> Result<u64, Box<dyn Error>> {
        let response = self.client.head(&self.url).send()?;
        let content_length = response.headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("Could not determine file size")?;
        
        Ok(content_length)
    }

    fn calculate_chunks(&self, total_size: u64, existing_size: Option<u64>) -> Result<Vec<(u64, u64)>, Box<dyn Error>> {
        let mut chunks = Vec::new();
        let chunk_size = CHUNK_SIZE;
        let mut start = existing_size.unwrap_or(0);

        while start < total_size {
            let end = (start + chunk_size).min(total_size);
            chunks.push((start, end));
            start = end;
        }

        Ok(chunks)
    }

    fn download_chunks_parallel(&self, chunks: Vec<(u64, u64)>, file: &File) -> Result<(), Box<dyn Error>> {
        let file = Arc::new(file);
        let client = Arc::new(self.client.clone());
        let url = Arc::new(self.url.clone());
        let quiet_mode = self.quiet_mode;

        chunks.par_iter().for_each(|&(start, end)| {
            let range = format!("bytes={}-{}", start, end - 1);
            let mut response = client.get(&*url)
                .header(reqwest::header::RANGE, &range)
                .send()
                .expect("Failed to send request");

            let mut buffer = Vec::new();
            response.copy_to(&mut buffer).expect("Failed to read response");

            let mut file = file.try_clone().expect("Failed to clone file");
            file.seek(SeekFrom::Start(start)).expect("Failed to seek file");
            file.write_all(&buffer).expect("Failed to write chunk");

            if !quiet_mode {
                print(&format!("Downloaded chunk {}-{}", start, end), quiet_mode);
            }
        });

        Ok(())
    }
} 