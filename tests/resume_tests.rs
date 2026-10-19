//! Regression tests for 1.7.1: `.part`/`.kget` resume, truncated bodies,
//! transient errors, checksum verification and raw (undecoded) bodies.
//!
//! A tiny blocking HTTP server is used instead of wiremock because these tests
//! need to send bodies shorter than their `Content-Length`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kget::{AdvancedDownloader, Config, DownloadOptions, Optimizer, ProxyConfig};
use sha2::Digest;
use tempfile::TempDir;

#[derive(Clone, Default)]
struct Behavior {
    /// Advertise and honour `Range` requests
    ranges: bool,
    /// Send only this many body bytes on the first body-carrying GET
    truncate_first: Option<usize>,
    /// Truncate every body to this many bytes
    truncate_all: Option<usize>,
    /// Answer the first GET with this status (and `Retry-After: 1`)
    fail_first_with: Option<u16>,
    /// Extra response headers
    headers: Vec<(&'static str, &'static str)>,
}

struct Server {
    base: String,
    /// `Range` header of every GET (empty string when absent)
    ranges: Arc<Mutex<Vec<String>>>,
}

fn serve(body: Vec<u8>, b: Behavior) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let gets = Arc::new(AtomicUsize::new(0));
    let body_gets = Arc::new(AtomicUsize::new(0));
    let body = Arc::new(body);
    {
        let log = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (log, gets, body_gets, body, b) = (
                    log.clone(),
                    gets.clone(),
                    body_gets.clone(),
                    body.clone(),
                    b.clone(),
                );
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).is_err() {
                        return;
                    }
                    let mut range = String::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range:") {
                            range = v.trim().to_string();
                        }
                    }
                    let is_head = request_line.starts_with("HEAD");
                    let mut extra = String::new();
                    for (k, v) in &b.headers {
                        extra.push_str(&format!("{k}: {v}\r\n"));
                    }
                    if b.ranges {
                        extra.push_str("Accept-Ranges: bytes\r\nETag: \"abc\"\r\n");
                    }

                    if is_head {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                            body.len()
                        );
                        return;
                    }

                    log.lock().unwrap().push(range.clone());
                    let n = gets.fetch_add(1, Ordering::SeqCst);
                    if n == 0
                        && let Some(code) = b.fail_first_with
                    {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {code} Busy\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        return;
                    }

                    let (status, start, end) = match parse_range(&range, body.len()) {
                        Some((s, e)) if b.ranges => ("206 Partial Content", s, e),
                        _ => ("200 OK", 0, body.len()),
                    };
                    let mut head = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n",
                        end - start
                    );
                    if status.starts_with("206") {
                        head.push_str(&format!(
                            "Content-Range: bytes {}-{}/{}\r\n",
                            start,
                            end - 1,
                            body.len()
                        ));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes());

                    let mut slice = &body[start..end];
                    let probe = range == "bytes=0-0";
                    if !probe {
                        let k = body_gets.fetch_add(1, Ordering::SeqCst);
                        if let Some(t) =
                            b.truncate_all
                                .or(if k == 0 { b.truncate_first } else { None })
                        {
                            slice = &slice[..t.min(slice.len())];
                        }
                    }
                    let _ = stream.write_all(slice);
                    let _ = stream.flush();
                });
            }
        });
    }
    Server { base, ranges: log }
}

fn parse_range(h: &str, len: usize) -> Option<(usize, usize)> {
    let spec = h.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: usize = a.parse().ok()?;
    let end = if b.is_empty() {
        len
    } else {
        b.parse::<usize>().ok()? + 1
    };
    (start < end && end <= len).then_some((start, end))
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 % 251) as u8).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(data))
}

fn optimizer(connections: usize) -> Optimizer {
    let mut cfg = Config::default().optimization;
    cfg.max_connections = connections;
    Optimizer::from_config(cfg)
}

