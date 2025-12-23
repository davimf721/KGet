use std::error::Error;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use url::Url;

use transmission_rpc::{
    types::{BasicAuth, Id, TorrentAddArgs, TorrentGetField, TorrentStatus},
    TransClient,
};

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::torrent::settings::TransmissionSettings;
use crate::torrent::TorrentCallbacks;
use crate::utils::print;

pub struct TorrentDownloader {
    url: String,
    output: String,
    quiet: bool,
    _proxy: ProxyConfig,
    optimizer: Optimizer,
    status_cb: Option<Arc<dyn Fn(String) + Send + Sync>>,
    progress_cb: Option<Arc<dyn Fn(f32) + Send + Sync>>,
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
            (Some(u), Some(p)) if !u.is_empty() => Some(BasicAuth { user: u, password: p }),
            _ => None,
        };

        (s.rpc_url(), auth, s.web_url())
    }

    pub async fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        
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

        let args = TorrentAddArgs {
            filename: Some(self.url.clone()),
            download_dir: Some(self.output.clone()),
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
                return Ok(());
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

        let _ = Self::open_url_system(&transmission_web_url);

        let progress = create_progress_bar(self.quiet, "Downloading torrent".to_string(), None, false);

        let mut attempt_count = 0u32;
        let max_attempts = 1800u32;

        loop {
            if attempt_count >= max_attempts {
                progress.finish_with_message("Download timeout or stalled.");
                return Err("Download timeout after 30 minutes or torrent stalled".into());
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
                    ]),
                    Some(vec![torrent_id.clone()]),
                )
                .await?;

            let Some(t) = torrent_info.arguments.torrents.first() else {
                progress.abandon_with_message("Torrent info not found.");
                return Err("Torrent info not found after adding.".into());
            };

            let percent_done = t.percent_done.unwrap_or(0.0).clamp(0.0, 1.0);
            self.emit_progress(percent_done);
            progress.set_position((percent_done * 100.0) as u64);

            if let Some(name) = &t.name {
                let speed_kb = t.rate_download.map_or(0, |rate| rate / 1024);
                let msg = format!("{} - {:.2}% - {} KB/s", name, percent_done * 100.0, speed_kb);
                progress.set_message(msg.clone());
                if attempt_count % 2 == 0 {
                    self.emit_status(msg);
                }
            }

            if let Some(error_code) = t.error {
                if (error_code as i32) != 0 {
                    let error_message = t.error_string.as_deref().unwrap_or("Unknown torrent error");
                    progress.abandon_with_message(format!("Torrent error: {}", error_message));
                    return Err(format!("Torrent error (code {:?}): {}", error_code, error_message).into());
                }
            }

            if percent_done >= 1.0 {
                self.emit_progress(1.0);
                break;
            }

            if let Some(status) = t.status {
                if matches!(status, TorrentStatus::Stopped) && attempt_count > 5 && percent_done < 1.0 {
                    progress.abandon_with_message("Torrent stopped and not progressing.");
                    return Err("Torrent stopped and not progressing.".into());
                }
            }

            attempt_count += 1;
            sleep(Duration::from_secs(1)).await;
        }

        progress.finish_with_message("Torrent completed successfully!");
        self.emit_status("Torrent completed successfully!".to_string());
        Ok(())
    }
}

pub fn download_via_transmission(
    magnet: &str,
    output_dir: &str,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,
    cb: TorrentCallbacks,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut d = TorrentDownloader::new(
        magnet.to_string(),
        output_dir.to_string(),
        quiet,
        proxy,
        optimizer,
    );

    if let Some(status) = cb.status {
        d.set_status_callback(move |s| status(s));
    }
    if let Some(progress) = cb.progress {
        d.set_progress_callback(move |p| progress(p));
    }

    tokio::runtime::Runtime::new()?.block_on(d.download())
}