//! The daemon's methods, independent of the transport.
//!
//! [`Service`] wraps a [`JobManager`] with input validation, path policy,
//! redaction and (optionally) the persistent queue. The daemon serves it over
//! its socket; `kget mcp` uses it directly when no daemon is running. Both
//! implement [`Backend`], so frontends do not care which one they talk to.

use super::paths::PathPolicy;
use super::protocol::{
    self, HistoryParams, IdParams, ListParams, RpcError, SubmitParams, SubscribeParams, WaitParams,
    codes, parse_params, redact_info,
};
use super::store::JobStore;
use super::{Logger, PROTOCOL_VERSION};
use crate::jobs::{DownloadState, JobEvent, JobId, JobManager};
use crate::protocol::Protocol;
use crate::queue::{DownloadHistory, redact_url};
use crate::runner::{self, DownloadJob};
use crate::utils;
use crate::ytdlp::VideoQuality;
use crate::{OverwritePolicy, events::JsonEvent};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Longest URL accepted by `submit`.
const MAX_URL_LEN: usize = 8192;
/// Most extra headers per job.
const MAX_HEADERS: usize = 64;

/// Something that answers daemon methods: the in-process [`Service`] or a
/// [`DaemonClient`](super::client::DaemonClient).
pub trait Backend: Send + Sync {
    /// Call `method` (see [`protocol`](super::protocol)) with JSON `params`.
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError>;

    /// Receive [`JobEvent`]s (as JSON) matching `params` until the receiver
    /// is dropped.
    fn subscribe(&self, params: SubscribeParams) -> Result<Receiver<Value>, RpcError>;

    /// `"daemon"` or `"in-process"`.
    fn mode(&self) -> &'static str;
}

/// Daemon methods over a [`JobManager`]. Cheap to clone.
#[derive(Clone)]
pub struct Service {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    manager: JobManager,
    policy: PathPolicy,
    store: Option<Arc<Mutex<JobStore>>>,
    logger: Arc<Logger>,
    started_at: u64,
    transport: Mutex<String>,
    shutdown: Arc<AtomicBool>,
    /// Serializes `submit` so two jobs never reserve the same file name
    submit_lock: Mutex<()>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service")
            .field("manager", &self.inner.manager)
            .field("policy", &self.inner.policy)
            .finish()
    }
}

impl Service {
    /// A service over `manager`, writing only where `policy` allows.
    pub fn new(manager: JobManager, policy: PathPolicy) -> Self {
        Self::build(manager, policy, None, Arc::new(Logger::disabled()))
    }

    /// Like [`new`](Self::new), saving jobs in `store` and logging to `logger`.
    pub fn with_store(
        manager: JobManager,
        policy: PathPolicy,
        store: JobStore,
        logger: Arc<Logger>,
    ) -> Self {
        Self::build(manager, policy, Some(Arc::new(Mutex::new(store))), logger)
    }

    fn build(
        manager: JobManager,
        policy: PathPolicy,
        store: Option<Arc<Mutex<JobStore>>>,
        logger: Arc<Logger>,
    ) -> Self {
        Self {
            inner: Arc::new(ServiceInner {
                manager,
                policy,
                store,
                logger,
                started_at: super::unix_now(),
                transport: Mutex::new("in-process".into()),
                shutdown: Arc::new(AtomicBool::new(false)),
                submit_lock: Mutex::new(()),
            }),
        }
    }

    /// The job manager.
    pub fn manager(&self) -> &JobManager {
        &self.inner.manager
    }

    /// Where clients may write.
    pub fn policy(&self) -> &PathPolicy {
        &self.inner.policy
    }

