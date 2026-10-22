//! Central job service: a download queue with bounded concurrency.
//!
//! [`JobManager`] owns every download submitted by any client (GUI, daemon,
//! MCP server, TUI…). It assigns a unique [`JobId`] to each job, starts at
//! most [`max_concurrent`](JobManager::max_concurrent) of them at a time and
//! broadcasts [`JobEvent`]s to any number of subscribers, each with its own
//! channel.
//!
//! Jobs run through the same dispatcher as the CLI ([`crate::runner`]), so
//! protocols, resume, checksums, history and extraction behave the same.
//!
//! The manager is cheap to clone (`Arc` inside) and `Send + Sync`, so a
//! daemon can share it across threads.
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::jobs::{DownloadState, JobEvent, JobManager};
//! use kget::runner::DownloadJob;
//!
//! let manager = JobManager::new(kget::Config::default());
//! let events = manager.subscribe();
//!
//! let id = manager.submit(DownloadJob::new("https://example.com/file.iso"));
//! for event in events {
//!     if let JobEvent::State { id: job, state, .. } = event
//!         && job == id
//!         && state.is_finished()
//!     {
//!         println!("{job} finished: {state}");
//!         break;
//!     }
//! }
//! ```

use crate::config::Config;
use crate::events::{JsonEvent, Phase};
use crate::protocol::Protocol;
use crate::runner::{self, DownloadJob, Outcome, Reporter, RunContext};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Identifier of one download job, unique within a [`JobManager`].
pub type JobId = u64;

/// Concurrency used by [`JobManager::new`].
pub const DEFAULT_MAX_CONCURRENT: usize = 3;

// ============================================================================
// Public types
// ============================================================================

/// Lifecycle state of a job.
///
/// ```text
/// Queued ──> Running ──> (Verifying) ──> Completed
///   │           │                    └─> Failed
///   │           ├──> Paused ──(resume)──> Queued
///   └───────────┴──> Cancelled ─(resume)─> Queued
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    /// Waiting for a free slot
    Queued,
    /// Transferring data
    Running,
    /// Checking the checksum of the downloaded file
    Verifying,
    /// Stopped by [`JobManager::pause`]; partial data is kept
    Paused,
    /// Stopped by [`JobManager::cancel`]; partial data is kept
    Cancelled,
    /// Finished successfully
    Completed,
    /// Finished with an error
    Failed,
}

impl DownloadState {
    /// Lowercase name (`"queued"`, `"running"`, …).
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadState::Queued => "queued",
            DownloadState::Running => "running",
            DownloadState::Verifying => "verifying",
            DownloadState::Paused => "paused",
            DownloadState::Cancelled => "cancelled",
            DownloadState::Completed => "completed",
            DownloadState::Failed => "failed",
        }
    }

    /// `Running` or `Verifying`.
    pub fn is_active(&self) -> bool {
        matches!(self, DownloadState::Running | DownloadState::Verifying)
    }

    /// The job is not queued or running (it can be resumed or removed).
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            DownloadState::Paused
                | DownloadState::Cancelled
                | DownloadState::Completed
                | DownloadState::Failed
        )
    }
}

impl fmt::Display for DownloadState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

/// Latest progress of a job.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JobProgress {
    /// Bytes downloaded
    pub bytes: u64,
    /// Total size, when known
    pub total: Option<u64>,
    /// 0.0 – 100.0, when known
    pub percent: Option<f64>,
    /// Current speed in bytes per second
    pub speed_bps: u64,
    /// Estimated seconds remaining
    pub eta_secs: Option<u64>,
}

/// Snapshot of a job, returned by [`JobManager::get`] and [`JobManager::list`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobInfo {
    /// Id assigned by the manager
    pub id: JobId,
    /// What to download
    pub job: DownloadJob,
    /// Protocol used (forced or detected)
    pub protocol: Protocol,
    /// Current state
    pub state: DownloadState,
    /// Unix timestamp (seconds) of [`JobManager::submit`]
    pub created_at: u64,
    /// Unix timestamp (seconds) of the latest start
    pub started_at: Option<u64>,
    /// Unix timestamp (seconds) of the latest finish
    pub finished_at: Option<u64>,
    /// Latest progress
    pub progress: JobProgress,
    /// Where the data was saved (single-file downloads)
    pub path: Option<String>,
    /// Error message of a failed job
    pub error: Option<String>,
    /// The data was handed to an external application (system torrent client)
    pub handed_off: bool,
}

