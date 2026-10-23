//! Native torrent client using librqbit
//!
//! This module provides a built-in BitTorrent client for downloading
//! magnet links without requiring an external torrent application.

use std::collections::BTreeSet;
use std::error::Error;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerOptions, Session,
    SessionOptions,
};

use crate::config::{ProxyConfig, TorrentConfig};
use crate::events::TorrentStats;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::torrent::selection::{self, FileSelection, SeedLimits, SeedStop};
use crate::torrent::{FileProgressCb, FilesCb, StatsCb, TorrentCallbacks, TorrentOptions};
use crate::utils::print;

// ============================================================================
// Types and Constants
// ============================================================================

type BoxError = Box<dyn Error + Send + Sync>;
type FileInfoList = Vec<(String, u64)>;

const METADATA_TIMEOUT_SECS: u64 = 120;
const PROGRESS_UPDATE_INTERVAL_MS: u64 = 1000;
const PROGRESS_EMIT_THRESHOLD: f32 = 5.0;

/// librqbit 9 combines DHT enablement and persistence under one option. KGet
/// keeps its sessions ephemeral, while still honouring the user's DHT and port
/// settings for peer discovery.
fn dht_options(enabled: bool, port: Option<u16>) -> Option<DhtSessionConfig> {
    enabled.then(|| DhtSessionConfig {
        port,
        persistence: None,
        ..Default::default()
    })
}

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

/// Bytes downloaded and total size of the selected files (every file when
/// `selected` is `None`).
fn calculate_progress(
    file_infos: &FileInfoList,
    file_progress: &[u64],
    selected: Option<&BTreeSet<usize>>,
    fallback: (u64, u64),
) -> (u64, u64) {
    if file_infos.is_empty() || file_progress.is_empty() {
        return fallback;
    }
    let wanted = |i: &usize| selected.is_none_or(|s| s.contains(i));
    let downloaded: u64 = file_progress
        .iter()
        .enumerate()
        .filter(|(i, _)| wanted(i))
        .map(|(_, b)| *b)
        .sum();
    let total: u64 = file_infos
        .iter()
        .enumerate()
        .filter(|(i, _)| wanted(i))
        .map(|(_, (_, size))| *size)
        .sum();
    (downloaded, total)
}

/// MiB/s (librqbit's speed unit) to bytes per second.
fn mibps_to_bps(mbps: f64) -> u64 {
    (mbps.max(0.0) * 1024.0 * 1024.0) as u64
}

/// `torrent_stats` payload from librqbit's statistics. librqbit 8 does not
/// tell seeders from leechers nor expose tracker states, so those stay empty.
fn swarm_stats(
    stats: &librqbit::TorrentStats,
    downloaded: u64,
    size: u64,
    files: (Option<usize>, usize),
) -> TorrentStats {
    let (peers, connecting, known, down, up) = match &stats.live {
        Some(live) => {
            let p = &live.snapshot.peer_stats;
            (
                p.live as u64,
                Some(p.connecting as u64),
                Some(p.seen as u64),
                mibps_to_bps(live.download_speed.mbps),
                mibps_to_bps(live.upload_speed.mbps),
            )
        }
        None => (0, None, None, 0, 0),
    };
    TorrentStats {
        peers,
        peers_connecting: connecting,
        peers_known: known,
        download_bps: down,
        upload_bps: up,
        downloaded,
        uploaded: stats.uploaded_bytes,
        size,
        ratio: selection::ratio(stats.uploaded_bytes, size),
        files_selected: Some(files.0.unwrap_or(files.1)),
        files_total: Some(files.1),
        ..Default::default()
    }
}

/// Run `fut`, giving up with [`crate::KgetError::Cancelled`] as soon as
/// `cancel` is set (checked every 200 ms).
async fn cancellable<T>(
    cancel: &AtomicBool,
    fut: impl Future<Output = Result<T, BoxError>>,
) -> Result<T, BoxError> {
    tokio::pin!(fut);
    loop {
        tokio::select! {
            r = &mut fut => return r,
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Box::new(crate::KgetError::Cancelled));
                }
            }
        }
    }
}

