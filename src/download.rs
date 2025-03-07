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

pub fn download(
    target: &str,
    quiet_mode: bool,
    output_filename: Option<String>,
) -> Result<(), Box<dyn Error>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let response = client.get(target).send()?;
    
    print(
        format!("HTTP request sent... {}", response.status()),
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
            format!("Length: {} ({})", 
                len, 
                format_size(len, DECIMAL)
            ), 
            quiet_mode
        );
    } else {
        print("Length: unknown".to_string(), quiet_mode);
    }

    if let Some(ct) = content_type {
        print(format!("Type: {}", ct), quiet_mode);
    }

    let fname = output_filename.unwrap_or_else(|| {
        target.split('/').last().unwrap_or("index.html").to_owned()
    });

    print(format!("Saving to: {}", fname), quiet_mode);

    let mut dest = File::create(&fname)?;
    let content_length = response.content_length();
    let progress = create_progress_bar(quiet_mode, fname.clone(), content_length);
    let mut source = response.take(content_length.unwrap_or(u64::MAX));
    let mut buffered_reader = progress.wrap_read(&mut source);
    std::io::copy(&mut buffered_reader, &mut dest)?;
    progress.finish_with_message("Download completed");
    Ok(())
}