//! yt-dlp integration for downloading video content.
//!
//! Routes YouTube, Vimeo, and other video platform URLs through the `yt-dlp`
//! command-line tool (or `youtube-dl` as a fallback) when available.
//!
//! # Locating yt-dlp
//!
//! GUI apps on macOS start with a minimal `PATH` (`/usr/bin:/bin:…`), so the
//! binary is searched, in order, in:
//!
//! 1. `KGET_YTDLP_PATH` (explicit path, highest priority)
//! 2. `ytdlp_path` in `config.json`
//! 3. every directory of `PATH`
//! 4. `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin`,
//!    `~/.local/bin` and `~/Library/Python/*/bin`
//!
//! The search runs no external process and its result is cached.

use std::collections::VecDeque;
use std::error::Error;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Environment variable with an explicit yt-dlp path (highest priority).
pub const YTDLP_PATH_ENV: &str = "KGET_YTDLP_PATH";

const CANDIDATES: &[&str] = if cfg!(windows) {
    &["yt-dlp.exe", "youtube-dl.exe"]
} else {
    &["yt-dlp", "youtube-dl"]
};

const FALLBACK_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"];

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

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Directories searched, in order: `PATH`, then common install locations.
pub fn ytdlp_search_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();

    out.extend(FALLBACK_DIRS.iter().map(PathBuf::from));
    if let Some(home) = dirs::home_dir() {
        // pipx / pip --user (Linux and macOS)
        out.push(home.join(".local/bin"));
        // pip --user on macOS: ~/Library/Python/3.x/bin
        if let Ok(rd) = std::fs::read_dir(home.join("Library/Python")) {
            for e in rd.flatten() {
                out.push(e.path().join("bin"));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()));
    out
}

/// Locate yt-dlp without caching (see the module docs for the search order).
pub fn locate_ytdlp() -> Option<String> {
    let explicit = std::env::var_os(YTDLP_PATH_ENV)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            crate::config::Config::load()
                .ok()
                .and_then(|c| c.ytdlp_path)
                .filter(|p| !p.trim().is_empty())
                .map(|p| PathBuf::from(crate::utils::expand_tilde(p.trim())))
        });
    if let Some(p) = explicit
        && is_executable(&p)
    {
        return Some(p.to_string_lossy().into_owned());
    }
    ytdlp_search_dirs().into_iter().find_map(|dir| {
        CANDIDATES
            .iter()
            .map(|name| dir.join(name))
            .find(|c| is_executable(c))
            .map(|c| c.to_string_lossy().into_owned())
    })
}

/// Returns the absolute path of yt-dlp (or youtube-dl), or `None`.
///
/// The result is cached for the lifetime of the process.
pub fn ytdlp_binary() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE.get_or_init(locate_ytdlp).clone()
}

/// Error message listing the searched locations and how to fix it.
pub fn ytdlp_not_found_message() -> String {
    let extra = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
        "~/.local/bin",
    ];
    format!(
        "yt-dlp not found. Searched PATH and: {}\n\
         Install with: brew install yt-dlp  (or: pipx install yt-dlp)\n\
         Or set {YTDLP_PATH_ENV}=/path/to/yt-dlp",
        extra.join(", ")
    )
}

/// Returns `true` if yt-dlp or youtube-dl is available in PATH.
pub fn ytdlp_available() -> bool {
    ytdlp_binary().is_some()
}

/// Quality preset for video downloads.
///
/// Passed to yt-dlp via `-f <format>`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VideoQuality {
    #[default]
    Best,
    P1080,
    P720,
    P480,
    P360,
    AudioOnly,
    Custom(String),
}

impl std::str::FromStr for VideoQuality {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "1080p" | "1080" => VideoQuality::P1080,
            "720p" | "720" => VideoQuality::P720,
            "480p" | "480" => VideoQuality::P480,
            "360p" | "360" => VideoQuality::P360,
            "audio" => VideoQuality::AudioOnly,
            "best" | "" => VideoQuality::Best,
            other => VideoQuality::Custom(other.to_string()),
        })
    }
}

impl VideoQuality {
    /// Parse a quality name (`best`, `1080p`, `720p`, `480p`, `360p`, `audio`,
    /// or a raw yt-dlp format string).
    pub fn parse(s: &str) -> Self {
        s.parse().unwrap_or_default()
    }

    pub fn yt_dlp_format(&self) -> &str {
        match self {
            VideoQuality::Best => "bestvideo+bestaudio/best",
            VideoQuality::P1080 => "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best",
            VideoQuality::P720 => "bestvideo[height<=720]+bestaudio/best[height<=720]/best",
            VideoQuality::P480 => "bestvideo[height<=480]+bestaudio/best[height<=480]/best",
            VideoQuality::P360 => "bestvideo[height<=360]+bestaudio/best[height<=360]/best",
            VideoQuality::AudioOnly => "bestaudio/best",
            VideoQuality::Custom(f) => f,
        }
    }
}

/// Progress reported by yt-dlp.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoProgress {
    /// 0.0 – 100.0
    pub percent: f64,
    /// Speed as printed by yt-dlp (e.g. `3.20MiB/s`)
    pub speed: String,
    /// ETA as printed by yt-dlp (e.g. `00:31`)
    pub eta: String,
}

/// Event emitted while yt-dlp runs.
#[derive(Debug, Clone, PartialEq)]
pub enum VideoEvent {
    /// Download progress
    Progress(VideoProgress),
    /// Any other line printed by yt-dlp
    Log(String),
    /// Final file path, after merging / post-processing
    Saved(String),
}