/// Event broadcast to every subscriber of a [`JobManager`].
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    /// A job changed state (including `Queued` when it is submitted or
    /// resumed). `error` is set for `Failed`.
    State {
        id: JobId,
        state: DownloadState,
        error: Option<String>,
    },
    /// A download event — the same objects `kget --jsonl` prints
    /// (`started`, `progress`, `status`, `verified`, `saved`, `error`…).
    Download { id: JobId, event: JsonEvent },
    /// A finished job was removed with [`JobManager::remove`].
    Removed { id: JobId },
}

impl JobEvent {
    /// The job this event belongs to.
    pub fn id(&self) -> JobId {
        match self {
            JobEvent::State { id, .. }
            | JobEvent::Download { id, .. }
            | JobEvent::Removed { id } => *id,
        }
    }
}

/// Error returned by [`JobManager`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    /// No job with this id
    NotFound(JobId),
    /// The operation is not allowed in the job's current state
    InvalidState {
        id: JobId,
        state: DownloadState,
        action: &'static str,
    },
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::NotFound(id) => write!(f, "No job with id {id}"),
            JobError::InvalidState { id, state, action } => {
                write!(f, "Cannot {action} job {id}: it is {state}")
            }
        }
    }
}

impl std::error::Error for JobError {}

// ============================================================================
// Manager
// ============================================================================

/// Download queue with bounded concurrency and event fan-out.
///
/// See the [module documentation](self) for an example.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let st = self.inner.lock();
        f.debug_struct("JobManager")
            .field("jobs", &st.jobs.len())
            .field("queued", &st.queue.len())
            .field("running", &st.running)
            .field("max_concurrent", &st.max_concurrent)
            .finish()
    }
}

struct Inner {
    state: Mutex<State>,
    changed: Condvar,
    subscribers: Mutex<Vec<Sender<JobEvent>>>,
    next_id: AtomicU64,
}

struct State {
    config: Config,
    record_history: bool,
    max_concurrent: usize,
    running: usize,
    queue: VecDeque<JobId>,
    jobs: BTreeMap<JobId, Entry>,
}

struct Entry {
    info: JobInfo,
    cancel: Arc<AtomicBool>,
    /// Why the running job was asked to stop
    stop: Option<DownloadState>,
}

impl JobManager {
    /// A manager running up to [`DEFAULT_MAX_CONCURRENT`] jobs at a time.
    pub fn new(config: Config) -> Self {
        Self::with_concurrency(config, DEFAULT_MAX_CONCURRENT)
    }

