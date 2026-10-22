//! Application-facing orchestration shared by GUI frontends.
//!
//! This module keeps UI worker plumbing out of `main.rs` so the binary can stay
//! thin and future frontends can reuse the same command/message contract.
//!
//! Every download is identified by a [`JobId`] chosen by the frontend, so
//! several downloads can run at the same time: messages carry the id of the
//! job they belong to and [`DownloadCommand::Cancel`] stops only that job.
//!
//! The worker is a thin adapter over [`JobManager`]: commands become
//! [`JobManager::submit`] / [`JobManager::cancel`] calls and [`JobEvent`]s are
//! translated back into [`WorkerToGuiMessage`]s. New frontends should use the
//! manager directly.

use crate::config::Config;
use crate::events::JsonEvent;
use crate::jobs::{DownloadState, JobEvent, JobManager};
use crate::protocol::{self, Protocol};
use crate::runner::DownloadJob;
use std::collections::HashMap;
use std::fmt;
use std::sync::mpsc::{Receiver as MpscReceiver, RecvTimeoutError, Sender as MpscSender};
use std::thread;
use std::time::Duration;

/// Identifier of one download job. In [`DownloadCommand`] and
/// [`WorkerToGuiMessage`] it is chosen by the frontend; a [`JobManager`]
/// assigns its own.
pub use crate::jobs::JobId;

/// Command sent from a frontend to the download worker.
#[derive(Debug, Clone)]
pub enum DownloadCommand {
    /// Start a new job.
    Start {
        id: JobId,
        url: String,
        output_path: String,
        is_advanced: bool,
        verify_iso: bool,
        expected_sha256: Option<String>,
    },
    /// Cancel one running job (its partial data is kept for resume).
    Cancel { id: JobId },
}

/// Message sent from the download worker back to a frontend.
#[derive(Debug, Clone)]
pub enum WorkerToGuiMessage {
    /// Progress update (about five per second).
    Progress {
        id: JobId,
        /// 0.0 – 1.0 (0.0 when the size is unknown)
        fraction: f32,
        bytes: u64,
        total: Option<u64>,
        speed_bps: u64,
        eta_secs: Option<u64>,
    },
    /// Human-readable status message.
    StatusUpdate { id: JobId, text: String },
    /// The job finished; `path` is where the data was saved.
    Completed { id: JobId, path: String },
    /// The job failed.
    Error { id: JobId, message: String },
    /// The job was cancelled.
    Cancelled { id: JobId },
}

impl WorkerToGuiMessage {
    /// The job this message belongs to.
    pub fn id(&self) -> JobId {
        match self {
            WorkerToGuiMessage::Progress { id, .. }
            | WorkerToGuiMessage::StatusUpdate { id, .. }
            | WorkerToGuiMessage::Completed { id, .. }
            | WorkerToGuiMessage::Error { id, .. }
            | WorkerToGuiMessage::Cancelled { id } => *id,
        }
    }
}

impl fmt::Display for WorkerToGuiMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerToGuiMessage::Progress { id, fraction, .. } => {
                write!(f, "[{id}] Progress({fraction:.2})")
            }
            WorkerToGuiMessage::StatusUpdate { id, text } => write!(f, "[{id}] Status: {text}"),
            WorkerToGuiMessage::Completed { id, path } => write!(f, "[{id}] Completed: {path}"),
            WorkerToGuiMessage::Error { id, message } => write!(f, "[{id}] Error: {message}"),
            WorkerToGuiMessage::Cancelled { id } => write!(f, "[{id}] Cancelled"),
        }
    }
}

/// Spawn the blocking download worker used by the desktop GUI.
///
/// Every started job runs immediately (no concurrency limit), as before the
/// job manager existed.
pub fn spawn_download_worker(
    config: Config,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
) -> thread::JoinHandle<()> {
    let manager = JobManager::with_concurrency(config, usize::MAX);
    spawn_download_worker_with_manager(manager, download_rx, status_tx)
}

/// Like [`spawn_download_worker`], but runs the jobs on an existing
/// [`JobManager`] (shared with other clients, with its own concurrency limit).
pub fn spawn_download_worker_with_manager(
    manager: JobManager,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || download_worker(manager, download_rx, status_tx))
}

/// The [`DownloadJob`] for a GUI [`DownloadCommand::Start`].
pub fn gui_job(
    url: &str,
    output_path: &str,
    is_advanced: bool,
    verify_iso: bool,
    expected_sha256: Option<String>,
) -> DownloadJob {
    let mut job = DownloadJob::new(url);
    job.output = Some(match protocol::detect(url) {
        // yt-dlp picks the file name; the GUI passes a file path.
        Protocol::Video => std::path::Path::new(output_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| ".".into()),
        _ => output_path.to_string(),
    });
    job.advanced = is_advanced;
    job.verify_iso = verify_iso;
    job.sha256 = expected_sha256;
    // Continue a `.part` left by a cancelled attempt.
    job.resume = true;
    job
}

