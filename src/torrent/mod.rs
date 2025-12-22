use std::error::Error;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;
use url::Url;
use transmission_rpc::{
    types::{BasicAuth, TorrentAddArgs, Id, TorrentGetField, TorrentStatus},
    TransClient,
};

use crate::config::ProxyConfig;
use crate::optimization::Optimizer;
use crate::progress::create_progress_bar;
use crate::utils::print;

pub struct TorrentDownloader {
    url: String,
    output: String,
    quiet: bool,
    proxy: ProxyConfig,
    optimizer: Optimizer,

    // Optional callbacks (useful for GUI)
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
            proxy,
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
            // "start" is a shell builtin, so we must call via cmd
            Command::new("cmd")
                .args(["/C", "start", "", url])
                .spawn()?;
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

    fn transmission_settings_from_env() -> (String, Option<BasicAuth>) {
        let url = std::env::var("KGET_TRANSMISSION_URL")
            .unwrap_or_else(|_| "http://localhost:9091/transmission/rpc".to_string());

        let user = std::env::var("KGET_TRANSMISSION_USER").ok();
        let pass = std::env::var("KGET_TRANSMISSION_PASS").ok();

        let auth = match (user, pass) {
            (Some(u), Some(p)) if !u.is_empty() => Some(BasicAuth { user: u, password: p }),
            _ => None,
        };

        (url, auth)
    }

    pub async fn download(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // NOTE: proxy config is NOT a Transmission RPC URL. Keep proxy for future peer/rpc proxying if needed.
        let (transmission_url, auth) = Self::transmission_settings_from_env();

        let url = Url::parse(&transmission_url).map_err(|e| {
            format!(
                "Invalid Transmission RPC URL '{}': {}. Set KGET_TRANSMISSION_URL.",
                transmission_url, e
            )
        })?;

        // Create Transmission RPC client (with auth only if provided)
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
                    "Failed to reach Transmission RPC at {}. \
Ensure Transmission is running and RPC is enabled, or set KGET_TRANSMISSION_URL. \
Falling back to opening magnet in your default torrent client. Details: {}",
                    transmission_url, e
                );
                self.emit_status(msg);

                // Fallback: open magnet in system default handler
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

        // Open Transmission web UI (best-effort)
        let transmission_web_url = std::env::var("KGET_TRANSMISSION_WEB")
            .unwrap_or_else(|_| "http://localhost:9091/transmission/web/".to_string());
        let _ = Self::open_url_system(&transmission_web_url);

        // CLI progress bar (GUI uses callback)
        let progress = create_progress_bar(self.quiet, "Downloading torrent".to_string(), None, false);

        let mut attempt_count = 0u32;
        let max_attempts = 1800u32; // 30 minutes

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
                        TorrentGetField::Eta,
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
                // Keep GUI status useful but not too spammy
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
                progress.set_message(format!("{} - Complete", t.name.as_deref().unwrap_or("Torrent")));
                self.emit_progress(1.0);
                break;
            }

            if let Some(status) = t.status {
                if matches!(status, TorrentStatus::Stopped) && attempt_count > 5 && percent_done < 1.0 {
                    progress.abandon_with_message("Torrent stopped and not progressing.");
                    return Err(format!(
                        "Torrent '{}' stopped and not progressing.",
                        t.name.as_deref().unwrap_or("Unknown")
                    ).into());
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