    /// Set to `true` by the `shutdown` method.
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        self.inner.shutdown.clone()
    }

    /// Transport name reported by `version`.
    pub fn set_transport(&self, name: &str) {
        *self
            .inner
            .transport
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = name.to_string();
    }

    pub(crate) fn logger(&self) -> &Logger {
        &self.inner.logger
    }

    /// Restore the jobs saved in the store into the manager. Returns how
    /// many were restored.
    pub fn restore_saved(&self) -> usize {
        let Some(store) = &self.inner.store else {
            return 0;
        };
        let saved = {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let _ = store.prune_finished(super::store::MAX_FINISHED);
            store.load_all().unwrap_or_default()
        };
        let mut n = 0;
        for info in saved {
            let id = info.id;
            let state = info.state;
            if self.inner.manager.restore(info) {
                n += 1;
                self.logger()
                    .info(&format!("restored job {id} (was {state})"));
            }
        }
        n
    }

    /// Mirror every state change into the store until the handle is stopped.
    pub fn start_persistence(&self) -> Option<PersistHandle> {
        let store = self.inner.store.clone()?;
        let rx = self.inner.manager.subscribe();
        let stop = Arc::new(AtomicBool::new(false));
        let manager = self.inner.manager.clone();
        let logger = self.inner.logger.clone();
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("kget-daemon-persist".into())
            .spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    let event = match rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(ev) => ev,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    let store = store.lock().unwrap_or_else(|e| e.into_inner());
                    let result = match &event {
                        JobEvent::Removed { id } => store.delete(*id),
                        JobEvent::State { id, .. }
                        | JobEvent::Download {
                            id,
                            event: JsonEvent::Saved { .. },
                        } => match manager.get(*id) {
                            Some(info) => store.save(&info),
                            None => Ok(()),
                        },
                        JobEvent::Download { .. } => Ok(()),
                    };
                    if let Err(e) = result {
                        logger.warn(&format!("cannot save job {}: {e}", event.id()));
                    }
                }
            })
            .ok()?;
        Some(PersistHandle { stop, thread })
    }

    /// Save every job as it should be restored: jobs in `active` (queued or
    /// running before shutdown) that are now paused are saved as queued.
    pub fn save_for_restart(&self, active: &HashSet<JobId>) {
        let Some(store) = &self.inner.store else {
            return;
        };
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        for mut info in self.inner.manager.list() {
            let interrupted = !info.state.is_finished() || info.state == DownloadState::Paused;
            if active.contains(&info.id) && interrupted {
                info.state = DownloadState::Queued;
            }
            if let Err(e) = store.save(&info) {
                self.logger()
                    .warn(&format!("cannot save job {}: {e}", info.id));
            }
        }
    }

    /// Answer one method call.
    pub fn handle(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let m = &self.inner.manager;
        match method {
            "version" => Ok(self.version()),
            "submit" => {
                let params: SubmitParams = parse_params(params)?;
                let job = self.build_job(params)?;
                let url = redact_url(&job.url);
                let id = m.submit(job);
                self.logger().info(&format!("submit job {id}: {url}"));
                self.snapshot(id)
            }
            "list" => {
                let p: ListParams = parse_params(params)?;
                let mut jobs: Vec<Value> = m
                    .list()
                    .iter()
                    .filter(|j| p.states.is_empty() || p.states.contains(&j.state))
                    .map(redact_info)
                    .collect();
                if let Some(limit) = p.limit {
                    let skip = jobs.len().saturating_sub(limit);
                    jobs.drain(..skip);
                }
                Ok(json!({ "jobs": jobs }))
            }
            "get" => {
                let p: IdParams = parse_params_required(params)?;
                self.snapshot(p.id)
            }
            "wait" => {
                let p: WaitParams = parse_params_required(params)?;
                let secs = p.timeout_secs.unwrap_or(60).min(3600);
                if m.get(p.id).is_none() {
                    return Err(crate::jobs::JobError::NotFound(p.id).into());
                }
                let finished = m.wait(p.id, Some(Duration::from_secs(secs))).is_some();
                Ok(json!({ "finished": finished, "job": self.snapshot(p.id)? }))
            }
            "cancel" | "pause" | "resume" => {
                let p: IdParams = parse_params_required(params)?;
                match method {
                    "cancel" => m.cancel(p.id)?,
                    "pause" => m.pause(p.id)?,
                    _ => m.resume(p.id)?,
                }
                self.logger().info(&format!("{method} job {}", p.id));
                self.snapshot(p.id)
            }
            "remove" => {
                let p: IdParams = parse_params_required(params)?;
                let info = m.remove(p.id)?;
                self.logger().info(&format!("remove job {}", p.id));
                Ok(redact_info(&info))
            }
            "history.query" => {
                let p: HistoryParams = parse_params(params)?;
                let query = p
                    .filter()
                    .query()
                    .map_err(|e| RpcError::invalid_params(e.to_string()))?;
                let entries = DownloadHistory::query(&query).map_err(RpcError::internal)?;
                let total = DownloadHistory::count(&query).map_err(RpcError::internal)?;
                Ok(json!({ "entries": entries, "total": total }))
            }
            "config.get" => Ok(self.config_json()),
            "shutdown" => {
                self.logger().info("shutdown requested by a client");
                self.inner.shutdown.store(true, Ordering::SeqCst);
                Ok(json!({ "ok": true }))
            }
            "subscribe" | "unsubscribe" | "auth" => Err(RpcError::new(
                codes::INVALID_REQUEST,
                format!("'{method}' is only available on a daemon connection"),
            )),
            other => Err(RpcError::method_not_found(other)),
        }
    }

    fn snapshot(&self, id: JobId) -> Result<Value, RpcError> {
        self.inner
            .manager
            .get(id)
            .map(|info| redact_info(&info))
            .ok_or_else(|| crate::jobs::JobError::NotFound(id).into())
    }

    fn version(&self) -> Value {
        json!({
            "protocol": PROTOCOL_VERSION,
            "kget": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "started_at": self.inner.started_at,
            "transport": *self.inner.transport.lock().unwrap_or_else(|e| e.into_inner()),
            "download_dir": self.inner.policy.root(),
            "allow_outside_download_dir": self.inner.policy.allows_outside(),
            "methods": protocol::METHODS,
        })
    }

    fn config_json(&self) -> Value {
        let mut config = self.inner.manager.config();
        config.proxy.password = None;
        config.proxy.url = config.proxy.url.as_deref().map(redact_url);
        json!({
            "config": config,
            "download_dir": self.inner.policy.root(),
            "allow_outside_download_dir": self.inner.policy.allows_outside(),
            "max_concurrent": self.inner.manager.max_concurrent(),
        })
    }

    /// Validate `params` and turn them into a job writing inside the policy.
    pub fn build_job(&self, p: SubmitParams) -> Result<DownloadJob, RpcError> {
        let url = p.url.trim().to_string();
        validate_source(&url)?;
        let mut job = DownloadJob::new(url);
        job.protocol = p.protocol;
        job.advanced = p.advanced;
        job.sha256 = p.sha256.map(|s| s.trim().to_ascii_lowercase());
        if let Some(h) = &job.sha256
            && (h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(RpcError::invalid_params(
                "sha256 must be 64 hexadecimal characters",
            ));
        }
        job.checksum = match p.checksum.as_deref() {
            Some(spec) => Some(
                runner::parse_checksum_spec(spec)
                    .map_err(|e| RpcError::invalid_params(e.to_string()))?,
            ),
            None => None,
        };
        if let Some(u) = &p.checksum_url {
            let ok = url::Url::parse(u)
                .map(|u| matches!(u.scheme(), "http" | "https"))
                .unwrap_or(false);
            if !ok {
                return Err(RpcError::invalid_params(
                    "checksum_url must be an http(s) URL",
                ));
            }
        }
        job.checksum_url = p.checksum_url;
        job.headers = validate_headers(p.headers)?;
        if let Some(q) = p.quality.as_deref() {
            job.quality = VideoQuality::parse(q);
        }
        job.resume = p.resume;
        job.retries = p.retries.unwrap_or(job.retries).clamp(1, 20);
        job.timeout = p.timeout.unwrap_or(job.timeout).min(3600);
        job.extract = p.extract;

        let policy = &self.inner.policy;
        let dir = policy.resolve_dir(p.dir.as_deref())?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| RpcError::internal(format!("Cannot create {}: {e}", dir.display())))?;
        let dir_str = dir.to_string_lossy().into_owned();
        let overwrite = p.overwrite.unwrap_or(OverwritePolicy::Rename);
        match job.protocol() {
            Protocol::Http | Protocol::Ftp | Protocol::Sftp | Protocol::WebDav => {
                let name = match p.filename.as_deref() {
                    Some(n) => n.trim().to_string(),
                    None => utils::get_filename_from_url_or_default(&job.url, "download"),
                };
                let _guard = self
                    .inner
                    .submit_lock
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let mut path = policy.resolve_file(Some(&dir_str), &name)?;
                if overwrite == OverwritePolicy::Rename && !job.resume {
                    // Pick the free name now, so the job resumes into the same
                    // file after a restart.
                    let mut taken = self.reserved_paths();
                    if path.exists() || taken.contains(&path) {
                        path = utils::unique_path(&dir, &name, &mut taken);
                    }
                    job.overwrite = OverwritePolicy::Overwrite;
                } else {
                    job.overwrite = overwrite;
                }
                job.output = Some(path.to_string_lossy().into_owned());
            }
            Protocol::Torrent | Protocol::Metalink | Protocol::Video => {
                if p.filename.is_some() {
                    return Err(RpcError::invalid_params(
                        "filename is not supported for torrents, Metalink and video downloads; use dir",
                    ));
                }
                job.overwrite = match overwrite {
                    OverwritePolicy::Rename => OverwritePolicy::Overwrite,
                    other => other,
                };
                let mut out = dir_str;
                if !out.ends_with(std::path::MAIN_SEPARATOR) {
                    out.push(std::path::MAIN_SEPARATOR);
                }
                job.output = Some(out);
            }
        }
        Ok(job)
    }

    /// Output files of jobs that are not finished.
    fn reserved_paths(&self) -> HashSet<PathBuf> {
        self.inner
            .manager
            .list()
            .into_iter()
            .filter(|j| !j.state.is_finished())
            .filter_map(|j| j.job.output.map(PathBuf::from))
            .collect()
    }
}