/// `AddTorrent` for a magnet link, `http(s)://…/.torrent` URL or local file.
fn add_source(source: &str) -> Result<AddTorrent<'static>, BoxError> {
    let lower = source.to_ascii_lowercase();
    if lower.starts_with("magnet:") || lower.starts_with("http://") || lower.starts_with("https://")
    {
        Ok(AddTorrent::from_url(source.to_string()))
    } else {
        let bytes = std::fs::read(source)
            .map_err(|e| format!("Cannot read torrent file '{}': {}", source, e))?;
        Ok(AddTorrent::from_bytes(bytes))
    }
}

/// File names and sizes of a `list_only` response, in torrent order.
fn listed_files(resp: &librqbit::ListOnlyResponse) -> Result<FileInfoList, BoxError> {
    let files = resp
        .info
        .iter_file_details()
        .enumerate()
        .map(|(_, fd)| {
            let name = fd.filename.to_string();
            (name, fd.len)
        })
        .collect();
    Ok(files)
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
    stats_cb: Option<StatsCb>,
    status_cb: Option<Arc<dyn Fn(String) + Send + Sync>>,
    progress_cb: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    selection: FileSelection,
    seed: SeedLimits,
}

/// What the progress and seeding loops need to know about the torrent.
#[derive(Clone, Copy)]
struct LoopCtx<'a> {
    name: &'a str,
    file_infos: &'a FileInfoList,
    /// Indices being downloaded (`None` = every file)
    selected: Option<&'a BTreeSet<usize>>,
    progress_bar: &'a indicatif::ProgressBar,
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
            stats_cb: None,
            status_cb: None,
            progress_cb: None,
            selection: FileSelection::default(),
            seed: SeedLimits::default(),
        }
    }

    /// Download only these files (`--select`, `--select-glob`).
    pub fn set_selection(&mut self, selection: FileSelection) {
        self.selection = selection;
    }

    /// Keep seeding after the download until one of these limits is reached.
    pub fn set_seed_limits(&mut self, seed: SeedLimits) {
        self.seed = seed;
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
        let selected: Option<BTreeSet<usize>> =
            handle.only_files().map(|v| v.into_iter().collect());

        if let Some(cb) = &self.files_cb {
            cb(file_infos.clone());
        }
        if !self.quiet {
            emit_files_json(&file_infos);
        }

        let (_, size) = calculate_progress(
            &file_infos,
            &vec![0; file_infos.len()],
            selected.as_ref(),
            (0, handle.stats().total_bytes),
        );
        match &selected {
            Some(sel) => self.emit_status(format!(
                "Downloading: {} ({} of {} files, {} bytes)",
                name,
                sel.len(),
                file_infos.len(),
                size
            )),
            None => self.emit_status(format!("Downloading: {} ({} bytes)", name, size)),
        }

        let progress_bar = create_progress_bar(
            self.quiet,
            format!("Downloading {}", name),
            Some(size),
            false,
        );

        let ctx = LoopCtx {
            name: &name,
            file_infos: &file_infos,
            selected: selected.as_ref(),
            progress_bar: &progress_bar,
        };
        if let Err(e) = self.run_download_loop(&handle, &ctx).await {
            drop(handle);
            session.stop().await;
            return Err(e);
        }

        self.emit_progress(1.0);
        progress_bar.finish_with_message(format!("✓ {} downloaded successfully!", name));
        self.emit_status(format!("Download complete: {}", name));

        if self.seed.seeds() {
            self.seed_until_limit(&handle, &ctx, size).await;
        }

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
            dht: dht_options(self.config.dht_enabled, Some(port)),
            persistence: None,
            // Only map ports automatically when the user did not pick one.
            listen: Some(ListenerOptions {
                listen_addr: ([0, 0, 0, 0], port).into(),
                enable_upnp_port_forwarding: self.config.port.is_none(),
                ..Default::default()
            }),
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

        let mut opts = AddTorrentOptions {
            overwrite: true,
            ..Default::default()
        };
        let mut add = add_source(&self.magnet)?;

        if !self.selection.is_empty() {
            // Fetch the file list first (the peers seen while resolving a
            // magnet are reused), then add the torrent with only the chosen
            // files so the others are never created on disk.
            self.emit_status("Fetching torrent metadata from peers...");
            let listed = cancellable(&self.cancel, async {
                session
                    .add_torrent(
                        add,
                        Some(AddTorrentOptions {
                            list_only: true,
                            ..Default::default()
                        }),
                    )
                    .await
                    .map_err(|e| BoxError::from(format!("Failed to add torrent: {e}")))
            })
            .await?;
            let AddTorrentResponse::ListOnly(resp) = listed else {
                return Err("Torrent metadata was not returned".into());
            };
            let names: Vec<String> = listed_files(&resp)?.into_iter().map(|f| f.0).collect();
            let only = self.selection.resolve(&names)?;
            if let Some(only) = &only {
                self.emit_status(format!("Selected {} of {} files", only.len(), names.len()));
            }
            opts.only_files = only;
            opts.initial_peers = Some(resp.seen_peers);
            add = AddTorrent::from_bytes(resp.torrent_bytes);
        }

        let added = cancellable(&self.cancel, async {
            session
                .add_torrent(add, Some(opts))
                .await
                .map_err(|e| BoxError::from(format!("Failed to add torrent: {e}")))
        })
        .await;
        match added {
            Ok(AddTorrentResponse::Added(_, handle)) => Ok(handle),
            Ok(AddTorrentResponse::AlreadyManaged(_, handle)) => {
                self.emit_status("Torrent already exists, resuming...");
                Ok(handle)
            }
            Ok(AddTorrentResponse::ListOnly(_)) => {
                Err("Torrent was added in list-only mode".into())
            }
            Err(e) => Err(e),
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
        ctx: &LoopCtx<'_>,
    ) -> Result<(), BoxError> {
        let LoopCtx {
            name,
            file_infos,
            selected,
            progress_bar,
        } = *ctx;
        let mut last_progress: f32 = 0.0;
        let mut last_bytes: u64 = 0;
        let mut last_time = Instant::now();

        loop {
            if self.is_cancelled() {
                progress_bar.abandon_with_message("Cancelled");
                return Err(Box::new(crate::KgetError::Cancelled));
            }
            let stats = handle.stats();
            let download_stats =
                self.compute_stats(&stats, file_infos, selected, last_bytes, last_time);

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
            if let Some(cb) = &self.stats_cb {
                cb(self.with_limits(swarm_stats(
                    &stats,
                    download_stats.downloaded,
                    download_stats.total,
                    (selected.map(|s| s.len()), file_infos.len()),
                )));
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

            let done =
                download_stats.downloaded >= download_stats.total && download_stats.total > 0;
            if done || (stats.finished && selected.is_some()) {
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

    /// Add the configured seeding limits to a stats payload.
    fn with_limits(&self, mut s: TorrentStats) -> TorrentStats {
        s.seed_ratio_limit = self.seed.ratio;
        s.seed_time_limit_secs = self.seed.time.map(|t| t.as_secs());
        s
    }

    /// Keep the torrent running after the download until a seeding limit is
    /// reached or the job is cancelled (either way the download succeeded).
    async fn seed_until_limit(
        &self,
        handle: &Arc<librqbit::ManagedTorrent>,
        ctx: &LoopCtx<'_>,
        size: u64,
    ) {
        let mut limits = Vec::new();
        if let Some(r) = self.seed.ratio {
            limits.push(format!("ratio {r}"));
        }
        if let Some(t) = self.seed.time {
            limits.push(format_eta(t.as_secs_f64()));
        }
        self.emit_status(format!("Seeding until {}", limits.join(" or ")));
        let start = Instant::now();
        loop {
            if self.is_cancelled() {
                self.emit_status("Seeding stopped");
                return;
            }
            let stats = handle.stats();
            if let Some(error) = &stats.error {
                self.emit_status(format!("Seeding stopped: {error}"));
                return;
            }
            let mut s = self.with_limits(swarm_stats(
                &stats,
                size,
                size,
                (ctx.selected.map(|s| s.len()), ctx.file_infos.len()),
            ));
            s.seeding = true;
            s.seeding_secs = Some(start.elapsed().as_secs());
            let up = humansize::format_size(s.upload_bps, humansize::BINARY);
            ctx.progress_bar.set_message(format!(
                "{} - seeding, ratio {:.2}, {} peers, up {}/s",
                ctx.name, s.ratio, s.peers, up
            ));
            if !self.quiet {
                ctx.progress_bar.suspend(|| {
                    println!(
                        "SEEDING: ratio {:.2} UP: {}/s PEERS: {}",
                        s.ratio, up, s.peers
                    )
                });
            }
            if let Some(cb) = &self.stats_cb {
                cb(s);
            }
            match self
                .seed
                .should_stop(stats.uploaded_bytes, size, start.elapsed())
            {
                Some(SeedStop::Ratio) => {
                    self.emit_status("Seed ratio reached");
                    return;
                }
                Some(SeedStop::Time) => {
                    self.emit_status("Seeding time limit reached");
                    return;
                }
                None => {}
            }
            tokio::time::sleep(Duration::from_millis(PROGRESS_UPDATE_INTERVAL_MS)).await;
        }
    }

    fn compute_stats(
        &self,
        stats: &librqbit::TorrentStats,
        file_infos: &FileInfoList,
        selected: Option<&BTreeSet<usize>>,
        last_bytes: u64,
        last_time: Instant,
    ) -> DownloadStats {
        let (downloaded, total) = calculate_progress(
            file_infos,
            &stats.file_progress,
            selected,
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
    downloader.stats_cb = opts.on_stats;
    downloader.set_selection(opts.selection);
    downloader.set_seed_limits(opts.seed);

    if let Some(status) = callbacks.status {
        downloader.set_status_callback(move |s| status(s));
    }

    if let Some(progress) = callbacks.progress {
        downloader.set_progress_callback(move |p| progress(p));
    }

    tokio::runtime::Runtime::new()?.block_on(downloader.download())
}

/// Fetch the metadata of a magnet link or `.torrent` and return its files
/// `(path, size)` in torrent order, without downloading anything.
pub fn list_files_native(
    source: &str,
    config: &TorrentConfig,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<(String, u64)>, BoxError> {
    let cancel = cancel.unwrap_or_default();
    tokio::runtime::Runtime::new()?.block_on(async {
        let opts = SessionOptions {
            dht: dht_options(config.dht_enabled, config.port),
            persistence: None,
            ..Default::default()
        };
        let session = Session::new_with_opts(std::env::temp_dir(), opts)
            .await
            .map_err(|e| format!("Failed to create torrent session: {}", e))?;
        let add = add_source(source)?;
        let listed = tokio::time::timeout(
            Duration::from_secs(METADATA_TIMEOUT_SECS),
            cancellable(&cancel, async {
                session
                    .add_torrent(
                        add,
                        Some(AddTorrentOptions {
                            list_only: true,
                            ..Default::default()
                        }),
                    )
                    .await
                    .map_err(|e| BoxError::from(format!("Failed to read torrent: {e}")))
            }),
        )
        .await;
        session.stop().await;
        match listed {
            Ok(Ok(AddTorrentResponse::ListOnly(resp))) => listed_files(&resp),
            Ok(Ok(_)) => Err("Torrent metadata was not returned".into()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Timeout waiting for torrent metadata. No peers found.".into()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_counts_only_selected_files() {
        let files: FileInfoList = vec![("a".into(), 100), ("b".into(), 50), ("c".into(), 10)];
        let done = [100, 25, 0];
        assert_eq!(calculate_progress(&files, &done, None, (0, 0)), (125, 160));
        let sel: BTreeSet<usize> = [1, 2].into();
        assert_eq!(
            calculate_progress(&files, &done, Some(&sel), (0, 0)),
            (25, 60)
        );
        assert_eq!(calculate_progress(&Vec::new(), &[], None, (7, 9)), (7, 9));
        assert_eq!(mibps_to_bps(1.0), 1_048_576);
    }

    #[test]
    fn eta_uses_integer_division() {
        assert_eq!(format_eta(119.0), "1m 59s");
        assert_eq!(format_eta(5.4), "5s");
        assert_eq!(format_eta(3_661.0), "1h 1m");
    }
}
