//! Where the TUI gets jobs and events from, and where it sends commands.
//!
//! The UI only talks to a [`JobSource`]. [`LocalSource`] runs downloads in
//! this process on a [`JobManager`]; a client of `kget daemon` only needs
//! another implementation of the trait (submit/pause/… as requests, events
//! from the daemon's stream) and a constructor in [`super::run`].

use kget::jobs::{JobEvent, JobId, JobInfo, JobManager};
use kget::queue::{DownloadHistory, HistoryEntry, HistoryQuery};
use kget::runner::DownloadJob;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// Commands and events of a set of download jobs.
pub trait JobSource {
    /// Short description shown in the title bar (`"local"`, a daemon address…).
    fn label(&self) -> String;
    /// Queue a download.
    fn submit(&self, job: DownloadJob) -> Result<JobId, String>;
    /// Pause a queued or running job.
    fn pause(&self, id: JobId) -> Result<(), String>;
    /// Queue a paused, cancelled or failed job again.
    fn resume(&self, id: JobId) -> Result<(), String>;
    /// Cancel a queued or running job.
    fn cancel(&self, id: JobId) -> Result<(), String>;
    /// Forget a finished job.
    fn remove(&self, id: JobId) -> Result<(), String>;
    /// Snapshots of every job (used at start-up and when an event mentions
    /// a job the UI does not know yet).
    fn snapshot(&self) -> Vec<JobInfo>;
    /// Events received since the last call (never blocks).
    fn poll_events(&mut self) -> Vec<JobEvent>;
    /// Persistent download history.
    fn history(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>, String>;
    /// Stop active jobs before the UI exits (partial data is kept).
    fn shutdown(&mut self);
}

/// Jobs run in this process by a [`JobManager`].
pub struct LocalSource {
    manager: JobManager,
    events: Receiver<JobEvent>,
}

impl LocalSource {
    pub fn new(manager: JobManager) -> Self {
        let events = manager.subscribe();
        Self { manager, events }
    }
}

impl JobSource for LocalSource {
    fn label(&self) -> String {
        format!("local, {} at a time", self.manager.max_concurrent())
    }

    fn submit(&self, job: DownloadJob) -> Result<JobId, String> {
        Ok(self.manager.submit(job))
    }

    fn pause(&self, id: JobId) -> Result<(), String> {
        self.manager.pause(id).map_err(|e| e.to_string())
    }

    fn resume(&self, id: JobId) -> Result<(), String> {
        self.manager.resume(id).map_err(|e| e.to_string())
    }

    fn cancel(&self, id: JobId) -> Result<(), String> {
        self.manager.cancel(id).map_err(|e| e.to_string())
    }

    fn remove(&self, id: JobId) -> Result<(), String> {
        self.manager
            .remove(id)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn snapshot(&self) -> Vec<JobInfo> {
        self.manager.list()
    }

    fn poll_events(&mut self) -> Vec<JobEvent> {
        self.events.try_iter().collect()
    }

    fn history(&self, query: &HistoryQuery) -> Result<Vec<HistoryEntry>, String> {
        DownloadHistory::query(query).map_err(|e| e.to_string())
    }

    fn shutdown(&mut self) {
        for info in self.manager.list() {
            if !info.state.is_finished() {
                let _ = self.manager.pause(info.id);
            }
        }
        // Give the workers a moment to flush their partial data.
        self.manager.wait_idle(Some(Duration::from_secs(5)));
    }
}