impl Backend for Service {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.handle(method, params)
    }

    fn subscribe(&self, params: SubscribeParams) -> Result<Receiver<Value>, RpcError> {
        let rx = self.inner.manager.subscribe();
        let (tx, out) = mpsc::channel();
        std::thread::Builder::new()
            .name("kget-subscriber".into())
            .spawn(move || {
                for event in rx {
                    if !event_matches(&event, &params) {
                        continue;
                    }
                    let Ok(v) = serde_json::to_value(&event) else {
                        continue;
                    };
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            })
            .map_err(RpcError::internal)?;
        Ok(out)
    }

    fn mode(&self) -> &'static str {
        "in-process"
    }
}

/// Handle of the persistence thread.
#[derive(Debug)]
pub struct PersistHandle {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl PersistHandle {
    /// Stop mirroring state changes and wait for the thread.
    pub fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.join();
    }
}

/// Whether `event` passes the `subscribe` filter.
pub fn event_matches(event: &JobEvent, params: &SubscribeParams) -> bool {
    if !params.ids.is_empty() && !params.ids.contains(&event.id()) {
        return false;
    }
    if !params.progress
        && let JobEvent::Download { event, .. } = event
    {
        return !matches!(
            event,
            JsonEvent::Progress { .. } | JsonEvent::FileProgress { .. }
        );
    }
    true
}