/// Parse one stdout line produced with KGet's `--progress-template` / `--print`.
pub fn parse_ytdlp_line(line: &str) -> VideoEvent {
    if let Some(rest) = line.strip_prefix("KGETPROG ") {
        let mut it = rest.split('|');
        let percent = it
            .next()
            .unwrap_or("")
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse()
            .unwrap_or(0.0);
        return VideoEvent::Progress(VideoProgress {
            percent,
            speed: it.next().unwrap_or("").trim().to_string(),
            eta: it.next().unwrap_or("").trim().to_string(),
        });
    }
    if let Some(path) = line.strip_prefix("KGETSAVED ") {
        return VideoEvent::Saved(path.trim().to_string());
    }
    VideoEvent::Log(line.to_string())
}

/// Download a video URL using yt-dlp and return the path of the saved file.
///
/// stdout is parsed into [`VideoEvent`]s; stderr is drained on a separate
/// thread (so yt-dlp can never block on a full pipe) and its last error line
/// is included in the returned error.
pub fn download_video<F>(
    url: &str,
    output_dir: &str,
    quality: &VideoQuality,
    on_event: F,
) -> Result<String, Box<dyn Error + Send + Sync>>
where
    F: Fn(VideoEvent) + Send + Sync,
{
    download_video_with_cancel(url, output_dir, quality, None, on_event)
}

/// Like [`download_video`]; setting `cancel` to `true` kills yt-dlp and
/// returns [`KgetError::Cancelled`](crate::KgetError::Cancelled).
pub fn download_video_with_cancel<F>(
    url: &str,
    output_dir: &str,
    quality: &VideoQuality,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    on_event: F,
) -> Result<String, Box<dyn Error + Send + Sync>>
where
    F: Fn(VideoEvent) + Send + Sync,
{
    use std::sync::atomic::Ordering;
    let bin = ytdlp_binary().ok_or_else(ytdlp_not_found_message)?;
    let output_dir = crate::utils::expand_tilde(output_dir);
    std::fs::create_dir_all(&output_dir)?;
    let template = Path::new(&output_dir).join("%(title)s.%(ext)s");

    let mut cmd = Command::new(&bin);
    cmd.arg("-f")
        .arg(quality.yt_dlp_format())
        .arg("-o")
        .arg(&template)
        .arg("--no-playlist")
        .arg("--newline")
        .arg("--progress")
        // Easy-to-parse progress line, independent of language or version
        .arg("--progress-template")
        .arg("download:KGETPROG %(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s")
        // Final path, printed after merging/post-processing
        .arg("--print")
        .arg("after_move:KGETSAVED %(filepath)s");
    if *quality == VideoQuality::AudioOnly {
        cmd.arg("-x").arg("--audio-format").arg("m4a");
    } else {
        cmd.arg("--merge-output-format").arg("mp4");
    }
    // "--" keeps a URL starting with "-" from being read as a flag
    cmd.arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start {bin}: {e}"))?;

    let stderr = child.stderr.take().ok_or("yt-dlp stderr unavailable")?;
    let stderr_tail = std::thread::spawn(move || {
        let mut tail = VecDeque::with_capacity(20);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tail.len() == 20 {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        tail.into_iter().collect::<Vec<_>>().join("\n")
    });

    let stdout = child.stdout.take();
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Kill yt-dlp when the download is cancelled (Ctrl+C, Pause in the app).
    let watcher = cancel.clone().map(|cancel| {
        let child = child.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    if let Ok(mut c) = child.lock() {
                        let _ = c.kill();
                    }
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        })
    });

    let mut saved: Option<String> = None;
    if let Some(stdout) = stdout {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let ev = parse_ytdlp_line(&line);
            if let VideoEvent::Saved(p) = &ev {
                saved = Some(p.clone());
            }
            on_event(ev);
        }
    }

    let status = child
        .lock()
        .map_err(|_| "yt-dlp process lock poisoned")?
        .wait()
        .map_err(|e| format!("yt-dlp process error: {e}"))?;
    done.store(true, Ordering::Relaxed);
    if let Some(w) = watcher {
        let _ = w.join();
    }
    let err_tail = stderr_tail.join().unwrap_or_default();
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(Box::new(crate::KgetError::Cancelled));
    }
    if !status.success() {
        let last = err_tail
            .lines()
            .rev()
            .find(|l| l.contains("ERROR"))
            .or_else(|| err_tail.lines().last())
            .unwrap_or("");
        return Err(format!(
            "yt-dlp failed (exit {}): {}",
            status.code().unwrap_or(-1),
            last.trim()
        )
        .into());
    }
    saved.ok_or_else(|| "yt-dlp finished but did not report the output file".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress_and_saved_lines() {
        assert_eq!(
            parse_ytdlp_line("KGETPROG  42.3%|3.20MiB/s|00:31"),
            VideoEvent::Progress(VideoProgress {
                percent: 42.3,
                speed: "3.20MiB/s".into(),
                eta: "00:31".into(),
            })
        );
        assert_eq!(
            parse_ytdlp_line("KGETSAVED /tmp/My Video.mp4"),
            VideoEvent::Saved("/tmp/My Video.mp4".into())
        );
        assert_eq!(
            parse_ytdlp_line("[info] x"),
            VideoEvent::Log("[info] x".into())
        );
    }

    #[test]
    fn quality_from_str() {
        assert_eq!(VideoQuality::parse("720p"), VideoQuality::P720);
        assert_eq!(VideoQuality::parse(""), VideoQuality::Best);
    }
}
