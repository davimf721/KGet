use std::error::Error;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use url::Url;

use transmission_rpc::{
    TransClient,
    types::{
        BasicAuth, Id, RatioMode, Torrent, TorrentAddArgs, TorrentGetField, TorrentSetArgs,
        TorrentStatus,
    },
};

use crate::config::ProxyConfig;
use crate::events::{TorrentStats, TrackerStatus};
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::torrent::settings::TransmissionSettings;
use crate::torrent::{TorrentCallbacks, TorrentOptions, TorrentOutcome};
use crate::utils::print;

pub struct TorrentDownloader {
    url: String,
    output: String,
    quiet: bool,
    _proxy: ProxyConfig,
    optimizer: Optimizer,
    status_cb: Option<Arc<dyn Fn(String) + Send + Sync>>,
    progress_cb: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    opts: TorrentOptions,
}

/// `torrent_stats` payload from a Transmission `torrent-get` reply.
fn swarm_stats(t: &Torrent) -> TorrentStats {
    let n = |v: Option<i64>| v.unwrap_or(0).max(0) as u64;
    let trackers = t
        .tracker_stats
        .iter()
        .flatten()
        .map(|ts| TrackerStatus {
            url: ts.announce.clone(),
            status: format!("{:?}", ts.announce_state).to_lowercase(),
            seeders: (ts.seeder_count >= 0).then_some(ts.seeder_count as u64),
            leechers: (ts.leecher_count >= 0).then_some(ts.leecher_count as u64),
            message: (!ts.last_announce_result.is_empty()).then(|| ts.last_announce_result.clone()),
        })
        .collect();
    let size = n(t.size_when_done);
    let left = n(t.left_until_done);
    TorrentStats {
        peers: n(t.peers_connected),
        // Peers we download from have (at least part of) what we need;
        // Transmission does not report seeders among connected peers.
        seeders: None,
        leechers: t.peers_getting_from_us.map(|v| v.max(0) as u64),
        download_bps: n(t.rate_download),
        upload_bps: n(t.rate_upload),
        downloaded: size.saturating_sub(left),
        uploaded: n(t.uploaded_ever),
        size,
        ratio: t.upload_ratio.map(|r| r.max(0.0) as f64).unwrap_or(0.0),
        seeding: matches!(t.status, Some(TorrentStatus::Seeding)),
        seeding_secs: t.seconds_seeding.map(|s| s.max(0) as u64),
        seed_ratio_limit: t
            .seed_ratio_limit
            .filter(|_| matches!(t.seed_ratio_mode, Some(RatioMode::Single)))
            .map(f64::from),
        files_total: t.files.as_ref().map(Vec::len),
        trackers,
        ..Default::default()
    }
}

impl TorrentDownloader {
    pub fn new(
        url: String,
        output: String,
        quiet: bool,
        proxy: ProxyConfig,
        optimizer: Optimizer,
    ) -> Self {
        Self {
            url,
            output,
            quiet,
            _proxy: proxy,
            optimizer,
            status_cb: None,
            progress_cb: None,
            opts: TorrentOptions::default(),
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

    fn open_url_system(url: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        #[cfg(windows)]
        {
            Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open").arg(url).spawn()?;
            return Ok(());
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Command::new("xdg-open").arg(url).spawn()?;
            return Ok(());
        }
        #[allow(unreachable_code)]
        Err("Unsupported platform for opening URLs".into())
    }

    fn transmission_settings() -> (String, Option<BasicAuth>, String) {
        let s = TransmissionSettings::from_env();

        let auth = match (s.username.clone(), s.password.clone()) {
            (Some(u), Some(p)) if !u.is_empty() => Some(BasicAuth {
                user: u,
                password: p,
            }),
            _ => None,
        };

        (s.rpc_url(), auth, s.web_url())
    }

    pub async fn download(&self) -> Result<TorrentOutcome, Box<dyn Error + Send + Sync>> {
        let (transmission_url, auth, transmission_web_url): (String, Option<BasicAuth>, String) =
            Self::transmission_settings();

        let url = Url::parse(&transmission_url).map_err(|e| {
            format!(
                "Invalid Transmission RPC URL '{}': {}. Set KGET_TRANSMISSION_HOST/PORT or KGET_TRANSMISSION_URL.",
                transmission_url, e
            )
        })?;

        let mut client = if let Some(auth) = auth {
            TransClient::with_auth(url, auth)
        } else {
            TransClient::new(url)
        };

        self.emit_status(format!("Adding torrent: {}", self.url));

        // The daemon resolves relative paths from its own working directory.
        let _ = std::fs::create_dir_all(&self.output);
        let download_dir = std::fs::canonicalize(&self.output)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| self.output.clone());

        let args = TorrentAddArgs {
            filename: Some(self.url.clone()),
            download_dir: Some(download_dir),
            paused: Some(false),
            peer_limit: Some(self.optimizer.get_peer_limit() as i64),
            ..Default::default()
        };

        let response = match client.torrent_add(args).await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!(
                    "Failed to reach Transmission RPC at {}. Falling back to opening magnet in your default torrent client. Details: {}",
                    transmission_url, e
                );
                self.emit_status(msg);

                Self::open_url_system(&self.url)?;
                return Ok(TorrentOutcome::HandedOff);
            }
        };

