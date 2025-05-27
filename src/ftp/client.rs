use std::error::Error;
use std::path::Path;
use std::io::{Read, Write};
use url::Url;
use suppaftp::FtpStream;
use crate::progress::create_progress_bar;
use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::utils::print;

pub struct FtpDownloader {
    url: String,
    output_path: String,
    quiet_mode: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
}

impl FtpDownloader {
    pub fn new(
        url: String,
        output_path: String,
        quiet_mode: bool,
        proxy: ProxyConfig,
        optimizer: Optimizer,
    ) -> Self {
        Self {
            url,
            output_path,
            quiet_mode,
            proxy,
            optimizer,
        }
    }

    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let url = Url::parse(&self.url)?;
        let host = url.host_str().ok_or("Invalid host")?;
        let port = url.port().unwrap_or(21);
        let path = url.path();

        print(&format!("Connecting to FTP server {}:{}...", host, port), self.quiet_mode);

        let mut ftp = if self.proxy.enabled {
            self.connect_via_proxy(host, port)?
        } else {
            FtpStream::connect((host, port))?
        };

        let username = url.username();
        let password = url.password().unwrap_or("anonymous");

        print(&format!("Logging in as {}...", username), self.quiet_mode);
        ftp.login(username, password)?;

        ftp.transfer_type(suppaftp::types::FileType::Binary)?;

        let size = ftp.size(path)? as u64;
        
        let progress = create_progress_bar(
            self.quiet_mode,
            format!("Downloading {}", path),
            Some(size),
            false
        );

        let mut file = std::fs::File::create(&self.output_path)?;

        // Download file
        let mut downloaded = 0;
        ftp.retr(path, |reader| {
            let mut buffer = vec![0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        file.write_all(&buffer[..n]).map_err(|e| suppaftp::FtpError::ConnectionError(e))?;
                        downloaded += n;
                        progress.set_position(downloaded as u64);
                    }
                    Err(e) => return Err(suppaftp::FtpError::ConnectionError(e)),
                }
            }
            Ok(())
        })?;

        progress.finish();
        print("Download completed successfully!", self.quiet_mode);

        Ok(())
    }

    fn connect_via_proxy(&self, host: &str, port: u16) -> Result<FtpStream, Box<dyn Error + Send + Sync>> {
        match self.proxy.proxy_type {
            crate::config::ProxyType::Http | crate::config::ProxyType::Https => {
                Err("HTTP/HTTPS proxy not supported for FTP".into())
            }
            crate::config::ProxyType::Socks5 => {
                if let Some(proxy_url) = &self.proxy.url {
                    let proxy = Url::parse(proxy_url)?;
                    let proxy_host = proxy.host_str().ok_or("Invalid proxy host")?;
                    let proxy_port = proxy.port().unwrap_or(1080);

                    let stream = socks::Socks5Stream::connect(
                        (proxy_host, proxy_port),
                        (host, port),
                    )?;

                    Ok(FtpStream::connect_with_stream(stream.into_inner())?)
                } else {
                    Err("Proxy URL not configured".into())
                }
            }
        }
    }
}