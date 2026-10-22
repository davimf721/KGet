//! Robustness tests: write errors (disk full), cancellation in the middle of
//! a chunk, remote file changing between runs, servers that ignore `Range`
//! or answer `416`, and stalled connections.
//!
//! Like `resume_tests.rs`, a tiny blocking HTTP server is used: these tests
//! need to throttle, stall and swap the served file between requests.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kget::{AdvancedDownloader, Config, DownloadOptions, KgetError, Optimizer, ProxyConfig};
use tempfile::TempDir;

const MIB: usize = 1024 * 1024;

// ── Test server ─────────────────────────────────────────────────────────────

/// The resource currently served.
#[derive(Clone)]
struct Resource {
    body: Arc<Vec<u8>>,
    etag: Option<&'static str>,
    last_modified: Option<&'static str>,
}

impl Resource {
    fn new(body: Vec<u8>, etag: Option<&'static str>, last_modified: Option<&'static str>) -> Self {
        Self {
            body: Arc::new(body),
            etag,
            last_modified,
        }
    }
}

struct Ctl {
    resource: Resource,
    /// `Accept-Ranges: bytes` on HEAD responses
    advertise_ranges: bool,
    /// Answer `Range` requests with 206 (otherwise 200 + the whole body)
    honor_ranges: bool,
    /// Answer every ranged GET (except the 1-byte probe) with 416
    always_416: bool,
    /// `If-Range` that doesn't match the current validator → 200 + whole body
    honor_if_range: bool,
    /// Pause after every 16 KiB of body
    throttle: Option<Duration>,
    /// The next `stall_count` body GETs send `stall_after` bytes, then hang
    stall_after: usize,
    stall_count: usize,
    /// `(Range, If-Range)` of every body GET (empty strings when absent)
    log: Vec<(String, String)>,
}

#[derive(Clone)]
struct Server {
    base: String,
    ctl: Arc<Mutex<Ctl>>,
}

impl Server {
    fn start(resource: Resource) -> Self {
        let ctl = Arc::new(Mutex::new(Ctl {
            resource,
            advertise_ranges: true,
            honor_ranges: true,
            always_416: false,
            honor_if_range: true,
            throttle: None,
            stall_after: 0,
            stall_count: 0,
            log: Vec::new(),
        }));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shared = ctl.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let ctl = shared.clone();
                std::thread::spawn(move || handle(stream, &ctl));
            }
        });
        Server { base, ctl }
    }

    fn url(&self, name: &str) -> String {
        format!("{}/{name}", self.base)
    }

    fn set(&self, f: impl FnOnce(&mut Ctl)) {
        f(&mut self.ctl.lock().unwrap());
    }

    fn log(&self) -> Vec<(String, String)> {
        self.ctl.lock().unwrap().log.clone()
    }

    fn clear_log(&self) {
        self.ctl.lock().unwrap().log.clear();
    }

    /// Ranges requested by body GETs (the 1-byte probe excluded).
    fn body_ranges(&self) -> Vec<String> {
        self.log()
            .into_iter()
            .map(|(r, _)| r)
            .filter(|r| r != "bytes=0-0")
            .collect()
    }
}

