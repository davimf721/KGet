//! Lossless media stream remuxer and transmuxer.
//!
//! Merges separate audio and video streams or converts transport stream (.ts)
//! and fragmented MP4 containers into clean MP4/MKV files using direct stream copy
//! (`-c copy`) without quality loss or re-encoding.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Environment variable for an explicit ffmpeg binary path.
pub const FFMPEG_PATH_ENV: &str = "KGET_FFMPEG_PATH";

const CANDIDATES: &[&str] = if cfg!(windows) {
    &["ffmpeg.exe"]
} else {
    &["ffmpeg"]
};

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

/// Search paths for ffmpeg.
pub fn ffmpeg_search_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();

    if cfg!(windows) {
        out.push(PathBuf::from(r"C:\ffmpeg\bin"));
        out.push(PathBuf::from(r"C:\Program Files\ffmpeg\bin"));
    } else {
        out.push(PathBuf::from("/usr/bin"));
        out.push(PathBuf::from("/usr/local/bin"));
        out.push(PathBuf::from("/opt/homebrew/bin"));
    }

    let mut seen = std::collections::HashSet::new();
    out.retain(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()));
    out
}

/// Locate ffmpeg binary on the host system.
pub fn ffmpeg_binary() -> Option<PathBuf> {
    static CACHED: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            if let Some(explicit) = std::env::var_os(FFMPEG_PATH_ENV) {
                let p = PathBuf::from(explicit);
                if is_executable(&p) {
                    return Some(p);
                }
            }

            for dir in ffmpeg_search_dirs() {
                for name in CANDIDATES {
                    let candidate = dir.join(name);
                    if is_executable(&candidate) {
                        return Some(candidate);
                    }
                }
            }
            None
        })
        .clone()
}

/// Check if ffmpeg is available.
pub fn is_ffmpeg_available() -> bool {
    ffmpeg_binary().is_some()
}

/// Options controlling stream remuxing.
#[derive(Debug, Clone)]
pub struct RemuxOptions {
    /// Target output format (e.g., "mp4", "mkv", "ts")
    pub target_format: String,
    /// Apply faststart flag for progressive MP4 web streaming
    pub faststart: bool,
    /// Overwrite destination if it already exists
    pub overwrite: bool,
}

impl Default for RemuxOptions {
    fn default() -> Self {
        Self {
            target_format: "mp4".into(),
            faststart: true,
            overwrite: true,
        }
    }
}

/// Build ffmpeg command line arguments for lossless stream copy.
pub fn build_remux_args(inputs: &[&Path], output: &Path, options: &RemuxOptions) -> Vec<String> {
    let mut args = Vec::new();
    if options.overwrite {
        args.push("-y".to_string());
    }
    for input in inputs {
        args.push("-i".to_string());
        args.push(input.to_string_lossy().into_owned());
    }
    // Lossless direct stream copy
    args.push("-c".to_string());
    args.push("copy".to_string());

    if inputs.len() > 1 {
        // Map video from first input, audio from second
        args.push("-map".to_string());
        args.push("0:v:0?".to_string());
        args.push("-map".to_string());
        args.push("1:a:0?".to_string());
    }

    if options.faststart && options.target_format.eq_ignore_ascii_case("mp4") {
        args.push("-movflags".to_string());
        args.push("+faststart".to_string());
    }

    args.push(output.to_string_lossy().into_owned());
    args
}

/// Remux input media streams into the target container without re-encoding.
pub fn remux_lossless(
    inputs: &[&Path],
    output: &Path,
    options: &RemuxOptions,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let binary = ffmpeg_binary().ok_or_else(|| {
        "ffmpeg was not found in PATH or KGET_FFMPEG_PATH. Install ffmpeg to enable lossless remuxing."
            .to_string()
    })?;

    let args = build_remux_args(inputs, output, options);
    let status = Command::new(binary)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()?;

    if !status.success() {
        return Err(format!("ffmpeg remux failed with status {status}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_input_remux_args() {
        let input = Path::new("input.ts");
        let output = Path::new("output.mp4");
        let opts = RemuxOptions::default();

        let args = build_remux_args(&[input], output, &opts);
        assert_eq!(
            args,
            vec![
                "-y",
                "-i",
                "input.ts",
                "-c",
                "copy",
                "-movflags",
                "+faststart",
                "output.mp4"
            ]
        );
    }

    #[test]
    fn dual_input_video_audio_merging_args() {
        let video = Path::new("video.mp4");
        let audio = Path::new("audio.m4a");
        let output = Path::new("final.mkv");
        let opts = RemuxOptions {
            target_format: "mkv".into(),
            faststart: false,
            overwrite: true,
        };

        let args = build_remux_args(&[video, audio], output, &opts);
        assert_eq!(
            args,
            vec![
                "-y",
                "-i",
                "video.mp4",
                "-i",
                "audio.m4a",
                "-c",
                "copy",
                "-map",
                "0:v:0?",
                "-map",
                "1:a:0?",
                "final.mkv"
            ]
        );
    }
}
