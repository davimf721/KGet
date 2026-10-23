//! The daemon process: single-instance lock, listener, connections and
//! clean shutdown.

use super::protocol::{
    self, AuthParams, EVENT_METHOD, RpcError, SubscribeParams, codes, parse_params,
};
use super::service::{PersistHandle, Service, event_matches};
use super::store::JobStore;
use super::transport::{self, Line, Listener, Stream, TransportKind};
use super::{DaemonPaths, Logger, MAX_REQUEST_BYTES, PathPolicy};
use crate::config::Config;
use crate::jobs::{JobEvent, JobManager};
use fs2::FileExt;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Simultaneous client connections; more are refused.
const MAX_CONNECTIONS: usize = 64;
/// How long a shutdown waits for running jobs to stop.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Options of [`run`]. `Default` reads everything from the config file and
/// environment.
#[derive(Debug, Clone)]
pub struct DaemonOptions {
    /// Runtime and config directories (`None` = [`DaemonPaths::resolve`])
    pub paths: Option<DaemonPaths>,
    /// Configuration (`None` = `config.json`)
    pub config: Option<Config>,
    /// Override `daemon.download_dir`
    pub download_dir: Option<PathBuf>,
    /// Override `daemon.allow_outside_download_dir`
    pub allow_outside_download_dir: Option<bool>,
    /// Override `daemon.max_concurrent`
    pub max_concurrent: Option<usize>,
    /// Transport (`None` = [`TransportKind::from_env`])
    pub transport: Option<TransportKind>,
    /// Stop cleanly on SIGINT/SIGTERM/SIGHUP (Ctrl+C on Windows). Only one
    /// handler can be installed per process, so embedders may opt out.
    pub handle_signals: bool,
    /// Also print log lines to stderr
    pub echo_log: bool,
    /// Record finished jobs in the download history
    pub record_history: bool,
}

impl Default for DaemonOptions {
    fn default() -> Self {
        Self {
            paths: None,
            config: None,
            download_dir: None,
            allow_outside_download_dir: None,
            max_concurrent: None,
            transport: None,
            handle_signals: true,
            echo_log: false,
            record_history: true,
        }
    }
}

/// Error returned by [`run`] when another daemon holds the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlreadyRunning {
    /// Process id of the running daemon, when known
    pub pid: Option<u32>,
}

impl std::fmt::Display for AlreadyRunning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pid {
            Some(pid) => write!(f, "The KGet daemon is already running (pid {pid})"),
            None => write!(f, "The KGet daemon is already running"),
        }
    }
}

impl std::error::Error for AlreadyRunning {}

/// Run the daemon until a `shutdown` request or a signal. Blocks.
pub fn run(options: DaemonOptions) -> Result<(), BoxError> {
    let paths = match options.paths.clone() {
        Some(p) => p,
        None => DaemonPaths::resolve()?,
    };
    super::create_private_dir(&paths.runtime_dir)?;

    // Single instance: the lock is held until this function returns.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.lock())?;
    let _ = super::restrict_to_owner(&paths.lock());
    // A daemon that is just stopping (`kget daemon stop && kget daemon`)
    // releases the lock within moments.
    let mut attempts = 0;
    while lock.try_lock_exclusive().is_err() {
        attempts += 1;
        if attempts > 30 {
            return Err(Box::new(AlreadyRunning {
                pid: paths.read_pid(),
            }));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    crate::config::write_atomic(
        &paths.pid(),
        std::process::id().to_string().as_bytes(),
        true,
    )?;

    let logger = Arc::new(Logger::open(&paths.log(), options.echo_log));
    let mut config = match options.config.clone() {
        Some(c) => c,
        None => Config::load()?,
    };
    if let Some(d) = &options.download_dir {
        config.daemon.download_dir = Some(d.to_string_lossy().into_owned());
    }
    if let Some(a) = options.allow_outside_download_dir {
        config.daemon.allow_outside_download_dir = a;
    }
    if let Some(n) = options.max_concurrent {
        config.daemon.max_concurrent = n;
    }
    let policy = PathPolicy::new(
        config.daemon.resolved_download_dir(),
        config.daemon.allow_outside_download_dir,
    )?;
    let manager = JobManager::with_concurrency(config.clone(), config.daemon.max_concurrent);
    manager.set_record_history(options.record_history);

    let store = JobStore::open(paths.database())?;
    let service = Service::with_store(manager, policy, store, logger.clone());
    let persist = service.start_persistence();
    let restored = service.restore_saved();

    let kind = options.transport.unwrap_or_else(TransportKind::from_env);
    let listener = Listener::bind(kind, &paths)?;
    service.set_transport(kind.as_str());
    logger.info(&format!(
        "KGet {} daemon started (pid {}, {} transport at {}, download dir {}{}, {} job(s) restored)",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        kind.as_str(),
        listener.file().display(),
        service.policy().root().display(),
        if service.policy().allows_outside() {
            ", writes outside allowed"
        } else {
            ""
        },
        restored
    ));

    let shutdown = service.shutdown_flag();
    if options.handle_signals {
        let flag = shutdown.clone();
        if let Err(e) = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst)) {
            logger.warn(&format!("cannot install the signal handler: {e}"));
        }
    }

    accept_loop(&listener, &service, &shutdown);

    logger.info("shutting down");
    drop(listener);
    stop(&service, persist);
    let _ = std::fs::remove_file(paths.pid());
    logger.info("stopped");
    drop(lock);
    Ok(())
}