    /// A manager running up to `max_concurrent` jobs at a time (at least 1).
    pub fn with_concurrency(config: Config, max_concurrent: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    config,
                    record_history: true,
                    max_concurrent: max_concurrent.max(1),
                    running: 0,
                    queue: VecDeque::new(),
                    jobs: BTreeMap::new(),
                }),
                changed: Condvar::new(),
                subscribers: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    /// Record finished jobs in the persistent download history (default
    /// `true`).
    pub fn set_record_history(&self, record: bool) {
        self.inner.lock().record_history = record;
    }

    /// Maximum number of jobs running at the same time.
    pub fn max_concurrent(&self) -> usize {
        self.inner.lock().max_concurrent
    }

    /// Change the concurrency limit (at least 1). Raising it starts queued
    /// jobs immediately; lowering it never stops running jobs.
    pub fn set_max_concurrent(&self, max_concurrent: usize) {
        self.inner.lock().max_concurrent = max_concurrent.max(1);
        self.inner.schedule();
    }

    /// Configuration used for jobs started from now on.
    pub fn config(&self) -> Config {
        self.inner.lock().config.clone()
    }

    /// Replace the configuration for jobs started from now on.
    pub fn set_config(&self, config: Config) {
        self.inner.lock().config = config;
    }

    /// Subscribe to events of every job. Each call returns a new channel;
    /// dropping the receiver unsubscribes.
    pub fn subscribe(&self) -> Receiver<JobEvent> {
        let (tx, rx) = mpsc::channel();
        self.inner.subscribers_lock().push(tx);
        rx
    }

    /// Queue a job and return its id. It starts as soon as a slot is free.
    pub fn submit(&self, job: DownloadJob) -> JobId {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let info = JobInfo {
            id,
            protocol: job.protocol(),
            job,
            state: DownloadState::Queued,
            created_at: unix_now(),
            started_at: None,
            finished_at: None,
            progress: JobProgress::default(),
            path: None,
            error: None,
            handed_off: false,
        };
        {
            let mut st = self.inner.lock();
            st.jobs.insert(
                id,
                Entry {
                    info,
                    cancel: Arc::new(AtomicBool::new(false)),
                    stop: None,
                },
            );
            st.queue.push_back(id);
            self.inner
                .broadcast(state_event(id, DownloadState::Queued, None));
        }
        self.inner.schedule();
        id
    }

    /// Cancel a queued or running job. Partial data is kept, so the job can
    /// be [resumed](Self::resume) later.
    pub fn cancel(&self, id: JobId) -> Result<(), JobError> {
        self.stop(id, DownloadState::Cancelled, "cancel")
    }

    /// Pause a queued or running job (the transfer stops and its partial
    /// data is kept). [`resume`](Self::resume) continues it.
    pub fn pause(&self, id: JobId) -> Result<(), JobError> {
        self.stop(id, DownloadState::Paused, "pause")
    }

    /// Queue a paused, cancelled or failed job again; it continues from its
    /// partial data where the protocol supports it.
    pub fn resume(&self, id: JobId) -> Result<(), JobError> {
        {
            let mut st = self.inner.lock();
            let entry = st.jobs.get_mut(&id).ok_or(JobError::NotFound(id))?;
            match entry.info.state {
                DownloadState::Paused | DownloadState::Cancelled | DownloadState::Failed => {}
                state => {
                    return Err(JobError::InvalidState {
                        id,
                        state,
                        action: "resume",
                    });
                }
            }
            entry.info.state = DownloadState::Queued;
            entry.info.job.resume = true;
            entry.info.error = None;
            entry.info.finished_at = None;
            entry.cancel = Arc::new(AtomicBool::new(false));
            entry.stop = None;
            st.queue.push_back(id);
            self.inner
                .broadcast(state_event(id, DownloadState::Queued, None));
        }
        self.inner.changed.notify_all();
        self.inner.schedule();
        Ok(())
    }

    /// Forget a finished job (paused, cancelled, completed or failed).
    pub fn remove(&self, id: JobId) -> Result<JobInfo, JobError> {
        let info = {
            let mut st = self.inner.lock();
            let entry = st.jobs.get(&id).ok_or(JobError::NotFound(id))?;
            if !entry.info.state.is_finished() {
                return Err(JobError::InvalidState {
                    id,
                    state: entry.info.state,
                    action: "remove",
                });
            }
            let info = st.jobs.remove(&id).map(|e| e.info);
            self.inner.broadcast(JobEvent::Removed { id });
            info
        }
        .ok_or(JobError::NotFound(id))?;
        Ok(info)
    }

    /// Cancel every queued and running job.
    pub fn cancel_all(&self) {
        let ids: Vec<JobId> = self
            .inner
            .lock()
            .jobs
            .values()
            .filter(|e| !e.info.state.is_finished())
            .map(|e| e.info.id)
            .collect();
        for id in ids {
            let _ = self.cancel(id);
        }
    }

    /// Snapshot of one job.
    pub fn get(&self, id: JobId) -> Option<JobInfo> {
        self.inner.lock().jobs.get(&id).map(|e| e.info.clone())
    }

    /// Snapshots of every job, oldest first.
    pub fn list(&self) -> Vec<JobInfo> {
        self.inner
            .lock()
            .jobs
            .values()
            .map(|e| e.info.clone())
            .collect()
    }

    /// Number of jobs currently running.
    pub fn running(&self) -> usize {
        self.inner.lock().running
    }

    /// Block until the job is finished (or `timeout` elapses) and return its
    /// snapshot. `None` if the job does not exist or the timeout elapsed.
    pub fn wait(&self, id: JobId, timeout: Option<Duration>) -> Option<JobInfo> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut st = self.inner.lock();
        loop {
            let info = st.jobs.get(&id)?.info.clone();
            if info.state.is_finished() {
                return Some(info);
            }
            st = match deadline {
                None => self
                    .inner
                    .changed
                    .wait(st)
                    .unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let left = d.checked_duration_since(Instant::now())?;
                    self.inner
                        .changed
                        .wait_timeout(st, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
            };
        }
    }

    /// Block until no job is queued or running (or `timeout` elapses).
    /// Returns `false` on timeout.
    pub fn wait_idle(&self, timeout: Option<Duration>) -> bool {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut st = self.inner.lock();
        loop {
            if st.running == 0 && st.queue.is_empty() {
                return true;
            }
            st = match deadline {
                None => self
                    .inner
                    .changed
                    .wait(st)
                    .unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let Some(left) = d.checked_duration_since(Instant::now()) else {
                        return false;
                    };
                    self.inner
                        .changed
                        .wait_timeout(st, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0
                }
            };
        }
    }

    fn stop(&self, id: JobId, target: DownloadState, action: &'static str) -> Result<(), JobError> {
        let mut st = self.inner.lock();
        let entry = st.jobs.get_mut(&id).ok_or(JobError::NotFound(id))?;
        match entry.info.state {
            DownloadState::Queued => {
                entry.info.state = target;
                entry.info.finished_at = Some(unix_now());
                st.queue.retain(|q| *q != id);
                self.inner.broadcast(state_event(id, target, None));
                drop(st);
                self.inner.changed.notify_all();
            }
            DownloadState::Running | DownloadState::Verifying => {
                // The worker reports the final state when it returns.
                entry.stop = Some(target);
                entry.cancel.store(true, Ordering::SeqCst);
            }
            state => return Err(JobError::InvalidState { id, state, action }),
        }
        Ok(())
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn subscribers_lock(&self) -> MutexGuard<'_, Vec<Sender<JobEvent>>> {
        self.subscribers.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Send `event` to every subscriber, dropping the ones that went away.
    ///
    /// Called with the state lock held, so subscribers see events in the same
    /// order as the state changes (lock order: state, then subscribers).
    fn broadcast(&self, event: JobEvent) {
        self.subscribers_lock()
            .retain(|tx| tx.send(event.clone()).is_ok());
    }

    /// Start queued jobs while slots are free.
    fn schedule(self: &Arc<Self>) {
        loop {
            let (id, job, ctx) = {
                let mut st = self.lock();
                if st.running >= st.max_concurrent {
                    return;
                }
                let Some(id) = st.queue.pop_front() else {
                    return;
                };
                let config = st.config.clone();
                let record_history = st.record_history;
                let Some(entry) = st.jobs.get_mut(&id) else {
                    continue;
                };
                if entry.info.state != DownloadState::Queued {
                    continue;
                }
                entry.info.state = DownloadState::Running;
                entry.info.started_at = Some(unix_now());
                entry.info.progress = JobProgress::default();
                let mut ctx = RunContext::new(config).with_cancel(entry.cancel.clone());
                ctx.record_history = record_history;
                let job = entry.info.job.clone();
                st.running += 1;
                self.broadcast(state_event(id, DownloadState::Running, None));
                (id, job, ctx)
            };

            let inner = self.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("kget-job-{id}"))
                .spawn(move || inner.run(id, job, ctx));
            if let Err(e) = spawned {
                self.finish(id, Err(format!("Cannot start download thread: {e}").into()));
            }
        }
    }

    /// Worker thread body.
    fn run(self: Arc<Self>, id: JobId, job: DownloadJob, ctx: RunContext) {
        let reporter = Reporter {
            quiet: true,
            events: Some({
                let inner = self.clone();
                Arc::new(move |ev: &JsonEvent| inner.on_event(id, ev))
            }),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runner::run_job(&job, &ctx, &reporter)
        }))
        .unwrap_or_else(|_| Err("Download thread panicked".into()));
        self.finish(id, result);
    }

    fn on_event(&self, id: JobId, ev: &JsonEvent) {
        let mut st = self.lock();
        if let Some(entry) = st.jobs.get_mut(&id) {
            match ev {
                JsonEvent::Progress {
                    bytes,
                    total,
                    percent,
                    speed_bps,
                    eta_secs,
                    ..
                } => {
                    entry.info.progress = JobProgress {
                        bytes: *bytes,
                        total: *total,
                        percent: *percent,
                        speed_bps: *speed_bps,
                        eta_secs: *eta_secs,
                    };
                }
                JsonEvent::Saved { path, bytes } => {
                    entry.info.path = Some(path.clone());
                    entry.info.progress.bytes = *bytes;
                }
                _ => {}
            }
        }
        self.broadcast(JobEvent::Download {
            id,
            event: ev.clone(),
        });
        // The runner announces checksum verification with a `phase` event
        // (both its own `--checksum` pass and the engine's `--sha256` hash).
        let verifying = matches!(
            ev,
            JsonEvent::Phase {
                phase: Phase::Verifying,
                ..
            }
        );
        if verifying
            && let Some(entry) = st.jobs.get_mut(&id)
            && entry.info.state == DownloadState::Running
        {
            entry.info.state = DownloadState::Verifying;
            self.broadcast(state_event(id, DownloadState::Verifying, None));
            drop(st);
            self.changed.notify_all();
        }
    }

    /// Record the result of a job, free its slot and start the next one.
    fn finish(
        self: &Arc<Self>,
        id: JobId,
        result: Result<Outcome, Box<dyn std::error::Error + Send + Sync>>,
    ) {
        {
            let mut st = self.lock();
            st.running = st.running.saturating_sub(1);
            if let Some(entry) = st.jobs.get_mut(&id) {
                let (state, error) = match (&result, entry.stop) {
                    // Finished before the stop request took effect.
                    (Ok(outcome), _) => {
                        match outcome {
                            Outcome::Saved(Some(p)) => {
                                entry.info.path = Some(p.to_string_lossy().into_owned())
                            }
                            Outcome::Saved(None) => {}
                            Outcome::HandedOff => entry.info.handed_off = true,
                        }
                        (DownloadState::Completed, None)
                    }
                    (Err(_), Some(stop)) => (stop, None),
                    (Err(e), None)
                        if matches!(
                            e.downcast_ref::<crate::KgetError>(),
                            Some(crate::KgetError::Cancelled)
                        ) =>
                    {
                        (DownloadState::Cancelled, None)
                    }
                    (Err(e), None) => (DownloadState::Failed, Some(e.to_string())),
                };
                entry.info.state = state;
                entry.info.error = error.clone();
                entry.info.finished_at = Some(unix_now());
                entry.stop = None;
                self.broadcast(state_event(id, state, error));
            }
        }
        self.changed.notify_all();
        self.schedule();
    }
}

fn state_event(id: JobId, state: DownloadState, error: Option<String>) -> JobEvent {
    JobEvent::State { id, state, error }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn manager_is_send_sync() {
        assert_send_sync::<JobManager>();
        assert_send_sync::<JobEvent>();
    }

    #[test]
    fn states_classify() {
        assert!(DownloadState::Verifying.is_active());
        assert!(!DownloadState::Queued.is_finished());
        assert!(DownloadState::Paused.is_finished());
        assert_eq!(
            serde_json::to_string(&DownloadState::Completed).unwrap(),
            "\"completed\""
        );
    }

    #[test]
    fn unknown_ids_are_errors() {
        let m = JobManager::new(Config::default());
        assert_eq!(m.cancel(42), Err(JobError::NotFound(42)));
        assert_eq!(m.resume(42), Err(JobError::NotFound(42)));
        assert!(m.get(42).is_none());
        assert!(m.list().is_empty());
    }
}
