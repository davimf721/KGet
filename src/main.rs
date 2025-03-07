extern crate indicatif;
extern crate reqwest;
extern crate mime;
extern crate humansize;

use clap::{Arg, ArgAction, Command};
use crate::download::download;
mod progress;
mod utils;
mod download;

fn main() {
    let matches = Command::new("KelpsGet")
        .version("0.1.0")
        .author("Davi Moreira Fuzatto")
        .about("wget clone written in Rust")
        .arg(Arg::new("URL")
            .required(true)
            .action(ArgAction::Set)
            .index(1)
            .help("URL to download"))
        .arg(
            Arg::new("output")
                .short('O')
                .long("output")
                .value_name("ARQUIVO")
                .help("Define o nome do arquivo de destino")
                .action(ArgAction::Set),
        )
        .get_matches();

    let url = matches.get_one::<String>("URL").unwrap();
    let output = matches.get_one::<String>("output").map(String::from);
    
    if let Err(e) = download(url, false, output) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}