//! Application-facing orchestration shared by GUI frontends.
//!
//! This module keeps UI worker plumbing out of `main.rs` so the binary can stay
//! thin and future frontends can reuse the same command/message contract.
//!
//! Every download is identified by a [`JobId`] chosen by the frontend, so
//! several downloads can run at the same time: messages carry the id of the
//! job they belong to and [`DownloadCommand::Cancel`] stops only that job.

use crate::DownloadOptions;
use crate::advanced_download::{AdvancedDownloader, ResumePolicy};
use crate::config::Config;
use crate::download::download_to_path as simple_download;
use crate::events::{JsonEvent, SpeedMeter};
use crate::optimization::Optimizer;
use crate::protocol::{self, Protocol};
use crate::queue::{DownloadHistory, EntryStatus, HistoryEntry};
use crate::torrent::{TorrentCallbacks, TorrentOptions, TorrentOutcome, download_torrent};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver as MpscReceiver, Sender as MpscSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Identifier of one download job, chosen by the frontend.
pub type JobId = u64;

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
pub fn spawn_download_worker(
    config: Config,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        download_worker(config, download_rx, status_tx);
    })
}

struct Job {
    cancel: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

fn download_worker(
    config: Config,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
) {
    let mut jobs: HashMap<JobId, Job> = HashMap::new();

    loop {
        match download_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(DownloadCommand::Start {
                id,
                url,
                output_path,
                is_advanced,
                verify_iso,
                expected_sha256,
            }) => {
                let cancel = Arc::new(AtomicBool::new(false));
                let job = JobSpec {
                    id,
                    url,
                    output_path,
                    is_advanced,
                    verify_iso,
                    expected_sha256,
                };
                let handle = {
                    let config = config.clone();
                    let tx = status_tx.clone();
                    let cancel = cancel.clone();
                    thread::spawn(move || run_job(job, config, cancel, tx))
                };
                jobs.insert(id, Job { cancel, handle });
            }
            Ok(DownloadCommand::Cancel { id }) => {
                if let Some(job) = jobs.get(&id) {
                    job.cancel.store(true, Ordering::SeqCst);
                    let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate {
                        id,
                        text: "Cancelling download...".into(),
                    });
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                for job in jobs.values() {
                    job.cancel.store(true, Ordering::SeqCst);
                }
                break;
            }
        }

        // Reap finished jobs.
        let finished: Vec<JobId> = jobs
            .iter()
            .filter(|(_, j)| j.handle.is_finished())
            .map(|(id, _)| *id)
            .collect();
        for id in finished {
            if let Some(job) = jobs.remove(&id) {
                let _ = job.handle.join();
            }
        }
    }
}

struct JobSpec {
    id: JobId,
    url: String,
    output_path: String,
    is_advanced: bool,
    verify_iso: bool,
    expected_sha256: Option<String>,
}

