//! Adaptive HTTP Live Streaming (HLS) manifest parser and segment stitcher.
//!
//! Parses HLS master and media playlists (.m3u8), resolves relative segment
//! URLs, selects optimal stream variants, and stitches segments into a unified
//! output media stream.

use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use url::Url;

/// Represents a variant stream within an HLS master playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlsVariant {
    /// Absolute URI of the variant media playlist.
    pub uri: String,
    /// Bitrate / bandwidth in bits per second.
    pub bandwidth: Option<u64>,
    /// Video resolution (width, height) if specified.
    pub resolution: Option<(u32, u32)>,
}

/// Represents a media chunk segment in an HLS stream.
#[derive(Debug, Clone, PartialEq)]
pub struct HlsSegment {
    /// Absolute URI of the segment file (.ts, .m4s).
    pub uri: String,
    /// Segment playback duration in seconds.
    pub duration_secs: f64,
}

/// Parsed HLS playlist type.
#[derive(Debug, Clone, PartialEq)]
pub enum HlsPlaylist {
    /// Master playlist containing multiple quality variants.
    Master(Vec<HlsVariant>),
    /// Media playlist containing sequential media segments.
    Media(Vec<HlsSegment>),
}

/// Check if a URL points to an HLS manifest.
pub fn is_hls_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let clean = lower.split(['?', '#']).next().unwrap_or(&lower);
    clean.ends_with(".m3u8") || lower.contains(".m3u8")
}

/// Resolve a relative segment or variant URI against a base manifest URL.
pub fn resolve_relative_url(base: &str, relative: &str) -> String {
    if relative.contains("://") {
        return relative.to_string();
    }
    if let Ok(base_parsed) = Url::parse(base)
        && let Ok(joined) = base_parsed.join(relative)
    {
        return joined.to_string();
    }
    relative.to_string()
}

/// Parse an HLS playlist (.m3u8 content) into a master or media playlist.
pub fn parse_m3u8(content: &str, base_url: &str) -> Result<HlsPlaylist, String> {
    let lines: Vec<&str> = content.lines().map(str::trim).collect();
    if lines.is_empty() || !lines[0].starts_with("#EXTM3U") {
        return Err("Invalid M3U8: missing #EXTM3U header".into());
    }

    let is_master = lines.iter().any(|l| l.starts_with("#EXT-X-STREAM-INF:"));

    if is_master {
        let mut variants = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            if let Some(meta) = line.strip_prefix("#EXT-X-STREAM-INF:") {
                let mut bandwidth = None;
                let mut resolution = None;

                for part in meta.split(',') {
                    let part = part.trim();
                    if let Some(bw_str) = part.strip_prefix("BANDWIDTH=") {
                        bandwidth = bw_str.parse::<u64>().ok();
                    } else if let Some(res_str) = part.strip_prefix("RESOLUTION=")
                        && let Some((w_str, h_str)) = res_str.split_once('x')
                        && let (Ok(w), Ok(h)) = (w_str.parse::<u32>(), h_str.parse::<u32>())
                    {
                        resolution = Some((w, h));
                    }
                }

                // Next non-empty, non-comment line is the URI
                let mut j = i + 1;
                while j < lines.len() && (lines[j].is_empty() || lines[j].starts_with('#')) {
                    j += 1;
                }
                if j < lines.len() {
                    let uri = resolve_relative_url(base_url, lines[j]);
                    variants.push(HlsVariant {
                        uri,
                        bandwidth,
                        resolution,
                    });
                    i = j;
                }
            }
            i += 1;
        }
        Ok(HlsPlaylist::Master(variants))
    } else {
        let mut segments = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            if let Some(inf) = line.strip_prefix("#EXTINF:") {
                let dur_str = inf.split(',').next().unwrap_or("0");
                let duration_secs = dur_str.trim().parse::<f64>().unwrap_or(0.0);

                let mut j = i + 1;
                while j < lines.len() && (lines[j].is_empty() || lines[j].starts_with('#')) {
                    j += 1;
                }
                if j < lines.len() {
                    let uri = resolve_relative_url(base_url, lines[j]);
                    segments.push(HlsSegment { uri, duration_secs });
                    i = j;
                }
            }
            i += 1;
        }
        Ok(HlsPlaylist::Media(segments))
    }
}

/// Select variant with the highest bandwidth/resolution.
pub fn select_best_variant(variants: &[HlsVariant]) -> Option<&HlsVariant> {
    variants.iter().max_by_key(|v| v.bandwidth.unwrap_or(0))
}

