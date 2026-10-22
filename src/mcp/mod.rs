//! MCP server (`kget mcp`): lets AI agents download files through KGet.
//!
//! Speaks the [Model Context Protocol](https://modelcontextprotocol.io) over
//! stdio: newline-delimited JSON-RPC 2.0 on stdin/stdout (`initialize`,
//! `ping`, `tools/list`, `tools/call`, `notifications/initialized`,
//! `notifications/cancelled`). Long downloads report
//! `notifications/progress` when the client sends a `progressToken`.
//! Nothing but protocol messages is ever written to stdout; diagnostics go to
//! stderr.
//!
//! The server is implemented by hand on `serde_json` rather than with the
//! `rmcp` SDK: KGet's engine is synchronous, the subset needed here is a few
//! hundred lines, and `rmcp` would pull an async runtime (tokio), macros and
//! a JSON-schema generator into every default build.
//!
//! Downloads go to a running daemon (`kget daemon`) when there is one, so
//! they survive the agent session; otherwise an in-process
//! [`JobManager`](crate::jobs::JobManager) runs them. Every write (and every
//! file read by `verify_checksum`) is restricted to one root directory:
//! `--root`, else the configured download directory.
//!
//! Register it with Claude Code:
//!
//! ```text
//! claude mcp add kget -- kget mcp
//! claude mcp add kget -- kget mcp --root ~/Downloads/agent
//! ```

pub mod tools;

use crate::config::Config;
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{RpcError, codes};
use crate::daemon::{Backend, PathPolicy, Service};
use crate::jobs::JobManager;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// MCP protocol revisions this server understands, newest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Largest message accepted on stdin (4 MiB).
const MAX_MESSAGE_BYTES: usize = 4 << 20;

/// Which job manager the MCP server uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendMode {
    /// The running daemon if there is one, else in-process
    #[default]
    Auto,
    /// Always an in-process job manager (downloads stop when the server exits)
    InProcess,
    /// The daemon, started in the background if needed
    Daemon,
}

/// Options of [`serve_stdio`].
#[derive(Debug, Clone, Default)]
pub struct McpOptions {
    /// Directory downloads may be written to (`None` = the configured
    /// download directory)
    pub root: Option<PathBuf>,
    /// Backend selection
    pub mode: BackendMode,
}

/// Serve MCP on stdin/stdout until stdin closes.
pub fn serve_stdio(options: McpOptions) -> Result<(), BoxError> {
    let (backend, policy) = build_backend(&options)?;
    eprintln!(
        "kget mcp: {} backend, writes restricted to {}",
        backend.mode(),
        policy.root().display()
    );
    let server = McpServer::new(backend, policy, Box::new(std::io::stdout()));
    server.run(std::io::stdin().lock());
    Ok(())
}

/// The backend and path policy for `options`.
pub fn build_backend(options: &McpOptions) -> Result<(Arc<dyn Backend>, PathPolicy), BoxError> {
    let config = Config::load()?;
    let root = options
        .root
        .clone()
        .unwrap_or_else(|| config.daemon.resolved_download_dir());
    let policy = PathPolicy::new(&root, false)?;
    let in_process = |config: Config| -> Arc<dyn Backend> {
        let max = config.daemon.max_concurrent;
        Arc::new(Service::new(
            JobManager::with_concurrency(config, max),
            policy.clone(),
        ))
    };
    let backend: Arc<dyn Backend> = match options.mode {
        BackendMode::InProcess => in_process(config),
        BackendMode::Daemon => Arc::new(DaemonClient::ensure_running()?),
        BackendMode::Auto => match DaemonClient::connect_default() {
            Ok(c) => Arc::new(c),
            Err(_) => in_process(config),
        },
    };
    Ok((backend, policy))
}