fn run_job(
    job: JobSpec,
    config: Config,
    cancel: Arc<AtomicBool>,
    tx: MpscSender<WorkerToGuiMessage>,
) {
    let id = job.id;
    let _ = tx.send(WorkerToGuiMessage::StatusUpdate {
        id,
        text: format!("Initializing: {}", job.url),
    });

    let optimizer = Optimizer::from_config(config.optimization.clone());
    let proxy = config.proxy.clone();

    let status = {
        let tx = tx.clone();
        Arc::new(move |text: String| {
            if !text.starts_with("PROGRESS:") {
                let _ = tx.send(WorkerToGuiMessage::StatusUpdate { id, text });
            }
        })
    };
    let meter = Arc::new(Mutex::new(SpeedMeter::new()));
    let on_bytes = {
        let tx = tx.clone();
        let meter = meter.clone();
        Arc::new(move |bytes: u64, total: Option<u64>| {
            let ev = meter
                .lock()
                .expect("speed meter poisoned")
                .progress(bytes, total);
            if let JsonEvent::Progress {
                percent,
                speed_bps,
                eta_secs,
                ..
            } = ev
            {
                let _ = tx.send(WorkerToGuiMessage::Progress {
                    id,
                    fraction: percent.map(|p| (p / 100.0) as f32).unwrap_or(0.0),
                    bytes,
                    total,
                    speed_bps,
                    eta_secs,
                });
            }
        })
    };

    let detected = protocol::detect(&job.url);
    let result: Result<String, Box<dyn Error + Send + Sync>> = match detected {
        Protocol::Torrent => {
            let tx_p = tx.clone();
            let callbacks = TorrentCallbacks {
                status: Some(status.clone()),
                progress: Some(Arc::new(move |p| {
                    let _ = tx_p.send(WorkerToGuiMessage::Progress {
                        id,
                        fraction: p,
                        bytes: 0,
                        total: None,
                        speed_bps: 0,
                        eta_secs: None,
                    });
                })),
            };
            download_torrent(
                &job.url,
                &job.output_path,
                true,
                proxy,
                optimizer,
                callbacks,
                TorrentOptions {
                    config: config.torrent.clone(),
                    cancel: Some(cancel.clone()),
                    on_files: None,
                    on_file_progress: None,
                },
            )
            .map(|outcome| match outcome {
                TorrentOutcome::Downloaded => job.output_path.clone(),
                TorrentOutcome::HandedOff => {
                    status("Opened in the system torrent client".into());
                    job.output_path.clone()
                }
            })
        }
        Protocol::Ftp => crate::ftp::FtpDownloader::new(
            job.url.clone(),
            job.output_path.clone(),
            true,
            proxy,
            optimizer,
        )
        .download()
        .map(|_| job.output_path.clone()),
        Protocol::Sftp => crate::sftp::SftpDownloader::new(
            job.url.clone(),
            job.output_path.clone(),
            true,
            proxy,
            optimizer,
        )
        .with_key_path(config.sftp.key_path.clone())
        .download()
        .map(|_| job.output_path.clone()),
        Protocol::Metalink => {
            let dir = std::path::Path::new(&job.output_path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".into());
            crate::metalink::download_metalink(&job.url, &dir, true, proxy, optimizer).map(|_| dir)
        }
        Protocol::Video => {
            let dir = std::path::Path::new(&job.output_path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".into());
            let quality = crate::ytdlp::VideoQuality::default();
            let tx_v = tx.clone();
            crate::ytdlp::download_video_with_cancel(
                &job.url,
                &dir,
                &quality,
                Some(cancel.clone()),
                move |ev| match ev {
                    crate::ytdlp::VideoEvent::Progress(p) => {
                        let _ = tx_v.send(WorkerToGuiMessage::Progress {
                            id,
                            fraction: (p.percent / 100.0) as f32,
                            bytes: 0,
                            total: None,
                            speed_bps: 0,
                            eta_secs: None,
                        });
                    }
                    crate::ytdlp::VideoEvent::Log(line) => {
                        let _ = tx_v.send(WorkerToGuiMessage::StatusUpdate { id, text: line });
                    }
                    crate::ytdlp::VideoEvent::Saved(_) => {}
                },
            )
        }
        Protocol::WebDav => {
            let options = DownloadOptions {
                cancel: Some(cancel.clone()),
                progress_callback: Some(on_bytes.clone()),
                expected_sha256: job.expected_sha256.clone(),
                ..Default::default()
            };
            crate::webdav::WebDavDownloader::new(
                job.url.clone(),
                job.output_path.clone(),
                true,
                proxy,
                optimizer,
            )
            .with_options(options)
            .download_to_path(Some(&*status))
            .map(|p| p.to_string_lossy().into_owned())
        }
        Protocol::Http if job.is_advanced => AdvancedDownloader::new(
            job.url.clone(),
            job.output_path.clone(),
            true,
            proxy,
            optimizer,
        )
        .and_then(|mut downloader| {
            downloader.set_cancel_token(cancel.clone());
            downloader.set_resume_policy(if job.verify_iso {
                ResumePolicy::AlwaysResume
            } else {
                ResumePolicy::AlwaysRestart
            });
            if let Some(expected_sha256) = job.expected_sha256.clone() {
                downloader.set_expected_sha256(expected_sha256);
            }
            let on_bytes = on_bytes.clone();
            downloader.set_byte_progress_callback(move |b, t| on_bytes(b, t));
            let status = status.clone();
            downloader.set_status_callback(move |msg| status(msg));
            downloader.download().map(|_| job.output_path.clone())
        }),
        Protocol::Http => {
            let options = DownloadOptions {
                quiet_mode: true,
                output_path: Some(job.output_path.clone()),
                verify_iso: job.verify_iso,
                expected_sha256: job.expected_sha256.clone(),
                resume: true,
                cancel: Some(cancel.clone()),
                progress_callback: Some(on_bytes.clone()),
                ..Default::default()
            };
            simple_download(&job.url, proxy, optimizer, options, Some(&*status))
                .map(|p| p.to_string_lossy().into_owned())
        }
    };

    record_history(&job, &result, cancel.load(Ordering::SeqCst));

    if cancel.load(Ordering::SeqCst) {
        let _ = tx.send(WorkerToGuiMessage::Cancelled { id });
        return;
    }
    match result {
        Ok(path) => {
            let _ = tx.send(WorkerToGuiMessage::Completed { id, path });
        }
        Err(e) => {
            if matches!(
                e.downcast_ref::<crate::KgetError>(),
                Some(crate::KgetError::Cancelled)
            ) {
                let _ = tx.send(WorkerToGuiMessage::Cancelled { id });
            } else {
                let _ = tx.send(WorkerToGuiMessage::Error {
                    id,
                    message: e.to_string(),
                });
            }
        }
    }
}

fn record_history(
    job: &JobSpec,
    result: &Result<String, Box<dyn Error + Send + Sync>>,
    cancelled: bool,
) {
    let dir = std::path::Path::new(&job.output_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".into());
    let mut entry = HistoryEntry::new(&job.url, &dir, job.expected_sha256.as_deref());
    let (status, error) = match result {
        _ if cancelled => (EntryStatus::Cancelled, None),
        Ok(path) => {
            entry.set_saved_file(std::path::Path::new(path));
            (EntryStatus::Completed, None)
        }
        Err(e) => (EntryStatus::Failed, Some(e.to_string())),
    };
    let _ = DownloadHistory::append(entry, status, error);
}
