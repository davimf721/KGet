//! Local transports: a Unix domain socket, or loopback TCP with a token.
//!
//! See the [module documentation](super) for why Windows uses TCP. The TCP
//! transport compiles everywhere (and can be forced on Unix with
//! `KGET_DAEMON_TRANSPORT=tcp`), so its code is type-checked and tested on
//! every platform.

use super::DaemonPaths;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

/// Which transport the daemon listens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Unix domain socket (`daemon.sock`, mode `0600`)
    Unix,
    /// `127.0.0.1` with a random token (`daemon.tcp`, mode `0600`)
    Tcp,
}

impl TransportKind {
    /// The platform default, overridable with `KGET_DAEMON_TRANSPORT=tcp|unix`.
    pub fn from_env() -> Self {
        match std::env::var("KGET_DAEMON_TRANSPORT").as_deref() {
            Ok("tcp") => TransportKind::Tcp,
            Ok("unix") if cfg!(unix) => TransportKind::Unix,
            _ if cfg!(unix) => TransportKind::Unix,
            _ => TransportKind::Tcp,
        }
    }

    /// `"unix"` or `"tcp"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            TransportKind::Unix => "unix",
            TransportKind::Tcp => "tcp",
        }
    }
}

/// Contents of `daemon.tcp`.
#[derive(Clone, Serialize, Deserialize)]
pub struct TcpEndpoint {
    /// Port on `127.0.0.1`
    pub port: u16,
    /// Secret every connection must send with `auth` first
    pub token: String,
}

impl std::fmt::Debug for TcpEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpEndpoint")
            .field("port", &self.port)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Where a client connects.
#[derive(Debug, Clone)]
pub enum Endpoint {
    /// Unix socket path
    Unix(PathBuf),
    /// Loopback TCP
    Tcp(TcpEndpoint),
}

impl Endpoint {
    /// The endpoint of the daemon using `paths`: the TCP endpoint file when
    /// present (TCP transport), else the Unix socket. `None` when neither
    /// exists (no daemon).
    pub fn discover(paths: &DaemonPaths) -> Option<Self> {
        if let Ok(text) = std::fs::read_to_string(paths.tcp_endpoint())
            && let Ok(ep) = serde_json::from_str::<TcpEndpoint>(&text)
        {
            return Some(Endpoint::Tcp(ep));
        }
        let socket = paths.socket();
        if cfg!(unix) && socket.exists() {
            return Some(Endpoint::Unix(socket));
        }
        None
    }

    /// Open a connection (with a connect timeout for TCP).
    pub fn connect(&self) -> io::Result<Stream> {
        match self {
            #[cfg(unix)]
            Endpoint::Unix(path) => Ok(Stream::Unix(UnixStream::connect(path)?)),
            #[cfg(not(unix))]
            Endpoint::Unix(_) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unix sockets are not available on this platform",
            )),
            Endpoint::Tcp(ep) => {
                let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, ep.port));
                let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
                s.set_nodelay(true)?;
                Ok(Stream::Tcp(s))
            }
        }
    }

    /// Token to send with `auth`, for TCP.
    pub fn token(&self) -> Option<&str> {
        match self {
            Endpoint::Tcp(ep) => Some(&ep.token),
            Endpoint::Unix(_) => None,
        }
    }
}

/// One connection, either transport.
#[derive(Debug)]
pub enum Stream {
    /// Unix socket
    #[cfg(unix)]
    Unix(UnixStream),
    /// Loopback TCP
    Tcp(TcpStream),
}

