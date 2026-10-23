//! UniFFI bindings over the KGet job engine, for Swift (iOS/iPadOS/macOS)
//! and Kotlin shells.
//!
//! The exported surface is deliberately small: a [`KgetEngine`] object that
//! wraps [`kget::JobManager`], a [`KgetEventListener`] callback for job
//! events, and plain records/enums (in [`types`]) mapped from the core types
//! so the core can evolve without breaking the generated bindings.
//!
//! Bindings are generated with `scripts/gen-bindings.sh` (library mode, from
//! the compiled `cdylib`).
//!
//! UniFFI stops a panic at the FFI boundary, but the generated bindings turn
//! one in a non-throwing method into a crash of the host app, so nothing here
//! may panic on bad input. The event thread catches panics from listeners.

// Embedded in apps: a panic here takes the host app down. Same lint policy
// as the core library.
#![cfg_attr(
    not(test),
    warn(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

use kget::config::{self, Config};
use kget::jobs::JobManager;
use kget::queue::{DownloadHistory, HistoryStore};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub mod types;

pub use types::{
    DownloadProtocol, DownloadRequest, EngineError, HistoryFilter, HistoryItem, HistoryOrder,
    HistoryStatus, HttpHeader, JobEventRecord, JobPhase, JobProgressRecord, JobSnapshot, JobState,
    OverwriteMode, TorrentFile, TorrentFileProgress,
};

uniffi::setup_scaffolding!();

/// How often the event thread checks whether its engine was dropped.
const EVENT_POLL: Duration = Duration::from_millis(200);

/// Receives job events. Implemented by the foreign side (Swift/Kotlin).
///
/// Called on a background thread owned by the engine, one event at a time
/// and in order. Hop to the main thread before touching UI state, and return
/// quickly: slow listeners delay later events (they are never dropped).
#[uniffi::export(foreign)]
pub trait KgetEventListener: Send + Sync {
    fn on_event(&self, event: JobEventRecord);
}

type ListenerSlot = Arc<Mutex<Option<Arc<dyn KgetEventListener>>>>;

/// Version of the KGet engine linked into the bindings.
#[uniffi::export]
pub fn kget_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Download engine: a queue of jobs with bounded concurrency.
///
/// Create one per process and keep it alive for the app's lifetime;
/// dropping it cancels every queued and running job (partial data is kept).
#[derive(uniffi::Object)]
pub struct KgetEngine {
    manager: JobManager,
    listener: ListenerSlot,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for KgetEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KgetEngine")
            .field("manager", &self.manager)
            .finish_non_exhaustive()
    }
}

#[uniffi::export]
impl KgetEngine {
    /// Create an engine.
    ///
    /// `config_dir` holds `config.json` and `history.db` (created if
    /// missing). Sandboxed apps should pass a directory inside their
    /// container; iOS has no default, so with `None` there it runs with
    /// default settings and does not record history. The directory applies
    /// to the whole process (the last engine created wins).
    #[uniffi::constructor]
    pub fn new(config_dir: Option<String>) -> Result<Arc<Self>, EngineError> {
        if let Some(dir) = config_dir.filter(|d| !d.trim().is_empty()) {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).map_err(|e| EngineError::Config {
                message: format!("{}: {e}", dir.display()),
            })?;
            config::set_config_dir_override(Some(dir));
        }
        let (config, record_history) = match config::config_dir() {
            Ok(_) => (
                Config::load().map_err(|e| EngineError::Config {
                    message: e.to_string(),
                })?,
                true,
            ),
            Err(_) => (Config::default(), false),
        };

