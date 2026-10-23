//! Native-messaging host for the KGet browser extension (`kget native-host`).
//!
//! The browser starts the host (through the launcher written by
//! [`install`]) when the extension calls `runtime.connectNative`, and talks
//! to it with [`framing`] on stdin/stdout. The host bridges the messages to
//! the daemon with a [`DaemonClient`], starting the daemon if needed, so
//! downloads keep running after the browser closes.
//!
//! # Messages
//!
//! Requests carry a caller-chosen `id` and a `type`; every request gets one
//! reply with the same `id`:
//!
//! ```json
//! {"id":7,"type":"download","url":"https://example.com/a.iso","filename":"a.iso",
//!  "referrer":"https://example.com/","turbo":false,"send_cookies":false}
//! {"id":7,"type":"download","ok":true,"result":{"id":3,"state":"queued",…}}
//! {"id":8,"type":"pause","ok":false,"error":{"code":-32001,"message":"Job 9 not found"}}
//! ```
//!
//! | Type | Fields | Result |
//! |------|--------|--------|
//! | `ping` | – | `{pong, kget, protocol}` (does not start the daemon) |
//! | `version` | – | `{kget, protocol, daemon: <daemon version>}` |
//! | `download` | `url`, `filename?`, `referrer?`, `headers?`, `cookies?`, `send_cookies?`, `turbo?` | job snapshot |
//! | `list` | `limit?` | `{jobs: [job]}` |
//! | `get` / `pause` / `resume` / `cancel` / `remove` | `id` (job id, as `job`) | job snapshot |
//! | `subscribe` | `ids?`, `progress?` | `{subscribed: true}`, then `{"type":"event","event":<job.event>}` pushes |
//! | `open` / `reveal` | `job` | `{path, launched}` |
//!
//! Because `id` is the request id, job ids are passed as `job`. When the
//! event stream ends (daemon stopped), the host pushes
//! `{"type":"event_stream_closed"}`.
//!
//! # Security
//!
//! - The browser only starts the host for the extension IDs in the
//!   manifest; the host also checks the caller it is given on the command
//!   line (`chrome-extension://<id>/` from Chromium, the add-on ID from
//!   Firefox) against `allowed_origins.json` and exits otherwise.
//! - Only a file *name* is forwarded, never a directory; the daemon
//!   confines it to its download directory.
//! - `Cookie`, `Authorization` and `Proxy-Authorization` headers are
//!   dropped unless the message sets `send_cookies` (the extension's
//!   opt-in), and `cookies` is ignored without it.
//! - Only `http`, `https`, `ftp`, `ftps` and `magnet` URLs are accepted.
//! - `open` refuses executables and scripts (use `reveal`).

pub mod framing;
pub mod install;

use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{RpcError, SubmitParams, SubscribeParams, codes};
use crate::daemon::service::Backend;
use framing::{FrameError, MAX_INCOMING};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Version of the extension ↔ host message protocol.
pub const NATIVE_PROTOCOL_VERSION: u32 = 1;

/// Set to skip launching the file manager in `open` / `reveal` (tests).
pub const NO_LAUNCH_ENV: &str = "KGET_NATIVE_HOST_NO_LAUNCH";

/// URL schemes a browser may hand over.
const ALLOWED_SCHEMES: &[&str] = &["http", "https", "ftp", "ftps", "magnet"];

/// Headers that carry credentials.
const SENSITIVE_HEADERS: &[&str] = &["cookie", "authorization", "proxy-authorization"];

/// Headers the downloader manages itself.
const MANAGED_HEADERS: &[&str] = &[
    "host",
    "range",
    "content-length",
    "connection",
    "transfer-encoding",
    "accept-encoding",
];

/// Extensions `open` refuses to launch.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "app",
    "applescript",
    "bat",
    "cmd",
    "com",
    "command",
    "cpl",
    "desktop",
    "exe",
    "hta",
    "jar",
    "js",
    "jse",
    "lnk",
    "msc",
    "msi",
    "msp",
    "pif",
    "ps1",
    "psm1",
    "reg",
    "scf",
    "scr",
    "sh",
    "terminal",
    "url",
    "vb",
    "vbe",
    "vbs",
    "workflow",
    "ws",
    "wsf",
    "wsh",
];

