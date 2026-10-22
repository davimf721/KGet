//! Mock server integration tests for KGet
//!
//! These tests use wiremock to simulate HTTP servers for download testing
//! without making real network requests to external servers.

use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ============================================================================
// Mock HTTP Server Tests
// ============================================================================

#[tokio::test]
async fn test_mock_server_basic_download() {
    // Start a mock server
    let mock_server = MockServer::start().await;

    // Define the expected response
    let body = b"Hello, this is test content!";

    Mock::given(method("GET"))
        .and(path("/test.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.to_vec())
                .insert_header("content-length", body.len().to_string())
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    // Verify mock is accessible
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/test.txt", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let content = response.bytes().await.unwrap();
    assert_eq!(&content[..], body);
}

#[tokio::test]
async fn test_mock_server_large_file() {
    let mock_server = MockServer::start().await;

    // Create a larger test file (1KB)
    let body: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();

    Mock::given(method("GET"))
        .and(path("/large.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.clone())
                .insert_header("content-length", body.len().to_string())
                .insert_header("content-type", "application/octet-stream"),
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/large.bin", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let content = response.bytes().await.unwrap();
    assert_eq!(content.len(), 1024);
}

#[tokio::test]
async fn test_mock_server_404_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/nonexistent.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/nonexistent.txt", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn test_mock_server_range_requests() {
    let mock_server = MockServer::start().await;

    let body = b"0123456789ABCDEFGHIJ";

    // Support for range requests (partial content)
    Mock::given(method("GET"))
        .and(path("/range.txt"))
        .and(header("range", "bytes=0-9"))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(b"0123456789".to_vec())
                .insert_header("content-range", "bytes 0-9/20")
                .insert_header("content-length", "10"),
        )
        .mount(&mock_server)
        .await;

    // Full file request
    Mock::given(method("GET"))
        .and(path("/range.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.to_vec())
                .insert_header("content-length", body.len().to_string())
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // Test full request
    let full_response = client
        .get(format!("{}/range.txt", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(full_response.status(), 200);

    // Test range request
    let range_response = client
        .get(format!("{}/range.txt", mock_server.uri()))
        .header("range", "bytes=0-9")
        .send()
        .await
        .unwrap();

    assert_eq!(range_response.status(), 206);
}

#[tokio::test]
async fn test_mock_server_json_response() {
    let mock_server = MockServer::start().await;
    let version = env!("CARGO_PKG_VERSION");

    // Test JSON response body parsing
    Mock::given(method("GET"))
        .and(path("/data.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(format!(r#"{{"name": "KGet", "version": "{}"}}"#, version)),
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/data.json", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    assert!(body.contains("KGet"));
    assert!(body.contains(version));

    // Verify it's valid JSON
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["name"], "KGet");
    assert_eq!(json["version"], version);
}

#[tokio::test]
async fn test_mock_server_head_request() {
    let mock_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/info.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "12345")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .head(format!("{}/info.txt", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let content_length = response
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(content_length, "12345");

    let accept_ranges = response
        .headers()
        .get("accept-ranges")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(accept_ranges, "bytes");
}

#[tokio::test]
async fn test_mock_server_redirects() {
    let mock_server = MockServer::start().await;

    // Setup redirect chain
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/final"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/final"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Final destination"))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .unwrap();

    let response = client
        .get(format!("{}/redirect", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(body, "Final destination");
}

#[tokio::test]
async fn test_mock_server_iso_detection() {
    let mock_server = MockServer::start().await;

    // Mock an ISO file response
    let iso_content = vec![0u8; 100]; // Small fake ISO

    Mock::given(method("GET"))
        .and(path("/test.iso"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(iso_content.clone())
                .insert_header("content-type", "application/x-iso9660-image")
                .insert_header("content-length", iso_content.len().to_string()),
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/test.iso", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("iso"));
}

// ============================================================================
// Library Function Tests with Mock Server
// ============================================================================

#[tokio::test]
async fn test_download_server_error_retries_and_fails() {
    // 5xx responses should be retried; after MAX_RETRIES attempts return an error.
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/flaky.txt"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .mount(&mock_server)
        .await;

    use kget::{DownloadOptions, Optimizer, ProxyConfig, download};
    let url = format!("{}/flaky.txt", mock_server.uri());
    let result = tokio::task::spawn_blocking(move || {
        download(
            &url,
            ProxyConfig::default(),
            Optimizer::new(),
            DownloadOptions {
                quiet_mode: true,
                output_path: Some("/tmp/kget_flaky_test.txt".to_string()),
                verify_iso: false,
                expected_sha256: None,
                ..Default::default()
            },
            None,
        )
    })
    .await
    .unwrap();
    assert!(result.is_err(), "503 should eventually fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("503") || msg.contains("after"),
        "unexpected error: {}",
        msg
    );
}

#[tokio::test]
async fn test_download_client_error_fails_immediately() {
    // 4xx responses must NOT be retried — fail immediately.
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/missing.txt"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&mock_server)
        .await;

    use kget::{DownloadOptions, Optimizer, ProxyConfig, download};
    let url = format!("{}/missing.txt", mock_server.uri());
    let result = tokio::task::spawn_blocking(move || {
        download(
            &url,
            ProxyConfig::default(),
            Optimizer::new(),
            DownloadOptions {
                quiet_mode: true,
                output_path: Some("/tmp/kget_missing_test.txt".to_string()),
                verify_iso: false,
                expected_sha256: None,
                ..Default::default()
            },
            None,
        )
    })
    .await
    .unwrap();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("404"));
}

#[tokio::test]
async fn test_file_hash_calculation() {
    use sha2::{Digest, Sha256};

    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("hash_test.txt");

    // Create a test file
    let content = b"Test content for hash verification";
    fs::write(&test_file, content).unwrap();

    // Calculate hash
    let mut hasher = Sha256::new();
    let file_content = fs::read(&test_file).unwrap();
    hasher.update(&file_content);
    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);

    // Verify hash is consistent
    assert_eq!(hash_hex.len(), 64); // SHA256 produces 64 hex chars

    // Verify same content produces same hash
    let mut hasher2 = Sha256::new();
    hasher2.update(content);
    let hash2 = hasher2.finalize();
    assert_eq!(hash, hash2);
}

#[tokio::test]
async fn test_simple_download_writes_expected_file() {
    use kget::{DownloadOptions, Optimizer, ProxyConfig, download};

    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("simple.txt");
    let body = b"KGet simple download integration test";

    Mock::given(method("GET"))
        .and(path("/simple.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.to_vec())
                .insert_header("content-length", body.len().to_string())
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    let url = format!("{}/simple.txt", mock_server.uri());
    let output_path_str = output_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        download(
            &url,
            ProxyConfig::default(),
            Optimizer::new(),
            DownloadOptions {
                quiet_mode: true,
                output_path: Some(output_path_str),
                verify_iso: false,
                expected_sha256: None,
                ..Default::default()
            },
            None,
        )
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(fs::read(output_path).unwrap(), body);
}

#[tokio::test]
async fn test_simple_download_verifies_expected_sha256() {
    use kget::{DownloadOptions, Optimizer, ProxyConfig, download};
    use sha2::{Digest, Sha256};

    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("verified.iso");
    let body = b"KGet SHA256 verification integration test";

    Mock::given(method("GET"))
        .and(path("/verified.iso"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.to_vec())
                .insert_header("content-length", body.len().to_string())
                .insert_header("content-type", "application/octet-stream"),
        )
        .mount(&mock_server)
        .await;

    let expected_hash = hex::encode(Sha256::digest(body));
    let url = format!("{}/verified.iso", mock_server.uri());
    let output_path_str = output_path.to_string_lossy().to_string();

    tokio::task::spawn_blocking(move || {
        download(
            &url,
            ProxyConfig::default(),
            Optimizer::new(),
            DownloadOptions {
                quiet_mode: true,
                output_path: Some(output_path_str),
                verify_iso: false,
                expected_sha256: Some(expected_hash),
                ..Default::default()
            },
            None,
        )
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(fs::read(output_path).unwrap(), body);
}

#[tokio::test]
async fn test_advanced_download_parallel_ranges_write_expected_file() {
    use kget::{AdvancedDownloader, Config, Optimizer, ProxyConfig};

    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("advanced.bin");
    let body: Vec<u8> = (0..(5 * 1024 * 1024)).map(|i| (i % 251) as u8).collect();
    let first_end = 4 * 1024 * 1024;

    Mock::given(method("HEAD"))
        .and(path("/advanced.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", body.len().to_string())
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/advanced.bin"))
        .and(header("range", format!("bytes=0-{}", first_end - 1)))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(body[..first_end].to_vec())
                .insert_header(
                    "content-range",
                    format!("bytes 0-{}/{}", first_end - 1, body.len()),
                )
                .insert_header("content-length", first_end.to_string()),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/advanced.bin"))
        .and(header(
            "range",
            format!("bytes={}-{}", first_end, body.len() - 1),
        ))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(body[first_end..].to_vec())
                .insert_header(
                    "content-range",
                    format!("bytes {}-{}/{}", first_end, body.len() - 1, body.len()),
                )
                .insert_header("content-length", (body.len() - first_end).to_string()),
        )
        .mount(&mock_server)
        .await;

    let mut config = Config::default();
    config.optimization.max_connections = 4;

    let url = format!("{}/advanced.bin", mock_server.uri());
    let output_path_str = output_path.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || {
        let downloader = AdvancedDownloader::new(
            url,
            output_path_str,
            true,
            ProxyConfig::default(),
            Optimizer::from_config(config.optimization),
        )?;
        downloader.download()
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(fs::read(output_path).unwrap(), body);
}

#[tokio::test]
async fn test_advanced_download_rejects_range_ignored_by_server() {
    use kget::{AdvancedDownloader, Optimizer, ProxyConfig};

    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("ignored-range.bin");
    let body = vec![7u8; 5 * 1024 * 1024];

    Mock::given(method("HEAD"))
        .and(path("/ignored-range.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", body.len().to_string())
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/ignored-range.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body)
                .insert_header("content-length", (5 * 1024 * 1024).to_string()),
        )
        .mount(&mock_server)
        .await;

    let url = format!("{}/ignored-range.bin", mock_server.uri());
    let output_path_str = output_path.to_string_lossy().to_string();
    let err = tokio::task::spawn_blocking(move || {
        let downloader = AdvancedDownloader::new(
            url,
            output_path_str,
            true,
            ProxyConfig::default(),
            Optimizer::new(),
        )
        .unwrap();
        downloader.download().unwrap_err().to_string()
    })
    .await
    .unwrap();
    assert!(err.contains("ignored the range request"));
}

// ============================================================================
// Concurrent Download Simulation
// ============================================================================

#[tokio::test]
async fn test_concurrent_mock_requests() {
    let mock_server = MockServer::start().await;

    // Setup multiple endpoints
    for i in 0..5 {
        let body = format!("Content for file {}", i);
        Mock::given(method("GET"))
            .and(path(format!("/file{}.txt", i)))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&mock_server)
            .await;
    }

    let client = reqwest::Client::new();
    let uri = mock_server.uri();

    // Make concurrent requests
    let mut handles = Vec::new();
    for i in 0..5 {
        let client = client.clone();
        let uri = uri.clone();
        handles.push(tokio::spawn(async move {
            client
                .get(format!("{}/file{}.txt", uri, i))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        }));
    }

    // Collect results
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }

    // Verify all requests completed successfully
    assert_eq!(results.len(), 5);
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result, &format!("Content for file {}", i));
    }
}

/// WebDAV URL with a user but no password: the password comes from the
/// (mock) keychain; a prompted password is saved once the server accepts it.
#[tokio::test]
async fn test_webdav_uses_keychain_and_saves_prompted_password() {
    use kget::credentials::{CredentialResolver, Keychain, SavePolicy, Target};
    use kget::webdav::WebDavDownloader;
    use kget::{Optimizer, ProxyConfig};
    use std::sync::Arc;

    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    for (file, auth) in [
        ("/saved.txt", "Basic YWxpY2U6czNjcmV0"), // alice:s3cret
        ("/typed.txt", "Basic YWxpY2U6dHlwZWQ="), // alice:typed
    ] {
        Mock::given(method("GET"))
            .and(path(file))
            .and(header("authorization", auth))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"ok".to_vec()))
            .mount(&mock_server)
            .await;
    }
    let base = mock_server.uri().replacen("http://", "webdav://alice@", 1);
    let target = Target::from_url(&base).unwrap();

    let fetch = |resolver: Arc<CredentialResolver>, name: &str| {
        let url = format!("{base}/{name}");
        let out = temp_dir.path().join(name).to_string_lossy().into_owned();
        tokio::task::spawn_blocking(move || {
            WebDavDownloader::new(url, out, true, ProxyConfig::default(), Optimizer::new())
                .with_credentials(resolver)
                .download()
        })
    };

    // 1. Saved password, no prompt
    let keychain = Keychain::mock();
    keychain.store(&target, "s3cret").unwrap();
    let resolver = Arc::new(
        CredentialResolver::with_keychain(keychain)
            .with_prompt(Arc::new(|_| panic!("must not prompt")))
            .quiet(true),
    );
    fetch(resolver, "saved.txt").await.unwrap().unwrap();
    assert_eq!(fs::read(temp_dir.path().join("saved.txt")).unwrap(), b"ok");

    // 2. Prompted password, saved after the server accepted it
    let keychain = Keychain::mock();
    let resolver = Arc::new(
        CredentialResolver::with_keychain(keychain.clone())
            .with_prompt(Arc::new(|_| Some("typed".to_string())))
            .with_confirm(Arc::new(|_| true))
            .save_policy(SavePolicy::Ask)
            .quiet(true),
    );
    fetch(resolver.clone(), "typed.txt").await.unwrap().unwrap();
    assert_eq!(resolver.finish(), vec![target.clone()]);
    assert_eq!(keychain.get(&target).unwrap().as_deref(), Some("typed"));
}

// ============================================================================
// Async turbo engine: work stealing, 1.7.1 resume, mirrors, HTTP/2
// ============================================================================

mod turbo {
    use super::*;
    use kget::http::HttpVersion;
    use kget::{AdvancedDownloader, Config, Optimizer, ProxyConfig};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use wiremock::{Request, Respond};

    const MIB: usize = 1024 * 1024;

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 13 % 251) as u8).collect()
    }

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::Digest;
        hex::encode(sha2::Sha256::digest(data))
    }

    fn optimizer(connections: usize) -> Optimizer {
        let mut cfg = Config::default().optimization;
        cfg.max_connections = connections;
        Optimizer::from_config(cfg)
    }

    /// Serves any `Range` of `body` and records every range it was asked for.
    /// Requests whose range starts at `slow_start` are delayed; once
    /// `fail_after` ranges were served, every further request gets a 403.
    #[derive(Clone)]
    struct Ranges {
        body: Arc<Vec<u8>>,
        seen: Arc<Mutex<Vec<(u64, u64)>>>,
        slow_start: Option<(u64, Duration)>,
        fail_after: Option<usize>,
    }

    impl Ranges {
        fn new(body: &[u8]) -> Self {
            Self {
                body: Arc::new(body.to_vec()),
                seen: Arc::default(),
                slow_start: None,
                fail_after: None,
            }
        }

        fn seen(&self) -> Vec<(u64, u64)> {
            self.seen.lock().unwrap().clone()
        }

        async fn mount(&self, server: &MockServer, file: &str) {
            Mock::given(method("HEAD"))
                .and(path(file))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-length", self.body.len().to_string())
                        .insert_header("accept-ranges", "bytes")
                        .insert_header("etag", "\"v1\""),
                )
                .mount(server)
                .await;
            Mock::given(method("GET"))
                .and(path(file))
                .respond_with(self.clone())
                .mount(server)
                .await;
        }
    }

    impl Respond for Ranges {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            let len = self.body.len() as u64;
            let range = req
                .headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.split_once('-'))
                .and_then(|(a, b)| Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()? + 1)));
            let Some((start, end)) = range else {
                return ResponseTemplate::new(200).set_body_bytes(self.body.to_vec());
            };
            let mut seen = self.seen.lock().unwrap();
            let fail = self.fail_after.is_some_and(|n| seen.len() >= n);
            seen.push((start, end));
            if fail {
                return ResponseTemplate::new(403);
            }
            let end = end.min(len);
            let mut t = ResponseTemplate::new(206)
                .set_body_bytes(self.body[start as usize..end as usize].to_vec())
                .insert_header("content-range", format!("bytes {start}-{}/{len}", end - 1))
                .insert_header("etag", "\"v1\"");
            if let Some((s, d)) = self.slow_start
                && s == start
            {
                t = t.set_delay(d);
            }
            t
        }
    }

    fn run(dl: AdvancedDownloader) -> Result<(), String> {
        dl.download().map_err(|e| e.to_string())
    }

    #[tokio::test]
    async fn slow_range_is_split_by_idle_connections() {
        let server = MockServer::start().await;
        let body = pattern(10 * MIB);
        let mut ranges = Ranges::new(&body);
        // Chunk 0 (0..4 MiB) stalls for 3 s before its first byte.
        ranges.slow_start = Some((0, Duration::from_secs(3)));
        ranges.mount(&server, "/ws.bin").await;

        let dir = TempDir::new().unwrap();
        let out = dir.path().join("ws.bin");
        let dl = AdvancedDownloader::new(
            format!("{}/ws.bin", server.uri()),
            out.to_string_lossy().into_owned(),
            true,
            ProxyConfig::default(),
            optimizer(2),
        )
        .unwrap();
        tokio::task::spawn_blocking(move || run(dl))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fs::read(&out).unwrap(), body, "no holes, no overlap damage");
        let seen = ranges.seen();
        // Chunk boundaries are 0, 4 MiB and 8 MiB; a split starts inside chunk 0.
        assert!(
            seen.iter().any(|&(s, _)| s > 0 && s < 4 * MIB as u64),
            "the stalled range was split: {seen:?}"
        );
        assert!(!dir.path().join("ws.bin.kget").exists());
    }

    #[tokio::test]
    async fn resumes_a_1_7_1_state_file() {
        let server = MockServer::start().await;
        let body = pattern(12 * MIB);
        let ranges = Ranges::new(&body);
        ranges.mount(&server, "/old.bin").await;
        let url = format!("{}/old.bin", server.uri());

        let dir = TempDir::new().unwrap();
        let out = dir.path().join("old.bin");
        // Exactly what 1.7.1 writes: chunk 0 done, chunk 1 half done, chunk 2 untouched.
        let m = MIB as u64;
        let mut part = body[..6 * MIB].to_vec();
        part.resize(body.len(), 0);
        fs::write(dir.path().join("old.bin.part"), &part).unwrap();
        let state = serde_json::json!({
            "version": 1,
            "url": url,
            "total_size": body.len(),
            "etag": "\"v1\"",
            "last_modified": null,
            "chunks": [
                { "start": 0, "end": 4 * m, "done": 4 * m },
                { "start": 4 * m, "end": 8 * m, "done": 2 * m },
                { "start": 8 * m, "end": 12 * m, "done": 0 },
            ],
        });
        fs::write(dir.path().join("old.bin.kget"), state.to_string()).unwrap();

        let dl = AdvancedDownloader::new(
            url,
            out.to_string_lossy().into_owned(),
            true,
            ProxyConfig::default(),
            optimizer(4),
        )
        .unwrap();
        tokio::task::spawn_blocking(move || run(dl))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fs::read(&out).unwrap(), body);
        let seen = ranges.seen();
        assert!(
            seen.iter().all(|&(s, _)| s >= 6 * m),
            "already-downloaded bytes are never requested again: {seen:?}"
        );
        assert!(seen.iter().any(|&(s, _)| s == 6 * m));
        assert!(seen.iter().any(|&(s, _)| s == 8 * m));
    }

    #[tokio::test]
    async fn metalink_mirror_failing_mid_download_is_dropped() {
        let bad = MockServer::start().await;
        let good = MockServer::start().await;
        let body = pattern(10 * MIB);
        let mut flaky = Ranges::new(&body);
        flaky.fail_after = Some(1); // serves one range, then 403 forever
        flaky.mount(&bad, "/m.bin").await;
        let healthy = Ranges::new(&body);
        healthy.mount(&good, "/m.bin").await;

        let dir = TempDir::new().unwrap();
        let manifest = dir.path().join("m.meta4");
        fs::write(
            &manifest,
            format!(
                r#"<?xml version="1.0"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="m.bin">
    <size>{}</size>
    <hash type="sha-256">{}</hash>
    <url priority="1">{}/m.bin</url>
    <url priority="2">{}/m.bin</url>
  </file>
</metalink>"#,
                body.len(),
                sha256_hex(&body),
                bad.uri(),
                good.uri()
            ),
        )
        .unwrap();

        let out_dir = dir.path().join("out");
        let (src, dst) = (
            manifest.to_string_lossy().into_owned(),
            out_dir.to_string_lossy().into_owned(),
        );
        tokio::task::spawn_blocking(move || {
            kget::metalink::download_metalink(
                &src,
                &dst,
                true,
                ProxyConfig::default(),
                optimizer(4),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(fs::read(out_dir.join("m.bin")).unwrap(), body);
        let (bad_seen, good_seen) = (flaky.seen(), healthy.seen());
        assert!(
            bad_seen.len() >= 2,
            "the bad mirror served and then failed: {bad_seen:?}"
        );
        // Parallel path: the good mirror never had to fetch the whole file.
        let good_bytes: u64 = good_seen.iter().map(|(s, e)| e - s).sum();
        assert!(
            good_bytes < body.len() as u64,
            "the healthy mirror only fetched what the bad one did not: {good_seen:?}"
        );
    }

    #[tokio::test]
    async fn http2_streams_are_multiplexed() {
        let server = MockServer::start().await;
        let body = pattern(9 * MIB);
        let ranges = Ranges::new(&body);
        ranges.mount(&server, "/h2.bin").await;

        let dir = TempDir::new().unwrap();
        let out = dir.path().join("h2.bin");
        let mut dl = AdvancedDownloader::new(
            format!("{}/h2.bin", server.uri()),
            out.to_string_lossy().into_owned(),
            true,
            ProxyConfig::default(),
            optimizer(8),
        )
        .unwrap();
        // wiremock speaks cleartext HTTP/2 (h2c) with prior knowledge.
        dl.set_http_version(HttpVersion::Http2PriorKnowledge)
            .unwrap();
        let statuses: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = statuses.clone();
        dl.set_status_callback(move |m| log.lock().unwrap().push(m));
        tokio::task::spawn_blocking(move || run(dl))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(fs::read(&out).unwrap(), body);
        let statuses = statuses.lock().unwrap().clone();
        assert!(
            statuses
                .iter()
                .any(|m| m.contains("HTTP/2") && m.contains("over 2 connection")),
            "8 streams over 2 HTTP/2 connections: {statuses:?}"
        );
    }

    #[tokio::test]
    async fn cancel_while_waiting_for_the_server_is_prompt() {
        let server = MockServer::start().await;
        let body = pattern(8 * MIB);
        let mut ranges = Ranges::new(&body);
        ranges.slow_start = Some((0, Duration::from_secs(30)));
        ranges.mount(&server, "/c.bin").await;

        let dir = TempDir::new().unwrap();
        let out = dir.path().join("c.bin");
        let mut dl = AdvancedDownloader::new(
            format!("{}/c.bin", server.uri()),
            out.to_string_lossy().into_owned(),
            true,
            ProxyConfig::default(),
            optimizer(1),
        )
        .unwrap();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        dl.set_cancel_token(cancel.clone());
        let started = std::time::Instant::now();
        let handle = tokio::task::spawn_blocking(move || run(dl));
        tokio::time::sleep(Duration::from_millis(500)).await;
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let err = handle.await.unwrap().unwrap_err();
        assert!(err.to_lowercase().contains("cancel"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "did not wait for the server"
        );
        assert!(dir.path().join("c.bin.kget").exists(), "state kept for resume");
    }
}