        let manager = JobManager::new(config);
        manager.set_record_history(record_history);
        let engine = Arc::new(Self {
            listener: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            manager,
        });
        engine.spawn_event_thread()?;
        Ok(engine)
    }

    /// Queue a download and return its job id.
    pub fn submit(&self, request: DownloadRequest) -> Result<u64, EngineError> {
        Ok(self.manager.submit(request.into_job()?))
    }

    /// Cancel a queued or running job (partial data is kept).
    pub fn cancel(&self, id: u64) -> Result<(), EngineError> {
        Ok(self.manager.cancel(id)?)
    }

    /// Pause a queued or running job (partial data is kept).
    pub fn pause(&self, id: u64) -> Result<(), EngineError> {
        Ok(self.manager.pause(id)?)
    }

    /// Queue a paused, cancelled or failed job again; it continues from its
    /// partial data where the protocol supports it.
    pub fn resume(&self, id: u64) -> Result<(), EngineError> {
        Ok(self.manager.resume(id)?)
    }

    /// Forget a finished job (paused, cancelled, completed or failed).
    pub fn remove(&self, id: u64) -> Result<(), EngineError> {
        self.manager.remove(id)?;
        Ok(())
    }

    /// Cancel every queued and running job.
    pub fn cancel_all(&self) {
        self.manager.cancel_all();
    }

    /// Snapshots of every job, oldest first.
    pub fn list(&self) -> Vec<JobSnapshot> {
        self.manager.list().into_iter().map(Into::into).collect()
    }

    /// Snapshot of one job.
    pub fn get(&self, id: u64) -> Option<JobSnapshot> {
        self.manager.get(id).map(Into::into)
    }

    /// Maximum number of jobs running at the same time.
    pub fn max_concurrent(&self) -> u32 {
        self.manager.max_concurrent().try_into().unwrap_or(u32::MAX)
    }

    /// Change the concurrency limit (at least 1). Raising it starts queued
    /// jobs immediately; lowering it never stops running jobs.
    pub fn set_max_concurrent(&self, max: u32) {
        self.manager.set_max_concurrent(max as usize);
    }

    /// Number of jobs currently running.
    pub fn running(&self) -> u32 {
        self.manager.running().try_into().unwrap_or(u32::MAX)
    }

    /// Receive events of every job from now on, replacing any previous
    /// listener.
    pub fn set_listener(&self, listener: Arc<dyn KgetEventListener>) {
        *lock(&self.listener) = Some(listener);
    }

    /// Stop delivering events.
    pub fn clear_listener(&self) {
        *lock(&self.listener) = None;
    }

    /// Query the persistent download history.
    pub fn history(&self, filter: HistoryFilter) -> Result<Vec<HistoryItem>, EngineError> {
        let history_err = |message: String| EngineError::History { message };
        config::config_dir().map_err(|e| history_err(e.to_string()))?;
        let store = HistoryStore::open(DownloadHistory::default_path())
            .map_err(|e| history_err(e.to_string()))?;
        let entries = store
            .query(&filter.into())
            .map_err(|e| history_err(e.to_string()))?;
        Ok(entries.into_iter().map(Into::into).collect())
    }
}

impl KgetEngine {
    /// Forward [`JobManager`] events to the current listener until the
    /// engine is dropped.
    fn spawn_event_thread(&self) -> Result<(), EngineError> {
        let events = self.manager.subscribe();
        let listener = self.listener.clone();
        let stop = self.stop.clone();
        std::thread::Builder::new()
            .name("kget-ffi-events".into())
            .spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let event = match events.recv_timeout(EVENT_POLL) {
                        Ok(event) => event,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    // Clone out of the lock so the listener may call
                    // `set_listener` / `clear_listener` itself.
                    let current = lock(&listener).clone();
                    if let Some(l) = current {
                        // A listener that panics (including a foreign callback
                        // failure UniFFI reports as a panic) loses that event
                        // only; this thread keeps forwarding.
                        let event = event.into();
                        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| l.on_event(event)));
                    }
                }
            })
            .map(drop)
            .map_err(|e| EngineError::Config {
                message: format!("Cannot start event thread: {e}"),
            })
    }
}

impl Drop for KgetEngine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.manager.cancel_all();
    }
}

/// Lock `m`, recovering from poisoning: the guarded values (the listener
/// slot) stay valid if a thread panicked while holding them, and one bad
/// callback must not wedge every later engine call. Same policy as the core
/// library.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