fn parse_params_required<T: for<'de> serde::Deserialize<'de>>(
    params: Value,
) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))
}

/// Accept URLs with a supported scheme and absolute paths of existing local
/// `.torrent` / Metalink files.
fn validate_source(url: &str) -> Result<(), RpcError> {
    if url.is_empty() {
        return Err(RpcError::invalid_params("url is required"));
    }
    if url.len() > MAX_URL_LEN {
        return Err(RpcError::invalid_params("url is too long"));
    }
    if url.chars().any(char::is_control) {
        return Err(RpcError::invalid_params("url contains control characters"));
    }
    if let Ok(u) = url::Url::parse(url)
        && u.scheme().len() > 1
    {
        return match u.scheme() {
            "http" | "https" | "ftp" | "ftps" | "sftp" | "webdav" | "webdavs" | "magnet" => Ok(()),
            other => Err(RpcError::invalid_params(format!(
                "unsupported URL scheme '{other}'"
            ))),
        };
    }
    let path = Path::new(url);
    let lower = url.to_ascii_lowercase();
    let manifest = [".torrent", ".meta4", ".metalink"]
        .iter()
        .any(|ext| lower.ends_with(ext));
    if path.is_absolute() && manifest && path.is_file() {
        return Ok(());
    }
    Err(RpcError::invalid_params(
        "url must be an http(s), ftp(s), sftp, webdav(s) or magnet URL, or the absolute path of a local .torrent/.meta4 file",
    ))
}

