//! Integration tests for KGet CLI
//!
//! These tests verify the command-line interface behavior.

use assert_cmd::Command;
use predicates::prelude::*;

/// Get the kget command for testing
fn kget() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("kget"))
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
fn test_cli_cache_flags() {
    kget()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-cache"));
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
        .args(["-q", "-p", "http://nonexistent-proxy:9999", "https://example.com/test.txt"])
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
    assert_eq!(config.optimization.compression, loaded.optimization.compression);
    assert_eq!(config.torrent.dht_enabled, loaded.torrent.dht_enabled);
}
