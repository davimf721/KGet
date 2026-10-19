//! Native torrent client using librqbit
//!
//! This module provides a built-in BitTorrent client for downloading
//! magnet links without requiring an external torrent application.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions};

use crate::config::{ProxyConfig, TorrentConfig};
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::torrent::{FileProgressCb, FilesCb, TorrentCallbacks, TorrentOptions};
use crate::utils::print;

// ============================================================================
// Types and Constants
// ============================================================================

type BoxError = Box<dyn Error + Send + Sync>;
type FileInfoList = Vec<(String, u64)>;

const METADATA_TIMEOUT_SECS: u64 = 120;
const PROGRESS_UPDATE_INTERVAL_MS: u64 = 1000;
const PROGRESS_EMIT_THRESHOLD: f32 = 5.0;

/// Download progress statistics
struct DownloadStats {
    downloaded: u64,
    total: u64,
    speed: f64,
    eta: String,
    progress_pct: f32,
}

impl DownloadStats {
    fn format_sizes(&self) -> (String, String) {
        (
            humansize::format_size(self.downloaded, humansize::BINARY),
            humansize::format_size(self.total, humansize::BINARY),
        )
    }

    fn format_speed(&self) -> String {
        humansize::format_size(self.speed as u64, humansize::BINARY)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn format_eta(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

fn calculate_progress(
    file_infos: &FileInfoList,
    file_progress: &[u64],
    fallback: (u64, u64),
) -> (u64, u64) {
    if file_infos.is_empty() || file_progress.is_empty() {
        return fallback;
    }

    let downloaded: u64 = file_progress.iter().sum();
    let total: u64 = file_infos.iter().map(|(_, size)| *size).sum();
    (downloaded, total)
}

fn emit_files_json(file_infos: &FileInfoList) {
    if file_infos.is_empty() {
        return;
    }
    let v: Vec<_> = file_infos
        .iter()
        .map(|(name, size)| serde_json::json!({ "name": name, "size": size }))
        .collect();
    println!("FILES: {}", serde_json::Value::Array(v));
}

fn emit_file_progress_json(file_infos: &FileInfoList, file_progress: &[u64]) {
    if file_infos.is_empty() || file_progress.is_empty() {
        return;
    }
    let v: Vec<_> = file_infos
        .iter()
        .enumerate()
        .map(|(i, (name, size))| {
            let downloaded = file_progress.get(i).copied().unwrap_or(0);
            let pct = if *size > 0 {
                (downloaded as f64 / *size as f64) * 100.0
            } else {
                100.0
            };
            serde_json::json!({
                "idx": i,
                "name": name,
                "downloaded": downloaded,
                "size": size,
                "pct": (pct * 10.0).round() / 10.0,
            })
        })
        .collect();
    println!("FILE_PROGRESS: {}", serde_json::Value::Array(v));
}

// ============================================================================
// NativeTorrentDownloader
// ============================================================================

/// Native torrent downloader using librqbit
pub struct NativeTorrentDownloader {
    magnet: String,
    output_dir: String,
    quiet: bool,
    proxy: ProxyConfig,
    _optimizer: Optimizer,
    config: TorrentConfig,
    cancel: Arc<AtomicBool>,
    files_cb: Option<FilesCb>,
    file_progress_cb: Option<FileProgressCb>,
    status_cb: Option<Arc<dyn Fn(String) + Send + Sync>>,
    progress_cb: Option<Arc<dyn Fn(f32) + Send + Sync>>,
}

impl NativeTorrentDownloader {
    pub fn new(
        magnet: String,
        output_dir: String,
        quiet: bool,
        proxy: ProxyConfig,
        optimizer: Optimizer,
    ) -> Self {
        Self {
            magnet,
            output_dir,
            quiet,
            proxy,
            _optimizer: optimizer,
            config: TorrentConfig::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            files_cb: None,
            file_progress_cb: None,
            status_cb: None,
            progress_cb: None,
        }
    }

    /// Apply port / DHT settings from the torrent configuration.
    pub fn set_config(&mut self, config: TorrentConfig) {
        self.config = config;
    }

    /// Token that stops the download when set to `true`.
    pub fn set_cancel_token(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = cancel;
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn set_status_callback<F>(&mut self, cb: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.status_cb = Some(Arc::new(cb));
    }

    pub fn set_progress_callback<F>(&mut self, cb: F)
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        self.progress_cb = Some(Arc::new(cb));
    }

    fn emit_status(&self, msg: impl Into<String>) {
        let msg = msg.into();
        if let Some(cb) = &self.status_cb {
            cb(msg.clone());
        }
        if !self.quiet {
            print(&msg, self.quiet);
        }
    }

    fn emit_progress(&self, p: f32) {
        if let Some(cb) = &self.progress_cb {
            cb(p.clamp(0.0, 1.0));
        }
    }

    pub async fn download(&self) -> Result<(), BoxError> {
        self.emit_status(format!("Starting native torrent download: {}", self.magnet));
        self.emit_progress(0.0);

        if self.proxy.enabled {
            self.emit_status(
                "Warning: the native torrent client does not use the configured proxy",
            );
        }

        let output_path = self.ensure_output_dir()?;
        let session = self.create_session(&output_path).await?;
        let handle = self.add_torrent(&session).await?;

        if let Err(e) = self.wait_for_metadata(&handle).await {
            session.stop().await;
            return Err(e);
        }

        let name = handle.name().unwrap_or_else(|| "Torrent".to_string());
        let file_infos = self.extract_file_infos(&handle);

        if let Some(cb) = &self.files_cb {
            cb(file_infos.clone());
        }
        if !self.quiet {
            emit_files_json(&file_infos);
        }

        let stats = handle.stats();
        self.emit_status(format!(
            "Downloading: {} ({} bytes)",
            name, stats.total_bytes
        ));

        let progress_bar = create_progress_bar(
            self.quiet,
            format!("Downloading {}", name),
            Some(stats.total_bytes),
            false,
        );

        if let Err(e) = self
            .run_download_loop(&handle, &name, &file_infos, &progress_bar)
            .await
        {
            drop(handle);
            session.stop().await;
            return Err(e);
        }

        self.emit_progress(1.0);
        progress_bar.finish_with_message(format!("✓ {} downloaded successfully!", name));
        self.emit_status(format!("Download complete: {}", name));

        drop(handle);
        session.stop().await;

        Ok(())
    }

    fn ensure_output_dir(&self) -> Result<PathBuf, BoxError> {
        let path = PathBuf::from(&self.output_dir);
        if !path.exists() {
            std::fs::create_dir_all(&path)?;
        }
        Ok(path)
    }

    async fn create_session(
        &self,
        output_path: &std::path::Path,
    ) -> Result<Arc<Session>, BoxError> {
        self.emit_status("Initializing BitTorrent session...");

        let port = self.config.port.unwrap_or(6881);
        let opts = SessionOptions {
            disable_dht: !self.config.dht_enabled,
            disable_dht_persistence: true,
            dht_config: None,
            persistence: None,
            listen_port_range: Some(port..port.saturating_add(8)),
            // Only map ports automatically when the user did not pick one.
            enable_upnp_port_forwarding: self.config.port.is_none(),
            ..Default::default()
        };

        let session = Session::new_with_opts(output_path.to_path_buf(), opts)
            .await
            .map_err(|e| format!("Failed to create torrent session: {}", e))?;
        Ok(session)
    }

    async fn add_torrent(
        &self,
        session: &Arc<Session>,
    ) -> Result<Arc<librqbit::ManagedTorrent>, BoxError> {
        self.emit_status("Adding torrent...");

        let opts = AddTorrentOptions {
            overwrite: true,
            ..Default::default()
        };

        let lower = self.magnet.to_ascii_lowercase();
        let add = if lower.starts_with("magnet:")
            || lower.starts_with("http://")
            || lower.starts_with("https://")
        {
            AddTorrent::from_url(&self.magnet)
        } else {
            let bytes = std::fs::read(&self.magnet)
                .map_err(|e| format!("Cannot read torrent file '{}': {}", self.magnet, e))?;
            AddTorrent::from_bytes(bytes)
        };

        match session.add_torrent(add, Some(opts)).await {
            Ok(AddTorrentResponse::Added(_, handle)) => Ok(handle),
            Ok(AddTorrentResponse::AlreadyManaged(_, handle)) => {
                self.emit_status("Torrent already exists, resuming...");
                Ok(handle)
            }
            Ok(AddTorrentResponse::ListOnly(_)) => {
                Err("Torrent was added in list-only mode".into())
            }
            Err(e) => Err(format!("Failed to add torrent: {}", e).into()),
        }
    }

    async fn wait_for_metadata(
        &self,
        handle: &Arc<librqbit::ManagedTorrent>,
    ) -> Result<(), BoxError> {
        self.emit_status("Fetching torrent metadata from peers...");

        let timeout = Duration::from_secs(METADATA_TIMEOUT_SECS);
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err("Timeout waiting for torrent metadata. No peers found.".into());
            }
            if self.is_cancelled() {
                return Err(Box::new(crate::KgetError::Cancelled));
            }

            let stats = handle.stats();

            if let Some(error) = &stats.error {
                return Err(format!("Torrent error: {}", error).into());
            }

            if stats.total_bytes > 0 {
                return Ok(());
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    fn extract_file_infos(&self, handle: &Arc<librqbit::ManagedTorrent>) -> FileInfoList {
        handle
            .with_metadata(|meta| {
                meta.file_infos
                    .iter()
                    .map(|f| (f.relative_filename.to_string_lossy().to_string(), f.len))
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn run_download_loop(
        &self,
        handle: &Arc<librqbit::ManagedTorrent>,
        name: &str,
        file_infos: &FileInfoList,
        progress_bar: &indicatif::ProgressBar,
    ) -> Result<(), BoxError> {
        let mut last_progress: f32 = 0.0;
        let mut last_bytes: u64 = 0;
        let mut last_time = Instant::now();

        loop {
            if self.is_cancelled() {
                progress_bar.abandon_with_message("Cancelled");
                return Err(Box::new(crate::KgetError::Cancelled));
            }
            let stats = handle.stats();
            let download_stats = self.compute_stats(&stats, file_infos, last_bytes, last_time);

            last_bytes = download_stats.downloaded;
            last_time = Instant::now();

            self.emit_progress(download_stats.progress_pct / 100.0);
            progress_bar.set_position(download_stats.downloaded);

            let (downloaded_str, total_str) = download_stats.format_sizes();
            let msg = format!(
                "{} - {:.1}% ({}/{})",
                name, download_stats.progress_pct, downloaded_str, total_str
            );
            progress_bar.set_message(msg.clone());

            if let Some(cb) = &self.file_progress_cb
                && !stats.file_progress.is_empty()
            {
                cb(stats.file_progress.clone());
            }

            // Line-oriented output for external parsers (never in quiet/JSONL mode)
            if !self.quiet {
                progress_bar.suspend(|| {
                    println!(
                        "PROGRESS: {:.1}% ({}/{}) SPEED: {}/s ETA: {}",
                        download_stats.progress_pct,
                        downloaded_str,
                        total_str,
                        download_stats.format_speed(),
                        download_stats.eta
                    );
                    emit_file_progress_json(file_infos, &stats.file_progress);
                });
            }

            if (download_stats.progress_pct - last_progress).abs() >= PROGRESS_EMIT_THRESHOLD {
                self.emit_status(msg);
                last_progress = download_stats.progress_pct;
            }

            if download_stats.downloaded >= download_stats.total && download_stats.total > 0 {
                break;
            }

            if let Some(error) = &stats.error {
                progress_bar.abandon_with_message(format!("Error: {}", error));
                return Err(format!("Download failed: {}", error).into());
            }

            tokio::time::sleep(Duration::from_millis(PROGRESS_UPDATE_INTERVAL_MS)).await;
        }

        Ok(())
    }

    fn compute_stats(
        &self,
        stats: &librqbit::TorrentStats,
        file_infos: &FileInfoList,
        last_bytes: u64,
        last_time: Instant,
    ) -> DownloadStats {
        let (downloaded, total) = calculate_progress(
            file_infos,
            &stats.file_progress,
            (stats.progress_bytes, stats.total_bytes),
        );

        let progress_pct = if total > 0 {
            (downloaded as f32 / total as f32) * 100.0
        } else {
            0.0
        };

        let elapsed = last_time.elapsed().as_secs_f64();
        let bytes_diff = downloaded.saturating_sub(last_bytes);
        let speed = if elapsed > 0.0 {
            bytes_diff as f64 / elapsed
        } else {
            0.0
        };

        let eta = if speed > 0.0 && total > downloaded {
            format_eta((total - downloaded) as f64 / speed)
        } else {
            "--".to_string()
        };

        DownloadStats {
            downloaded,
            total,
            speed,
            eta,
            progress_pct,
        }
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Download a magnet link or `.torrent` using the native torrent client
pub fn download_magnet_native(
    magnet: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    callbacks: TorrentCallbacks,
    opts: TorrentOptions,
) -> Result<(), BoxError> {
    let mut downloader = NativeTorrentDownloader::new(
        magnet.to_string(),
        output_dir.to_string(),
        quiet,
        proxy,
        optimizer,
    );
    downloader.set_config(opts.config);
    if let Some(cancel) = opts.cancel {
        downloader.set_cancel_token(cancel);
    }
    downloader.files_cb = opts.on_files;
    downloader.file_progress_cb = opts.on_file_progress;

    if let Some(status) = callbacks.status {
        downloader.set_status_callback(move |s| status(s));
    }

    if let Some(progress) = callbacks.progress {
        downloader.set_progress_callback(move |p| progress(p));
    }

    tokio::runtime::Runtime::new()?.block_on(downloader.download())
}

#[cfg(test)]
mod tests {
    use super::format_eta;

    #[test]
    fn eta_uses_integer_division() {
        assert_eq!(format_eta(119.0), "1m 59s");
        assert_eq!(format_eta(5.4), "5s");
        assert_eq!(format_eta(3_661.0), "1h 1m");
    }
}