/// Writes JSON-RPC messages, one per line, from any thread.
#[derive(Clone)]
pub(crate) struct Output {
    inner: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Output {
    pub(crate) fn send(&self, msg: &Value) {
        let mut line = msg.to_string();
        line.push('\n');
        let mut w = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let _ = w.write_all(line.as_bytes()).and_then(|_| w.flush());
    }
}

/// An MCP session over any reader/writer pair.
pub struct McpServer {
    backend: Arc<dyn Backend>,
    policy: PathPolicy,
    out: Output,
    /// Cancel flags of in-flight `tools/call` requests, by request id
    in_flight: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// Set when stdin closes
    closing: Arc<AtomicBool>,
}

impl McpServer {
    /// A session using `backend`, restricted to `policy`, writing to `out`.
    pub fn new(backend: Arc<dyn Backend>, policy: PathPolicy, out: Box<dyn Write + Send>) -> Self {
        Self {
            backend,
            policy,
            out: Output {
                inner: Arc::new(Mutex::new(out)),
            },
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            closing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Process messages from `input` until it closes. Tool calls run on their
    /// own threads so `ping` and cancellations are answered meanwhile.
    pub fn run<R: BufRead>(&self, mut input: R) {
        let mut workers: Vec<JoinHandle<()>> = Vec::new();
        loop {
            let line =
                match crate::daemon::transport::read_line_limited(&mut input, MAX_MESSAGE_BYTES) {
                    Ok(crate::daemon::transport::Line::Data(l)) => l,
                    Ok(crate::daemon::transport::Line::TooLarge) => {
                        self.error(&Value::Null, codes::INVALID_REQUEST, "Message too large");
                        break;
                    }
                    Ok(crate::daemon::transport::Line::Eof) | Err(_) => break,
                };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(Value::Array(batch)) => {
                    for msg in batch {
                        workers.extend(self.dispatch(msg));
                    }
                }
                Ok(msg) => workers.extend(self.dispatch(msg)),
                Err(e) => self.error(
                    &Value::Null,
                    codes::PARSE_ERROR,
                    &format!("Parse error: {e}"),
                ),
            }
            workers.retain(|w| !w.is_finished());
        }
        // stdin closed: stop waiting for downloads; answer what is pending.
        self.closing.store(true, Ordering::SeqCst);
        for w in workers {
            let _ = w.join();
        }
    }

    fn lock_in_flight(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.in_flight.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn error(&self, id: &Value, code: i64, message: &str) {
        self.out.send(&crate::daemon::protocol::failure(
            id,
            &RpcError::new(code, message),
        ));
    }

    /// Handle one message; returns the worker thread of a tool call.
    fn dispatch(&self, msg: Value) -> Option<JoinHandle<()>> {
        let id = msg.get("id").cloned();
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            // A response to a request we never sent, or garbage.
            if msg.get("result").is_none() && msg.get("error").is_none() {
                self.error(
                    &id.unwrap_or(Value::Null),
                    codes::INVALID_REQUEST,
                    "Missing method",
                );
            }
            return None;
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            self.notification(method, &params);
            return None;
        };
        let result = match method {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::definitions() })),
            "tools/call" => return self.spawn_tool_call(id, params),
            other => Err(RpcError::method_not_found(other)),
        };
        self.out.send(&match result {
            Ok(v) => crate::daemon::protocol::success(&id, v),
            Err(e) => crate::daemon::protocol::failure(&id, &e),
        });
        None
    }

    fn notification(&self, method: &str, params: &Value) {
        if method == "notifications/cancelled"
            && let Some(req) = params.get("requestId")
            && let Some(flag) = self.lock_in_flight().get(&req.to_string())
        {
            flag.store(true, Ordering::SeqCst);
        }
        // notifications/initialized and others need no action.
    }

    fn spawn_tool_call(&self, id: Value, params: Value) -> Option<JoinHandle<()>> {
        let key = id.to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.lock_in_flight().insert(key.clone(), cancel.clone());
        let progress = params
            .get("_meta")
            .and_then(|m| m.get("progressToken"))
            .filter(|t| t.is_string() || t.is_number())
            .cloned()
            .map(|token| tools::Progress::new(self.out.clone(), token));
        let ctx = tools::ToolContext {
            backend: self.backend.clone(),
            policy: self.policy.clone(),
            progress,
            cancel: cancel.clone(),
            closing: self.closing.clone(),
        };
        let out = self.out.clone();
        let in_flight = self.in_flight.clone();
        let handle = std::thread::Builder::new()
            .name("kget-mcp-tool".into())
            .spawn(move || {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = tools::call(&ctx, name, &args);
                in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&key);
                // A cancelled request gets no response (MCP spec).
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                out.send(&match result {
                    Ok(v) => crate::daemon::protocol::success(&id, v),
                    Err(e) => crate::daemon::protocol::failure(&id, &e),
                });
            });
        handle.ok()
    }
}

/// Result of `initialize`: the client's protocol version when supported,
/// else our newest.
fn initialize_result(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "kget",
            "title": "KGet download manager",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "KGet downloads files (HTTP/HTTPS, FTP, SFTP, WebDAV, magnet/torrent, Metalink, video pages via yt-dlp) into one allowed directory. Use `download` with wait=true for small files (the result has the saved path), or wait=false for large ones and poll `download_status`. Paths are relative to the allowed directory; `get_config` shows it. Verify integrity with a `checksum` on `download` or with `verify_checksum` afterwards.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_negotiation() {
        let v = initialize_result(&json!({"protocolVersion": "2025-06-18"}));
        assert_eq!(v["protocolVersion"], "2025-06-18");
        let v = initialize_result(&json!({"protocolVersion": "1999-01-01"}));
        assert_eq!(v["protocolVersion"], SUPPORTED_PROTOCOL_VERSIONS[0]);
    }
}
