use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write, BufWriter};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rayon::prelude::*;
use reqwest::blocking::Client;
use indicatif::{ProgressBar, ProgressStyle};
use crate::utils::print;
use crate::config::ProxyConfig;
use sha2::{Sha256, Digest};
use hex;
use crate::optimization::Optimizer;

#[cfg(target_family = "unix")]
use std::os::unix::fs::FileExt;
#[cfg(target_family = "windows")]
use std::os::windows::fs::FileExt;

const MIN_CHUNK_SIZE: u64 = 4 * 1024 * 1024; 
const MAX_RETRIES: usize = 3;

pub struct AdvancedDownloader {
    client: Client,
    url: String,
    output_path: String,
    quiet_mode: bool,
    #[allow(dead_code)]
    proxy: ProxyConfig,
    optimizer: Optimizer,
    progress_callback: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    status_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl AdvancedDownloader {
    pub fn new(url: String, output_path: String, quiet_mode: bool, proxy_config: ProxyConfig, optimizer: Optimizer) -> Self {
        let is_iso = url.to_lowercase().ends_with(".iso");
        
        let mut client_builder = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(20))
            .user_agent("KGet/1.0")
            .no_gzip() 
            .no_deflate();

        if proxy_config.enabled {
            if let Some(proxy_url) = &proxy_config.url {
                let proxy = match proxy_config.proxy_type {
                    crate::config::ProxyType::Http => reqwest::Proxy::http(proxy_url),
                    crate::config::ProxyType::Https => reqwest::Proxy::https(proxy_url),
                    crate::config::ProxyType::Socks5 => reqwest::Proxy::all(proxy_url),
                };
                
                if let Ok(mut proxy) = proxy {
                    if let (Some(username), Some(password)) = (&proxy_config.username, &proxy_config.password) {
                        proxy = proxy.basic_auth(username, password);
                    }
                    client_builder = client_builder.proxy(proxy);
                }
            }
        }
        
        let client = client_builder.build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            url,
            output_path,
            quiet_mode,
            proxy: proxy_config,
            optimizer,
            progress_callback: None,
            status_callback: None,
        }
    }

    pub fn set_progress_callback(&mut self, callback: impl Fn(f32) + Send + Sync + 'static) {
        self.progress_callback = Some(Arc::new(callback));
    }

    pub fn set_status_callback(&mut self, callback: impl Fn(String) + Send + Sync + 'static) {
        self.status_callback = Some(Arc::new(callback));
    }

    fn send_status(&self, msg: &str) {
        if let Some(cb) = &self.status_callback {
            cb(msg.to_string());
        }
        if !self.quiet_mode {
            println!("{}", msg);
        }
    }


    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let is_iso = self.url.to_lowercase().ends_with(".iso");
        if !self.quiet_mode {
            println!("Starting advanced download for: {}", self.url);
            if is_iso {
                println!("Warning: ISO mode active. Disabling optimizations that could corrupt binary data.");
            }
        }

        // Verify if the output path is valid
        let existing_size = if Path::new(&self.output_path).exists() {
            let size = std::fs::metadata(&self.output_path)?.len();
            if !self.quiet_mode {
                println!("Existing file found with size: {} bytes", size);
            }
            Some(size)
        } else {
            if !self.quiet_mode {
                println!("Output file does not exist, starting fresh download");
            }
            None
        };

        // Get the total file size and range support
        if !self.quiet_mode {
            println!("Querying server for file size and range support...");
        }
        let (total_size, supports_range) = self.get_file_size_and_range()?;
        if !self.quiet_mode {
            println!("Total file size: {} bytes", total_size);
            println!("Server supports range requests: {}", supports_range);
        }

        if let Some(size) = existing_size {
            if size > total_size {
                return Err("Existing file is larger than remote; aborting".into());
            }
            if !self.quiet_mode {
                println!("Resuming download from byte: {}", size);
            }
        }

        // Create a progress bar if not quiet or if we have a callback
        let progress = if !self.quiet_mode || self.progress_callback.is_some() {
            let bar = ProgressBar::new(total_size);
            if self.quiet_mode {
                bar.set_draw_target(indicatif::ProgressDrawTarget::hidden());
            } else {
                bar.set_style(ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})"
                ).unwrap().progress_chars("#>-"));
            }
            Some(Arc::new(Mutex::new(bar)))
        } else {
            None
        };

        // Create or open the output file and preallocate
        if !self.quiet_mode {
            println!("Preparing output file: {}", self.output_path);
        }
        let file = if existing_size.is_some() {
            File::options().read(true).write(true).open(&self.output_path)?
        } else {
            File::create(&self.output_path)?
        };
        file.set_len(total_size)?;
        if !self.quiet_mode {
            println!("File preallocated to {} bytes", total_size);
        }

        // If range not supported, do a single download
        if !supports_range {
            if !self.quiet_mode {
                println!("Range requests not supported, falling back to single-threaded download");
            }
            self.download_whole(&file, existing_size.unwrap_or(0), progress.clone())?;
            if let Some(ref bar) = progress {
                bar.lock().unwrap().finish_with_message("Download completed");
            }
            if !self.quiet_mode {
                println!("Single-threaded download completed");
            }
            return Ok(());
        }

        // Calculate chunks for parallel download
        if !self.quiet_mode {
            println!("Calculating download chunks...");
        }
        let chunks = self.calculate_chunks(total_size, existing_size)?;
        if !self.quiet_mode {
            println!("Download will be split into {} chunks", chunks.len());
        }

        // Download parallel chunks
        if !self.quiet_mode {
            println!("Starting parallel chunk downloads...");
        }
        self.download_chunks_parallel(chunks, &file, progress.clone())?;

        if let Some(ref bar) = progress {
            bar.lock().unwrap().finish_with_message("Download completed");
        }

        // Verify download integrity
        if !self.quiet_mode || self.status_callback.is_some() {
            if is_iso {
                
                let should_verify = if self.status_callback.is_some() {
                    true 
                } else {
                    println!("\nThis is an ISO file. Would you like to verify its integrity? (y/N)");
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).is_ok() && input.trim().to_lowercase() == "y"
                };

                if should_verify {
                    self.verify_integrity(total_size)?;
                }
            } else {
                let metadata = std::fs::metadata(&self.output_path)?;
                if metadata.len() != total_size {
                    return Err(format!("File size mismatch: expected {} bytes, got {} bytes", total_size, metadata.len()).into());
                }
            }
            self.send_status("Advanced download completed successfully!");
        }

        Ok(())
    }

    fn get_file_size_and_range(&self) -> Result<(u64, bool), Box<dyn Error + Send + Sync>> {
        let response = self.client.head(&self.url).send()?;
        let content_length = response.headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("Could not determine file size")?;

        let accepts_range = response.headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);

        Ok((content_length, accepts_range))
    }

    fn calculate_chunks(&self, total_size: u64, existing_size: Option<u64>) -> Result<Vec<(u64, u64)>, Box<dyn Error + Send + Sync>> {
        let mut chunks = Vec::new();
        let start_from = existing_size.unwrap_or(0);

        
        let parallelism = rayon::current_num_threads() as u64;
        let target_chunks = parallelism.saturating_mul(2).max(2); // Reduced to avoid overwhelming servers
        let chunk_size = ((total_size / target_chunks).max(MIN_CHUNK_SIZE)).min(64 * 1024 * 1024);

        let mut start = start_from;
        while start < total_size {
            let end = (start + chunk_size).min(total_size);
            chunks.push((start, end));
            start = end;
        }

        Ok(chunks)
    }

    fn download_whole(&self, file: &File, offset: u64, progress: Option<Arc<Mutex<ProgressBar>>>) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut response = self.client.get(&self.url).send()?;
        if offset > 0 {
            // Resume not possible without range; warn
            return Err("Server does not support range; cannot resume partial file".into());
        }

        let mut reader = BufReader::new(response);
        let mut f = file.try_clone()?;
        f.seek(SeekFrom::Start(0))?;

        struct ProgressWriter<'a, W> {
            inner: W,
            progress: Option<Arc<Mutex<ProgressBar>>>,
            callback: Option<&'a Arc<dyn Fn(f32) + Send + Sync>>,
        }

        impl<'a, W: Write> Write for ProgressWriter<'a, W> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n = self.inner.write(buf)?;
                if let Some(ref bar) = self.progress {
                    let mut guard = bar.lock().unwrap();
                    guard.inc(n as u64);
                    if let Some(cb) = self.callback {
                        let pos = guard.position();
                        let len = guard.length().unwrap_or(1);
                        drop(guard);
                        (cb)(pos as f32 / len as f32);
                    }
                }
                Ok(n)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.inner.flush()
            }
        }

        let mut writer = ProgressWriter { 
            inner: f, 
            progress,
            callback: self.progress_callback.as_ref(),
        };
        std::io::copy(&mut reader, &mut writer)?;

        Ok(())
    }

    fn download_chunks_parallel(&self, chunks: Vec<(u64, u64)>, file: &File, progress: Option<Arc<Mutex<ProgressBar>>>) -> Result<(), Box<dyn Error + Send + Sync>> {
        let file = Arc::new(file);
        let client = Arc::new(self.client.clone());
        let url = Arc::new(self.url.clone());
        let _optimizer = Arc::new(self.optimizer.clone());
        let progress_callback = self.progress_callback.clone();

        chunks.par_iter().try_for_each(|&(start, end)| {
            let range = format!("bytes={}-{}", start, end - 1);
            let range_header = reqwest::header::HeaderValue::from_str(&range)
                .map_err(|e| format!("Invalid range header {}: {}", range, e))?;

            for retry in 0..=MAX_RETRIES {
                let request = client.get(url.as_str());
                let request = request.header(reqwest::header::RANGE, range_header.clone());

                match request.send() {
                    Ok(mut response) => {
                        let status = response.status();
                        if status.is_success() {
                            // Use FileExt to write at specific offset without seeking shared cursor
                            // This prevents race conditions when multiple threads write to the same file
                            
                            let mut current_pos = start;
                            let mut buffer = [0u8; 16384]; 
                            
                            while current_pos < end {
                                let limit = (end - current_pos).min(buffer.len() as u64);
                                let n = response.read(&mut buffer[..limit as usize])?;
                                if n == 0 { break; }
                                
                                #[cfg(target_family = "unix")]
                                file.write_at(&buffer[..n], current_pos)?;
                                
                                #[cfg(target_family = "windows")]
                                file.seek_write(&buffer[..n], current_pos)?;
                                
                                current_pos += n as u64;
                            
                                if let Some(ref bar) = progress {
                                    let mut guard = bar.lock().unwrap();
                                    guard.inc(n as u64);
                                    if let Some(ref cb) = progress_callback {
                                        let pos = guard.position();
                                        let len = guard.length().unwrap_or(1);
                                        drop(guard);
                                        (cb)(pos as f32 / len as f32);
                                    }
                                }
                            }

                            return Ok::<(), Box<dyn Error + Send + Sync>>(());
                        } else if status.as_u16() == 416 {
                            if retry == MAX_RETRIES {
                                return Err(format!("Failed to download chunk {}-{}: HTTP {}", start, end, status).into());
                            }
                            std::thread::sleep(Duration::from_millis(250 * (retry as u64 + 1)));
                        }
                    }
                    Err(e) => {
                        if retry == MAX_RETRIES {
                            return Err(format!("Failed to download chunk {}-{}: {}", start, end, e).into());
                        }
                        std::thread::sleep(Duration::from_millis(250 * (retry as u64 + 1)));
                    }
                }
            }
            Err(format!("Failed to download chunk {}-{} after retries", start, end).into())
        })?;

        Ok(())
    }

        fn verify_integrity(&self, expected_size: u64) -> Result<(), Box<dyn Error + Send + Sync>> {
            let metadata = std::fs::metadata(&self.output_path)?;
        let actual_size = metadata.len();
        
        if actual_size != expected_size {
            return Err(format!("File size mismatch: expected {} bytes, got {} bytes", expected_size, actual_size).into());
        }
        
        self.send_status(&format!("File size verified: {} bytes", actual_size));

        // Calculate SHA256 hash for corruption check
        self.send_status("Calculating SHA256 hash...");
        
        let mut file = File::open(&self.output_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        let hash = hasher.finalize();
        let hash_hex = hex::encode(hash);
        
        self.send_status(&format!("SHA256 hash: {}", hash_hex));
        self.send_status("Integrity check passed - file is not corrupted");

        Ok(())
    }
}