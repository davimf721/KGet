//! Integration tests for KGet CLI
//!
//! These tests verify the command-line interface behavior.

use assert_cmd::Command;
use predicates::prelude::*;

/// Isolated config directory, so tests never touch the developer's real config.
fn test_config_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kget-cli-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Get the kget command for testing
fn kget() -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("kget"));
    cmd.env("KGET_CONFIG_DIR", test_config_dir());
    cmd
}

/// kget with its own empty config directory.
fn kget_in(dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("kget"));
    cmd.env("KGET_CONFIG_DIR", dir);
    cmd
}

// ============================================================================
// CLI Basic Tests
// ============================================================================

#[test]
fn test_cli_version_short() {
    kget()
        .arg("-v")
        .assert()
        .success()
        .stdout(predicate::str::contains("Kget"));
}

#[test]
fn test_cli_version_long() {
    kget()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("Kget"));
}

#[test]
fn test_cli_help_short() {
    kget()
        .arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn test_cli_help_long() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("download"))
        .stdout(predicate::str::contains("URL"));
}

// ============================================================================
// CLI Argument Parsing Tests
// ============================================================================

#[test]
fn test_cli_quiet_mode_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("-q"))
        .stdout(predicate::str::contains("--quiet"));
}

#[test]
fn test_cli_advanced_mode_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("-a"))
        .stdout(predicate::str::contains("--advanced"));
}

#[test]
fn test_cli_output_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("-O"))
        .stdout(predicate::str::contains("--output"));
}

#[test]
fn test_cli_proxy_flags() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--proxy"))
        .stdout(predicate::str::contains("--proxy-user"))
        .stdout(predicate::str::contains("--proxy-pass"));
}

#[test]
fn test_cli_torrent_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("-t"))
        .stdout(predicate::str::contains("--torrent"));
}

#[test]
fn test_cli_ftp_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--ftp"));
}

#[test]
fn test_cli_sftp_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--sftp"));
}

#[test]
fn test_cli_speed_limit_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("-l"));
}

#[test]
fn test_cli_cache_flag_is_hidden_but_accepted() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-cache").not());
    kget().args(["--no-cache", "--doctor"]).assert().success();
}

#[test]
fn test_cli_interactive_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("-i"))
        .stdout(predicate::str::contains("--interactive"));
}

#[test]
fn test_cli_jsonl_flag() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--jsonl"));
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_cli_invalid_url_handling() {
    // This should fail gracefully with an invalid URL
    kget()
        .args(["-q", "not-a-valid-url-at-all"])
        .assert()
        .failure();
}

#[test]
fn test_cli_nonexistent_proxy() {
    // Using a non-existent proxy should fail
    kget()
        .args([
            "-q",
            "-p",
            "http://nonexistent-proxy:9999",
            "https://example.com/test.txt",
        ])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .failure();
}

// ============================================================================
// Feature Tests
// ============================================================================

#[test]
fn test_cli_gui_flag_present() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--gui"));
}

// ============================================================================
// Version String Format
// ============================================================================

#[test]
fn test_version_format() {
    let output = kget()
        .arg("-v")
        .output()
        .expect("Failed to execute command");

    let version_string = String::from_utf8_lossy(&output.stdout);

    // Should contain "Kget" and a version number
    assert!(version_string.contains("Kget"));
    // Version should be in format X.Y.Z
    assert!(version_string.contains('.'));
}

// ============================================================================
// Integration with Config
// ============================================================================

#[test]
fn test_config_directory_creation() {
    use kget::Config;

    // Loading config should not panic even if directory doesn't exist
    let result = Config::load();
    assert!(result.is_ok());
}

#[test]
fn test_config_save_and_load() {
    use kget::Config;

    // This test verifies the serialization/deserialization cycle
    let config = Config::default();
    let json = serde_json::to_string_pretty(&config).unwrap();
    let loaded: Config = serde_json::from_str(&json).unwrap();

    assert_eq!(config.proxy.enabled, loaded.proxy.enabled);
    assert_eq!(
        config.optimization.compression,
        loaded.optimization.compression
    );
    assert_eq!(config.torrent.dht_enabled, loaded.torrent.dht_enabled);
}

// ============================================================================
// Torrent Backend Detection
// ============================================================================

#[cfg(feature = "torrent-native")]
#[test]
fn test_cli_native_torrent_backend_available() {
    // When torrent-native feature is enabled, the binary should handle magnet links
    // This just verifies the binary compiled with the feature
    kget().arg("--version").assert().success();
}

// ============================================================================
// Magnet Link Detection
// ============================================================================

#[test]
fn test_cli_magnet_link_detection() {
    // Test that magnet links are recognized (will fail to download but shouldn't crash)
    let result = kget()
        .args(["-q", "magnet:?xt=urn:btih:invalidhash"])
        .timeout(std::time::Duration::from_secs(5))
        .assert();

    // Should either fail gracefully or timeout - not crash
    // The important thing is no panic
    let _ = result;
}

// ============================================================================
// Environment Variable Tests
// ============================================================================