fn handle(mut stream: std::net::TcpStream, ctl: &Mutex<Ctl>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let (mut range, mut if_range) = (String::new(), String::new());
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("range:") {
            range = v.trim().to_string();
        } else if lower.starts_with("if-range:") {
            if_range = line["if-range:".len()..].trim().to_string();
        }
    }

    let is_head = request_line.starts_with("HEAD");
    let (res, advertise, honor_ranges, always_416, honor_if_range, throttle, stall) = {
        let mut c = ctl.lock().unwrap();
        let probe = range == "bytes=0-0";
        let stall = if !is_head && !probe && c.stall_count > 0 {
            c.stall_count -= 1;
            Some(c.stall_after)
        } else {
            None
        };
        if !is_head {
            c.log.push((range.clone(), if_range.clone()));
        }
        (
            c.resource.clone(),
            c.advertise_ranges,
            c.honor_ranges,
            c.always_416 && !probe,
            c.honor_if_range,
            c.throttle,
            stall,
        )
    };
    let len = res.body.len();

    let mut headers = String::new();
    if let Some(e) = res.etag {
        headers.push_str(&format!("ETag: {e}\r\n"));
    }
    if let Some(lm) = res.last_modified {
        headers.push_str(&format!("Last-Modified: {lm}\r\n"));
    }
    if advertise {
        headers.push_str("Accept-Ranges: bytes\r\n");
    }

    if is_head {
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n{headers}Connection: close\r\n\r\n"
        );
        return;
    }

    let validator_matches = if_range.is_empty()
        || res.etag == Some(if_range.as_str())
        || res.last_modified == Some(if_range.as_str());
    let use_range = honor_ranges && (validator_matches || !honor_if_range);
    let parsed = parse_range(&range, len);

    if !range.is_empty() && use_range && (always_416 || parsed.is_none()) {
        let _ = write!(
            stream,
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\n{headers}Connection: close\r\n\r\n"
        );
        return;
    }
    let (status, start, end) = match parsed {
        Some((s, e)) if use_range => ("206 Partial Content", s, e),
        _ => ("200 OK", 0, len),
    };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{headers}Connection: close\r\n",
        end - start
    );
    if start != 0 || end != len || status.starts_with("206") {
        head.push_str(&format!(
            "Content-Range: bytes {start}-{}/{len}\r\n",
            end - 1
        ));
    }
    head.push_str("\r\n");
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }

    let slice = &res.body[start..end];
    if let Some(n) = stall {
        let _ = stream.write_all(&slice[..n.min(slice.len())]);
        let _ = stream.flush();
        // Hang without closing; the client must give up on its own.
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    for piece in slice.chunks(16 * 1024) {
        if stream.write_all(piece).is_err() {
            return;
        }
        if let Some(d) = throttle {
            std::thread::sleep(d);
        }
    }
    let _ = stream.flush();
}

fn parse_range(h: &str, len: usize) -> Option<(usize, usize)> {
    let spec = h.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: usize = a.parse().ok()?;
    let end = if b.is_empty() {
        len
    } else {
        (b.parse::<usize>().ok()? + 1).min(len)
    };
    (start < end).then_some((start, end))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| ((i * 7 + seed as usize * 13) % 251) as u8)
        .collect()
}

fn optimizer(connections: usize) -> Optimizer {
    let mut cfg = Config::default().optimization;
    cfg.max_connections = connections;
    Optimizer::from_config(cfg)
}

fn sidecar(out: &Path, ext: &str) -> PathBuf {
    let mut name = out.file_name().unwrap().to_os_string();
    name.push(ext);
    out.with_file_name(name)
}

fn turbo(url: &str, out: &Path, connections: usize) -> AdvancedDownloader {
    let mut dl = AdvancedDownloader::new(
        url.to_string(),
        out.to_string_lossy().into_owned(),
        true,
        ProxyConfig::default(),
        optimizer(connections),
    )
    .unwrap();
    dl.set_resume_policy(kget::ResumePolicy::AlwaysResume);
    dl
}

/// Turbo download cancelled once more than `after` bytes were written.
fn turbo_cancelled_after(url: &str, out: &Path, after: u64) -> String {
    let mut dl = turbo(url, out, 1);
    let cancel = Arc::new(AtomicBool::new(false));
    dl.set_cancel_token(cancel.clone());
    dl.set_byte_progress_callback(move |bytes, _| {
        if bytes > after {
            cancel.store(true, Ordering::SeqCst);
        }
    });
    let err = dl.download().unwrap_err();
    assert!(
        matches!(err.downcast_ref::<KgetError>(), Some(KgetError::Cancelled)),
        "cancellation is reported as such: {err}"
    );
    err.to_string()
}