        let torrent_id = match &response.arguments {
            transmission_rpc::types::TorrentAddedOrDuplicate::TorrentAdded(added) => {
                added.id.map(Id::Id).ok_or_else(|| {
                    Box::<dyn Error + Send + Sync>::from("TorrentAdded response missing ID")
                })?
            }
            transmission_rpc::types::TorrentAddedOrDuplicate::TorrentDuplicate(duplicate) => {
                duplicate.id.map(Id::Id).ok_or_else(|| {
                    Box::<dyn Error + Send + Sync>::from("TorrentDuplicate response missing ID")
                })?
            }
            _ => {
                return Err(Box::<dyn Error + Send + Sync>::from(
                    "Failed to get torrent ID from response",
                ));
            }
        };

        // Opening the web UI is opt-in.
        if std::env::var("KGET_TRANSMISSION_OPEN_WEB").is_ok_and(|v| v == "1") {
            let _ = Self::open_url_system(&transmission_web_url);
        }

        // The daemon enforces the seed ratio itself (it keeps seeding after
        // KGet returns); it has no seeding-time limit, only an idle limit.
        if let Some(ratio) = self.opts.seed.ratio {
            let mut args = TorrentSetArgs::default();
            args.seed_ratio_limit = Some(ratio);
            args.seed_ratio_mode = Some(RatioMode::Single);
            if let Err(e) = client
                .torrent_set(args, Some(vec![torrent_id.clone()]))
                .await
            {
                self.emit_status(format!("Could not set the seed ratio: {e}"));
            }
        }
        if self.opts.seed.time.is_some() {
            self.emit_status("--seed-time is not supported by the Transmission backend");
        }
        let mut selection_applied = self.opts.selection.is_empty();

        let progress =
            create_progress_bar(self.quiet, "Downloading torrent".to_string(), None, false);

