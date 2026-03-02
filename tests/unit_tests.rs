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
            "default.zip"
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
        assert_eq!(config.proxy.url, Some("http://proxy.example.com:8080".to_string()));
    }
}

// ============================================================================
// Download Module Tests
// ============================================================================

mod download_tests {
    use kget::download::{validate_filename, check_disk_space};
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
    }

    #[test]
    fn test_download_options_custom() {
        let options = DownloadOptions {
            quiet_mode: true,
            output_path: Some("/tmp/download.iso".to_string()),
            verify_iso: true,
        };
        
        assert!(options.quiet_mode);
        assert_eq!(options.output_path, Some("/tmp/download.iso".to_string()));
        assert!(options.verify_iso);
    }

    #[test]
    fn test_download_options_clone() {
        let original = DownloadOptions {
            quiet_mode: true,
            output_path: Some("file.txt".to_string()),
            verify_iso: false,
        };
        
        let cloned = original.clone();
        assert_eq!(original.quiet_mode, cloned.quiet_mode);
        assert_eq!(original.output_path, cloned.output_path);
        assert_eq!(original.verify_iso, cloned.verify_iso);
    }
}

// ============================================================================
// Optimizer Tests
// ============================================================================

mod optimizer_tests {
    use kget::{Optimizer, Config};

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
        let bar = create_progress_bar(false, "Downloading file.zip".to_string(), Some(1024 * 1024), false);
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
