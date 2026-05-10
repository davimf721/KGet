//! Application-facing orchestration shared by GUI frontends.
//!
//! This module keeps UI worker plumbing out of `main.rs` so the binary can stay
//! thin and future frontends can reuse the same command/message contract.

use crate::DownloadOptions;
use crate::advanced_download::AdvancedDownloader;
use crate::config::Config;
use crate::download::download as simple_download;
use crate::optimization::Optimizer;
use crate::torrent::{TorrentCallbacks, download_magnet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver as MpscReceiver, Sender as MpscSender};
use std::thread;
use std::time::Duration;

/// Command sent from a frontend to the download worker.
#[derive(Debug, Clone)]
pub enum DownloadCommand {
    Start {
        url: String,
        output_path: String,
        is_advanced: bool,
        verify_iso: bool,
        expected_sha256: Option<String>,
    },
    Cancel,
}

/// Message sent from the download worker back to a frontend.
#[derive(Debug, Clone)]
pub enum WorkerToGuiMessage {
    Progress(f32),
    StatusUpdate(String),
    Completed(String),
    Error(String),
}

impl fmt::Display for WorkerToGuiMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerToGuiMessage::Progress(p) => write!(f, "Progress({:.2})", p),
            WorkerToGuiMessage::StatusUpdate(s) => write!(f, "Status: {}", s),
            WorkerToGuiMessage::Completed(s) => write!(f, "Completed: {}", s),
            WorkerToGuiMessage::Error(e) => write!(f, "Error: {}", e),
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

fn download_worker(
    config: Config,
    download_rx: MpscReceiver<DownloadCommand>,
    status_tx: MpscSender<WorkerToGuiMessage>,
) {
    let cancel_token = Arc::new(AtomicBool::new(false));
    let mut download_handle: Option<thread::JoinHandle<()>> = None;

    loop {
        match download_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(command) => match command {
                DownloadCommand::Start {
                    url,
                    output_path,
                    is_advanced,
                    verify_iso,
                    expected_sha256,
                } => {
                    cancel_token.store(false, Ordering::SeqCst);
                    let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate(format!(
                        "Initializing: {}",
                        url
                    )));

                    let optimizer = Optimizer::from_config(config.optimization.clone());
                    let proxy = config.proxy.clone();
                    let cancel_token_clone = cancel_token.clone();
                    let status_tx_clone = status_tx.clone();

                    download_handle = Some(thread::spawn(move || {
                        let result = if url.starts_with("magnet:?") {
                            let callbacks = TorrentCallbacks {
                                status: Some(Arc::new({
                                    let status_tx = status_tx_clone.clone();
                                    move |msg| {
                                        status_tx.send(WorkerToGuiMessage::StatusUpdate(msg)).ok();
                                    }
                                })),
                                progress: Some(Arc::new({
                                    let status_tx = status_tx_clone.clone();
                                    move |p| {
                                        status_tx.send(WorkerToGuiMessage::Progress(p)).ok();
                                    }
                                })),
                            };

                            download_magnet(&url, &output_path, true, proxy, optimizer, callbacks)
                        } else if is_advanced {
                            let mut downloader = AdvancedDownloader::new(
                                url.clone(),
                                output_path.clone(),
                                true,
                                proxy,
                                optimizer,
                            );

                            downloader.set_cancel_token(cancel_token_clone.clone());
                            if let Some(expected_sha256) = expected_sha256.clone() {
                                downloader.set_expected_sha256(expected_sha256);
                            }

                            let status_tx_cb = status_tx_clone.clone();
                            downloader.set_progress_callback(move |p| {
                                status_tx_cb.send(WorkerToGuiMessage::Progress(p)).ok();
                            });

                            let status_tx_cb = status_tx_clone.clone();
                            downloader.set_status_callback(move |msg| {
                                status_tx_cb
                                    .send(WorkerToGuiMessage::StatusUpdate(msg))
                                    .ok();
                            });

                            downloader.download()
                        } else {
                            let options = DownloadOptions {
                                quiet_mode: true,
                                output_path: Some(output_path.clone()),
                                verify_iso,
                                expected_sha256: expected_sha256.clone(),
                            };

                            let status_tx_cb = status_tx_clone.clone();
                            let status_cb = move |msg: String| {
                                status_tx_cb
                                    .send(WorkerToGuiMessage::StatusUpdate(msg))
                                    .ok();
                            };

                            simple_download(&url, proxy, optimizer, options, Some(&status_cb))
                        };

                        report_download_result(
                            result,
                            output_path,
                            &cancel_token_clone,
                            &status_tx_clone,
                        );
                    }));
                }
                DownloadCommand::Cancel => {
                    cancel_token.store(true, Ordering::SeqCst);
                    let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate(
                        "Cancelling download...".into(),
                    ));
                }
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(handle) = download_handle.take() {
                    if handle.is_finished() {
                        let _ = handle.join();
                    } else {
                        download_handle = Some(handle);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn report_download_result(
    result: Result<(), Box<dyn Error + Send + Sync>>,
    output_path: String,
    cancel_token: &AtomicBool,
    status_tx: &MpscSender<WorkerToGuiMessage>,
) {
    if cancel_token.load(Ordering::SeqCst) {
        let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate(
            "Download cancelled".into(),
        ));
        return;
    }

    match result {
        Ok(_) => {
            let _ = status_tx.send(WorkerToGuiMessage::Completed(output_path));
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("cancelled") {
                let _ = status_tx.send(WorkerToGuiMessage::StatusUpdate(
                    "Download cancelled".into(),
                ));
            } else {
                let _ = status_tx.send(WorkerToGuiMessage::Error(err_msg));
            }
        }
    }
}
