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
//! 1. Password embedded in URL (`sftp://user:pass@host/path`, percent-encoded)
//! 2. Password saved in the system keychain for `sftp://user@host`
//! 3. SSH agent (if running)
//! 4. The configured key (`sftp.key_path` in `config.json`)
//! 5. Default key files (`~/.ssh/id_ed25519`, `~/.ssh/id_rsa`, `~/.ssh/id_ecdsa`);
//!    passphrase-protected keys use a passphrase saved in the keychain or,
//!    on a terminal, a prompt
//! 6. On a terminal, a password prompt
//!
//! Steps 2, 5 (passphrases) and 6 need a
//! [`CredentialResolver`](crate::credentials::CredentialResolver) attached
//! with [`SftpDownloader::with_credentials`].
//!
//! # Paths
//!
//! `sftp://host/abs/path` is absolute; `sftp://host/~/file` is relative to the
//! user's home directory. Percent-encoded characters (`%20`) are decoded.
//!
//! # Host keys
//!
//! Like OpenSSH's `StrictHostKeyChecking=accept-new`: a known key must match,
//! and the key of a new host is added to `~/.ssh/known_hosts`.
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

use crate::ByteProgressCallback;
use crate::config::ProxyConfig;
use crate::credentials::{CredentialResolver, Target, ssh_key_is_encrypted};
use crate::optimization::Optimizer;
use crate::progress::{Throttle, create_progress_bar};
use std::error::Error;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// SFTP file downloader using SSH.
///
/// Downloads files securely over SSH File Transfer Protocol.
///
/// Authentication tries, in order:
/// 1. Password from the URL (`sftp://user:pass@host/path`)
/// 2. Password saved in the system keychain (with [`with_credentials`](Self::with_credentials))
/// 3. Running SSH agent
/// 4. Default key files in `~/.ssh/`
/// 5. Password prompt (interactive resolver only)
pub struct SftpDownloader {
    url: String,
    output: String,
    quiet: bool,
    #[allow(dead_code)]
    proxy: ProxyConfig,
    #[allow(dead_code)]
    optimizer: Optimizer,
    key_path: Option<PathBuf>,
    resume: bool,
    progress_callback: Option<ByteProgressCallback>,
    credentials: Option<Arc<CredentialResolver>>,
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
            key_path: None,
            resume: true,
            progress_callback: None,
            credentials: None,
        }
    }

    /// Private key tried before the default `~/.ssh/id_*` files.
    pub fn with_key_path(mut self, key_path: Option<String>) -> Self {
        self.key_path = key_path
            .filter(|k| !k.trim().is_empty())
            .map(|k| PathBuf::from(crate::utils::expand_tilde(k.trim())));
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

    /// Use saved passwords/key passphrases from the system keychain, prompt
    /// for missing ones (interactive resolver) and report accepted ones.
    pub fn with_credentials(mut self, credentials: Arc<CredentialResolver>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Download the file via SFTP.
    pub fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let parsed =
            Url::parse(&self.url).map_err(|e| format!("Invalid SFTP URL '{}': {}", self.url, e))?;

        if parsed.scheme() != "sftp" {
            return Err(format!("Expected sftp:// URL, got scheme '{}'", parsed.scheme()).into());
        }

        let host = parsed
            .host_str()
            .ok_or("SFTP URL is missing a host (expected sftp://user@host/path)")?;
        let port = parsed.port().unwrap_or(22);
        let decoded_path = urlencoding::decode(parsed.path())?.into_owned();
        // "/~/file" is relative to the home directory (SFTP resolves relative
        // paths from there).
        let remote_path = decoded_path
            .strip_prefix("/~/")
            .unwrap_or(&decoded_path)
            .to_string();
        let remote_path = remote_path.as_str();

        if remote_path.is_empty() || remote_path == "/" {
            return Err(
                "SFTP URL must include a file path (e.g., sftp://user@host/path/to/file)".into(),
            );
        }

        let username = if parsed.username().is_empty() {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "anonymous".to_string())
        } else {
            urlencoding::decode(parsed.username())?.into_owned()
        };

        if !self.quiet {
            println!("Connecting to {}:{} ...", host, port);
        }

        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("Cannot resolve {}: {}", host, e))?
            .next()
            .ok_or_else(|| format!("Cannot resolve {}", host))?;
        let tcp = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(20))
            .map_err(|e| format!("Cannot connect to {}:{} — {}", host, port, e))?;

        let mut sess =
            ssh2::Session::new().map_err(|e| format!("SSH session init failed: {}", e))?;
        sess.set_timeout(60_000);
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .map_err(|e| format!("SSH handshake with {}:{} failed: {}", host, port, e))?;

        // Verify host key against ~/.ssh/known_hosts
        self.check_host_key(&sess, host, port)?;

        // Authenticate: password from URL → keychain → SSH agent → key files → prompt
        let creds = self.credentials.as_deref();
        let target = Target::from_url(&self.url);
        if let Some(password) = parsed.password() {
            let password = urlencoding::decode(password)?.into_owned();
            sess.userauth_password(&username, &password)
                .map_err(|e| format!("Password authentication for '{}' failed: {}", username, e))?;
            if let (Some(r), Some(t)) = (creds, &target) {
                r.provided(t, &password);
                r.accepted(t);
            }
        } else {
            if let (Some(r), Some(t)) = (creds, &target)
                && let Some(saved) = r.lookup(t)
                && (sess.userauth_password(&username, &saved).is_err() || !sess.authenticated())
                && !self.quiet
            {
                eprintln!("Warning: the password saved for {t} was rejected");
            }
            if !sess.authenticated() {
                self.try_key_auth(&sess, &username)?;
            }
            if !sess.authenticated()
                && let (Some(r), Some(t)) = (creds, &target)
                && let Some(password) = r.prompt(t, "SSH password")
            {
                sess.userauth_password(&username, &password).map_err(|e| {
                    format!("Password authentication for '{}' failed: {}", username, e)
                })?;
                if sess.authenticated() {
                    r.accepted(t);
                }
            }
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

        let progress = create_progress_bar(self.quiet, remote_path.to_string(), file_size, false);

        let mut remote_file = sftp
            .open(remote)
            .map_err(|e| format!("Cannot open remote file '{}': {}", remote_path, e))?;

        let final_path = PathBuf::from(&self.output);
        if let Some(parent) = final_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let part = crate::download::part_path_for(&final_path);
        let mut offset = if self.resume {
            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        if file_size.is_some_and(|s| offset > s) {
            offset = 0;
        }
        if offset > 0 {
            remote_file
                .seek(SeekFrom::Start(offset))
                .map_err(|e| format!("SFTP seek error: {}", e))?;
            if !self.quiet {
                println!("Resuming at byte {}", offset);
            }
        }

        let mut dest = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(&part)
            .map_err(|e| format!("Cannot create local file '{}': {}", part.display(), e))?;

        progress.set_position(offset);
        let mut downloaded = offset;
        let mut throttle = Throttle::new(200);
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let n = remote_file
                .read(&mut buffer)
                .map_err(|e| format!("SFTP read error: {}", e))?;
            if n == 0 {
                break;
            }
            dest.write_all(&buffer[..n])
                .map_err(|e| format!("Write error to '{}': {}", part.display(), e))?;
            downloaded += n as u64;
            progress.set_position(downloaded);
            if let Some(cb) = &self.progress_callback {
                let pct = file_size
                    .map(|s| downloaded as f64 / s.max(1) as f64 * 100.0)
                    .unwrap_or(0.0);
                if throttle.ready(pct) {
                    cb(downloaded, file_size);
                }
            }
        }
        dest.flush()?;
        drop(dest);

        if let Some(expected) = file_size
            && downloaded != expected
        {
            return Err(format!(
                "Incomplete SFTP download: {downloaded} of {expected} bytes; run again to resume"
            )
            .into());
        }
        std::fs::rename(&part, &final_path)?;
        if let Some(cb) = &self.progress_callback {
            cb(downloaded, Some(downloaded));
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
    /// - **Not found** in known_hosts → add the key to `~/.ssh/known_hosts` and continue.
    /// - **Mismatch** → hard error (possible MITM attack).
    fn check_host_key(
        &self,
        sess: &ssh2::Session,
        host: &str,
        port: u16,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let (key_bytes, key_type) = sess
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
                let format = match key_type {
                    ssh2::HostKeyType::Rsa => ssh2::KnownHostKeyFormat::SshRsa,
                    ssh2::HostKeyType::Dss => ssh2::KnownHostKeyFormat::SshDss,
                    ssh2::HostKeyType::Ecdsa256 => ssh2::KnownHostKeyFormat::Ecdsa256,
                    ssh2::HostKeyType::Ecdsa384 => ssh2::KnownHostKeyFormat::Ecdsa384,
                    ssh2::HostKeyType::Ecdsa521 => ssh2::KnownHostKeyFormat::Ecdsa521,
                    ssh2::HostKeyType::Ed25519 => ssh2::KnownHostKeyFormat::Ed25519,
                    ssh2::HostKeyType::Unknown => ssh2::KnownHostKeyFormat::Unknown,
                };
                let saved = known_hosts
                    .add(&check_host, key_bytes, "added by kget", format)
                    .and_then(|_| {
                        if let Some(dir) = known_hosts_path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        known_hosts.write_file(&known_hosts_path, ssh2::KnownHostFileKind::OpenSSH)
                    });
                if !self.quiet {
                    match saved {
                        Ok(()) => eprintln!(
                            "Warning: permanently added '{}' to the list of known hosts ({}).",
                            check_host,
                            known_hosts_path.display()
                        ),
                        Err(e) => eprintln!(
                            "Warning: host '{}' is not in {} and could not be added ({}); connecting anyway.",
                            check_host,
                            known_hosts_path.display(),
                            e
                        ),
                    }
                }
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
                return Err(format!(
                    "Host key check failed for '{}': internal libssh2 error. \
                     Connection aborted to prevent security bypass.",
                    host
                )
                .into());
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
        if let Ok(mut agent) = sess.agent()
            && agent.connect().is_ok()
            && agent.list_identities().is_ok()
            && let Ok(identities) = agent.identities()
        {
            for identity in &identities {
                if agent.userauth(username, identity).is_ok() && sess.authenticated() {
                    return Ok(());
                }
            }
        }

        // 2. Configured key, then default key files
        let home = dirs::home_dir().unwrap_or_default();

        let key_candidates = self.key_path.iter().cloned().chain([
            home.join(".ssh/id_ed25519"),
            home.join(".ssh/id_rsa"),
            home.join(".ssh/id_ecdsa"),
        ]);

        for key_path in key_candidates {
            if !key_path.exists() {
                continue;
            }
            if sess
                .userauth_pubkey_file(username, None, &key_path, None)
                .is_ok()
                && sess.authenticated()
            {
                return Ok(());
            }
            // Passphrase-protected key: saved passphrase, then a prompt.
            let Some(r) = self.credentials.as_deref() else {
                continue;
            };
            if !ssh_key_is_encrypted(&key_path) {
                continue;
            }
            let target = Target::ssh_key(&key_path);
            let try_passphrase = |passphrase: &str| {
                sess.userauth_pubkey_file(username, None, &key_path, Some(passphrase))
                    .is_ok()
                    && sess.authenticated()
            };
            if r.lookup(&target).is_some_and(|p| try_passphrase(&p)) {
                return Ok(());
            }
            let what = format!("Passphrase for key '{}'", key_path.display());
            if let Some(passphrase) = r.prompt(&target, &what)
                && try_passphrase(&passphrase)
            {
                r.accepted(&target);
                return Ok(());
            }
        }

        // Return Ok regardless — caller checks sess.authenticated()
        Ok(())
    }
}
