//! Unit tests for KGet
//!
//! These tests verify the core functionality without making real network requests.

// ============================================================================
// Utils Module Tests
// ============================================================================

mod utils_tests {
    use kget::{get_filename_from_url_or_default, resolve_output_path};

    #[test]
    fn test_get_filename_from_url_simple() {
        let url = "https://example.com/files/document.pdf";
        let filename = get_filename_from_url_or_default(url, "default.txt");
        assert_eq!(filename, "document.pdf");
    }

    #[test]
    fn test_get_filename_from_url_with_query_params() {
        let url = "https://example.com/download/file.zip?token=abc123";
        let filename = get_filename_from_url_or_default(url, "default.txt");
        // URL parsing should still extract the filename
        assert_eq!(filename, "file.zip");
    }

    #[test]
    fn test_get_filename_from_url_trailing_slash() {
        let url = "https://example.com/directory/";
        let filename = get_filename_from_url_or_default(url, "index.html");
        assert_eq!(filename, "index.html");
    }

    #[test]
    fn test_get_filename_from_url_no_path() {
        let url = "https://example.com";
        let filename = get_filename_from_url_or_default(url, "default.txt");
        assert_eq!(filename, "default.txt");
    }

    #[test]
    fn test_get_filename_from_invalid_url() {
        let url = "not a valid url";
        let filename = get_filename_from_url_or_default(url, "fallback.txt");
        assert_eq!(filename, "fallback.txt");
    }

    #[test]
    fn test_get_filename_from_ftp_url() {
        let url = "ftp://ftp.example.com/pub/archive.tar.gz";
        let filename = get_filename_from_url_or_default(url, "default.tar.gz");
        assert_eq!(filename, "archive.tar.gz");
    }

    #[test]
    fn test_get_filename_from_url_encoded() {
        let url = "https://example.com/files/my%20document.pdf";
        let filename = get_filename_from_url_or_default(url, "default.pdf");
        assert_eq!(filename, "my%20document.pdf");
    }

    #[test]
    fn test_resolve_output_path_none() {
        let result = resolve_output_path(None, "https://example.com/file.zip", "default.zip");
        assert_eq!(result, "file.zip");
    }

    #[test]
    fn test_resolve_output_path_with_filename() {
        let result = resolve_output_path(
            Some("custom_name.zip".to_string()),
            "https://example.com/file.zip",
            "default.zip",
        );
        assert_eq!(result, "custom_name.zip");
    }
}

// ============================================================================
// Config Module Tests
// ============================================================================

mod config_tests {
    use kget::{Config, ProxyType};

    #[test]
    fn test_config_default_values() {
        let config = Config::default();

        // Proxy defaults
        assert!(!config.proxy.enabled);
        assert!(config.proxy.url.is_none());
        assert!(config.proxy.username.is_none());
        assert!(config.proxy.password.is_none());

        // Optimization defaults
        assert!(config.optimization.compression);
        assert_eq!(config.optimization.compression_level, 6);
        assert!(config.optimization.cache_enabled);
        assert_eq!(config.optimization.max_connections, 4);

        // Torrent defaults
        assert!(!config.torrent.enabled);
        assert!(config.torrent.dht_enabled);
        assert_eq!(config.torrent.max_peers, 50);

        // FTP defaults
        assert!(config.ftp.passive_mode);
        assert_eq!(config.ftp.default_port, 21);

        // SFTP defaults
        assert_eq!(config.sftp.default_port, 22);
    }

