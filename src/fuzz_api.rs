//! Entry points for the `fuzz/` targets and `tests/fuzz_regressions.rs`.
//!
//! Each function feeds untrusted bytes to one parser and panics only when an
//! invariant is broken, so a fuzzer crash is always a real bug. Not a stable
//! API.

// Panicking on a broken invariant is this module's job (the fuzzer and the
// regression test catch it); nothing else in the library calls it.
#![allow(clippy::expect_used, clippy::panic)]

use crate::advanced_download::ResumeState;
use crate::download::SingleStreamState;

/// `true` for a name that is safe as a single path component.
fn is_safe_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 255
        && !name.contains(['/', '\\', '\0'])
}

fn assert_sanitized(input: &str, out: &str) {
    assert!(is_safe_component(out), "unsafe name {out:?} from {input:?}");
    assert!(
        !out.starts_with(|c: char| c == '.' || c.is_whitespace())
            && !out.ends_with(|c: char| c == '.' || c.is_whitespace()),
        "untrimmed name {out:?} from {input:?}"
    );
}

/// Metalink XML parser and manifest file names.
pub fn metalink(data: &[u8]) {
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(doc) = crate::metalink::parse(xml) else {
        return;
    };
    assert!(!doc.files.is_empty());
    for f in &doc.files {
        assert!(!f.urls.is_empty());
        assert!(f.urls.windows(2).all(|w| w[0].priority <= w[1].priority));
        let _ = f.best_hash();
        assert_sanitized(&f.name, &crate::metalink::sanitise_filename(&f.name));
    }
}

/// Magnet / torrent source detection.
pub fn magnet(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    let protocol = crate::protocol::detect(&s);
    if crate::torrent::is_supported_magnet_link(&s) || crate::torrent::is_torrent_file(&s) {
        assert_eq!(protocol, crate::Protocol::Torrent, "{s:?}");
    }
    let _ = crate::torrent::is_supported_torrent_source(&s);
}

/// `Content-Disposition` filename extraction, then sanitizing.
pub fn content_disposition(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    if let Some(name) = crate::download::parse_content_disposition_filename(&s) {
        assert!(!name.is_empty());
        assert_sanitized(&name, &crate::utils::sanitize_filename(&name));
    }
}

/// `validate_filename` and `sanitize_filename`.
pub fn filename(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    if crate::download::validate_filename(&s).is_ok() {
        assert!(is_safe_component(&s), "accepted {s:?}");
    }
    let clean = crate::utils::sanitize_filename(&s);
    assert_sanitized(&s, &clean);
    assert_eq!(crate::utils::sanitize_filename(&clean), clean, "idempotent");
}

/// URL / protocol detection and URL-derived file names.
pub fn url(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    let _ = crate::protocol::detect(&s).as_str();
    let _ = crate::webdav::is_webdav_url(&s);
    let _ = crate::metalink::is_metalink(&s);
    let _ = crate::ytdlp::is_video_url(&s);
    let name = crate::utils::get_filename_from_url_or_default(&s, "download");
    assert!(is_safe_component(&name), "unsafe name {name:?} from {s:?}");
}

/// `.kget` resume state, as read by the turbo and simple downloaders.
pub fn resume_state(data: &[u8]) {
    if let Some(state) = ResumeState::parse(data)
        && state.is_consistent()
    {
        assert!(state.done_bytes() <= state.total_size);
    }
    if let Some(state) = SingleStreamState::parse(data) {
        let _ = state.trusted_prefix();
        assert!(!state.conflicts_with(&state));
    }
}

/// Checksum sidecar files (`SHA256SUMS`, `file.sha256`, BSD format).
pub fn checksum_sidecar(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    let (name, content) = s.split_once('\n').unwrap_or(("file.iso", &s));
    if let Some((algo, hash)) = crate::checksum::parse_sidecar(content, name) {
        assert!(
            hash.len() == algo.hex_len() && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "{algo:?} hash {hash:?}"
        );
    }
}

/// `config.json` and the values derived from it.
pub fn config(data: &[u8]) {
    let Ok(cfg) = serde_json::from_slice::<crate::Config>(data) else {
        return;
    };
    let optimizer = crate::Optimizer::from_config(cfg.optimization.clone());
    assert!((1..=32).contains(&optimizer.max_connections()));
    let json = serde_json::to_vec(&cfg).expect("config serializes");
    serde_json::from_slice::<crate::Config>(&json).expect("config round-trips");
    if let Some(url) = &cfg.proxy.url {
        let _ = crate::http::normalize_proxy_url(url, cfg.proxy.proxy_type);
        let _ = crate::config::ProxyType::from_url(url);
    }
}

/// JSONL events built from untrusted strings stay one valid JSON line.
pub fn jsonl(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    let event = crate::JsonEvent::Error {
        code: s.chars().take(16).collect(),
        message: s.to_string(),
        retryable: data.first().is_some_and(|b| b & 1 == 1),
    };
    let line = event.to_line();
    assert!(!line.contains('\n'));
    let v: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
    assert_eq!(v["message"].as_str(), Some(&*s));
}

/// `--since` / `--until` date bounds of the history query.
pub fn date_bound(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    if let (Ok(start), Ok(end)) = (
        crate::queue::parse_date_bound(&s, false),
        crate::queue::parse_date_bound(&s, true),
    ) {
        assert!(end >= start, "{s:?}: end {end} < start {start}");
    }
}

/// `--checksum <algo>:<hex>` / bare hex specs.
pub fn checksum_spec(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    if let Ok((algo, hex)) = crate::runner::parse_checksum_spec(&s) {
        // `<algo>:<hex>` accepts any length (by design); bare hex is inferred
        // from its length.
        if !s.contains(':') {
            assert_eq!(
                crate::checksum::ChecksumAlgorithm::from_hex_len(&hex),
                Some(algo),
                "{s:?}"
            );
        }
        assert!(
            !hex.is_empty()
                && hex
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
}

/// `-i <file>` URL lists.
pub fn url_list(data: &[u8]) {
    let s = String::from_utf8_lossy(data);
    let urls = crate::runner::parse_url_list(&s);
    assert!(urls.len() <= s.lines().count());
    for u in &urls {
        assert!(
            !u.is_empty() && !u.starts_with('#') && u.trim() == u,
            "{u:?}"
        );
    }
}