fn simple(
    url: &str,
    out: &std::path::Path,
    opts: DownloadOptions,
) -> Result<std::path::PathBuf, String> {
    let options = DownloadOptions {
        quiet_mode: true,
        output_path: Some(out.to_string_lossy().into_owned()),
        ..opts
    };
    kget::download_to_path(url, ProxyConfig::default(), Optimizer::new(), options, None)
        .map_err(|e| e.to_string())
}

fn advanced(
    url: &str,
    out: &std::path::Path,
    connections: usize,
    sha: Option<&str>,
) -> Result<(), String> {
    let mut dl = AdvancedDownloader::new(
        url.to_string(),
        out.to_string_lossy().into_owned(),
        true,
        ProxyConfig::default(),
        optimizer(connections),
    )
    .map_err(|e| e.to_string())?;
    if let Some(h) = sha {
        dl.set_expected_sha256(h);
    }
    dl.download().map_err(|e| e.to_string())
}

// ── Simple downloader (P0-8) ────────────────────────────────────────────────

#[test]
fn simple_body_shorter_than_content_length_resumes() {
    let body = pattern(300_000);
    let srv = serve(
        body.clone(),
        Behavior {
            ranges: true,
            truncate_first: Some(1000),
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("f.bin");
    let path = simple(&format!("{}/f.bin", srv.base), &out, Default::default()).unwrap();
    assert_eq!(path, out);
    assert_eq!(std::fs::read(&out).unwrap(), body);
    let ranges = srv.ranges.lock().unwrap().clone();
    assert!(
        ranges.contains(&"bytes=1000-".to_string()),
        "resumed at the exact offset: {ranges:?}"
    );
    assert!(!dir.path().join("f.bin.part").exists());
}

#[test]
fn simple_leaves_part_file_on_failure_and_no_final_file() {
    let body = pattern(50_000);
    let srv = serve(
        body,
        Behavior {
            truncate_all: Some(100),
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("f.bin");
    let opts = DownloadOptions {
        retries: 2,
        ..Default::default()
    };
    let err = simple(&format!("{}/f.bin", srv.base), &out, opts).unwrap_err();
    assert!(
        err.contains("Incomplete") || err.contains("interrupted"),
        "{err}"
    );
    assert!(
        !out.exists(),
        "a truncated file must never get the final name"
    );
    assert!(dir.path().join("f.bin.part").exists());
}

#[test]
fn simple_continue_flag_resumes_existing_part() {
    let body = pattern(200_000);
    let srv = serve(
        body.clone(),
        Behavior {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("f.bin");
    std::fs::write(dir.path().join("f.bin.part"), &body[..50_000]).unwrap();
    let opts = DownloadOptions {
        resume: true,
        ..Default::default()
    };
    simple(&format!("{}/f.bin", srv.base), &out, opts).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), body);
    assert_eq!(srv.ranges.lock().unwrap()[0], "bytes=50000-");
}

#[test]
fn simple_429_respects_retry_after() {
    let body = pattern(10_000);
    let srv = serve(
        body.clone(),
        Behavior {
            fail_first_with: Some(429),
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("f.bin");
    let started = Instant::now();
    simple(&format!("{}/f.bin", srv.base), &out, Default::default()).unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "waited for Retry-After"
    );
    assert_eq!(std::fs::read(&out).unwrap(), body);
}

#[test]
fn simple_no_clobber_skips_existing_file() {
    let srv = serve(pattern(10), Behavior::default());
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("keep.txt");
    std::fs::write(&out, b"original").unwrap();
    let opts = DownloadOptions {
        overwrite: kget::OverwritePolicy::Skip,
        ..Default::default()
    };
    simple(&format!("{}/keep.txt", srv.base), &out, opts).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"original");
    assert!(srv.ranges.lock().unwrap().is_empty(), "no request was made");
}

#[test]
fn download_does_not_brotli_decode() {
    // Not valid brotli on purpose: a decoding client would fail or alter it.
    let body = pattern(4096);
    let srv = serve(
        body.clone(),
        Behavior {
            headers: vec![("Content-Encoding", "br")],
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("archive.tar.br");
    simple(
        &format!("{}/archive.tar.br", srv.base),
        &out,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&out).unwrap(),
        body,
        "bytes saved exactly as sent"
    );
}

#[test]
fn content_disposition_name_is_sanitized_not_rejected() {
    let srv = serve(
        pattern(10),
        Behavior {
            headers: vec![(
                "Content-Disposition",
                "attachment; filename=\"../evil/name.txt\"",
            )],
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let options = DownloadOptions {
        quiet_mode: true,
        output_path: Some(format!("{}/", dir.path().display())),
        ..Default::default()
    };
    let path = kget::download_to_path(
        &format!("{}/download", srv.base),
        ProxyConfig::default(),
        Optimizer::new(),
        options,
        None,
    )
    .unwrap();
    assert_eq!(path.parent().unwrap(), dir.path());
    assert_eq!(path.file_name().unwrap(), "_evil_name.txt");
}

#[test]
fn simple_sha256_mismatch_fails_without_final_file() {
    let srv = serve(pattern(1000), Behavior::default());
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("f.bin");
    let opts = DownloadOptions {
        expected_sha256: Some("0".repeat(64)),
        ..Default::default()
    };
    let err = simple(&format!("{}/f.bin", srv.base), &out, opts).unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
    assert!(!out.exists());
}

// ── Turbo downloader (P0-1, P0-2, P0-7) ─────────────────────────────────────

const MIB: usize = 1024 * 1024;

#[test]
fn advanced_short_chunk_is_retried_from_offset() {
    let body = pattern(10 * MIB);
    let srv = serve(
        body.clone(),
        Behavior {
            ranges: true,
            truncate_first: Some(1000),
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("a.bin");
    advanced(&format!("{}/a.bin", srv.base), &out, 2, None).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), body, "no holes in the file");
    let ranges = srv.ranges.lock().unwrap().clone();
    // 2 connections → chunks start at 0, 4 MiB and 8 MiB; one of them was cut short.
    let resumed = [1000, 4 * MIB + 1000, 8 * MIB + 1000].iter().any(|off| {
        ranges
            .iter()
            .any(|r| r.starts_with(&format!("bytes={off}-")))
    });
    assert!(
        resumed,
        "the short chunk was resumed from its offset: {ranges:?}"
    );
    assert!(!dir.path().join("a.bin.part").exists());
    assert!(!dir.path().join("a.bin.kget").exists());
}

#[test]
fn advanced_503_then_206_succeeds_with_backoff() {
    let body = pattern(5 * MIB);
    let srv = serve(
        body.clone(),
        Behavior {
            ranges: true,
            fail_first_with: Some(503),
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("a.bin");
    advanced(&format!("{}/a.bin", srv.base), &out, 2, None).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), body);
}

#[test]
fn advanced_preallocated_file_without_state_is_not_trusted() {
    let body = pattern(5 * MIB);
    let srv = serve(
        body.clone(),
        Behavior {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("a.bin");
    // Zero-filled files with the full size, as left by 1.7.0 or a crash.
    std::fs::write(&out, vec![0u8; body.len()]).unwrap();
    std::fs::write(dir.path().join("a.bin.part"), vec![0u8; body.len()]).unwrap();
    advanced(&format!("{}/a.bin", srv.base), &out, 2, None).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), body);
}

#[test]
fn advanced_resume_after_interrupt_completes_correctly() {
    let body = pattern(10 * MIB);
    let srv = serve(
        body.clone(),
        Behavior {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("a.bin");
    let url = format!("{}/a.bin", srv.base);

    // 1 connection → 2 chunks of 5 MiB. Chunk 0 complete, chunk 1 half done.
    let half = 5 * MIB + 5 * MIB / 2;
    let mut part = body[..half].to_vec();
    part.resize(body.len(), 0);
    std::fs::write(dir.path().join("a.bin.part"), &part).unwrap();
    let state = serde_json::json!({
        "version": 1,
        "url": url,
        "total_size": body.len(),
        "etag": "\"abc\"",
        "last_modified": null,
        "chunks": [
            { "start": 0, "end": 5 * MIB, "done": 5 * MIB },
            { "start": 5 * MIB, "end": 10 * MIB, "done": 5 * MIB / 2 },
        ],
    });
    std::fs::write(dir.path().join("a.bin.kget"), state.to_string()).unwrap();

    advanced(&url, &out, 1, None).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), body);
    let ranges = srv.ranges.lock().unwrap().clone();
    let body_ranges: Vec<_> = ranges.iter().filter(|r| *r != "bytes=0-0").collect();
    assert_eq!(
        body_ranges,
        vec![&format!("bytes={}-{}", half, body.len() - 1)],
        "only the missing bytes are requested"
    );
}

#[test]
fn advanced_quiet_with_sha256_mismatch_fails() {
    let body = pattern(5 * MIB);
    let srv = serve(
        body,
        Behavior {
            ranges: true,
            ..Default::default()
        },
    );
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("a.bin");
    let err = advanced(
        &format!("{}/a.bin", srv.base),
        &out,
        2,
        Some(&"0".repeat(64)),
    )
    .unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
    assert!(!out.exists());
}

#[test]
fn advanced_no_range_server_still_verifies_sha256() {
    let body = pattern(100_000);
    let srv = serve(body.clone(), Behavior::default());
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("a.bin");
    let url = format!("{}/a.bin", srv.base);
    let err = advanced(&url, &out, 2, Some(&"0".repeat(64))).unwrap_err();
    assert!(err.contains("mismatch"), "{err}");
    advanced(&url, &out, 2, Some(&sha256_hex(&body))).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), body);
}

// ── Proxy (P0-4) ────────────────────────────────────────────────────────────

#[test]
fn proxy_http_type_applies_to_https_targets() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    {
        let seen = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).unwrap_or(0);
                let first = String::from_utf8_lossy(&buf[..n])
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                seen.lock().unwrap().push(first);
                let _ = stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
            }
        });
    }
    let proxy = ProxyConfig {
        enabled: true,
        url: Some(format!("http://{addr}")),
        ..ProxyConfig::default()
    };
    let dir = TempDir::new().unwrap();
    let options = DownloadOptions {
        quiet_mode: true,
        output_path: Some(dir.path().join("x").to_string_lossy().into_owned()),
        retries: 1,
        ..Default::default()
    };
    let _ = kget::download(
        "https://example.invalid/x",
        proxy,
        Optimizer::new(),
        options,
        None,
    );
    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|l| l.starts_with("CONNECT example.invalid:443")),
        "HTTPS target went through the HTTP proxy: {seen:?}"
    );
}

#[test]
fn invalid_proxy_is_an_error_not_a_direct_connection() {
    let proxy = ProxyConfig {
        enabled: true,
        url: Some("::not a url::".into()),
        ..ProxyConfig::default()
    };
    assert!(
        AdvancedDownloader::new(
            "http://x/".into(),
            "x".into(),
            true,
            proxy,
            Optimizer::new()
        )
        .is_err()
    );
}

// ── Builder (P1-10) ─────────────────────────────────────────────────────────

#[test]
fn builder_does_not_retry_404() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    {
        let hits = hits.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                hits.fetch_add(1, Ordering::SeqCst);
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
    }
    let dir = TempDir::new().unwrap();
    let err = kget::builder(format!("http://{addr}/missing.bin"))
        .output(dir.path().join("m.bin").to_string_lossy())
        .quiet(true)
        .download()
        .unwrap_err();
    assert!(matches!(err, kget::KgetError::NotFound(_)), "{err:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "404 is permanent");
}