    #[test]
    fn test_proxy_type_variants() {
        let http = ProxyType::Http;
        let https = ProxyType::Https;
        let socks5 = ProxyType::Socks5;

        // Just verify they can be created and cloned
        let _http_clone = http.clone();
        let _https_clone = https.clone();
        let _socks5_clone = socks5.clone();
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();

        // Test JSON serialization
        let json = serde_json::to_string(&config);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("\"enabled\""));
        assert!(json_str.contains("\"compression\""));
    }

    #[test]
    fn test_config_deserialization() {
        let json = r#"{
            "proxy": {
                "enabled": true,
                "url": "http://proxy.example.com:8080",
                "username": null,
                "password": null,
                "proxy_type": "Http"
            },
            "optimization": {
                "compression": true,
                "compression_level": 6,
                "cache_enabled": true,
                "cache_dir": "~/.cache/kget",
                "speed_limit": null,
                "max_connections": 4
            },
            "torrent": {
                "enabled": false,
                "download_dir": null,
                "max_peers": 50,
                "max_seeds": 25,
                "port": null,
                "dht_enabled": true,
                "max_peer_connections": 50,
                "max_upload_slots": 4
            },
            "ftp": {
                "passive_mode": true,
                "default_port": 21
            },
            "sftp": {
                "default_port": 22,
                "key_path": null
            }
        }"#;

        let config: Result<Config, _> = serde_json::from_str(json);
        assert!(config.is_ok());

        let config = config.unwrap();
        assert!(config.proxy.enabled);
        assert_eq!(
            config.proxy.url,
            Some("http://proxy.example.com:8080".to_string())
        );
    }
}

// ============================================================================
// Download Module Tests
// ============================================================================

mod content_disposition_tests {
    use kget::download::parse_content_disposition_filename;

