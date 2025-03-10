use clap::Parser;
use std::error::Error;
use crate::download::download;
use crate::advanced_download::AdvancedDownloader;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL do arquivo para download
    url: String,

    /// Nome do arquivo de saída
    #[arg(short = 'O', long = "output")]
    output: Option<String>,

    /// Modo silencioso (sem barra de progresso)
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// Usar download avançado (paralelo e resumível)
    #[arg(short = 'a', long = "advanced")]
    advanced: bool,
}

mod download;
mod progress;
mod utils;
mod advanced_download;

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if args.advanced {
        let downloader = AdvancedDownloader::new(
            args.url,
            args.output.unwrap_or_else(|| "output".to_string()),
            args.quiet
        );
        downloader.download()?;
    } else {
        download(&args.url, args.quiet, args.output)?;
    }

    Ok(())
}