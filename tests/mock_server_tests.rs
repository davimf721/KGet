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
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{"name": "KGet", "version": "{}"}}"#,
            version
        )))
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
                extra_headers: Vec::new(),
            },
            None,
        )
    })
    .await
    .unwrap();
    assert!(result.is_err(), "503 should eventually fail");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("503") || msg.contains("after"), "unexpected error: {}", msg);
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
                extra_headers: Vec::new(),
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
                extra_headers: Vec::new(),
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
                extra_headers: Vec::new(),
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
    assert!(err.contains("ignored range request"));
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