        // Fail only when there is no progress at all for this long.
        let stall_limit = std::env::var("KGET_TRANSMISSION_STALL_MINUTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30);
        let stall_limit = Duration::from_secs(stall_limit.max(1) * 60);
        let mut last_percent = -1.0f32;
        let mut last_change = std::time::Instant::now();
        let mut attempt_count = 0u32;

        loop {
            if last_change.elapsed() >= stall_limit {
                progress.finish_with_message("Download stalled.");
                return Err(format!(
                    "Torrent made no progress for {} minutes",
                    stall_limit.as_secs() / 60
                )
                .into());
            }

            let torrent_info = client
                .torrent_get(
                    Some(vec![
                        TorrentGetField::PercentDone,
                        TorrentGetField::Status,
                        TorrentGetField::Name,
                        TorrentGetField::RateDownload,
                        TorrentGetField::Error,
                        TorrentGetField::ErrorString,
                        TorrentGetField::Files,
                        TorrentGetField::PeersConnected,
                        TorrentGetField::PeersGettingFromUs,
                        TorrentGetField::RateUpload,
                        TorrentGetField::UploadRatio,
                        TorrentGetField::UploadedEver,
                        TorrentGetField::SizeWhenDone,
                        TorrentGetField::LeftUntilDone,
                        TorrentGetField::SecondsSeeding,
                        TorrentGetField::SeedRatioLimit,
                        TorrentGetField::SeedRatioMode,
                        TorrentGetField::TrackerStats,
                    ]),
                    Some(vec![torrent_id.clone()]),
                )
                .await?;

            let Some(t) = torrent_info.arguments.torrents.first() else {
                progress.abandon_with_message("Torrent info not found.");
                return Err("Torrent info not found after adding.".into());
            };

            // The file list of a magnet appears once the metadata arrives.
            if !selection_applied && let Some(files) = t.files.as_ref().filter(|f| !f.is_empty()) {
                let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
                if let Some(wanted) = self.opts.selection.resolve(&names)? {
                    let unwanted: Vec<usize> =
                        (0..names.len()).filter(|i| !wanted.contains(i)).collect();
                    let mut args = TorrentSetArgs::default();
                    args.files_unwanted = Some(unwanted);
                    client
                        .torrent_set(args, Some(vec![torrent_id.clone()]))
                        .await?;
                    self.emit_status(format!(
                        "Selected {} of {} files",
                        wanted.len(),
                        names.len()
                    ));
                }
                selection_applied = true;
            }
            if let Some(cb) = &self.opts.on_stats {
                cb(swarm_stats(t));
            }

            let percent_done = t.percent_done.unwrap_or(0.0).clamp(0.0, 1.0);
            if percent_done > last_percent {
                last_percent = percent_done;
                last_change = std::time::Instant::now();
            }
            self.emit_progress(percent_done);
            progress.set_position((percent_done * 100.0) as u64);

            if let Some(name) = &t.name {
                let speed_kb = t.rate_download.map_or(0, |rate| rate / 1024);
                let msg = format!(
                    "{} - {:.2}% - {} KB/s",
                    name,
                    percent_done * 100.0,
                    speed_kb
                );
                progress.set_message(msg.clone());
                if attempt_count.is_multiple_of(2) {
                    self.emit_status(msg);
                }
            }

            if let Some(error_code) = t.error
                && (error_code as i32) != 0
            {
                let error_message = t.error_string.as_deref().unwrap_or("Unknown torrent error");
                progress.abandon_with_message(format!("Torrent error: {}", error_message));
                return Err(
                    format!("Torrent error (code {:?}): {}", error_code, error_message).into(),
                );
            }

            if percent_done >= 1.0 {
                self.emit_progress(1.0);
                break;
            }

            if let Some(status) = t.status
                && matches!(status, TorrentStatus::Stopped)
                && attempt_count > 5
                && percent_done < 1.0
            {
                progress.abandon_with_message("Torrent stopped and not progressing.");
                return Err("Torrent stopped and not progressing.".into());
            }

            attempt_count += 1;
            sleep(Duration::from_secs(1)).await;
        }

        progress.finish_with_message("Torrent completed successfully!");
        self.emit_status("Torrent completed successfully!".to_string());
        Ok(TorrentOutcome::Downloaded)
    }
}

pub fn download_via_transmission(
    magnet: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    cb: TorrentCallbacks,
    opts: TorrentOptions,
) -> Result<TorrentOutcome, Box<dyn Error + Send + Sync>> {
    let mut d = TorrentDownloader::new(
        magnet.to_string(),
        output_dir.to_string(),
        quiet,
        proxy,
        optimizer,
    );
    d.opts = opts;

    if let Some(status) = cb.status {
        d.set_status_callback(move |s| status(s));
    }
    if let Some(progress) = cb.progress {
        d.set_progress_callback(move |p| progress(p));
    }

    tokio::runtime::Runtime::new()?.block_on(d.download())
}
