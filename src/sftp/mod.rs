use std::error::Error;
use std::io::Read;
use crate::config::ProxyConfig;
use crate::optimization::Optimizer;

pub struct SftpDownloader {
    url: String,
    output: String,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
}

impl SftpDownloader {
    pub fn new(url: String, output: String, quiet: bool, proxy: ProxyConfig, optimizer: Optimizer) -> Self {
        Self {
            url,
            output,
            quiet,
            proxy,
            optimizer,
        }
    }

    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let tcp = std::net::TcpStream::connect(&self.url)?;
        let mut sess = ssh2::Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;
        
        let sftp = sess.sftp()?;
        let mut remote_file = sftp.open(std::path::Path::new(&self.url))?;
        let mut contents = Vec::new();
        remote_file.read_to_end(&mut contents)?;
        
        std::fs::write(&self.output, contents)?;
        
        if !self.quiet {
            println!("Downloaded {} to {}", self.url, self.output);
        }
        Ok(())
    }
}