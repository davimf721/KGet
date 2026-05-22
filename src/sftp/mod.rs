//! SFTP (SSH File Transfer Protocol) download support.
//!
//! This module provides secure file downloads over SSH using SFTP.
//!
//! # Security
//!
//! SFTP provides encrypted file transfer, unlike plain FTP.
//! Authentication is handled via SSH (password or key-based).
//!
//! # Authentication Order
//!
//! 1. Password embedded in URL (`sftp://user:pass@host/path`)
//! 2. SSH agent (if running)
//! 3. Default key files (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`)
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::sftp::SftpDownloader;
//! use kget::{ProxyConfig, Optimizer};
//!
//! // Password authentication
//! let downloader = SftpDownloader::new(
//!     "sftp://user:pass@server.com:22/path/to/file".to_string(),
//!     "local_file.txt".to_string(),
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! );
//!
//! // Key-based (uses SSH agent or ~/.ssh/id_* automatically)
//! let downloader = SftpDownloader::new(
//!     "sftp://user@server.com/path/to/file".to_string(),
//!     "local_file.txt".to_string(),
//!     false,
//!     ProxyConfig::default(),
//!     Optimizer::new(),
//! );
//!
//! downloader.download().unwrap();
//! ```

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use std::error::Error;
use std::io::{Read, Write};
use url::Url;

/// SFTP file downloader using SSH.
///
/// Downloads files securely over SSH File Transfer Protocol.
///
/// Authentication tries, in order:
/// 1. Password from the URL (`sftp://user:pass@host/path`)
/// 2. Running SSH agent
/// 3. Default key files in `~/.ssh/`
pub struct SftpDownloader {
    url: String,
    output: String,
    quiet: bool,
    #[allow(dead_code)]
    proxy: ProxyConfig,
    #[allow(dead_code)]
    optimizer: Optimizer,
}

impl SftpDownloader {
    /// Create a new SFTP downloader.
    ///
    /// # Arguments
    ///
    /// * `url` - SFTP URL (e.g., `sftp://user:pass@host/path` or `sftp://user@host/path`)
    /// * `output` - Local path to save the file
    /// * `quiet` - Suppress console output
    /// * `proxy` - Proxy configuration (not used for SFTP; SSH tunneling must be configured at OS level)
    /// * `optimizer` - Optimizer instance
    pub fn new(
        url: String,
        output: String,
        quiet: bool,
        proxy: ProxyConfig,
        optimizer: Optimizer,
    ) -> Self {
        Self {
            url,
            output,
            quiet,
            proxy,
            optimizer,
        }
    }

    /// Download the file via SFTP.
    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let parsed = Url::parse(&self.url)
            .map_err(|e| format!("Invalid SFTP URL '{}': {}", self.url, e))?;

        if parsed.scheme() != "sftp" {
            return Err(format!(
                "Expected sftp:// URL, got scheme '{}'",
                parsed.scheme()
            )
            .into());
        }

        let host = parsed
            .host_str()
            .ok_or("SFTP URL is missing a host (expected sftp://user@host/path)")?;
        let port = parsed.port().unwrap_or(22);
        let remote_path = parsed.path();

        if remote_path.is_empty() || remote_path == "/" {
            return Err("SFTP URL must include a file path (e.g., sftp://user@host/path/to/file)"
                .into());
        }