/// Find which allowed origin the browser started the host for.
///
/// Chromium passes `chrome-extension://<id>/` (and on Windows
/// `--parent-window=<n>`); Firefox passes the manifest path and the add-on
/// ID. Returns the matching entry of `allowed`.
pub fn check_caller(args: &[String], allowed: &[String]) -> Result<String, String> {
    let normalize = |s: &str| s.trim().trim_end_matches('/').to_string();
    for arg in args {
        let a = normalize(arg);
        if a.is_empty() || a.starts_with("--") {
            continue;
        }
        if let Some(found) = allowed.iter().find(|o| normalize(o) == a) {
            return Ok(found.clone());
        }
    }
    let shown: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with("--"))
        .collect();
    if shown.is_empty() {
        Err("kget native-host is started by the browser extension; run `kget native-host install` to register it".into())
    } else {
        Err(format!(
            "caller {} is not an allowed extension (see `kget native-host install --extension-id`)",
            shown.join(" ")
        ))
    }
}

/// A file name the daemon can confine: the last path component, without
/// control characters. `None` when nothing usable is left.
pub fn sanitize_filename(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']).to_string();
    if clean.is_empty() || clean == "." || clean == ".." || clean.len() > 255 {
        None
    } else {
        Some(clean)
    }
}

/// Whether `url` is an absolute URL with a scheme the host accepts.
pub fn url_allowed(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| ALLOWED_SCHEMES.contains(&u.scheme()))
        .unwrap_or(false)
}