fn simple_opts(out: &Path) -> DownloadOptions {
    DownloadOptions {
        quiet_mode: true,
        output_path: Some(out.to_string_lossy().into_owned()),
        ..Default::default()
    }
}

fn simple(url: &str, opts: DownloadOptions) -> Result<PathBuf, String> {
    kget::download_to_path(url, ProxyConfig::default(), Optimizer::new(), opts, None)
        .map_err(|e| e.to_string())
}

/// Simple download cancelled once more than `after` bytes were written.
fn simple_cancelled_after(url: &str, out: &Path, after: u64) {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let opts = DownloadOptions {
        cancel: Some(cancel),
        progress_callback: Some(Arc::new(move |bytes, _| {
            if bytes > after {
                flag.store(true, Ordering::SeqCst);
            }
        })),
        ..simple_opts(out)
    };
    let err = kget::download_to_path(url, ProxyConfig::default(), Optimizer::new(), opts, None)
        .unwrap_err();
    assert!(
        matches!(err.downcast_ref::<KgetError>(), Some(KgetError::Cancelled)),
        "cancellation is reported as such: {err}"
    );
}

fn resume_opts(out: &Path) -> DownloadOptions {
    DownloadOptions {
        resume: true,
        ..simple_opts(out)
    }
}

fn kget_state(out: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(sidecar(out, ".kget")).unwrap()).unwrap()
}

// ── Cancellation in the middle of a chunk ───────────────────────────────────

#[test]
fn turbo_cancel_mid_chunk_keeps_state_and_resumes_byte_identical() {
    let body = pattern(10 * MIB, 1);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| c.throttle = Some(Duration::from_millis(2)));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("t.bin");
    let url = srv.url("t.bin");

    turbo_cancelled_after(&url, &out, 2 * MIB as u64);
    assert!(!out.exists(), "no final file after a cancel");
    assert!(sidecar(&out, ".part").exists());
    let state = kget_state(&out);
    let done: u64 = state["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["done"].as_u64().unwrap())
        .sum();
    assert!(
        done > 0 && done < body.len() as u64,
        "partial progress recorded: {done}"
    );
    // The recorded bytes really are on disk.
    let part = std::fs::read(sidecar(&out, ".part")).unwrap();
    for c in state["chunks"].as_array().unwrap() {
        let (s, d) = (
            c["start"].as_u64().unwrap() as usize,
            c["done"].as_u64().unwrap() as usize,
        );
        assert_eq!(&part[s..s + d], &body[s..s + d], "chunk at {s} matches");
    }

    srv.set(|c| c.throttle = None);
    srv.clear_log();
    turbo(&url, &out, 1).download().unwrap();
    assert!(
        std::fs::read(&out).unwrap() == body,
        "byte-identical result"
    );
    assert!(!sidecar(&out, ".part").exists());
    assert!(!sidecar(&out, ".kget").exists());
    let ranges = srv.body_ranges();
    assert!(
        !ranges.iter().any(|r| r.starts_with("bytes=0-")),
        "completed bytes are not fetched again: {ranges:?}"
    );
}

#[test]
fn simple_cancel_mid_body_keeps_part_and_state_and_resumes_byte_identical() {
    let body = pattern(4 * MIB, 2);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| c.throttle = Some(Duration::from_millis(2)));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("s.bin");
    let url = srv.url("s.bin");

    simple_cancelled_after(&url, &out, MIB as u64);
    assert!(!out.exists(), "no final file after a cancel");
    let part_len = std::fs::metadata(sidecar(&out, ".part")).unwrap().len();
    assert!(part_len > 0 && part_len < body.len() as u64, "{part_len}");
    let state = kget_state(&out);
    assert_eq!(state["etag"], "\"v1\"", "validators saved: {state}");
    assert_eq!(state["total_size"], body.len() as u64);

    srv.set(|c| c.throttle = None);
    srv.clear_log();
    simple(&url, resume_opts(&out)).unwrap();
    assert!(
        std::fs::read(&out).unwrap() == body,
        "byte-identical result"
    );
    assert_eq!(
        srv.log()[0],
        (format!("bytes={part_len}-"), "\"v1\"".to_string()),
        "resumed at the exact offset with If-Range"
    );
    assert!(!sidecar(&out, ".part").exists());
    assert!(!sidecar(&out, ".kget").exists());
}

