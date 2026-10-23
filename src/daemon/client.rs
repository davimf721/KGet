//! Blocking client for the daemon.
//!
//! Each call opens its own short connection (cheap on a local socket), so a
//! long `wait` never blocks other calls from the same client and the client
//! is freely shareable between threads.

use super::protocol::{
    self, EVENT_METHOD, IdParams, PROTOCOL_VERSION, RpcError, SubmitParams, SubscribeParams, codes,
};
use super::service::Backend;
use super::transport::{self, Endpoint, Line, Stream};
use super::{DaemonPaths, MAX_REQUEST_BYTES};
use serde_json::{Value, json};
use std::io::{BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// How long [`DaemonClient::ensure_running`] waits for a new daemon.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// Client of a running daemon.
#[derive(Debug)]
pub struct DaemonClient {
    endpoint: Endpoint,
    paths: DaemonPaths,
    next_id: AtomicU64,
}

impl DaemonClient {
    /// A client for `endpoint` (no connection is made).
    pub fn new(endpoint: Endpoint, paths: DaemonPaths) -> Self {
        Self {
            endpoint,
            paths,
            next_id: AtomicU64::new(1),
        }
    }

    /// Connect to the daemon using `paths` and check its protocol version.
    pub fn connect(paths: &DaemonPaths) -> Result<Self, RpcError> {
        let endpoint = Endpoint::discover(paths).ok_or_else(|| {
            RpcError::new(codes::INTERNAL_ERROR, "The KGet daemon is not running")
        })?;
        let client = Self::new(endpoint, paths.clone());
        let v = client.call("version", Value::Null)?;
        let proto = v.get("protocol").and_then(Value::as_u64).unwrap_or(0);
        if proto != u64::from(PROTOCOL_VERSION) {
            return Err(RpcError::new(
                codes::VERSION_MISMATCH,
                format!(
                    "The running daemon speaks protocol {proto}, this kget speaks {PROTOCOL_VERSION}; restart it with `kget daemon stop && kget daemon`"
                ),
            ));
        }
        Ok(client)
    }

    /// Connect to the daemon at the default location.
    pub fn connect_default() -> Result<Self, RpcError> {
        let paths = DaemonPaths::resolve().map_err(RpcError::internal)?;
        Self::connect(&paths)
    }

    /// Connect to the daemon at the default location, starting one in the
    /// background (`kget daemon --foreground`, detached) if needed.
    pub fn ensure_running() -> Result<Self, RpcError> {
        let paths = DaemonPaths::resolve().map_err(RpcError::internal)?;
        let exe = std::env::current_exe().map_err(RpcError::internal)?;
        Self::ensure_running_with(&paths, &exe, &[])
    }

    /// Connect to the daemon using `paths`, starting `exe daemon --foreground
    /// <extra_args>` if none answers.
    pub fn ensure_running_with(
        paths: &DaemonPaths,
        exe: &Path,
        extra_args: &[String],
    ) -> Result<Self, RpcError> {
        if let Ok(c) = Self::connect(paths) {
            return Ok(c);
        }
        spawn_daemon(exe, extra_args)
            .map_err(|e| RpcError::internal(format!("Cannot start the KGet daemon: {e}")))?;
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            match Self::connect(paths) {
                Ok(c) => return Ok(c),
                Err(e) if e.code == codes::VERSION_MISMATCH => return Err(e),
                Err(e) if Instant::now() >= deadline => {
                    return Err(RpcError::internal(format!(
                        "The KGet daemon did not start in time ({}); see {}",
                        e.message,
                        paths.log().display()
                    )));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    /// Directories this client uses.
    pub fn paths(&self) -> &DaemonPaths {
        &self.paths
    }

    /// Open an (authenticated) connection.
    fn open(&self) -> Result<(BufReader<Stream>, Stream), RpcError> {
        let stream = self.endpoint.connect().map_err(|e| {
            RpcError::new(
                codes::INTERNAL_ERROR,
                format!("Cannot reach the KGet daemon: {e}"),
            )
        })?;
        let writer = stream.try_clone().map_err(RpcError::internal)?;
        let mut conn = (BufReader::new(stream), writer);
        if let Some(token) = self.endpoint.token() {
            let token = token.to_string();
            self.exchange(&mut conn, "auth", json!({ "token": token }))?;
        }
        Ok(conn)
    }

    /// Send one request on `conn` and read its response, skipping
    /// notifications.
    fn exchange(
        &self,
        conn: &mut (BufReader<Stream>, Stream),
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut line = protocol::request(id, method, params).to_string();
        line.push('\n');
        conn.1
            .write_all(line.as_bytes())
            .and_then(|_| conn.1.flush())
            .map_err(|e| RpcError::internal(format!("Lost the daemon connection: {e}")))?;
        loop {
            let line = match transport::read_line_limited(&mut conn.0, MAX_REQUEST_BYTES * 16) {
                Ok(Line::Data(l)) => l,
                Ok(Line::TooLarge) => return Err(RpcError::internal("Response too large")),
                Ok(Line::Eof) => {
                    return Err(RpcError::internal("The daemon closed the connection"));
                }
                Err(e) => {
                    return Err(RpcError::internal(format!(
                        "Lost the daemon connection: {e}"
                    )));
                }
            };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(err) = msg.get("error") {
                return Err(serde_json::from_value(err.clone())
                    .unwrap_or_else(|_| RpcError::internal(err.to_string())));
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Call `method` with `params` on a new connection.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let mut conn = self.open()?;
        let result = self.exchange(&mut conn, method, params);
        conn.0.get_ref().shutdown();
        result
    }

    /// `submit`: queue a download; returns the job snapshot.
    pub fn submit(&self, params: &SubmitParams) -> Result<Value, RpcError> {
        self.call(
            "submit",
            serde_json::to_value(params).map_err(RpcError::internal)?,
        )
    }

    /// `get`: snapshot of one job.
    pub fn get(&self, id: u64) -> Result<Value, RpcError> {
        self.call("get", json!(IdParams { id }))
    }

    /// `list`: snapshots of every job.
    pub fn list(&self) -> Result<Vec<Value>, RpcError> {
        let v = self.call("list", Value::Null)?;
        Ok(v.get("jobs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Ask the daemon to stop (jobs are saved and resumed on the next start).
    pub fn shutdown(&self) -> Result<(), RpcError> {
        self.call("shutdown", Value::Null).map(|_| ())
    }

    /// Events (JSON [`JobEvent`](crate::jobs::JobEvent)s) matching `params`,
    /// on a dedicated connection that stays open until the receiver is
    /// dropped and the next event arrives.
    pub fn events(&self, params: &SubscribeParams) -> Result<Receiver<Value>, RpcError> {
        let mut conn = self.open()?;
        self.exchange(
            &mut conn,
            "subscribe",
            serde_json::to_value(params).map_err(RpcError::internal)?,
        )?;
        let (tx, rx) = mpsc::channel();
        let (mut reader, writer) = conn;
        std::thread::Builder::new()
            .name("kget-daemon-client-events".into())
            .spawn(move || {
                let _writer = writer;
                while let Ok(Line::Data(line)) =
                    transport::read_line_limited(&mut reader, MAX_REQUEST_BYTES * 16)
                {
                    let Ok(mut msg) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if msg.get("method").and_then(Value::as_str) != Some(EVENT_METHOD) {
                        continue;
                    }
                    if tx.send(msg["params"].take()).is_err() {
                        break;
                    }
                }
                reader.get_ref().shutdown();
            })
            .map_err(RpcError::internal)?;
        Ok(rx)
    }
}

impl Backend for DaemonClient {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        DaemonClient::call(self, method, params)
    }

    fn subscribe(&self, params: SubscribeParams) -> Result<Receiver<Value>, RpcError> {
        self.events(&params)
    }

    fn mode(&self) -> &'static str {
        "daemon"
    }
}

/// Start `exe daemon --foreground <extra_args>` detached from this process
/// (own process group / no console), with stdio closed. Returns its pid.
pub fn spawn_daemon(exe: &Path, extra_args: &[String]) -> std::io::Result<u32> {
    let mut cmd = Command::new(exe);
    cmd.arg("daemon")
        .arg("--foreground")
        .args(extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Not killed by Ctrl+C in the terminal that started it.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    Ok(cmd.spawn()?.id())
}
