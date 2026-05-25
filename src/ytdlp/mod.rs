//! yt-dlp integration for downloading video content.
//!
//! Routes YouTube, Vimeo, and other video platform URLs through the `yt-dlp`
//! command-line tool (or `youtube-dl` as a fallback) when available.

use std::error::Error;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

/// Known video platform URL patterns.
const VIDEO_HOSTS: &[&str] = &[
    "youtube.com/watch",
    "youtube.com/shorts",
    "youtu.be/",
    "vimeo.com/",
    "dailymotion.com/video",
    "twitch.tv/",
    "twitter.com/i/status",
    "x.com/i/status",
    "instagram.com/reel",
    "instagram.com/p/",
    "tiktok.com/",
    "facebook.com/watch",
    "facebook.com/videos",
    "bilibili.com/video",
    "rumble.com/v",
    "odysee.com/@",
];

/// Returns `true` if the URL points to a known video platform.
pub fn is_video_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    VIDEO_HOSTS.iter().any(|h| lower.contains(h))
}

/// Returns the name of the yt-dlp binary found in PATH, or `None`.
pub fn ytdlp_binary() -> Option<String> {
    for candidate in &["yt-dlp", "youtube-dl"] {
        let ok = Command::new("which")
            .arg(candidate)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(candidate.to_string());
        }
    }
    // Also try running the binary directly (in case `which` isn't available)
    for candidate in &["yt-dlp", "youtube-dl"] {
        let ok = Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Returns `true` if yt-dlp or youtube-dl is available in PATH.
pub fn ytdlp_available() -> bool {
    ytdlp_binary().is_some()
}

/// Quality preset for video downloads.
///
/// Passed to yt-dlp via `-f <format>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoQuality {
    Best,
    P1080,
    P720,
    P480,
    P360,
    AudioOnly,
    Custom(String),
}

impl VideoQuality {
    pub fn from_str(s: &str) -> Self {
        match s {
            "1080p" | "1080" => VideoQuality::P1080,
            "720p" | "720"   => VideoQuality::P720,
            "480p" | "480"   => VideoQuality::P480,
            "360p" | "360"   => VideoQuality::P360,
            "audio"          => VideoQuality::AudioOnly,
            "best" | ""      => VideoQuality::Best,
            other            => VideoQuality::Custom(other.to_string()),
        }
    }

    pub fn yt_dlp_format(&self) -> &str {
        match self {
            VideoQuality::Best      => "bestvideo+bestaudio/best",
            VideoQuality::P1080     => "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best",
            VideoQuality::P720      => "bestvideo[height<=720]+bestaudio/best[height<=720]/best",
            VideoQuality::P480      => "bestvideo[height<=480]+bestaudio/best[height<=480]/best",
            VideoQuality::P360      => "bestvideo[height<=360]+bestaudio/best[height<=360]/best",
            VideoQuality::AudioOnly => "bestaudio/best",
            VideoQuality::Custom(f) => f,
        }
    }
}

impl Default for VideoQuality {
    fn default() -> Self { VideoQuality::Best }
}

/// Download a video URL using yt-dlp.
///
/// Streams yt-dlp's progress lines through `status_cb` so callers can
/// forward them to a progress bar or GUI.
pub fn download_video<F>(
    url: &str,
    output_dir: &str,
    quality: &VideoQuality,
    quiet: bool,
    status_cb: Option<F>,
) -> Result<(), Box<dyn Error + Send + Sync>>
where
    F: Fn(String) + Send + Sync + 'static,
{
    let bin = ytdlp_binary().ok_or(
        "yt-dlp not found. Install with: brew install yt-dlp  (or pip install yt-dlp)"
    )?;

    let output_template = format!(
        "{}/%(title)s.%(ext)s",
        output_dir.trim_end_matches('/')
    );

    let mut cmd = Command::new(&bin);
    cmd.arg("-f").arg(quality.yt_dlp_format())
        .arg("-o").arg(&output_template)
        .arg("--merge-output-format").arg("mp4")
        .arg("--no-playlist")
        .arg("--newline");   // one progress line per update

    if quiet {
        cmd.arg("--quiet").arg("--no-warnings");
    } else {
        cmd.arg("--progress");
    }

    cmd.arg(url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to start {bin}: {e}"))?;

    // Stream stdout lines (progress)
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(ref cb) = status_cb {
                cb(line);
            }
        }
    }

    let status = child.wait()
        .map_err(|e| format!("yt-dlp process error: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "yt-dlp exited with code {}",
            status.code().unwrap_or(-1)
        ).into())
    }
}
