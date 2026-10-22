//! Throughput benchmark for the turbo (multi-connection) engine.
//!
//! Ignored by default; run against a local server with:
//!
//! ```text
//! cargo test --release --test bench_turbo -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Only the public `AdvancedDownloader` API is used, so the same file can be
//! run against older releases to compare engines.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kget::{AdvancedDownloader, Config, Optimizer, ProxyConfig};
use tempfile::TempDir;

const MIB: usize = 1024 * 1024;

/// Serve `body` with range support. A request whose range starts at
/// `slow_start` is trickled at `slow_bps`; every other request is capped at
/// `per_conn_bps` (0 = unlimited).
fn serve(body: Arc<Vec<u8>>, per_conn_bps: usize, slow: Option<(usize, usize)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
                        return;
                    }
                    let mut range = None;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range:") {
                            range = v.trim().strip_prefix("bytes=").map(str::to_string);
                        }
                    }
                    let len = body.len();
                    if request_line.starts_with("HEAD") {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nETag: \"bench\"\r\n\r\n"
                        );
                        continue;
                    }
                    let (start, end) = range
                        .and_then(|r| {
                            let (a, b) = r.split_once('-')?;
                            let a: usize = a.parse().ok()?;
                            let b = if b.is_empty() { len } else { b.parse::<usize>().ok()? + 1 };
                            Some((a, b.min(len)))
                        })
                        .unwrap_or((0, len));
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{len}\r\nETag: \"bench\"\r\n\r\n",
                        end - start,
                        start,
                        end - 1
                    );
                    if stream.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    let bps = match slow {
                        Some((s, bps)) if s == start => bps,
                        _ => per_conn_bps,
                    };
                    let step = if bps == 0 { 256 * 1024 } else { (bps / 20).max(1) };
                    let mut pos = start;
                    while pos < end {
                        let n = step.min(end - pos);
                        if stream.write_all(&body[pos..pos + n]).is_err() {
                            return;
                        }
                        pos += n;
                        if bps != 0 {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            });
        }
    });
    base
}

fn run(name: &str, size: usize, connections: usize, per_conn_bps: usize, slow: Option<(usize, usize)>) {
    let body: Arc<Vec<u8>> = Arc::new((0..size).map(|i| (i * 31 % 251) as u8).collect());
    let base = serve(body.clone(), per_conn_bps, slow);
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("bench.bin");
    let mut cfg = Config::default().optimization;
    cfg.max_connections = connections;
    let dl = AdvancedDownloader::new(
        format!("{base}/bench.bin"),
        out.to_string_lossy().into_owned(),
        true,
        ProxyConfig::default(),
        Optimizer::from_config(cfg),
    )
    .unwrap();
    let t = Instant::now();
    dl.download().unwrap();
    let secs = t.elapsed().as_secs_f64();
    assert_eq!(std::fs::metadata(&out).unwrap().len() as usize, size);
    println!(
        "BENCH {name}: {} MiB, {connections} conn, {secs:.2} s, {:.1} MiB/s",
        size / MIB,
        size as f64 / MIB as f64 / secs
    );
}

#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn bench_loopback_unthrottled() {
    run("loopback", 512 * MIB, 8, 0, None);
}

#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn bench_capped_connections() {
    // Every connection capped at 8 MiB/s: throughput should scale with connections.
    run("capped-8MiB/s", 64 * MIB, 8, 8 * MIB, None);
}

#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn bench_one_slow_connection() {
    // The first range trickles at 512 KiB/s; the rest run at 16 MiB/s per
    // connection. Without work stealing the slow range holds up the tail.
    run("slow-tail", 32 * MIB, 4, 16 * MIB, Some((0, 512 * 1024)));
}
