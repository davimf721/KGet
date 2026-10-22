//! Background download daemon (`kget daemon`).
//!
//! One long-lived process owns a [`JobManager`](crate::jobs::JobManager) and
//! serves it to local clients — `kget queue …`, `kget mcp`, the macOS app or
//! a browser extension's native-messaging host — over a local socket:
//!
//! - **macOS / Linux:** a Unix domain socket (`daemon.sock`, mode `0600`) in
//!   a `0700` runtime directory.
//! - **Windows:** loopback TCP on `127.0.0.1` with a random token stored in
//!   `daemon.tcp` (owner-only file). The standard library has no named-pipe
//!   API, and a pipe server needs either `unsafe` overlapped I/O through
//!   `windows-sys` or an async runtime; a token-protected loopback socket
//!   uses only `std`, behaves the same on every OS (set
//!   `KGET_DAEMON_TRANSPORT=tcp` to use it on Unix too) and is what a
//!   native-messaging host would bridge to anyway.
//!
//! The wire protocol is newline-delimited JSON-RPC 2.0; see [`protocol`] for
//! the methods and their JSON shapes. Queued, running and paused jobs are
//! saved in `jobs.db` (SQLite, see [`store`]) so a restarted daemon resumes
//! them. A lock file keeps a single instance per runtime directory, and the
//! daemon logs to `daemon.log` in the config directory (URLs are redacted,
//! header values and tokens are never logged).
//!
//! **Files**
//!
//! | File | Directory | Purpose |
//! |------|-----------|---------|
//! | `daemon.sock` | runtime | Unix socket (`0600`) |
//! | `daemon.tcp` | runtime | TCP port and token (`0600`), TCP transport only |
//! | `daemon.lock` / `daemon.pid` | runtime | single instance, process id |
//! | `jobs.db` | config | persistent queue |
//! | `daemon.log` | config | log file (rotated at 5 MiB) |
//!
//! The runtime directory is `KGET_RUNTIME_DIR`, else `KGET_CONFIG_DIR`, else
//! `$XDG_RUNTIME_DIR/kget` (Linux), else the config directory.
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::daemon::client::DaemonClient;
//! use kget::daemon::protocol::SubmitParams;
//!
//! let client = DaemonClient::ensure_running()?; // starts `kget daemon` if needed
//! let info = client.submit(&SubmitParams::new("https://example.com/file.iso"))?;
//! println!("queued job {}", info["id"]);
//! # Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
//! ```

pub mod cli;
pub mod client;
mod log;
pub mod paths;
pub mod protocol;
pub mod server;
pub mod service;
pub mod store;
pub mod transport;

pub use log::Logger;
pub use paths::{PathError, PathPolicy};
pub use protocol::{PROTOCOL_VERSION, RpcError};
pub use server::{DaemonOptions, run};
pub use service::{Backend, Service};

use std::io;
use std::path::{Path, PathBuf};

/// Largest request line the daemon accepts (1 MiB).
pub const MAX_REQUEST_BYTES: usize = 1 << 20;

/// Where the daemon keeps its socket, lock, database and log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonPaths {
    /// Runtime directory (socket, lock, pid, TCP endpoint)
    pub runtime_dir: PathBuf,
    /// Config directory (database, log)
    pub config_dir: PathBuf,
}

impl DaemonPaths {
    /// The default locations (see the [module documentation](self)).
    pub fn resolve() -> io::Result<Self> {
        let config_dir = crate::config::config_dir()?;
        let runtime_dir = env_dir("KGET_RUNTIME_DIR")
            .or_else(|| env_dir("KGET_CONFIG_DIR"))
            .or_else(xdg_runtime_dir)
            .unwrap_or_else(|| config_dir.clone());
        Ok(Self {
            runtime_dir,
            config_dir,
        })
    }

    /// Explicit directories (tests, embedders).
    pub fn in_dirs(config_dir: impl AsRef<Path>, runtime_dir: impl AsRef<Path>) -> Self {
        Self {
            runtime_dir: runtime_dir.as_ref().to_path_buf(),
            config_dir: config_dir.as_ref().to_path_buf(),
        }
    }

    /// Unix socket path.
    pub fn socket(&self) -> PathBuf {
        self.runtime_dir.join("daemon.sock")
    }

    /// TCP endpoint file (port and token).
    pub fn tcp_endpoint(&self) -> PathBuf {
        self.runtime_dir.join("daemon.tcp")
    }

    /// Single-instance lock file.
    pub fn lock(&self) -> PathBuf {
        self.runtime_dir.join("daemon.lock")
    }

    /// File holding the daemon's process id.
    pub fn pid(&self) -> PathBuf {
        self.runtime_dir.join("daemon.pid")
    }

    /// Persistent queue database.
    pub fn database(&self) -> PathBuf {
        self.config_dir.join("jobs.db")
    }

    /// Log file.
    pub fn log(&self) -> PathBuf {
        self.config_dir.join("daemon.log")
    }

    /// Process id written by a running (or crashed) daemon.
    pub fn read_pid(&self) -> Option<u32> {
        std::fs::read_to_string(self.pid())
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn xdg_runtime_dir() -> Option<PathBuf> {
    env_dir("XDG_RUNTIME_DIR").map(|d| d.join("kget"))
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn xdg_runtime_dir() -> Option<PathBuf> {
    None
}

/// Create `dir` (and parents) private to the owner on Unix. An existing
/// directory is left as it is (it may be shared, like the config dir).
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Restrict `path` to the owner (`0600`) on Unix. On Windows files in the
/// per-user profile (`%APPDATA%`) are already private to the user.
pub(crate) fn restrict_to_owner(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
