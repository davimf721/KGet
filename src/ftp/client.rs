//! FTP client implementation.

use crate::ByteProgressCallback;
use crate::config::{FtpConfig, ProxyConfig};
use crate::credentials::{CredentialResolver, Target};
use crate::optimization::Optimizer;
use crate::progress::{Throttle, create_progress_bar};
use crate::utils::print;
use std::error::Error;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use suppaftp::FtpStream;
use url::Url;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// FTP file downloader with progress tracking.
///
/// Supports:
/// - Anonymous FTP (use "anonymous" as username)
/// - Authenticated FTP with username/password in URL (percent-encoded), or a
///   user name only with the password from the system keychain or a prompt
///   (see [`with_credentials`](Self::with_credentials))
/// - Binary transfer mode (safe for all file types)
/// - SOCKS5 proxy connections (with optional credentials)
/// - Resuming an interrupted transfer (`REST`) from `<file>.part`
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
    config: FtpConfig,
    resume: bool,
    progress_callback: Option<ByteProgressCallback>,
    credentials: Option<Arc<CredentialResolver>>,
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
            config: FtpConfig::default(),
            resume: true,
            progress_callback: None,
            credentials: None,
        }
    }

    /// Use the passive-mode / default-port settings from the configuration.
    pub fn with_config(mut self, config: FtpConfig) -> Self {
        self.config = config;
        self
    }

    /// Continue an existing `<file>.part` (default `true`).
    pub fn with_resume(mut self, resume: bool) -> Self {
        self.resume = resume;
        self
    }

    /// Structured progress callback `(bytes, total)`.
    pub fn with_progress_callback(mut self, cb: ByteProgressCallback) -> Self {
        self.progress_callback = Some(cb);
        self
    }

    /// Look up (or prompt for) the password when the URL has a user but no
    /// password, and report accepted passwords back to the resolver.
    pub fn with_credentials(mut self, credentials: Arc<CredentialResolver>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Download the file.
    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let url = Url::parse(&self.url)?;
        if url.scheme().eq_ignore_ascii_case("ftps") {
            return Err("FTPS (ftp over TLS) is not supported yet; use sftp:// or ftp://".into());
        }
        let host = url.host_str().ok_or("Invalid host")?;
        let port = url.port().unwrap_or(self.config.default_port);
        let path = urlencoding::decode(url.path())?.into_owned();
        // url.username() is "" when the URL has no user — fall back to "anonymous"
        let username = if url.username().is_empty() {
            "anonymous".to_string()
        } else {
            urlencoding::decode(url.username())?.into_owned()
        };
        let anonymous = matches!(username.as_str(), "anonymous" | "ftp");
        let target = Target::from_url(&self.url).filter(|_| !anonymous);
        let creds = self.credentials.as_deref().zip(target.as_ref());
        let password = match url.password() {
            Some(p) => {
                let p = urlencoding::decode(p)?.into_owned();
                if let Some((r, t)) = creds {
                    r.provided(t, &p);
                }
                p
            }
            None => creds
                .and_then(|(r, t)| r.resolve(t, "FTP password"))
                .unwrap_or_else(|| "anonymous".to_string()),
        };
        // suppaftp writes these values into line-oriented FTP commands.  URL
        // percent-decoding happens above, so reject control characters after
        // decoding to prevent a URL from injecting an extra FTP command.
        validate_command_arg("FTP user", &username)?;
        validate_command_arg("FTP password", &password)?;
        validate_command_arg("FTP path", &path)?;

        print(
            &format!("Connecting to FTP server {}:{}...", host, port),
            self.quiet_mode,
        );

        let mut ftp = if self.proxy.enabled {
            self.connect_via_proxy(host, port)?
        } else {
            let addr = (host, port)
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| format!("Cannot resolve host {host}"))?;
            FtpStream::connect_timeout(addr, CONNECT_TIMEOUT)?
        };
        ftp.get_ref().set_read_timeout(Some(IO_TIMEOUT))?;
        ftp.get_ref().set_write_timeout(Some(IO_TIMEOUT))?;
        if !self.config.passive_mode {
            ftp.set_mode(suppaftp::Mode::Active);
        }

        print(&format!("Logging in as {}...", username), self.quiet_mode);
        ftp.login(&username, &password)?;
        if let Some((r, t)) = creds {
            r.accepted(t);
        }

        ftp.transfer_type(suppaftp::types::FileType::Binary)?;

        // Not every server implements SIZE: fall back to a spinner.
        let size = ftp.size(&path).ok().map(|s| s as u64);

        let final_path = PathBuf::from(&self.output_path);
        if let Some(parent) = final_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let part = crate::download::part_path_for(&final_path);

        let mut offset = if self.resume {
            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        if size.is_some_and(|s| offset > s) {
            offset = 0;
        }
        // An offset past usize::MAX (32-bit targets) restarts rather than
        // resuming at a truncated position.
        if offset > 0 && !usize::try_from(offset).is_ok_and(|o| ftp.resume_transfer(o).is_ok()) {
            print(
                "Server does not support resume (REST); restarting",
                self.quiet_mode,
            );
            offset = 0;
        }
        if offset > 0 {
            print(&format!("Resuming at byte {offset}"), self.quiet_mode);
        }

        let progress = create_progress_bar(
            self.quiet_mode,
            format!("Downloading {}", path),
            size,
            false,
        );
        progress.set_position(offset);

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(&part)?;

        let mut downloaded = offset;
        let mut throttle = Throttle::new(200);
        let cb = self.progress_callback.clone();
        if size != Some(offset) {
            ftp.retr(&path, |reader| {
                let mut buffer = vec![0; 64 * 1024];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            file.write_all(&buffer[..n])
                                .map_err(suppaftp::FtpError::ConnectionError)?;
                            downloaded += n as u64;
                            progress.set_position(downloaded);
                            if let Some(cb) = &cb {
                                let pct = size
                                    .map(|s| downloaded as f64 / s.max(1) as f64 * 100.0)
                                    .unwrap_or(0.0);
                                if throttle.ready(pct) {
                                    cb(downloaded, size);
                                }
                            }
                        }
                        Err(e) => return Err(suppaftp::FtpError::ConnectionError(e)),
                    }
                }
                Ok(())
            })?;
        }
        file.flush()?;
        drop(file);
        let _ = ftp.quit();

        if let Some(expected) = size
            && downloaded != expected
        {
            return Err(format!(
                "Incomplete FTP download: {downloaded} of {expected} bytes; run again to resume"
            )
            .into());
        }
        if let Some(cb) = &cb {
            cb(downloaded, Some(downloaded));
        }
        std::fs::rename(&part, Path::new(&self.output_path))?;

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
                Err("HTTP/HTTPS proxy not supported for FTP (use a SOCKS5 proxy)".into())
            }
            crate::config::ProxyType::Socks5 => {
                let proxy_url = self
                    .proxy
                    .url
                    .as_deref()
                    .ok_or("Proxy URL not configured")?;
                let proxy_url =
                    crate::http::normalize_proxy_url(proxy_url, crate::config::ProxyType::Socks5);
                let proxy = Url::parse(&proxy_url)?;
                let proxy_host = proxy.host_str().ok_or("Invalid proxy host")?;
                let proxy_port = proxy.port().unwrap_or(1080);

                let stream = match (&self.proxy.username, &self.proxy.password) {
                    (Some(u), Some(p)) => socks::Socks5Stream::connect_with_password(
                        (proxy_host, proxy_port),
                        (host, port),
                        u,
                        p,
                    )?,
                    _ => socks::Socks5Stream::connect((proxy_host, proxy_port), (host, port))?,
                };

                Ok(FtpStream::connect_with_stream(stream.into_inner())?)
            }
        }
    }
}

fn validate_command_arg(field: &str, value: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if value.contains(['\r', '\n']) {
        return Err(format!("{field} must not contain CR or LF").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_command_arg;

    #[test]
    fn ftp_command_arguments_reject_newlines() {
        assert!(validate_command_arg("path", "file%0A").is_ok());
        assert!(validate_command_arg("path", "file\r\nDELE important").is_err());
        assert!(validate_command_arg("user", "alice\nNOOP").is_err());
    }
}
