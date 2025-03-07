extern crate clap;
extern crate indicatif;
extern crate reqwest;
extern crate mime;
extern crate humansize;

use clap::{Arg, App};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use std::fs::File;
use std::time::Duration;
use humansize::{format_size, DECIMAL};
use mime::Mime;
use std::error::Error;

fn create_progress_bar(quiet_mode: bool, msg: String, length: Option<u64>) -> ProgressBar {
    let bar = if quiet_mode {
        ProgressBar::hidden()
    } else {
        match length {
            Some(len) => ProgressBar::new(len),
            None => ProgressBar::new_spinner(),
        }
    };

    bar.set_message(msg);
    
    if let Some(_) = length {
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{msg} {spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} eta: {eta}")
                .unwrap()
                .progress_chars("=> ")
        );
    } else {
        bar.set_style(ProgressStyle::default_spinner());
    }

    bar
}

fn print(msg: String, quiet_mode: bool) {
    if !quiet_mode {
        println!("{}", msg);
    }
}

fn download(
    target: &str,
    quiet_mode: bool,
    output_filename: Option<String>, // Novo parâmetro
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

let mut dest = File::create(&fname)?; // Use referência para não mover
let content_length = response.content_length();
let  source = response;

// Clonamos fname para passar à barra de progresso
let progress = create_progress_bar(quiet_mode, fname.clone(), content_length); 
    
    let mut source = progress.wrap_read(source);
    std::io::copy(&mut source, &mut dest)?;

    progress.finish_with_message("Download completed");
    Ok(())
}

fn main() {
    let matches = App::new("KelpsGet")
        .version("0.1.0")
        .author("Davi Moreira Fuzatto")
        .about("wget clone written in Rust")
        .arg(Arg::with_name("URL")
            .required(true)
            .takes_value(true)
            .index(1)
            .help("URL to download"))
            .arg(
                Arg::with_name("output")
                    .short("O")
                    .long("output")
                    .value_name("ARQUIVO")
                    .help("Define o nome do arquivo de destino")
                    .takes_value(true),
            )
        .get_matches();

    let url = matches.value_of("URL").unwrap();

    let output = matches.value_of("output").map(String::from);
    
    if let Err(e) = download(url, false, output) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}