        let username = if parsed.username().is_empty() {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "anonymous".to_string())
        } else {
            parsed.username().to_string()
        };

        if !self.quiet {
            println!("Connecting to {}:{} ...", host, port);
        }

        let tcp = std::net::TcpStream::connect((host, port))
            .map_err(|e| format!("Cannot connect to {}:{} — {}", host, port, e))?;

        let mut sess = ssh2::Session::new()
            .map_err(|e| format!("SSH session init failed: {}", e))?;
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .map_err(|e| format!("SSH handshake with {}:{} failed: {}", host, port, e))?;

        // Verify host key against ~/.ssh/known_hosts
        self.check_host_key(&sess, host, port)?;

        // Authenticate: password from URL → SSH agent → default key files
        if let Some(password) = parsed.password() {
            sess.userauth_password(&username, password)
                .map_err(|e| format!("Password authentication for '{}' failed: {}", username, e))?;
        } else {
            self.try_key_auth(&sess, &username)?;
        }

        if !sess.authenticated() {
            return Err(format!(
                "SFTP authentication failed for user '{}' on {}:{}. \
                 Provide password in URL (sftp://user:pass@host/path) or configure SSH keys/agent.",
                username, host, port
            )
            .into());
        }

        if !self.quiet {
            println!("Authenticated as '{}'. Opening SFTP channel...", username);
        }

        let sftp = sess
            .sftp()
            .map_err(|e| format!("Failed to open SFTP channel: {}", e))?;

        let remote = std::path::Path::new(remote_path);

        // Get file size for the progress bar (best-effort; fall back to spinner)
        let file_size = sftp.stat(remote).ok().and_then(|s| s.size);

        if !self.quiet {
            match file_size {
                Some(sz) => println!("Remote file size: {} bytes", sz),
                None => println!("Remote file size unknown"),
            }
        }

        let progress = create_progress_bar(
            self.quiet,
            remote_path.to_string(),
            file_size,
            false,
        );

        let mut remote_file = sftp
            .open(remote)
            .map_err(|e| format!("Cannot open remote file '{}': {}", remote_path, e))?;

        let mut dest = std::fs::File::create(&self.output)
            .map_err(|e| format!("Cannot create local file '{}': {}", self.output, e))?;

        let mut buffer = [0u8; 32768];
        loop {
            let n = remote_file
                .read(&mut buffer)
                .map_err(|e| format!("SFTP read error: {}", e))?;
            if n == 0 {
                break;
            }
            dest.write_all(&buffer[..n])
                .map_err(|e| format!("Write error to '{}': {}", self.output, e))?;
            progress.inc(n as u64);
        }

        progress.finish_with_message("Download complete");

        if !self.quiet {
            println!("Saved to '{}'", self.output);
        }

        Ok(())
    }

    /// Verify the server's host key against ~/.ssh/known_hosts.
    ///
    /// Behaviour mirrors OpenSSH's `StrictHostKeyChecking=accept-new`:
    /// - **Match** in known_hosts → proceed silently.
    /// - **Not found** in known_hosts → warn and continue (first connection).
    /// - **Mismatch** → hard error (possible MITM attack).
    fn check_host_key(
        &self,
        sess: &ssh2::Session,
        host: &str,
        port: u16,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let (key_bytes, _key_type) = sess
            .host_key()
            .ok_or("Server did not provide a host key — connection refused")?;

        let home = match dirs::home_dir() {
            Some(h) => h,
            None => {
                if !self.quiet {
                    eprintln!("Warning: Could not find home directory; skipping known_hosts check");
                }
                return Ok(());
            }
        };

        let known_hosts_path = home.join(".ssh/known_hosts");

        let mut known_hosts = sess
            .known_hosts()
            .map_err(|e| format!("Failed to open known_hosts: {}", e))?;

        if known_hosts_path.exists() {
            known_hosts
                .read_file(&known_hosts_path, ssh2::KnownHostFileKind::OpenSSH)
                .map_err(|e| format!("Failed to read known_hosts: {}", e))?;
        }

        let check_host = if port == 22 {
            host.to_string()
        } else {
            format!("[{}]:{}", host, port)
        };

        match known_hosts.check(&check_host, key_bytes) {
            ssh2::CheckResult::Match => {}
            ssh2::CheckResult::NotFound => {
                eprintln!(
                    "Warning: The host '{}' is not in your known_hosts file ({}).\n\
                     Connecting anyway. To suppress this warning, add the host key:\n\
                     ssh-keyscan -p {} {} >> {}",
                    host,
                    known_hosts_path.display(),
                    port,
                    host,
                    known_hosts_path.display()
                );
            }
            ssh2::CheckResult::Mismatch => {
                return Err(format!(
                    "WARNING: Host key verification FAILED for '{}'!\n\
                     The host key has changed. This could indicate a MITM attack.\n\
                     If you trust the new key, remove the old entry:\n\
                     ssh-keygen -R '{}'",
                    host, check_host
                )
                .into());
            }
            ssh2::CheckResult::Failure => {
                eprintln!(
                    "Warning: Could not check host key for '{}'; proceeding without verification",
                    host
                );
            }
        }

        Ok(())
    }

    /// Try SSH agent first, then fall back to well-known key files.
    fn try_key_auth(
        &self,
        sess: &ssh2::Session,
        username: &str,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // 1. SSH agent
        if let Ok(mut agent) = sess.agent() {
            if agent.connect().is_ok() && agent.list_identities().is_ok() {
                if let Ok(identities) = agent.identities() {
                    for identity in &identities {
                        if agent.userauth(username, identity).is_ok() && sess.authenticated() {
                            return Ok(());
                        }
                    }
                }
            }
        }

        // 2. Default key files
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return Ok(()), // Let caller handle the unauthenticated state
        };

        let key_candidates = [
            home.join(".ssh/id_ed25519"),
            home.join(".ssh/id_rsa"),
            home.join(".ssh/id_ecdsa"),
        ];

        for key_path in &key_candidates {
            if key_path.exists() {
                if sess
                    .userauth_pubkey_file(username, None, key_path, None)
                    .is_ok()
                    && sess.authenticated()
                {
                    return Ok(());
                }
            }
        }

        // Return Ok regardless — caller checks sess.authenticated()
        Ok(())
    }
}