impl Stream {
    /// Another handle on the same connection (reader/writer split).
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(match self {
            #[cfg(unix)]
            Stream::Unix(s) => Stream::Unix(s.try_clone()?),
            Stream::Tcp(s) => Stream::Tcp(s.try_clone()?),
        })
    }

    /// Timeout of blocking reads (`None` = wait forever).
    pub fn set_read_timeout(&self, t: Option<Duration>) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.set_read_timeout(t),
            Stream::Tcp(s) => s.set_read_timeout(t),
        }
    }

    /// Close both directions (wakes a thread blocked reading it).
    pub fn shutdown(&self) {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            Stream::Tcp(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    fn set_nonblocking(&self, nb: bool) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.set_nonblocking(nb),
            Stream::Tcp(s) => s.set_nonblocking(nb),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.read(buf),
            Stream::Tcp(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.write(buf),
            Stream::Tcp(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.flush(),
            Stream::Tcp(s) => s.flush(),
        }
    }
}

/// Listening socket of the daemon. Dropping it removes the socket / endpoint
/// file.
#[derive(Debug)]
pub struct Listener {
    kind: ListenerKind,
    /// File removed on drop (socket or endpoint file)
    file: PathBuf,
    token: Option<String>,
}

#[derive(Debug)]
enum ListenerKind {
    #[cfg(unix)]
    Unix(UnixListener),
    Tcp(TcpListener),
}

impl Listener {
    /// Listen on `kind` in the runtime directory of `paths`. The caller must
    /// hold the single-instance lock (a stale socket file is replaced).
    pub fn bind(kind: TransportKind, paths: &DaemonPaths) -> io::Result<Self> {
        super::create_private_dir(&paths.runtime_dir)?;
        // A crashed daemon may have left these behind.
        let _ = std::fs::remove_file(paths.socket());
        let _ = std::fs::remove_file(paths.tcp_endpoint());
        match kind {
            #[cfg(unix)]
            TransportKind::Unix => bind_unix(&paths.socket()),
            #[cfg(not(unix))]
            TransportKind::Unix => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Unix sockets are not available on this platform",
            )),
            TransportKind::Tcp => bind_tcp(&paths.tcp_endpoint()),
        }
    }

    /// The token clients must present (TCP only).
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Socket or endpoint file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// `"unix"` or `"tcp"`.
    pub fn kind(&self) -> TransportKind {
        match self.kind {
            #[cfg(unix)]
            ListenerKind::Unix(_) => TransportKind::Unix,
            ListenerKind::Tcp(_) => TransportKind::Tcp,
        }
    }

    /// Non-blocking accept: `Ok(None)` when no client is waiting.
    pub fn poll_accept(&self) -> io::Result<Option<Stream>> {
        let res = match &self.kind {
            #[cfg(unix)]
            ListenerKind::Unix(l) => l.accept().map(|(s, _)| Stream::Unix(s)),
            ListenerKind::Tcp(l) => l.accept().and_then(|(s, addr)| {
                // Bound to 127.0.0.1, but never trust a non-loopback peer.
                if !addr.ip().is_loopback() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "non-loopback peer",
                    ));
                }
                s.set_nodelay(true)?;
                Ok(Stream::Tcp(s))
            }),
        };
        match res {
            Ok(s) => {
                s.set_nonblocking(false)?;
                Ok(Some(s))
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Bind in a private temporary directory, restrict the socket to the owner,
/// then move it into place: no other user can ever connect, even between
/// `bind` and `chmod`.
#[cfg(unix)]
fn bind_unix(socket: &Path) -> io::Result<Listener> {
    use std::os::unix::fs::PermissionsExt;
    let dir = socket.parent().unwrap_or(Path::new("."));
    let staging = dir.join(format!(".daemon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)?;
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700))?;
    let tmp = staging.join("s");
    let result = (|| -> io::Result<UnixListener> {
        let listener = UnixListener::bind(&tmp)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        std::fs::rename(&tmp, socket)?;
        listener.set_nonblocking(true)?;
        Ok(listener)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    Ok(Listener {
        kind: ListenerKind::Unix(result?),
        file: socket.to_path_buf(),
        token: None,
    })
}

fn bind_tcp(endpoint_file: &Path) -> io::Result<Listener> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let token = random_token()?;
    let ep = TcpEndpoint {
        port,
        token: token.clone(),
    };
    let text = serde_json::to_string(&ep).map_err(io::Error::other)?;
    crate::config::write_atomic(endpoint_file, text.as_bytes(), true)?;
    Ok(Listener {
        kind: ListenerKind::Tcp(listener),
        file: endpoint_file.to_path_buf(),
        token: Some(token),
    })
}

/// 256 random bits from the OS, hex-encoded.
pub fn random_token() -> io::Result<String> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(hex::encode(buf))
}

/// Constant-time comparison (the token must not leak through timing).
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Outcome of [`read_line_limited`].
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// One line, without the trailing newline
    Data(String),
    /// The line exceeded the limit (the connection should be closed)
    TooLarge,
    /// The peer closed the connection
    Eof,
}

/// Read one `\n`-terminated line of at most `max` bytes.
pub fn read_line_limited<R: BufRead>(reader: &mut R, max: usize) -> io::Result<Line> {
    let mut buf = Vec::new();
    let n = reader
        .by_ref()
        .take(max as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(Line::Eof);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
    } else if buf.len() > max {
        return Ok(Line::TooLarge);
    }
    String::from_utf8(buf)
        .map(Line::Data)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_limit() {
        let mut r = io::Cursor::new(b"abc\r\nlonger-than-limit\nx".to_vec());
        assert_eq!(
            read_line_limited(&mut r, 8).unwrap(),
            Line::Data("abc".into())
        );
        assert_eq!(read_line_limited(&mut r, 8).unwrap(), Line::TooLarge);
    }

    #[test]
    fn tokens() {
        let t = random_token().unwrap();
        assert_eq!(t.len(), 64);
        assert_ne!(t, random_token().unwrap());
        assert!(tokens_match(&t, &t.clone()));
        assert!(!tokens_match(&t, "short"));
    }
}