// ── Remote file changes between runs ────────────────────────────────────────

/// Interrupt a download of `v1`, swap the server to `v2`, resume.
fn turbo_change_between_runs(v1: Resource, v2: Resource) {
    let srv = Server::start(v1);
    srv.set(|c| c.throttle = Some(Duration::from_millis(2)));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("c.bin");
    let url = srv.url("c.bin");

    turbo_cancelled_after(&url, &out, MIB as u64);
    assert!(sidecar(&out, ".kget").exists());

    let expected = v2.body.clone();
    srv.set(|c| {
        c.resource = v2;
        c.throttle = None;
    });
    turbo(&url, &out, 1).download().unwrap();
    assert!(
        std::fs::read(&out).unwrap() == *expected,
        "the new version only, never a splice of both"
    );
}

#[test]
fn turbo_restarts_when_etag_changes() {
    turbo_change_between_runs(
        Resource::new(pattern(10 * MIB, 1), Some("\"v1\""), None),
        Resource::new(pattern(10 * MIB, 2), Some("\"v2\""), None),
    );
}

#[test]
fn turbo_restarts_when_last_modified_changes() {
    turbo_change_between_runs(
        Resource::new(
            pattern(10 * MIB, 1),
            None,
            Some("Mon, 01 Jan 2024 00:00:00 GMT"),
        ),
        Resource::new(
            pattern(10 * MIB, 2),
            None,
            Some("Tue, 02 Jan 2024 00:00:00 GMT"),
        ),
    );
}

#[test]
fn turbo_restarts_when_size_changes() {
    turbo_change_between_runs(
        Resource::new(pattern(10 * MIB, 1), None, None),
        Resource::new(pattern(9 * MIB + 123, 2), None, None),
    );
}

fn simple_change_between_runs(v1: Resource, v2: Resource, honor_if_range: bool) {
    let srv = Server::start(v1);
    srv.set(|c| {
        c.throttle = Some(Duration::from_millis(2));
        c.honor_if_range = honor_if_range;
    });
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("c.bin");
    let url = srv.url("c.bin");

    simple_cancelled_after(&url, &out, MIB as u64);
    assert!(sidecar(&out, ".part").exists());

    let expected = v2.body.clone();
    srv.set(|c| {
        c.resource = v2;
        c.throttle = None;
    });
    simple(&url, resume_opts(&out)).unwrap();
    assert!(
        std::fs::read(&out).unwrap() == *expected,
        "the new version only, never a splice of both"
    );
    assert!(!sidecar(&out, ".kget").exists());
}

#[test]
fn simple_resume_restarts_when_etag_changes() {
    simple_change_between_runs(
        Resource::new(pattern(4 * MIB, 1), Some("\"v1\""), None),
        Resource::new(pattern(4 * MIB, 2), Some("\"v2\""), None),
        true,
    );
}

#[test]
fn simple_resume_restarts_when_etag_changes_even_if_server_ignores_if_range() {
    simple_change_between_runs(
        Resource::new(pattern(4 * MIB, 1), Some("\"v1\""), None),
        Resource::new(pattern(4 * MIB, 2), Some("\"v2\""), None),
        false,
    );
}

#[test]
fn simple_resume_restarts_when_last_modified_changes() {
    simple_change_between_runs(
        Resource::new(
            pattern(4 * MIB, 1),
            None,
            Some("Mon, 01 Jan 2024 00:00:00 GMT"),
        ),
        Resource::new(
            pattern(4 * MIB, 2),
            None,
            Some("Tue, 02 Jan 2024 00:00:00 GMT"),
        ),
        false,
    );
}