fn accept_loop(listener: &Listener, service: &Service, shutdown: &Arc<AtomicBool>) {
    let connections = Arc::new(AtomicUsize::new(0));
    let token: Option<Arc<str>> = listener.token().map(Arc::from);
    while !shutdown.load(Ordering::SeqCst) {
        match listener.poll_accept() {
            Ok(Some(stream)) => {
                if connections.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    service.logger().warn("too many connections; refusing one");
                    stream.shutdown();
                    continue;
                }
                connections.fetch_add(1, Ordering::SeqCst);
                let service = service.clone();
                let token = token.clone();
                let count = connections.clone();
                let spawned = std::thread::Builder::new()
                    .name("kget-daemon-conn".into())
                    .spawn(move || {
                        let log_to = service.clone();
                        // A panic drops this client only; the slot is freed
                        // either way (a leaked one used to count toward
                        // MAX_CONNECTIONS forever).
                        let served = crate::guard::catch(|| {
                            if let Ok(conn) = Connection::new(stream, service, token) {
                                conn.serve();
                            }
                        });
                        if let Err(msg) = served {
                            log_to
                                .logger()
                                .warn(&format!("connection handler panicked: {msg}"));
                        }
                        count.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    connections.fetch_sub(1, Ordering::SeqCst);
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                service.logger().warn(&format!("accept failed: {e}"));
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// Pause running jobs and save the queue so the next start resumes it.
fn stop(service: &Service, persist: Option<PersistHandle>) {
    if let Some(p) = persist {
        p.stop();
    }
    let manager = service.manager();
    let active: HashSet<_> = manager
        .list()
        .into_iter()
        .filter(|j| !j.state.is_finished())
        .map(|j| j.id)
        .collect();
    for id in &active {
        let _ = manager.pause(*id);
    }
    if !manager.wait_idle(Some(SHUTDOWN_GRACE)) {
        service
            .logger()
            .warn("some jobs did not stop in time; they will resume on the next start");
    }
    service.save_for_restart(&active);
}

/// One client connection.
struct Connection {
    reader: BufReader<Stream>,
    writer: Arc<Mutex<Stream>>,
    service: Service,
    token: Option<Arc<str>>,
    authenticated: bool,
    subscription: Option<Arc<AtomicBool>>,
    /// Subscription to start once the `subscribe` response is written
    pending: Option<(Receiver<JobEvent>, SubscribeParams)>,
}

impl Connection {
    fn new(stream: Stream, service: Service, token: Option<Arc<str>>) -> std::io::Result<Self> {
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer: Arc::new(Mutex::new(writer)),
            service,
            authenticated: token.is_none(),
            token,
            subscription: None,
            pending: None,
        })
    }

    fn serve(mut self) {
        loop {
            let line = match transport::read_line_limited(&mut self.reader, MAX_REQUEST_BYTES) {
                Ok(Line::Data(l)) => l,
                Ok(Line::TooLarge) => {
                    let err = RpcError::new(
                        codes::REQUEST_TOO_LARGE,
                        format!("Request exceeds {MAX_REQUEST_BYTES} bytes"),
                    );
                    self.send(&protocol::failure(&Value::Null, &err));
                    break;
                }
                Ok(Line::Eof) | Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let (reply, close) = match serde_json::from_str::<Value>(&line) {
                Err(e) => (
                    Some(protocol::failure(
                        &Value::Null,
                        &RpcError::new(codes::PARSE_ERROR, format!("Parse error: {e}")),
                    )),
                    false,
                ),
                Ok(Value::Array(batch)) if batch.is_empty() => (
                    Some(protocol::failure(
                        &Value::Null,
                        &RpcError::new(codes::INVALID_REQUEST, "Empty batch"),
                    )),
                    false,
                ),
                Ok(Value::Array(batch)) => {
                    let mut out = Vec::new();
                    let mut close = false;
                    for req in batch {
                        let (r, c) = self.handle(req);
                        out.extend(r);
                        close |= c;
                        if close {
                            break;
                        }
                    }
                    ((!out.is_empty()).then_some(Value::Array(out)), close)
                }
                Ok(req) => self.handle(req),
            };
            if let Some(reply) = reply
                && !self.send(&reply)
            {
                break;
            }
            self.start_pending_subscription();
            if close {
                break;
            }
        }
        if let Some(stop) = self.subscription.take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.reader.get_ref().shutdown();
    }

    fn send(&self, msg: &Value) -> bool {
        write_line(&self.writer, msg)
    }

    /// Handle one request; returns the response (none for notifications) and
    /// whether to close the connection.
    fn handle(&mut self, req: Value) -> (Option<Value>, bool) {
        let Some(obj) = req.as_object() else {
            let err = RpcError::new(codes::INVALID_REQUEST, "Request must be an object");
            return (Some(protocol::failure(&Value::Null, &err)), false);
        };
        let id = obj.get("id").cloned();
        let reply_id = id.clone().unwrap_or(Value::Null);
        let valid_id = matches!(
            id,
            None | Some(Value::Null | Value::Number(_) | Value::String(_))
        );
        let method = obj.get("method").and_then(Value::as_str);
        let (Some(method), true, Some("2.0")) =
            (method, valid_id, obj.get("jsonrpc").and_then(Value::as_str))
        else {
            let err = RpcError::new(
                codes::INVALID_REQUEST,
                "Not a JSON-RPC 2.0 request (need \"jsonrpc\":\"2.0\" and a method)",
            );
            return (Some(protocol::failure(&reply_id, &err)), false);
        };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let mut close = false;

        let result = if !self.authenticated && method != "auth" {
            close = true;
            Err(RpcError::new(
                codes::UNAUTHORIZED,
                "Authenticate first with the 'auth' method",
            ))
        } else {
            match method {
                "auth" => {
                    let p: AuthParams = parse_params(params).unwrap_or_default();
                    match &self.token {
                        None => Ok(json!({ "ok": true })),
                        Some(t) if transport::tokens_match(t, &p.token) => {
                            self.authenticated = true;
                            Ok(json!({ "ok": true }))
                        }
                        Some(_) => {
                            close = true;
                            self.service
                                .logger()
                                .warn("rejected a client with a wrong token");
                            Err(RpcError::new(codes::UNAUTHORIZED, "Wrong token"))
                        }
                    }
                }
                "subscribe" => parse_params::<SubscribeParams>(params).map(|p| {
                    if let Some(stop) = self.subscription.take() {
                        stop.store(true, Ordering::SeqCst);
                    }
                    // Subscribe now so no event is missed, forward after replying.
                    self.pending = Some((self.service.manager().subscribe(), p));
                    json!({ "subscribed": true })
                }),
                "unsubscribe" => {
                    if let Some(stop) = self.subscription.take() {
                        stop.store(true, Ordering::SeqCst);
                    }
                    Ok(json!({ "subscribed": false }))
                }
                m if self.service.shutdown_flag().load(Ordering::SeqCst) && m != "version" => Err(
                    RpcError::new(codes::SHUTTING_DOWN, "The daemon is shutting down"),
                ),
                // A panicking method gets an error reply; the connection and
                // the daemon keep serving.
                m => crate::guard::catch(|| self.service.handle(m, params)).unwrap_or_else(|msg| {
                    self.service
                        .logger()
                        .warn(&format!("method '{m}' panicked: {msg}"));
                    Err(RpcError::internal(format!("'{m}' panicked: {msg}")))
                }),
            }
        };

        let reply = id.map(|_| match &result {
            Ok(v) => protocol::success(&reply_id, v.clone()),
            Err(e) => protocol::failure(&reply_id, e),
        });
        (reply, close)
    }

    fn start_pending_subscription(&mut self) {
        let Some((rx, params)) = self.pending.take() else {
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        self.subscription = Some(stop.clone());
        let writer = self.writer.clone();
        let _ = std::thread::Builder::new()
            .name("kget-daemon-events".into())
            .spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match rx.recv_timeout(Duration::from_millis(250)) {
                        Ok(event) => {
                            if !event_matches(&event, &params) {
                                continue;
                            }
                            let params = serde_json::to_value(&event).unwrap_or(Value::Null);
                            if !write_line(&writer, &protocol::notification(EVENT_METHOD, params)) {
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
    }
}

fn write_line(writer: &Mutex<Stream>, msg: &Value) -> bool {
    let mut line = msg.to_string();
    line.push('\n');
    let mut w = writer.lock().unwrap_or_else(|e| e.into_inner());
    w.write_all(line.as_bytes()).and_then(|_| w.flush()).is_ok()
}
