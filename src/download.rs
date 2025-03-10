use reqwest::blocking::Client;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use std::fs::File;
use std::time::Duration;
use std::error::Error;
use crate::progress::create_progress_bar;
use crate::utils::print;
use humansize::{format_size, DECIMAL};
use mime::Mime;
use std::io::Read;
use std::path::Path;


const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_secs(2);

fn check_disk_space(path: &Path, required_size: u64) -> Result<(), Box<dyn Error>> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let available_space = fs2::available_space(dir)?;
    
    if available_space < required_size {
        return Err(format!(
            "Espaço insuficiente em disco. Necessário: {}, Disponível: {}", 
            format_size(required_size, DECIMAL),
            format_size(available_space, DECIMAL)
        ).into());
    }
    Ok(())
}

fn validate_filename(filename: &str) -> Result<(), Box<dyn Error>> {
    if filename.contains(std::path::MAIN_SEPARATOR) {
        return Err("Nome do arquivo não pode conter separadores de diretório".into());
    }
    if filename.is_empty() {
        return Err("Nome do arquivo não pode estar vazio".into());
    }
    Ok(())
}

pub fn download(
    target: &str,
    quiet_mode: bool,
    output_filename: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let mut retries = 0;
    let response = loop {
        match client.get(target).send() {
            Ok(resp) => break resp,
            Err(e) => {
                retries += 1;
                if retries >= MAX_RETRIES {
                    return Err(format!("Falha após {} tentativas: {}", MAX_RETRIES, e).into());
                }
                print(&format!("Tentativa {} falhou, tentando novamente em {} segundos...", 
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

    if let Some(ct) = content_type {
        print(&format!("Type: {}", ct), quiet_mode);
    }

    let fname = output_filename.unwrap_or_else(|| {
        target.split('/').last().unwrap_or("index.html").to_owned()
    });

    validate_filename(&fname)?;

    if let Some(len) = content_length {
        check_disk_space(Path::new(&fname), len)?;
    }

    print(&format!("Saving to: {}", fname), quiet_mode);

    let mut dest = File::create(&fname)?;
    let content_length = response.content_length();
    let progress = create_progress_bar(quiet_mode, fname.clone(), content_length, false);
    let mut source = response.take(content_length.unwrap_or(u64::MAX));
    let mut buffered_reader = progress.wrap_read(&mut source);
    std::io::copy(&mut buffered_reader, &mut dest)?;
    progress.finish_with_message("Download completed");
    Ok(())
}