#[test]
fn simple_resume_restarts_when_size_changes() {
    simple_change_between_runs(
        Resource::new(pattern(4 * MIB, 1), None, None),
        Resource::new(pattern(5 * MIB + 7, 2), None, None),
        false,
    );
}

#[test]
fn simple_resume_of_interrupted_turbo_part_trusts_only_completed_prefix() {
    // A turbo `.part` is preallocated to the full size. Resuming it in simple
    // mode used to request `bytes=<full size>-`, get 416 and rename a file
    // full of zeros as complete.
    let body = pattern(10 * MIB, 3);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| c.throttle = Some(Duration::from_millis(2)));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("x.bin");
    let url = srv.url("x.bin");

    turbo_cancelled_after(&url, &out, 2 * MIB as u64);
    assert_eq!(
        std::fs::metadata(sidecar(&out, ".part")).unwrap().len(),
        body.len() as u64,
        "turbo preallocates"
    );
    srv.set(|c| c.throttle = None);
    simple(&url, resume_opts(&out)).unwrap();
    assert!(std::fs::read(&out).unwrap() == body, "no zero-filled holes");
}

#[test]
fn turbo_rejects_corrupted_state_that_claims_missing_chunks_are_done() {
    // `"chunks": []` matched the remote file and nothing was downloaded: the
    // zero-filled preallocated `.part` became the final file.
    let body = pattern(5 * MIB, 4);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("k.bin");
    let url = srv.url("k.bin");
    std::fs::write(sidecar(&out, ".part"), vec![0u8; body.len()]).unwrap();
    for chunks in [
        serde_json::json!([]),
        serde_json::json!([{ "start": 0, "end": 1024, "done": 1024 }]),
        serde_json::json!([{ "start": 0, "end": body.len(), "done": u64::MAX }]),
        serde_json::json!([
            { "start": 0, "end": body.len(), "done": 0 },
            { "start": u64::MAX, "end": 1, "done": 2 },
        ]),
    ] {
        let state = serde_json::json!({
            "version": 1, "url": url, "total_size": body.len(),
            "etag": "\"v1\"", "last_modified": null, "chunks": chunks,
        });
        std::fs::write(sidecar(&out, ".kget"), state.to_string()).unwrap();
        std::fs::write(sidecar(&out, ".part"), vec![0u8; body.len()]).unwrap();
        turbo(&url, &out, 1).download().unwrap();
        assert!(
            std::fs::read(&out).unwrap() == body,
            "state {chunks} ignored"
        );
        std::fs::remove_file(&out).unwrap();
    }
}

// ── Range fallback ──────────────────────────────────────────────────────────

#[test]
fn turbo_falls_back_to_single_stream_when_get_ignores_range() {
    // HEAD advertises ranges, but GET always answers 200 + the whole body.
    let body = pattern(9 * MIB, 5);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| c.honor_ranges = false);
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("r.bin");
    turbo(&srv.url("r.bin"), &out, 2).download().unwrap();
    assert!(std::fs::read(&out).unwrap() == body);
    assert!(!sidecar(&out, ".part").exists());
    assert!(!sidecar(&out, ".kget").exists());
}

#[test]
fn turbo_falls_back_to_single_stream_on_416() {
    let body = pattern(9 * MIB, 6);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| c.always_416 = true);
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("r.bin");
    turbo(&srv.url("r.bin"), &out, 2).download().unwrap();
    assert!(std::fs::read(&out).unwrap() == body);
    assert!(!sidecar(&out, ".kget").exists());
}

#[test]
fn turbo_resume_against_server_that_stopped_honoring_ranges_restarts() {
    let body = pattern(10 * MIB, 7);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| c.throttle = Some(Duration::from_millis(2)));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("r.bin");
    let url = srv.url("r.bin");
    turbo_cancelled_after(&url, &out, MIB as u64);
    srv.set(|c| {
        c.throttle = None;
        c.honor_ranges = false;
    });
    turbo(&url, &out, 1).download().unwrap();
    assert!(std::fs::read(&out).unwrap() == body);
}

