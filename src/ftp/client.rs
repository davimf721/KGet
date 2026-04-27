//! FTP client implementation.

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::utils::print;
use std::error::Error;
use std::io::Write;
use suppaftp::FtpStream;
use url::Url;

/// FTP file downloader with progress tracking.
///
/// Supports:
/// - Anonymous FTP (use "anonymous" as username)
/// - Authenticated FTP with username/password in URL
/// - Binary transfer mode (safe for all file types)
/// - SOCKS5 proxy connections
///
/// # URL Format
///
/// ```text
/// ftp://[user[:password]@]host[:port]/path/to/file
/// ```
///
/// # Example
///
/// ```rust,no_run
/// use kget::ftp::FtpDownloader;
/// use kget::{ProxyConfig, Optimizer};
///
/// // Anonymous download
/// let dl = FtpDownloader::new(
///     "ftp://ftp.gnu.org/gnu/emacs/emacs-28.2.tar.gz".to_string(),
///     "emacs-28.2.tar.gz".to_string(),
///     false,
///     ProxyConfig::default(),
///     Optimizer::new(),
/// );
/// dl.download().expect("FTP download failed");
///
/// // Authenticated download
/// let dl = FtpDownloader::new(
///     "ftp://user:pass@private-server.com/file.zip".to_string(),
///     "file.zip".to_string(),
///     false,
///     ProxyConfig::default(),
///     Optimizer::new(),
/// );
/// dl.download().expect("FTP download failed");
/// ```
pub struct FtpDownloader {
    url: String,
    output_path: String,
    quiet_mode: bool,
    proxy: ProxyConfig,
    #[allow(dead_code)]
    optimizer: Optimizer,
}

impl FtpDownloader {
    /// Create a new FTP downloader.
    ///
    /// # Arguments
    ///
    /// * `url` - FTP URL including path to file
    /// * `output_path` - Local path to save the file
    /// * `quiet_mode` - Suppress console output
    /// * `proxy` - Proxy configuration (SOCKS5 only)
    /// * `optimizer` - Optimizer instance
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

        print(
            &format!("Connecting to FTP server {}:{}...", host, port),
            self.quiet_mode,
        );

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
            false,
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
                        file.write_all(&buffer[..n])
                            .map_err(|e| suppaftp::FtpError::ConnectionError(e))?;
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

    fn connect_via_proxy(
        &self,
        host: &str,
        port: u16,
    ) -> Result<FtpStream, Box<dyn Error + Send + Sync>> {
        match self.proxy.proxy_type {
            crate::config::ProxyType::Http | crate::config::ProxyType::Https => {
                Err("HTTP/HTTPS proxy not supported for FTP".into())
            }
            crate::config::ProxyType::Socks5 => {
                if let Some(proxy_url) = &self.proxy.url {
                    let proxy = Url::parse(proxy_url)?;
                    let proxy_host = proxy.host_str().ok_or("Invalid proxy host")?;
                    let proxy_port = proxy.port().unwrap_or(1080);

                    let stream =
                        socks::Socks5Stream::connect((proxy_host, proxy_port), (host, port))?;

                    Ok(FtpStream::connect_with_stream(stream.into_inner())?)
                } else {
                    Err("Proxy URL not configured".into())
                }
            }
        }
    }
}