fn validate_headers(headers: Vec<(String, String)>) -> Result<Vec<(String, String)>, RpcError> {
    if headers.len() > MAX_HEADERS {
        return Err(RpcError::invalid_params(format!(
            "at most {MAX_HEADERS} headers"
        )));
    }
    for (name, value) in &headers {
        let name_ok = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
        if !name_ok {
            return Err(RpcError::invalid_params(format!(
                "invalid header name '{name}'"
            )));
        }
        if value.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
            return Err(RpcError::invalid_params(format!(
                "header '{name}' contains a line break"
            )));
        }
    }
    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> (Service, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), false).unwrap();
        let m = JobManager::new(crate::Config::default());
        m.set_record_history(false);
        (Service::new(m, policy), dir)
    }

    #[test]
    fn build_job_places_files_inside_the_root() {
        let (s, dir) = service();
        let root = dir.path().canonicalize().unwrap();
        let job = s
            .build_job(SubmitParams::new("https://example.com/a/file.iso"))
            .unwrap();
        assert_eq!(
            job.output.as_deref(),
            Some(root.join("file.iso").to_str().unwrap())
        );
        std::fs::write(root.join("file.iso"), b"x").unwrap();
        let job = s
            .build_job(SubmitParams::new("https://example.com/a/file.iso"))
            .unwrap();
        assert_eq!(
            job.output.as_deref(),
            Some(root.join("file (1).iso").to_str().unwrap())
        );
        let mut p = SubmitParams::new("magnet:?xt=urn:btih:abc");
        p.dir = Some("t".into());
        let job = s.build_job(p).unwrap();
        assert!(job.output.unwrap().ends_with(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn build_job_rejects_bad_input() {
        let (s, _dir) = service();
        let bad = |f: &dyn Fn(&mut SubmitParams)| {
            let mut p = SubmitParams::new("https://example.com/f");
            f(&mut p);
            s.build_job(p).unwrap_err().code
        };
        assert_eq!(
            bad(&|p| p.dir = Some("../x".into())),
            codes::PATH_NOT_ALLOWED
        );
        assert_eq!(bad(&|p| p.dir = Some("/".into())), codes::PATH_NOT_ALLOWED);
        assert_eq!(
            bad(&|p| p.filename = Some("../../etc/passwd".into())),
            codes::PATH_NOT_ALLOWED
        );
        assert_eq!(
            bad(&|p| p.url = "file:///etc/passwd".into()),
            codes::INVALID_PARAMS
        );
        assert_eq!(
            bad(&|p| p.url = "/etc/passwd".into()),
            codes::INVALID_PARAMS
        );
        assert_eq!(
            bad(&|p| p.headers = vec![("X".into(), "a\r\nEvil: 1".into())]),
            codes::INVALID_PARAMS
        );
        assert_eq!(
            bad(&|p| p.sha256 = Some("zz".into())),
            codes::INVALID_PARAMS
        );
    }

    #[test]
    fn unknown_methods_and_ids() {
        let (s, _dir) = service();
        assert_eq!(
            s.handle("nope", Value::Null).unwrap_err().code,
            codes::METHOD_NOT_FOUND
        );
        assert_eq!(
            s.handle("get", json!({"id": 9})).unwrap_err().code,
            codes::JOB_NOT_FOUND
        );
        assert_eq!(
            s.handle("get", Value::Null).unwrap_err().code,
            codes::INVALID_PARAMS
        );
        let v = s.handle("version", Value::Null).unwrap();
        assert_eq!(v["protocol"], PROTOCOL_VERSION);
    }
}