#[test]
fn simple_resume_when_server_returns_200_restarts_cleanly() {
    let body = pattern(2 * MIB, 8);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| c.throttle = Some(Duration::from_millis(2)));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("r.bin");
    let url = srv.url("r.bin");
    simple_cancelled_after(&url, &out, 256 * 1024);
    srv.set(|c| {
        c.throttle = None;
        c.honor_ranges = false;
    });
    simple(&url, resume_opts(&out)).unwrap();
    assert!(std::fs::read(&out).unwrap() == body);
}

#[test]
fn simple_resume_416_with_complete_part_finishes_without_refetching() {
    let body = pattern(300_000, 9);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("f.bin");
    std::fs::write(sidecar(&out, ".part"), &body).unwrap();
    simple(&srv.url("f.bin"), resume_opts(&out)).unwrap();
    assert!(std::fs::read(&out).unwrap() == body);
    assert_eq!(srv.log().len(), 1, "a single (416) request");
}

#[test]
fn simple_resume_416_with_oversized_part_restarts() {
    let body = pattern(300_000, 10);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("f.bin");
    std::fs::write(sidecar(&out, ".part"), pattern(400_000, 11)).unwrap();
    simple(&srv.url("f.bin"), resume_opts(&out)).unwrap();
    assert!(std::fs::read(&out).unwrap() == body);
}

// ── Stalled connections and timeouts ────────────────────────────────────────

#[test]
fn turbo_stalled_chunk_times_out_and_resumes_from_offset() {
    let body = pattern(5 * MIB, 12);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| {
        c.stall_after = 64 * 1024;
        c.stall_count = 1;
    });
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("st.bin");
    let mut dl = turbo(&srv.url("st.bin"), &out, 1);
    dl.set_timeout_secs(1).unwrap();
    let started = Instant::now();
    dl.download().unwrap();
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(std::fs::read(&out).unwrap() == body);
    let ranges = srv.body_ranges();
    assert!(
        ranges.iter().any(|r| r.starts_with("bytes=65536-")),
        "retried from the stall offset: {ranges:?}"
    );
}

#[test]
fn turbo_permanently_stalled_server_fails_with_resumable_state() {
    let body = pattern(5 * MIB, 13);
    let srv = Server::start(Resource::new(body, Some("\"v1\""), None));
    srv.set(|c| {
        c.stall_after = 0;
        c.stall_count = usize::MAX;
    });
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("st.bin");
    let mut dl = turbo(&srv.url("st.bin"), &out, 1);
    dl.set_timeout_secs(1).unwrap();
    dl.set_retries(2);
    let started = Instant::now();
    let err = dl.download().unwrap_err().to_string();
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "bounded by timeout × retries"
    );
    assert!(err.contains("failed after"), "{err}");
    assert!(!out.exists());
    assert!(sidecar(&out, ".part").exists());
    assert!(sidecar(&out, ".kget").exists());
}

#[test]
fn simple_stalled_body_times_out_and_resumes_from_offset() {
    let body = pattern(MIB, 14);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    srv.set(|c| {
        c.stall_after = 100_000;
        c.stall_count = 1;
    });
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("st.bin");
    let opts = DownloadOptions {
        timeout_secs: 1,
        ..simple_opts(&out)
    };
    let started = Instant::now();
    simple(&srv.url("st.bin"), opts).unwrap();
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(std::fs::read(&out).unwrap() == body);
    let log = srv.log();
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(
        log[1].0.starts_with("bytes="),
        "resumed with a range: {log:?}"
    );
}