/// A GUI job tracked by the worker.
struct Tracked {
    gui_id: JobId,
    output_path: String,
    saved: Option<String>,
}

fn download_worker(
    manager: JobManager,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
) {
    // Subscribe before submitting anything so no event is missed.
    let events = manager.subscribe();
    let mut jobs: HashMap<JobId, Tracked> = HashMap::new();
    let mut by_gui: HashMap<JobId, JobId> = HashMap::new();

    loop {
        match download_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(DownloadCommand::Start {
                id,
                url,
                output_path,
                is_advanced,
                verify_iso,
                expected_sha256,
            }) => {
                let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate {
                    id,
                    text: format!("Initializing: {url}"),
                });
                let job = gui_job(&url, &output_path, is_advanced, verify_iso, expected_sha256);
                let job_id = manager.submit(job);
                jobs.insert(
                    job_id,
                    Tracked {
                        gui_id: id,
                        output_path,
                        saved: None,
                    },
                );
                by_gui.insert(id, job_id);
            }
            Ok(DownloadCommand::Cancel { id }) => {
                if let Some(job_id) = by_gui.get(&id)
                    && manager.cancel(*job_id).is_ok()
                {
                    let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate {
                        id,
                        text: "Cancelling download...".into(),
                    });
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                for job_id in jobs.keys() {
                    let _ = manager.cancel(*job_id);
                }
                break;
            }
        }

        while let Ok(event) = events.try_recv() {
            let job_id = event.id();
            let Some(tracked) = jobs.get_mut(&job_id) else {
                continue;
            };
            let (message, finished) = translate(&manager, tracked, event);
            if let Some(m) = message {
                let _ = status_tx.send(m);
            }
            if finished {
                if let Some(t) = jobs.remove(&job_id) {
                    by_gui.remove(&t.gui_id);
                }
                let _ = manager.remove(job_id);
            }
        }
    }
}

/// Translate one manager event into a GUI message; the flag is `true` when
/// the job is over.
fn translate(
    manager: &JobManager,
    tracked: &mut Tracked,
    event: JobEvent,
) -> (Option<WorkerToGuiMessage>, bool) {
    let id = tracked.gui_id;
    match event {
        JobEvent::Download { event, .. } => {
            let message = match event {
                JsonEvent::Progress {
                    bytes,
                    total,
                    percent,
                    speed_bps,
                    eta_secs,
                    progress,
                } => Some(WorkerToGuiMessage::Progress {
                    id,
                    fraction: percent.map(|p| p / 100.0).or(progress).unwrap_or(0.0) as f32,
                    bytes,
                    total,
                    speed_bps,
                    eta_secs,
                }),
                JsonEvent::Status { message } => {
                    Some(WorkerToGuiMessage::StatusUpdate { id, text: message })
                }
                JsonEvent::Saved { path, .. } => {
                    tracked.saved = Some(path);
                    None
                }
                _ => None,
            };
            (message, false)
        }
        JobEvent::State {
            id: job_id,
            state,
            error,
        } => match state {
            DownloadState::Queued | DownloadState::Running => (None, false),
            DownloadState::Verifying => (
                Some(WorkerToGuiMessage::StatusUpdate {
                    id,
                    text: "Verifying checksum...".into(),
                }),
                false,
            ),
            DownloadState::Completed => {
                let path = tracked
                    .saved
                    .clone()
                    .or_else(|| manager.get(job_id).and_then(|i| i.path))
                    .unwrap_or_else(|| tracked.output_path.clone());
                (Some(WorkerToGuiMessage::Completed { id, path }), true)
            }
            DownloadState::Failed => (
                Some(WorkerToGuiMessage::Error {
                    id,
                    message: error.unwrap_or_else(|| "Download failed".into()),
                }),
                true,
            ),
            DownloadState::Paused | DownloadState::Cancelled => {
                (Some(WorkerToGuiMessage::Cancelled { id }), true)
            }
        },
        JobEvent::Removed { .. } => (None, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_jobs_resume_and_use_folders_for_video() {
        let job = gui_job("https://example.com/a.iso", "/tmp/a.iso", true, true, None);
        assert_eq!(job.output.as_deref(), Some("/tmp/a.iso"));
        assert!(job.advanced && job.verify_iso && job.resume);
        let video = gui_job(
            "https://www.youtube.com/watch?v=x",
            "/tmp/dl/watch",
            false,
            false,
            None,
        );
        assert_eq!(video.output.as_deref(), Some("/tmp/dl"));
    }
}