/// Download all segments of an HLS playlist and stitch them into the destination file.
pub fn download_hls_stream(
    manifest_url: &str,
    dest_path: &Path,
    client: &reqwest::blocking::Client,
    extra_headers: &[(String, String)],
    progress_callback: Option<&dyn Fn(usize, usize)>,
) -> Result<u64, Box<dyn Error + Send + Sync>> {
    let mut req = client.get(manifest_url);
    req = crate::http::apply_headers(req, extra_headers);
    let manifest_body = req.send()?.text()?;

    let playlist = parse_m3u8(&manifest_body, manifest_url)
        .map_err(|e| format!("Failed to parse M3U8 manifest: {e}"))?;

    let segments = match playlist {
        HlsPlaylist::Media(segs) => segs,
        HlsPlaylist::Master(variants) => {
            let best = select_best_variant(&variants)
                .ok_or_else(|| "No stream variants found in master playlist".to_string())?;
            let mut variant_req = client.get(&best.uri);
            variant_req = crate::http::apply_headers(variant_req, extra_headers);
            let variant_body = variant_req.send()?.text()?;
            match parse_m3u8(&variant_body, &best.uri)
                .map_err(|e| format!("Failed to parse variant M3U8: {e}"))?
            {
                HlsPlaylist::Media(segs) => segs,
                HlsPlaylist::Master(_) => {
                    return Err("Nested master playlists are not supported".into());
                }
            }
        }
    };

    if segments.is_empty() {
        return Err("HLS stream contains no segments".into());
    }

    let mut output_file = File::create(dest_path)?;
    let mut total_bytes = 0u64;
    let total_segments = segments.len();

    for (idx, segment) in segments.iter().enumerate() {
        let mut seg_req = client.get(&segment.uri);
        seg_req = crate::http::apply_headers(seg_req, extra_headers);
        let resp = seg_req.send()?;
        if !resp.status().is_success() {
            return Err(format!(
                "Failed to download segment {} ({}): HTTP {}",
                idx + 1,
                segment.uri,
                resp.status()
            )
            .into());
        }
        let bytes = resp.bytes()?;
        output_file.write_all(&bytes)?;
        total_bytes += bytes.len() as u64;

        if let Some(cb) = progress_callback {
            cb(idx + 1, total_segments);
        }
    }

    output_file.flush()?;
    Ok(total_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hls_master_playlist() {
        let master = "#EXTM3U\n\
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=720x480\n\
low.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=2560000,RESOLUTION=1280x720\n\
mid.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n\
high.m3u8\n";

        let playlist = parse_m3u8(master, "https://cdn.example.com/live/master.m3u8").unwrap();
        match playlist {
            HlsPlaylist::Master(variants) => {
                assert_eq!(variants.len(), 3);
                assert_eq!(variants[0].bandwidth, Some(1280000));
                assert_eq!(variants[0].resolution, Some((720, 480)));
                assert_eq!(variants[0].uri, "https://cdn.example.com/live/low.m3u8");

                let best = select_best_variant(&variants).unwrap();
                assert_eq!(best.bandwidth, Some(5000000));
                assert_eq!(best.uri, "https://cdn.example.com/live/high.m3u8");
            }
            HlsPlaylist::Media(_) => panic!("Expected master playlist"),
        }
    }

    #[test]
    fn parse_hls_media_playlist() {
        let media = "#EXTM3U\n\
#EXT-X-TARGETDURATION:10\n\
#EXTINF:9.009,\n\
segment_0.ts\n\
#EXTINF:9.009,\n\
segment_1.ts\n\
#EXT-X-ENDLIST\n";

        let playlist = parse_m3u8(media, "https://cdn.example.com/vod/playlist.m3u8").unwrap();
        match playlist {
            HlsPlaylist::Media(segments) => {
                assert_eq!(segments.len(), 2);
                assert_eq!(segments[0].duration_secs, 9.009);
                assert_eq!(segments[0].uri, "https://cdn.example.com/vod/segment_0.ts");
                assert_eq!(segments[1].uri, "https://cdn.example.com/vod/segment_1.ts");
            }
            HlsPlaylist::Master(_) => panic!("Expected media playlist"),
        }
    }

    #[test]
    fn relative_url_resolution() {
        assert_eq!(
            resolve_relative_url("https://example.com/stream/index.m3u8", "chunk_1.ts"),
            "https://example.com/stream/chunk_1.ts"
        );
        assert_eq!(
            resolve_relative_url(
                "https://example.com/stream/index.m3u8",
                "https://other.com/chunk_1.ts"
            ),
            "https://other.com/chunk_1.ts"
        );
    }
}