#[test]
fn simple_permanently_stalled_server_fails_and_keeps_part() {
    let body = pattern(MIB, 15);
    let srv = Server::start(Resource::new(body, Some("\"v1\""), None));
    srv.set(|c| {
        c.stall_after = 0;
        c.stall_count = usize::MAX;
    });
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("st.bin");
    let opts = DownloadOptions {
        timeout_secs: 1,
        retries: 2,
        ..simple_opts(&out)
    };
    let started = Instant::now();
    let err = simple(&srv.url("st.bin"), opts).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(err.contains("-c to resume"), "{err}");
    assert!(!out.exists());
    assert!(sidecar(&out, ".part").exists());
}

// ── Write errors (disk full) ────────────────────────────────────────────────

/// Real ENOSPC: `<file>.part` is a symlink to `/dev/full`.
#[cfg(target_os = "linux")]
#[test]
fn simple_enospc_fails_fast_with_clear_error_and_no_final_file() {
    let body = pattern(MIB, 16);
    let srv = Server::start(Resource::new(body, Some("\"v1\""), None));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("full.bin");
    std::os::unix::fs::symlink("/dev/full", sidecar(&out, ".part")).unwrap();
    let err = kget::download_to_path(
        &srv.url("full.bin"),
        ProxyConfig::default(),
        Optimizer::new(),
        simple_opts(&out),
        None,
    )
    .unwrap_err();
    let io = err
        .downcast_ref::<KgetError>()
        .and_then(|e| match e {
            KgetError::Io(io) => Some(io.kind()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("an I/O error: {err}"));
    assert_eq!(io, std::io::ErrorKind::StorageFull, "{err}");
    assert_eq!(srv.log().len(), 1, "a write error is not retried");
    assert!(!out.exists(), "never a file with the final name");
}

/// Run the CLI with a maximum file size (`ulimit -f`), so writing `.part`
/// fails with `EFBIG` like on a full disk. Portable to every Unix.
#[cfg(unix)]
fn kget_with_file_limit(dir: &Path, args: &[&str]) -> std::process::Output {
    let mut script = String::from("trap '' XFSZ; ulimit -f 1024; exec \"$0\"");
    for i in 1..=args.len() {
        script.push_str(&format!(" \"${i}\""));
    }
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_kget"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn cli_simple_write_error_fails_keeps_part_and_resumes_byte_identical() {
    let body = pattern(4 * MIB, 17);
    let srv = Server::start(Resource::new(body.clone(), Some("\"v1\""), None));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("big.bin");
    let url = srv.url("big.bin");

    let o = kget_with_file_limit(dir.path(), &["-q", "-O", out.to_str().unwrap(), &url]);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "failure is reported: {stderr}");
    assert!(stderr.contains("write"), "clear error: {stderr}");
    assert!(!out.exists(), "never a file with the final name");
    let part_len = std::fs::metadata(sidecar(&out, ".part")).unwrap().len();
    assert!(part_len > 0 && part_len < body.len() as u64, "{part_len}");
    assert_eq!(srv.log().len(), 1, "a write error is not retried");

    let o = std::process::Command::new(env!("CARGO_BIN_EXE_kget"))
        .args(["-q", "-c", "-O", out.to_str().unwrap(), &url])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(
        std::fs::read(&out).unwrap() == body,
        "byte-identical result"
    );
    assert_eq!(
        srv.log().last().unwrap().0,
        format!("bytes={part_len}-"),
        "resumed after the bytes that reached the disk"
    );
}

#[cfg(unix)]
#[test]
fn cli_turbo_write_error_fails_without_final_file() {
    let body = pattern(8 * MIB, 18);
    let srv = Server::start(Resource::new(body, Some("\"v1\""), None));
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("big.bin");
    let o = kget_with_file_limit(
        dir.path(),
        &["-q", "-a", "-O", out.to_str().unwrap(), &srv.url("big.bin")],
    );
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "failure is reported: {stderr}");
    assert!(stderr.contains("Error"), "clear error: {stderr}");
    assert!(!out.exists(), "never a file with the final name");
}