/// Turn a `download` message into `submit` parameters, applying the
/// security rules (scheme allowlist, file name only, credential headers
/// only with `send_cookies`).
pub fn submit_params(msg: &Value) -> Result<SubmitParams, RpcError> {
    let url = msg
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if url.is_empty() {
        return Err(RpcError::invalid_params("url is required"));
    }
    if !url_allowed(url) {
        return Err(RpcError::invalid_params(
            "only http, https, ftp, ftps and magnet URLs can be downloaded",
        ));
    }
    let mut p = SubmitParams::new(url);
    if let Some(name) = msg.get("filename").and_then(Value::as_str) {
        p.filename = sanitize_filename(name);
    }
    p.advanced = msg.get("turbo").and_then(Value::as_bool).unwrap_or(false);
    // Optional history tags (string or array of strings); the daemon
    // validates them.
    p.tags = match msg.get("tags") {
        Some(Value::String(t)) => vec![t.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    let send_cookies = msg
        .get("send_cookies")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut headers: Vec<(String, String)> = Vec::new();
    if let Some(map) = msg.get("headers").and_then(Value::as_object) {
        for (name, value) in map {
            let Some(value) = value.as_str() else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if MANAGED_HEADERS.contains(&lower.as_str())
                || (!send_cookies && SENSITIVE_HEADERS.contains(&lower.as_str()))
            {
                continue;
            }
            headers.push((name.clone(), value.to_string()));
        }
    }
    let has = |headers: &[(String, String)], name: &str| {
        headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
    };
    if let Some(referrer) = msg.get("referrer").and_then(Value::as_str)
        && !has(&headers, "referer")
        && url::Url::parse(referrer).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
    {
        headers.push(("Referer".into(), referrer.to_string()));
    }
    if send_cookies
        && let Some(cookies) = msg.get("cookies").and_then(Value::as_str)
        && !cookies.trim().is_empty()
    {
        headers.retain(|(n, _)| !n.eq_ignore_ascii_case("cookie"));
        headers.push(("Cookie".into(), cookies.trim().to_string()));
    }
    p.headers = headers;
    Ok(p)
}

/// Whether `open` must refuse `path`.
pub fn is_executable_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXECUTABLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Opens daemon connections (or anything else implementing [`Backend`]).
pub type Connector = Box<dyn Fn() -> Result<Arc<dyn Backend>, RpcError> + Send + Sync>;

/// Writes framed messages from any thread.
#[derive(Clone)]
struct Output {
    inner: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Output {
    fn send(&self, msg: &Value) -> Result<(), FrameError> {
        let mut w = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        framing::write_message(&mut *w, msg)
    }
}

/// One native-messaging session.
pub struct NativeHost {
    connect: Connector,
    backend: Mutex<Option<Arc<dyn Backend>>>,
    out: Output,
    subscribed: Arc<AtomicBool>,
    launch: bool,
}

impl NativeHost {
    /// A session writing to `writer` and reaching the daemon with `connect`.
    pub fn new(connect: Connector, writer: Box<dyn Write + Send>) -> Self {
        Self {
            connect,
            backend: Mutex::new(None),
            out: Output {
                inner: Arc::new(Mutex::new(writer)),
            },
            subscribed: Arc::new(AtomicBool::new(false)),
            launch: std::env::var_os(NO_LAUNCH_ENV).is_none(),
        }
    }

    /// Answer messages from `reader` until it ends. A malformed frame that
    /// can be skipped gets an error reply; a truncated or oversized one ends
    /// the session with an error.
    pub fn run<R: Read>(&self, mut reader: R) -> Result<(), FrameError> {
        loop {
            match framing::read_message(&mut reader, MAX_INCOMING) {
                Ok(None) => return Ok(()),
                Ok(Some(msg)) => {
                    let reply = self.handle(&msg);
                    self.send_reply(&reply)?;
                }
                Err(FrameError::InvalidJson(e)) => {
                    let err = RpcError::new(codes::PARSE_ERROR, format!("invalid JSON: {e}"));
                    self.send_reply(&error_reply(&Value::Null, "", &err))?;
                }
                Err(e) => {
                    let err = RpcError::new(codes::REQUEST_TOO_LARGE, e.to_string());
                    let _ = self.out.send(&error_reply(&Value::Null, "", &err));
                    return Err(e);
                }
            }
        }
    }

    fn send_reply(&self, reply: &Value) -> Result<(), FrameError> {
        match self.out.send(reply) {
            Err(FrameError::TooLarge(n)) => {
                let err = RpcError::internal(format!("reply of {n} bytes exceeds 1 MB"));
                let id = reply.get("id").cloned().unwrap_or(Value::Null);
                let ty = reply.get("type").and_then(Value::as_str).unwrap_or("");
                self.out.send(&error_reply(&id, ty, &err))
            }
            other => other,
        }
    }

    /// The reply to one request.
    pub fn handle(&self, msg: &Value) -> Value {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let ty = msg.get("type").and_then(Value::as_str).unwrap_or("");
        match self.dispatch(ty, msg) {
            Ok(result) => json!({"id": id, "type": ty, "ok": true, "result": result}),
            Err(e) => error_reply(&id, ty, &e),
        }
    }

    fn dispatch(&self, ty: &str, msg: &Value) -> Result<Value, RpcError> {
        match ty {
            "ping" => Ok(json!({
                "pong": true,
                "kget": env!("CARGO_PKG_VERSION"),
                "protocol": NATIVE_PROTOCOL_VERSION,
            })),
            "version" => {
                let daemon = self.call("version", Value::Null)?;
                Ok(json!({
                    "kget": env!("CARGO_PKG_VERSION"),
                    "protocol": NATIVE_PROTOCOL_VERSION,
                    "daemon": daemon,
                }))
            }
            "download" => {
                let params = submit_params(msg)?;
                self.call(
                    "submit",
                    serde_json::to_value(&params).map_err(RpcError::internal)?,
                )
            }
            "list" => {
                let limit = msg
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(200)
                    .clamp(1, 1000);
                self.call("list", json!({ "limit": limit }))
            }
            "get" | "pause" | "resume" | "cancel" | "remove" => {
                self.call(ty, json!({ "id": job_id(msg)? }))
            }
            "subscribe" => self.subscribe(msg),
            "open" | "reveal" => self.open(ty == "reveal", job_id(msg)?),
            "" => Err(RpcError::new(codes::INVALID_REQUEST, "type is required")),
            other => Err(RpcError::new(
                codes::METHOD_NOT_FOUND,
                format!("Unknown message type '{other}'"),
            )),
        }
    }

    /// The backend, connecting (and starting the daemon) on first use.
    fn backend(&self) -> Result<Arc<dyn Backend>, RpcError> {
        let mut slot = self.backend.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(b) = slot.as_ref() {
            return Ok(b.clone());
        }
        let b = (self.connect)()?;
        *slot = Some(b.clone());
        Ok(b)
    }

    /// Call a daemon method, reconnecting once if the daemon went away.
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        match self.backend()?.call(method, params.clone()) {
            Err(e) if is_connection_error(&e) => {
                *self.backend.lock().unwrap_or_else(|e| e.into_inner()) = None;
                self.backend()?.call(method, params)
            }
            other => other,
        }
    }

    fn subscribe(&self, msg: &Value) -> Result<Value, RpcError> {
        if self.subscribed.load(Ordering::SeqCst) {
            return Ok(json!({ "subscribed": true }));
        }
        let params = SubscribeParams {
            ids: msg
                .get("ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default(),
            progress: msg.get("progress").and_then(Value::as_bool).unwrap_or(true),
        };
        let rx = match self.backend()?.subscribe(params.clone()) {
            Err(e) if is_connection_error(&e) => {
                *self.backend.lock().unwrap_or_else(|e| e.into_inner()) = None;
                self.backend()?.subscribe(params)?
            }
            other => other?,
        };
        self.subscribed.store(true, Ordering::SeqCst);
        let out = self.out.clone();
        let flag = self.subscribed.clone();
        std::thread::Builder::new()
            .name("kget-native-host-events".into())
            .spawn(move || {
                for event in rx {
                    if out.send(&json!({"type": "event", "event": event})).is_err() {
                        break;
                    }
                }
                flag.store(false, Ordering::SeqCst);
                let _ = out.send(&json!({"type": "event_stream_closed"}));
            })
            .map_err(RpcError::internal)?;
        Ok(json!({ "subscribed": true }))
    }

    /// Open a finished download, or show it in the file manager.
    fn open(&self, reveal: bool, id: u64) -> Result<Value, RpcError> {
        let job = self.call("get", json!({ "id": id }))?;
        let path = job
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| job.pointer("/job/output").and_then(Value::as_str))
            .map(PathBuf::from)
            .ok_or_else(|| RpcError::new(codes::INVALID_STATE, "This download has no file yet"))?;
        let target = if reveal {
            if path.exists() {
                path.clone()
            } else {
                // Nothing written yet (or a directory output): show the folder.
                path.parent()
                    .filter(|p| p.is_dir())
                    .map(Path::to_path_buf)
                    .ok_or_else(|| RpcError::new(codes::INVALID_STATE, "The file does not exist"))?
            }
        } else {
            if job.get("state").and_then(Value::as_str) != Some("completed") {
                return Err(RpcError::new(
                    codes::INVALID_STATE,
                    "Only completed downloads can be opened",
                ));
            }
            if !path.exists() {
                return Err(RpcError::new(
                    codes::INVALID_STATE,
                    "The file does not exist",
                ));
            }
            if is_executable_path(&path) {
                return Err(RpcError::new(
                    codes::INVALID_STATE,
                    "KGet does not open programs or scripts from the browser; use \"Show in folder\"",
                ));
            }
            path.clone()
        };
        let launched = self.launch && launch_file_manager(&target, reveal && target == path);
        Ok(json!({ "path": target, "launched": launched }))
    }
}

fn error_reply(id: &Value, ty: &str, e: &RpcError) -> Value {
    json!({
        "id": id,
        "type": ty,
        "ok": false,
        "error": {"code": e.code, "message": e.message},
    })
}

fn job_id(msg: &Value) -> Result<u64, RpcError> {
    msg.get("job")
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::invalid_params("job (a job id) is required"))
}

fn is_connection_error(e: &RpcError) -> bool {
    e.code == codes::INTERNAL_ERROR
        && (e.message.starts_with("Cannot reach")
            || e.message.starts_with("Lost the daemon")
            || e.message.starts_with("The daemon closed"))
}

/// Open `path` with its default application, or select it in the file
/// manager (`select`). Returns whether a process was started.
fn launch_file_manager(path: &Path, select: bool) -> bool {
    use std::process::{Command, Stdio};
    let mut cmd;
    if cfg!(target_os = "macos") {
        cmd = Command::new("open");
        if select {
            cmd.arg("-R");
        }
        cmd.arg(path);
    } else if cfg!(windows) {
        cmd = Command::new("explorer");
        if select {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.raw_arg(format!("/select,\"{}\"", path.display()));
            }
        } else {
            cmd.arg(path);
        }
    } else {
        cmd = Command::new("xdg-open");
        let target = if select {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        cmd.arg(target);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
            true
        }
        Err(_) => false,
    }
}