    #[test]
    fn test_plain_filename() {
        assert_eq!(
            parse_content_disposition_filename(r#"attachment; filename="report.pdf""#),
            Some("report.pdf".to_string())
        );
    }

    #[test]
    fn test_filename_without_quotes() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=archive.tar.gz"),
            Some("archive.tar.gz".to_string())
        );
    }

    #[test]
    fn test_rfc5987_filename_star_takes_priority() {
        let header = r#"attachment; filename="fallback.zip"; filename*=UTF-8''my%20file.zip"#;
        assert_eq!(
            parse_content_disposition_filename(header),
            Some("my file.zip".to_string())
        );
    }

    #[test]
    fn test_no_filename() {
        assert_eq!(parse_content_disposition_filename("inline"), None);
    }

    #[test]
    fn test_empty_filename_falls_through() {
        assert_eq!(parse_content_disposition_filename(r#"attachment; filename="""#), None);
    }
}

mod download_tests {
    use kget::download::{check_disk_space, validate_filename};
    use tempfile::TempDir;

    #[test]
    fn test_validate_filename_valid() {
        assert!(validate_filename("document.pdf").is_ok());
        assert!(validate_filename("my-file_v2.0.tar.gz").is_ok());
        assert!(validate_filename("file").is_ok());
    }

    #[test]
    fn test_validate_filename_empty() {
        let result = validate_filename("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_filename_with_separator() {
        let sep = std::path::MAIN_SEPARATOR;
        let invalid_name = format!("path{}file.txt", sep);
        let result = validate_filename(&invalid_name);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("separator"));
    }

    #[test]
    fn test_validate_filename_rejects_all_platform_separators() {
        assert!(validate_filename("path/file.txt").is_err());
        assert!(validate_filename(r"path\file.txt").is_err());
    }

    #[test]
    fn test_verify_iso_integrity_emits_sha256_callback() {
        use kget::{verify_file_sha256, verify_iso_integrity};
        use std::sync::{Arc, Mutex};

        let temp_dir = TempDir::new().unwrap();
        let iso_path = temp_dir.path().join("tiny.iso");
        std::fs::write(&iso_path, b"kget iso verification test").unwrap();

        let messages = Arc::new(Mutex::new(Vec::new()));
        let messages_for_cb = messages.clone();
        verify_iso_integrity(
            &iso_path,
            Some(&move |msg| {
                messages_for_cb.lock().unwrap().push(msg);
            }),
        )
        .unwrap();

        let messages = messages.lock().unwrap();
        assert!(
            messages
                .iter()
                .any(|msg| msg.contains("Calculating SHA256"))
        );
        assert!(messages.iter().any(|msg| msg.starts_with("SHA256: ")));

        drop(messages);
        let hash = verify_file_sha256(&iso_path, None, None).unwrap();
        assert!(verify_file_sha256(&iso_path, Some(&hash), None).is_ok());
        assert!(verify_file_sha256(&iso_path, Some("deadbeef"), None).is_err());
    }

    #[test]
    fn test_check_disk_space_sufficient() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("test_file.txt");

        // 1 byte should always be available
        let result = check_disk_space(&test_path, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_disk_space_insufficient() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("huge_file.bin");

        // Request impossibly large amount (1 exabyte)
        let result = check_disk_space(&test_path, u64::MAX);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Insufficient"));
    }
}

// ============================================================================
// DownloadOptions Tests
// ============================================================================

mod download_options_tests {
    use kget::DownloadOptions;

    #[test]
    fn test_download_options_default() {
        let options = DownloadOptions::default();

        assert!(!options.quiet_mode);
        assert!(options.output_path.is_none());
        assert!(!options.verify_iso);
        assert!(options.expected_sha256.is_none());
    }

    #[test]
    fn test_download_options_custom() {
        let options = DownloadOptions {
            quiet_mode: true,
            output_path: Some("/tmp/download.iso".to_string()),
            verify_iso: true,
            expected_sha256: Some("abc123".to_string()),
        };

        assert!(options.quiet_mode);
        assert_eq!(options.output_path, Some("/tmp/download.iso".to_string()));
        assert!(options.verify_iso);
        assert_eq!(options.expected_sha256, Some("abc123".to_string()));
    }

    #[test]
    fn test_download_options_clone() {
        let original = DownloadOptions {
            quiet_mode: true,
            output_path: Some("file.txt".to_string()),
            verify_iso: false,
            expected_sha256: None,
        };

        let cloned = original.clone();
        assert_eq!(original.quiet_mode, cloned.quiet_mode);
        assert_eq!(original.output_path, cloned.output_path);
        assert_eq!(original.verify_iso, cloned.verify_iso);
        assert_eq!(original.expected_sha256, cloned.expected_sha256);
    }
}

// ============================================================================
// Optimizer Tests
// ============================================================================

mod optimizer_tests {
    use kget::{Config, Optimizer};

    #[test]
    fn test_optimizer_creation() {
        let config = Config::default();
        let optimizer = Optimizer::from_config(config.optimization.clone());

        assert!(optimizer.speed_limit.is_none());
    }

    #[test]
    fn test_optimizer_with_speed_limit() {
        let mut config = Config::default();
        config.optimization.speed_limit = Some(1_000_000); // 1 MB/s

        let optimizer = Optimizer::from_config(config.optimization);
        assert_eq!(optimizer.speed_limit, Some(1_000_000));
    }

    #[test]
    fn test_optimizer_clone() {
        let config = Config::default();
        let optimizer = Optimizer::from_config(config.optimization);
        let cloned = optimizer.clone();

        assert_eq!(optimizer.speed_limit, cloned.speed_limit);
    }

    #[test]
    fn test_optimizer_max_connections_is_clamped() {
        let mut config = Config::default();
        config.optimization.max_connections = 0;
        assert_eq!(
            Optimizer::from_config(config.optimization.clone()).max_connections(),
            1
        );

        config.optimization.max_connections = 128;
        assert_eq!(
            Optimizer::from_config(config.optimization).max_connections(),
            32
        );
    }
}

// ============================================================================
// App Contract Tests
// ============================================================================

mod app_contract_tests {
    use kget::app::WorkerToGuiMessage;

    #[test]
    fn test_worker_message_display_is_stable() {
        assert_eq!(
            WorkerToGuiMessage::Progress(0.42).to_string(),
            "Progress(0.42)"
        );
        assert_eq!(
            WorkerToGuiMessage::StatusUpdate("Ready".to_string()).to_string(),
            "Status: Ready"
        );
        assert_eq!(
            WorkerToGuiMessage::Completed("/tmp/file.bin".to_string()).to_string(),
            "Completed: /tmp/file.bin"
        );
        assert_eq!(
            WorkerToGuiMessage::Error("boom".to_string()).to_string(),
            "Error: boom"
        );
    }
}

// ============================================================================
// Torrent Contract Tests
// ============================================================================

mod torrent_contract_tests {
    use kget::torrent::is_supported_magnet_link;

    #[test]
    fn test_supported_magnet_link_detection() {
        assert!(is_supported_magnet_link(
            "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn=file"
        ));
        assert!(is_supported_magnet_link(
            "magnet:?xt=urn:btmh:1220abcdef&dn=file"
        ));
        assert!(!is_supported_magnet_link(
            "https://example.com/file.torrent"
        ));
        assert!(!is_supported_magnet_link("magnet:?dn=missing-info-hash"));
    }
}

// ============================================================================
// Progress Bar Tests
// ============================================================================

mod progress_tests {
    use kget::create_progress_bar;

    #[test]
    fn test_create_progress_bar_quiet_mode() {
        let bar = create_progress_bar(true, "Downloading...".to_string(), Some(1000), false);
        // In quiet mode, bar should be hidden (we can't directly test this, but it shouldn't panic)
        bar.finish();
    }

    #[test]
    fn test_create_progress_bar_with_length() {
        let bar = create_progress_bar(
            false,
            "Downloading file.zip".to_string(),
            Some(1024 * 1024),
            false,
        );
        assert_eq!(bar.length(), Some(1024 * 1024));
        bar.finish();
    }

    #[test]
    fn test_create_progress_bar_spinner() {
        let bar = create_progress_bar(false, "Connecting...".to_string(), None, false);
        // Spinner mode (no length)
        assert_eq!(bar.length(), None);
        bar.finish();
    }

    #[test]
    fn test_create_progress_bar_parallel_mode() {
        let bar = create_progress_bar(false, "Parallel download".to_string(), Some(5000), true);
        assert_eq!(bar.length(), Some(5000));
        bar.finish();
    }

    #[test]
    fn test_progress_bar_increment() {
        let bar = create_progress_bar(true, "Test".to_string(), Some(100), false);
        bar.inc(50);
        assert_eq!(bar.position(), 50);
        bar.inc(50);
        assert_eq!(bar.position(), 100);
        bar.finish();
    }
}

// ============================================================================
// URL Parsing Edge Cases
// ============================================================================

mod url_edge_cases {
    use kget::get_filename_from_url_or_default;

    #[test]
    fn test_magnet_link() {
        let url = "magnet:?xt=urn:btih:1234567890abcdef&dn=test";
        let filename = get_filename_from_url_or_default(url, "torrent_download");
        // Magnet links don't have a traditional path
        assert_eq!(filename, "torrent_download");
    }

    #[test]
    fn test_very_long_filename() {
        let long_name = "a".repeat(255);
        let url = format!("https://example.com/{}.txt", long_name);
        let filename = get_filename_from_url_or_default(&url, "default.txt");
        assert_eq!(filename, format!("{}.txt", long_name));
    }

    #[test]
    fn test_special_characters_in_url() {
        let url = "https://example.com/file[1](2).txt";
        let filename = get_filename_from_url_or_default(url, "default.txt");
        // Should handle special chars
        assert!(filename.contains("file"));
    }

    #[test]
    fn test_sftp_url() {
        let url = "sftp://user@host.com/path/to/file.dat";
        let filename = get_filename_from_url_or_default(url, "default.dat");
        assert_eq!(filename, "file.dat");
    }

    #[test]
    fn test_file_url() {
        let url = "file:///home/user/downloads/local_file.txt";
        let filename = get_filename_from_url_or_default(url, "default.txt");
        assert_eq!(filename, "local_file.txt");
    }
}
