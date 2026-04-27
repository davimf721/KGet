//! Native torrent client using librqbit
//!
//! This module provides a built-in BitTorrent client for downloading
//! magnet links without requiring an external torrent application.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions};

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::torrent::TorrentCallbacks;
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
    match seconds {
        s if s < 60.0 => format!("{:.0}s", s),
        s if s < 3600.0 => format!("{:.0}m {:.0}s", s / 60.0, s % 60.0),
        s => format!("{:.0}h {:.0}m", s / 3600.0, (s % 3600.0) / 60.0),
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

fn build_file_json(name: &str, size: u64) -> String {
    format!(
        r#"{{"name":"{}","size":{}}}"#,
        name.replace('\"', "\\\""),
        size
    )
}

fn build_file_progress_json(
    idx: usize,
    name: &str,
    downloaded: u64,
    size: u64,
    pct: f64,
) -> String {
    format!(
        r#"{{"idx":{},"name":"{}","downloaded":{},"size":{},"pct":{:.1}}}"#,
        idx,
        name.replace('\"', "\\\""),
        downloaded,
        size,
        pct
    )
}

fn emit_files_json(file_infos: &FileInfoList) {
    if file_infos.is_empty() {
        return;
    }

    let json: Vec<String> = file_infos
        .iter()
        .map(|(name, size)| build_file_json(name, *size))
        .collect();
    println!("FILES: [{}]", json.join(","));
}

fn emit_file_progress_json(file_infos: &FileInfoList, file_progress: &[u64]) {
    if file_infos.is_empty() || file_progress.is_empty() {
        return;
    }

    let json: Vec<String> = file_infos
        .iter()
        .enumerate()
        .map(|(i, (name, size))| {
            let downloaded = file_progress.get(i).copied().unwrap_or(0);
            let pct = if *size > 0 {
                (downloaded as f64 / *size as f64) * 100.0
            } else {
                100.0
            };
            build_file_progress_json(i, name, downloaded, *size, pct)
        })
        .collect();

    println!("FILE_PROGRESS: [{}]", json.join(","));
}

// ============================================================================
// NativeTorrentDownloader
// ============================================================================

/// Native torrent downloader using librqbit
pub struct NativeTorrentDownloader {
    magnet: String,
    output_dir: String,
    quiet: bool,
    _proxy: ProxyConfig,
    _optimizer: Optimizer,
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
            _proxy: proxy,
            _optimizer: optimizer,
            status_cb: None,
            progress_cb: None,
        }
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

        let output_path = self.ensure_output_dir()?;
        let session = self.create_session(&output_path).await?;
        let handle = self.add_torrent(&session).await?;

        self.wait_for_metadata(&handle).await?;

        let name = handle.name().unwrap_or_else(|| "Torrent".to_string());
        let file_infos = self.extract_file_infos(&handle);

        emit_files_json(&file_infos);

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

        self.run_download_loop(&handle, &name, &file_infos, &progress_bar)
            .await?;

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

    async fn create_session(&self, output_path: &PathBuf) -> Result<Arc<Session>, BoxError> {
        self.emit_status("Initializing BitTorrent session...");

        let opts = SessionOptions {
            disable_dht: false,
            disable_dht_persistence: true,
            dht_config: None,
            persistence: None,
            listen_port_range: Some(6881..6889),
            enable_upnp_port_forwarding: true,
            ..Default::default()
        };

        let session = Session::new_with_opts(output_path.clone(), opts)
            .await
            .map_err(|e| format!("Failed to create torrent session: {}", e))?;
        Ok(session)
    }

    async fn add_torrent(
        &self,
        session: &Arc<Session>,
    ) -> Result<Arc<librqbit::ManagedTorrent>, BoxError> {
        self.emit_status("Adding magnet link...");

        let opts = AddTorrentOptions {
            overwrite: true,
            ..Default::default()
        };

        match session
            .add_torrent(AddTorrent::from_url(&self.magnet), Some(opts))
            .await
        {
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

            // Output for external parsers
            println!(
                "PROGRESS: {:.1}% ({}/{}) SPEED: {}/s ETA: {}",
                download_stats.progress_pct,
                downloaded_str,
                total_str,
                download_stats.format_speed(),
                download_stats.eta
            );
            emit_file_progress_json(file_infos, &stats.file_progress);

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

/// Download a magnet link using the native torrent client
pub fn download_magnet_native(
    magnet: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    callbacks: TorrentCallbacks,
) -> Result<(), BoxError> {
    let mut downloader = NativeTorrentDownloader::new(
        magnet.to_string(),
        output_dir.to_string(),
        quiet,
        proxy,
        optimizer,
    );

    if let Some(status) = callbacks.status {
        downloader.set_status_callback(move |s| status(s));
    }

    if let Some(progress) = callbacks.progress {
        downloader.set_progress_callback(move |p| progress(p));
    }

    tokio::runtime::Runtime::new()?.block_on(downloader.download())
}