#[test]
fn test_cli_respects_env_vars() {
    // Test that KGET_QUIET environment variable works
    kget()
        .env("KGET_QUIET", "1")
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn test_cli_torrent_backend_env() {
    // Test that KGET_TORRENT_BACKEND env var is recognized
    kget()
        .env("KGET_TORRENT_BACKEND", "native")
        .arg("--help")
        .assert()
        .success();
}

// ============================================================================
// Output Format Tests
// ============================================================================

#[test]
fn test_cli_quiet_produces_minimal_output() {
    // In quiet mode, help should still work but download would be silent
    kget()
        .args(["-q", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

// ============================================================================
// Concurrent Execution Tests
// ============================================================================

#[test]
fn test_cli_multiple_help_calls() {
    use std::thread;

    // Run multiple instances simultaneously - should not conflict
    let handles: Vec<_> = (0..3)
        .map(|_| {
            thread::spawn(|| {
                kget().arg("--help").assert().success();
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

// ============================================================================
// 1.7.1 regressions
// ============================================================================

#[test]
fn cli_flags_do_not_persist_config() {
    let dir = tempfile::tempdir().unwrap();
    kget_in(dir.path())
        .args([
            "-q",
            "-l",
            "100",
            "--proxy",
            "http://127.0.0.1:9",
            "--retries",
            "1",
            "--timeout",
            "2",
            "http://127.0.0.1:9/file.bin",
        ])
        .assert()
        .failure();
    assert!(
        !dir.path().join("config.json").exists(),
        "CLI flags must not be written to config.json"
    );
}

#[test]
fn save_config_never_stores_proxy_password() {
    let dir = tempfile::tempdir().unwrap();
    kget_in(dir.path())
        .args([
            "--save-config",
            "--proxy",
            "socks5://127.0.0.1:1080",
            "--proxy-user",
            "me",
            "--proxy-pass",
            "secret",
            "-l",
            "2048",
        ])
        .assert()
        .success();
    let text = std::fs::read_to_string(dir.path().join("config.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["proxy"]["password"], serde_json::Value::Null);
    assert_eq!(v["proxy"]["username"], "me");
    assert_eq!(v["proxy"]["proxy_type"], "Socks5");
    assert_eq!(v["optimization"]["speed_limit"], 2048);
}

#[test]
fn config_with_missing_fields_still_starts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.json"),
        r#"{"proxy":{"enabled":false}}"#,
    )
    .unwrap();
    kget_in(dir.path()).arg("--doctor").assert().success();
}

#[test]
fn corrupt_config_is_backed_up_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.json"), "{not json").unwrap();
    kget_in(dir.path())
        .arg("--doctor")
        .assert()
        .success()
        .stderr(predicate::str::contains("invalid config"));
    assert!(dir.path().join("config.json.bak").exists());
}

#[test]
fn jsonl_output_is_valid_json_per_line() {
    let dir = tempfile::tempdir().unwrap();
    let out = kget_in(dir.path())
        .current_dir(dir.path())
        .args([
            "--jsonl",
            "--retries",
            "1",
            "--timeout",
            "2",
            "http://127.0.0.1:9/x.bin",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let mut events = Vec::new();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON: {line:?} ({e})"));
        assert_eq!(v["v"], 1, "every event carries the protocol version");
        events.push(v["event"].as_str().unwrap().to_string());
    }
    assert_eq!(events.first().map(String::as_str), Some("started"));
    assert_eq!(events.last().map(String::as_str), Some("error"));
}

#[test]
fn doctor_reports_environment() {
    kget()
        .arg("--doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("KGet"))
        .stdout(predicate::str::contains("yt-dlp"))
        .stdout(predicate::str::contains("Features"));
}

#[test]
fn invalid_checksum_spec_is_rejected() {
    kget()
        .args(["--checksum", "crc32:zz", "http://127.0.0.1:9/x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("checksum"));
}

#[test]
fn new_flags_are_documented() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--continue"))
        .stdout(predicate::str::contains("--no-clobber"))
        .stdout(predicate::str::contains("--retries"))
        .stdout(predicate::str::contains("--timeout"))
        .stdout(predicate::str::contains("--jobs"))
        .stdout(predicate::str::contains("--save-config"))
        .stdout(predicate::str::contains("--doctor"));
}

#[test]
fn history_redacts_credentials_from_cli_downloads() {
    let dir = tempfile::tempdir().unwrap();
    kget_in(dir.path())
        .current_dir(dir.path())
        .args([
            "-q",
            "--retries",
            "1",
            "--timeout",
            "2",
            "http://user:hunter2@127.0.0.1:9/x.bin",
        ])
        .assert()
        .failure();
    let out = kget_in(dir.path())
        .args(["--history", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let history = String::from_utf8(out.stdout).unwrap();
    assert!(!history.contains("hunter2"));
    assert!(history.contains("127.0.0.1"));
    let raw = std::fs::read(dir.path().join("history.db")).unwrap();
    assert!(!raw.windows(7).any(|w| w == b"hunter2"));
}

/// Run `kget --history <args>` in `dir` and parse the JSON array.
fn history_json(dir: &std::path::Path, args: &[&str]) -> Vec<serde_json::Value> {
    let out = kget_in(dir)
        .arg("--history")
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn names(v: &[serde_json::Value]) -> Vec<&str> {
    v.iter().map(|e| e["filename"].as_str().unwrap()).collect()
}

#[test]
fn history_filters_and_json_output() {
    let dir = tempfile::tempdir().unwrap();
    // A history.json from KGet 1.7 in KGET_CONFIG_DIR is migrated on first use
    std::fs::write(
        dir.path().join("history.json"),
        r#"[
 {"id":"1","url":"https://example.com/a.iso","filename":"a.iso","output_dir":"/d","status":"completed",
  "bytes_total":10,"sha256":null,"expected_sha256":null,"created_at":1790812800,"finished_at":null,"error":null},
 {"id":"2","url":"https://cdn.example.com/b.mp4","filename":"b.mp4","output_dir":"/d","status":"failed",
  "bytes_total":null,"sha256":null,"expected_sha256":null,"created_at":1791504000,"finished_at":null,"error":"x"},
 {"id":"3","url":"https://other.org/c.zip","filename":"c.zip","output_dir":"/d","status":"failed",
  "bytes_total":99,"sha256":null,"expected_sha256":null,"created_at":1791590400,"finished_at":null,"error":"y"}
]"#,
    )
    .unwrap();

    assert_eq!(
        names(&history_json(dir.path(), &[])),
        ["c.zip", "b.mp4", "a.iso"]
    );
    assert!(dir.path().join("history.json.bak").exists());
    assert_eq!(
        names(&history_json(dir.path(), &["--status", "failed"])),
        ["c.zip", "b.mp4"]
    );
    assert_eq!(
        names(&history_json(dir.path(), &["--domain", "example.com"])),
        ["b.mp4", "a.iso"]
    );
    assert_eq!(
        names(&history_json(
            dir.path(),
            &["--status", "failed", "--domain", "example.com"]
        )),
        ["b.mp4"]
    );
    // 2026-10-01 = 1790812800, 2026-10-09 = 1791504000, 2026-10-10 = 1791590400
    assert_eq!(
        names(&history_json(dir.path(), &["--since", "2026-10-02"])),
        ["c.zip", "b.mp4"]
    );
    assert_eq!(
        names(&history_json(dir.path(), &["--until", "2026-10-09"])),
        ["b.mp4", "a.iso"]
    );
    assert_eq!(
        names(&history_json(dir.path(), &["--search", "C.ZIP"])),
        ["c.zip"]
    );
    assert_eq!(
        names(&history_json(dir.path(), &["--type", "video"])),
        ["b.mp4"]
    );
    assert_eq!(
        names(&history_json(
            dir.path(),
            &["--sort", "largest", "--history-limit", "1"]
        )),
        ["c.zip"]
    );
    assert_eq!(
        names(&history_json(dir.path(), &["--history-offset", "2"])),
        ["a.iso"]
    );
    let entry = &history_json(dir.path(), &["--search", "a.iso"])[0];
    assert_eq!(entry["status"], "completed");
    assert_eq!(entry["output_dir"], "/d");
    assert_eq!(entry["bytes_total"], 10);

    // --jsonl: one object per line
    let out = kget_in(dir.path())
        .args(["--history", "--json", "--jsonl"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), 3);
    for line in text.lines() {
        serde_json::from_str::<serde_json::Value>(line).unwrap();
    }

    // Human table
    kget_in(dir.path())
        .args(["--history", "--status", "failed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("c.zip"))
        .stdout(predicate::str::contains("a.iso").not());

    // Bad values are reported
    kget_in(dir.path())
        .args(["--history", "--status", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown status"));
    kget_in(dir.path())
        .args(["--history", "--since", "someday"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid date"));
    // Filters need --history
    kget_in(dir.path())
        .args(["--status", "failed"])
        .assert()
        .failure();

    // --history-clear failed removes only failures
    kget_in(dir.path())
        .args(["--history-clear", "failed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 2"));
    assert_eq!(names(&history_json(dir.path(), &[])), ["a.iso"]);
}

#[test]
fn history_survives_concurrent_processes() {
    let dir = tempfile::tempdir().unwrap();
    // Several kget processes recording at the same time (each fails fast)
    let children: Vec<_> = (0..6)
        .map(|i| {
            std::process::Command::new(assert_cmd::cargo::cargo_bin!("kget"))
                .env("KGET_CONFIG_DIR", dir.path())
                .current_dir(dir.path())
                .args([
                    "-q",
                    "--retries",
                    "1",
                    "--timeout",
                    "2",
                    &format!("http://127.0.0.1:9/file{i}.bin"),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut c in children {
        c.wait().unwrap();
    }
    let all = history_json(dir.path(), &["--status", "failed"]);
    assert_eq!(all.len(), 6);
    let mut files = names(&all);
    files.sort();
    assert_eq!(
        files,
        [
            "file0.bin",
            "file1.bin",
            "file2.bin",
            "file3.bin",
            "file4.bin",
            "file5.bin"
        ]
    );
    assert!(all.iter().all(|e| e["protocol"] == "http"));
}
