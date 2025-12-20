use reqwest::blocking::Client;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use std::fs::File;
use std::time::Duration;
use std::error::Error;
use crate::progress::create_progress_bar;
use crate::utils::{self, print};
use humansize::{format_size, DECIMAL};
use mime::Mime;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use sha2::Digest;
use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::DownloadOptions;

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(2);

pub fn check_disk_space(path: &Path, required_size: u64) -> Result<(), Box<dyn Error + Send + Sync>> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let available_space = fs2::available_space(dir)?;
    
    if available_space < required_size {
        return Err(format!(
            "Insufficient disk space. Required: {}, Available: {}", 
            format_size(required_size, DECIMAL),
            format_size(available_space, DECIMAL)
        ).into());
    }
    Ok(())
}

pub fn validate_filename(filename: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if filename.contains(std::path::MAIN_SEPARATOR) {
        return Err("Filename cannot contain directory separators".into());
    }
    if filename.is_empty() {
        return Err("Filename cannot be empty".into());
    }
    Ok(())
}

pub fn download(
    target: &str,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    options: DownloadOptions, 
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let quiet_mode = options.quiet_mode;
    // ... restante da lógica de download (client builder, retries, etc) ...

    let mut client_builder = Client::builder()
        .timeout(Duration::from_secs(30));

    if proxy.enabled {
        if let Some(proxy_url) = &proxy.url {
            let proxy_client = match proxy.proxy_type {
                crate::config::ProxyType::Http => reqwest::Proxy::http(proxy_url),
                crate::config::ProxyType::Https => reqwest::Proxy::https(proxy_url),
                crate::config::ProxyType::Socks5 => reqwest::Proxy::all(proxy_url),
            };
            if let Ok(mut proxy_client) = proxy_client {
                if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
                    proxy_client = proxy_client.basic_auth(username, password);
                }
                client_builder = client_builder.proxy(proxy_client);
            }
        }
    }

    let client = client_builder.build()?;

    let mut retries = 0;
    let response = loop {
        match client.get(target).send() {
            Ok(resp) => break resp,
            Err(e) => {
                retries += 1;
                if retries >= MAX_RETRIES {
                    return Err(format!("Failed after {} attempts: {}", MAX_RETRIES, e).into());
                }
                print(&format!("Attempt {} failed, retrying in {} seconds...", 
                    retries, RETRY_DELAY.as_secs()), quiet_mode);
                std::thread::sleep(RETRY_DELAY);
            }
        }
    };
    
    print(
        &format!("HTTP request sent... {}", response.status()),
        quiet_mode
    );

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()).into());
    }

    let content_length = response.headers()
        .get(CONTENT_LENGTH)
        .and_then(|ct_len| ct_len.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let content_type = response.headers()
        .get(CONTENT_TYPE)
        .and_then(|ct| ct.to_str().ok())
        .and_then(|s| s.parse::<Mime>().ok());

    if let Some(len) = content_length {
        print(
            &format!("Length: {} ({})", 
                len, 
                format_size(len, DECIMAL)
            ), 
            quiet_mode
        );
    } else {
        print("Length: unknown", quiet_mode);
    }

    if let Some(ref ct) = content_type {
        print(&format!("Type: {}", ct), quiet_mode);
    }

    let is_iso = target.to_lowercase().ends_with(".iso") 
        || content_type.as_ref().map_or(false, |ct| ct.essence_str() == "application/x-iso9001" || ct.essence_str() == "application/x-cd-image");

    if is_iso {
        print("ISO file detected. Ensuring raw download to prevent corruption...", quiet_mode);
    }

    let mut tentative_path: PathBuf;

    if let Some(output_arg_str) = options.output_path { // Corrigido para output_path
        let user_path = PathBuf::from(output_arg_str.clone());

        let is_target_dir = user_path.is_dir() || 
                              output_arg_str.ends_with(std::path::MAIN_SEPARATOR);

        if is_target_dir {
            let base_filename = utils::get_filename_from_url_or_default(target, "downloaded_file");
            validate_filename(&base_filename)?;
            tentative_path = user_path.join(base_filename);
        } else {
            if let Some(file_name_osstr) = user_path.file_name() {
                if let Some(file_name_str) = file_name_osstr.to_str() {
                    if file_name_str.is_empty() {
                        return Err(format!("Invalid output path, does not specify a file name: {}", user_path.display()).into());
                    }
                    validate_filename(file_name_str)?;
                } else {
                    return Err("Output filename contains invalid characters (non-UTF-8)".into());
                }
            } else {
                return Err(format!("Invalid output path, does not specify a file name: {}", user_path.display()).into());
            }
            tentative_path = user_path;
        }
    } else {
        let base_filename = utils::get_filename_from_url_or_default(target, "downloaded_file");
        validate_filename(&base_filename)?;
        tentative_path = PathBuf::from(base_filename);
    }

    let final_path: PathBuf = if tentative_path.is_absolute() {
        tentative_path
    } else {
        let current_dir = std::env::current_dir()
            .map_err(|e| format!("Failed to get current directory: {}", e))?;
        current_dir.join(tentative_path)
    };

    if let Some(parent_dir) = final_path.parent() {
        if !parent_dir.as_os_str().is_empty() && parent_dir != Path::new("/") && !parent_dir.exists() {
            std::fs::create_dir_all(parent_dir)
                .map_err(|e| format!("Failed to create directory {}: {}", parent_dir.display(), e))?;
            if !quiet_mode {
                print(&format!("Created directory: {}", parent_dir.display()), quiet_mode);
            }
        }
    }
    
    if !quiet_mode {
        print(&format!("Saving to: {}", final_path.display()), quiet_mode);
    }

    if let Some(len) = content_length {
        check_disk_space(&final_path, len)?;
    }

    let mut dest = File::create(&final_path).map_err(|e| {
        format!("Failed to create file {}: {}", final_path.display(), e)
    })?;
    
    let response_content_length = response.content_length();
    let progress_bar_filename = final_path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let progress = create_progress_bar(quiet_mode, progress_bar_filename, response_content_length, false);
    
    let mut source = response.take(response_content_length.unwrap_or(u64::MAX));
    let mut buffered_reader = progress.wrap_read(&mut source);
    
    let mut buffer = Vec::new();
    buffered_reader.read_to_end(&mut buffer)?;
    
    // ... lógica de salvamento existente ...
        dest.write_all(&buffer)?;
        progress.finish_with_message("Download completed\n");

    // A LIB decide se verifica baseada na OPÇÃO recebida, não em perguntas
    if is_iso && options.verify_iso {
        verify_iso_integrity(&final_path)?;
    }

    Ok(())
}

// Tornamos pública para que outros desenvolvedores possam usar só a verificação se quiserem
pub fn verify_iso_integrity(path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("Calculating SHA256 hash... (this may take a while for large ISOs)");
    let mut file = File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    let hash = hex::encode(hasher.finalize());
    println!("Integrity check finished.");
    println!("SHA256: {}", hash);
    println!("You can compare this hash with the one provided by the source.");
    Ok(())
}