/// `kget native-host <caller…>`: check the caller and serve stdin/stdout.
/// Returns the process exit code.
pub fn run(caller_args: &[String]) -> Result<i32, BoxError> {
    let layout = install::Layout::resolve()?;
    let allowed = install::load_allowlist(&layout);
    if let Err(msg) = check_caller(caller_args, &allowed) {
        eprintln!("kget native-host: {msg}");
        return Ok(2);
    }
    let connect: Connector =
        Box::new(|| DaemonClient::ensure_running().map(|c| Arc::new(c) as Arc<dyn Backend>));
    let host = NativeHost::new(connect, Box::new(std::io::stdout()));
    match host.run(std::io::stdin().lock()) {
        Ok(()) => Ok(0),
        Err(e) => {
            eprintln!("kget native-host: {e}");
            Ok(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::{PathPolicy, Service};
    use crate::jobs::JobManager;

    #[test]
    fn caller_must_be_allowed() {
        let allowed = install::default_origins();
        let chrome = format!(
            "chrome-extension://{}/",
            install::DEFAULT_CHROME_EXTENSION_ID
        );
        assert_eq!(
            check_caller(std::slice::from_ref(&chrome), &allowed).unwrap(),
            chrome
        );
        // Windows Chromium adds --parent-window
        assert!(check_caller(&[chrome.clone(), "--parent-window=0".into()], &allowed).is_ok());
        // Firefox: manifest path, then the add-on ID
        let ff = vec![
            "/x/com.kget.native_host.json".to_string(),
            install::DEFAULT_FIREFOX_EXTENSION_ID.to_string(),
        ];
        assert!(check_caller(&ff, &allowed).is_ok());
        assert!(
            check_caller(
                &["chrome-extension://abcdefghijklmnopabcdefghijklmnop/".into()],
                &allowed
            )
            .is_err()
        );
        assert!(check_caller(&["evil@example.com".into()], &allowed).is_err());
        assert!(check_caller(&[], &allowed).is_err());
    }

    #[test]
    fn credentials_are_stripped_without_opt_in() {
        let msg = json!({
            "url": "https://example.com/f.zip",
            "headers": {"Cookie": "a=1", "Authorization": "Bearer x",
                        "Proxy-Authorization": "y", "X-Test": "1", "Range": "bytes=0-"},
            "cookies": "sid=2",
            "referrer": "https://example.com/page",
        });
        let p = submit_params(&msg).unwrap();
        let names: Vec<&str> = p.headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["X-Test", "Referer"]);

        let mut msg = msg;
        msg["send_cookies"] = json!(true);
        let p = submit_params(&msg).unwrap();
        let get = |n: &str| {
            p.headers
                .iter()
                .find(|(h, _)| h.eq_ignore_ascii_case(n))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("cookie"), Some("sid=2"));
        assert_eq!(get("authorization"), Some("Bearer x"));
        assert_eq!(get("range"), None);
    }

    #[test]
    fn urls_and_filenames_are_validated() {
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "/etc/passwd",
            "data:text/plain,x",
            "",
        ] {
            assert!(submit_params(&json!({ "url": bad })).is_err(), "{bad}");
        }
        assert!(submit_params(&json!({"url": "magnet:?xt=urn:btih:abc"})).is_ok());
        let p = submit_params(&json!({
            "url": "https://example.com/x",
            "filename": "/Users/me/Downloads/../../etc/report.pdf"
        }))
        .unwrap();
        assert_eq!(p.filename.as_deref(), Some("report.pdf"));
        let p =
            submit_params(&json!({"url": "https://example.com/x", "tags": ["web", 3]})).unwrap();
        assert_eq!(p.tags, vec!["web"]);
        let p = submit_params(&json!({"url": "https://example.com/x", "tags": "one"})).unwrap();
        assert_eq!(p.tags, vec!["one"]);
        assert_eq!(
            sanitize_filename(r"C:\Users\me\a.zip").as_deref(),
            Some("a.zip")
        );
        assert_eq!(sanitize_filename(".."), None);
        assert_eq!(sanitize_filename("dir/"), None);
        assert_eq!(sanitize_filename("a\u{0}b.txt").as_deref(), Some("ab.txt"));
        assert!(is_executable_path(Path::new("/d/setup.EXE")));
        assert!(!is_executable_path(Path::new("/d/movie.mkv")));
    }

    fn in_process() -> (NativeHost, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), false).unwrap();
        let m = JobManager::new(crate::Config::default());
        m.set_record_history(false);
        let service = Service::new(m, policy);
        let connect: Connector =
            Box::new(move || Ok(Arc::new(service.clone()) as Arc<dyn Backend>));
        (NativeHost::new(connect, Box::new(std::io::sink())), dir)
    }

    #[test]
    fn requests_get_replies_with_their_id() {
        let (host, _dir) = in_process();
        let r = host.handle(&json!({"id": "a", "type": "ping"}));
        assert_eq!(r["id"], "a");
        assert_eq!(r["ok"], true);
        assert_eq!(r["result"]["protocol"], NATIVE_PROTOCOL_VERSION);
        let r = host.handle(&json!({"id": 2, "type": "nope"}));
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"]["code"], codes::METHOD_NOT_FOUND);
        let r = host.handle(&json!({"id": 3, "type": "pause"}));
        assert_eq!(r["error"]["code"], codes::INVALID_PARAMS);
        let r = host.handle(&json!({"id": 4, "type": "download", "url": "file:///x"}));
        assert_eq!(r["error"]["code"], codes::INVALID_PARAMS);
    }
}
