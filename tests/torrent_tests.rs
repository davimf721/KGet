//! Torrent module tests for KGet
//!
//! These tests verify torrent-related functionality including magnet link parsing,
//! native torrent client (when feature enabled), and configuration.

// ============================================================================
// Magnet Link Parsing Tests
// ============================================================================

mod magnet_parsing_tests {
    /// Extract display name from magnet link (simulating the parsing logic)
    fn extract_dn_from_magnet(magnet: &str) -> Option<String> {
        if !magnet.starts_with("magnet:") {
            return None;
        }

        if let Some(dn_start) = magnet.find("dn=") {
            let after_dn = &magnet[dn_start + 3..];
            let name = if let Some(amp_pos) = after_dn.find('&') {
                &after_dn[..amp_pos]
            } else {
                after_dn
            };
            Some(
                urlencoding::decode(name)
                    .unwrap_or_else(|_| name.into())
                    .to_string(),
            )
        } else {
            None
        }
    }

    /// Extract info hash from magnet link
    fn extract_hash_from_magnet(magnet: &str) -> Option<String> {
        if let Some(xt_start) = magnet.find("xt=urn:btih:") {
            let after_xt = &magnet[xt_start + 12..];
            let hash = if let Some(amp_pos) = after_xt.find('&') {
                &after_xt[..amp_pos]
            } else {
                after_xt
            };
            Some(hash.to_string())
        } else {
            None
        }
    }

    #[test]
    fn test_extract_dn_simple() {
        let magnet = "magnet:?xt=urn:btih:abc123&dn=MyTorrent&tr=http://tracker.com";
        let name = extract_dn_from_magnet(magnet);
        assert_eq!(name, Some("MyTorrent".to_string()));
    }

    #[test]
    fn test_extract_dn_url_encoded() {
        let magnet = "magnet:?xt=urn:btih:abc123&dn=My%20Awesome%20Torrent";
        let name = extract_dn_from_magnet(magnet);
        assert_eq!(name, Some("My Awesome Torrent".to_string()));
    }

    #[test]
    fn test_extract_dn_with_special_chars() {
        let magnet = "magnet:?xt=urn:btih:abc123&dn=File%5B2024%5D%20-%20Test";
        let name = extract_dn_from_magnet(magnet);
        assert_eq!(name, Some("File[2024] - Test".to_string()));
    }

    #[test]
    fn test_extract_dn_at_end() {
        let magnet = "magnet:?xt=urn:btih:abc123&dn=FinalName";
        let name = extract_dn_from_magnet(magnet);
        assert_eq!(name, Some("FinalName".to_string()));
    }

    #[test]
    fn test_extract_dn_missing() {
        let magnet = "magnet:?xt=urn:btih:abc123&tr=http://tracker.com";
        let name = extract_dn_from_magnet(magnet);
        assert_eq!(name, None);
    }

    #[test]
    fn test_extract_hash_v1() {
        let magnet = "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn=Test";
        let hash = extract_hash_from_magnet(magnet);
        assert_eq!(
            hash,
            Some("1234567890abcdef1234567890abcdef12345678".to_string())
        );
    }

    #[test]
    fn test_extract_hash_base32() {
        // Base32 encoded hash (common in magnet links)
        let magnet = "magnet:?xt=urn:btih:CIHGJHVTSVMVFYHFNJBH63T7O3D2E4N7";
        let hash = extract_hash_from_magnet(magnet);
        assert_eq!(hash, Some("CIHGJHVTSVMVFYHFNJBH63T7O3D2E4N7".to_string()));
    }

    #[test]
    fn test_not_magnet_link() {
        let url = "https://example.com/file.torrent";
        let name = extract_dn_from_magnet(url);
        assert_eq!(name, None);
    }

    #[test]
    fn test_magnet_link_detection() {
        assert!("magnet:?xt=urn:btih:abc".starts_with("magnet:"));
        assert!(!"https://example.com".starts_with("magnet:"));
        assert!(!"magnet".starts_with("magnet:"));
    }
}

// ============================================================================
// Torrent Configuration Tests
// ============================================================================

mod torrent_config_tests {
    use kget::Config;

    #[test]
    fn test_torrent_config_defaults() {
        let config = Config::default();

        assert!(!config.torrent.enabled);
        assert!(config.torrent.dht_enabled);
        assert_eq!(config.torrent.max_peers, 50);
        assert_eq!(config.torrent.max_seeds, 25);
        assert_eq!(config.torrent.max_peer_connections, 50);
        assert_eq!(config.torrent.max_upload_slots, 4);
        assert!(config.torrent.port.is_none());
        assert!(config.torrent.download_dir.is_none());
    }

    #[test]
    fn test_torrent_config_custom_values() {
        let json = r#"{
            "proxy": { "enabled": false, "proxy_type": "Http", "host": "", "port": 0, "username": "", "password": "" },
            "optimization": { "compression": true, "compression_level": 6, "cache_enabled": true, "cache_dir": "~/.cache/kget", "max_connections": 4 },
            "torrent": {
                "enabled": true,
                "download_dir": "/downloads/torrents",
                "max_peers": 100,
                "max_seeds": 50,
                "port": 6881,
                "dht_enabled": false,
                "max_peer_connections": 200,
                "max_upload_slots": 8
            },
            "ftp": { "passive_mode": true, "default_port": 21 },
            "sftp": { "default_port": 22 }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();

        assert!(config.torrent.enabled);
        assert!(!config.torrent.dht_enabled);
        assert_eq!(config.torrent.max_peers, 100);
        assert_eq!(config.torrent.max_seeds, 50);
        assert_eq!(config.torrent.port, Some(6881));
        assert_eq!(
            config.torrent.download_dir,
            Some("/downloads/torrents".to_string())
        );
    }

    #[test]
    fn test_torrent_config_serialization_roundtrip() {
        let mut config = Config::default();
        config.torrent.enabled = true;
        config.torrent.max_peers = 75;
        config.torrent.port = Some(51413);

        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(config.torrent.enabled, loaded.torrent.enabled);
        assert_eq!(config.torrent.max_peers, loaded.torrent.max_peers);
        assert_eq!(config.torrent.port, loaded.torrent.port);
    }
}

// ============================================================================
// Torrent Backend Selection Tests
// ============================================================================

mod backend_selection_tests {
    use std::env;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn get_selected_backend() -> String {
        env::var("KGET_TORRENT_BACKEND")
            .unwrap_or_else(|_| {
                #[cfg(feature = "torrent-native")]
                {
                    "native".to_string()
                }
                #[cfg(not(feature = "torrent-native"))]
                {
                    "external".to_string()
                }
            })
            .to_lowercase()
    }

    #[test]
    fn test_default_backend_selection() {
        let _guard = env_lock().lock().unwrap();
        // Clear env var temporarily
        let original = env::var("KGET_TORRENT_BACKEND").ok();
        // SAFETY: We're in a single-threaded test context
        unsafe {
            env::remove_var("KGET_TORRENT_BACKEND");
        }

        let backend = get_selected_backend();

        #[cfg(feature = "torrent-native")]
        assert_eq!(backend, "native");

        #[cfg(not(feature = "torrent-native"))]
        assert_eq!(backend, "external");

        // Restore
        if let Some(val) = original {
            // SAFETY: We're in a single-threaded test context
            unsafe {
                env::set_var("KGET_TORRENT_BACKEND", val);
            }
        }
    }

    #[test]
    fn test_env_backend_override() {
        let _guard = env_lock().lock().unwrap();
        let original = env::var("KGET_TORRENT_BACKEND").ok();

        // SAFETY: We're in a single-threaded test context
        unsafe {
            env::set_var("KGET_TORRENT_BACKEND", "transmission");
        }
        assert_eq!(get_selected_backend(), "transmission");

        unsafe {
            env::set_var("KGET_TORRENT_BACKEND", "NATIVE");
        }
        assert_eq!(get_selected_backend(), "native"); // Should lowercase

        unsafe {
            env::set_var("KGET_TORRENT_BACKEND", "External");
        }
        assert_eq!(get_selected_backend(), "external");

        // Restore
        // SAFETY: Restoring original environment state
        unsafe {
            match original {
                Some(val) => env::set_var("KGET_TORRENT_BACKEND", val),
                None => env::remove_var("KGET_TORRENT_BACKEND"),
            }
        }
    }
}

// ============================================================================
// Progress Calculation Tests (simulating native torrent progress)
// ============================================================================

mod progress_calculation_tests {
    /// Calculate progress from file progress percentages
    fn calculate_total_progress(file_sizes: &[u64], file_progress: &[f64]) -> f64 {
        if file_sizes.is_empty() || file_progress.is_empty() {
            return 0.0;
        }

        let total_size: u64 = file_sizes.iter().sum();
        if total_size == 0 {
            return 0.0;
        }

        let downloaded: f64 = file_sizes
            .iter()
            .zip(file_progress.iter())
            .map(|(&size, &progress)| size as f64 * progress / 100.0)
            .sum();

        (downloaded / total_size as f64) * 100.0
    }

    #[test]
    fn test_progress_single_file_complete() {
        let sizes = vec![1000];
        let progress = vec![100.0];
        assert!((calculate_total_progress(&sizes, &progress) - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_single_file_partial() {
        let sizes = vec![1000];
        let progress = vec![50.0];
        assert!((calculate_total_progress(&sizes, &progress) - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_multiple_files_equal_size() {
        let sizes = vec![1000, 1000, 1000, 1000];
        let progress = vec![100.0, 50.0, 0.0, 50.0];
        // (1000*1.0 + 1000*0.5 + 1000*0.0 + 1000*0.5) / 4000 = 2000/4000 = 50%
        assert!((calculate_total_progress(&sizes, &progress) - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_weighted_by_size() {
        // Large file 100% complete, small file 0%
        let sizes = vec![9000, 1000];
        let progress = vec![100.0, 0.0];
        // 9000 / 10000 = 90%
        assert!((calculate_total_progress(&sizes, &progress) - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_empty() {
        assert_eq!(calculate_total_progress(&[], &[]), 0.0);
    }

    #[test]
    fn test_progress_zero_total_size() {
        let sizes = vec![0, 0];
        let progress = vec![100.0, 100.0];
        assert_eq!(calculate_total_progress(&sizes, &progress), 0.0);
    }
}

// ============================================================================
// ETA Calculation Tests
// ============================================================================

mod eta_calculation_tests {
    fn format_eta(remaining_bytes: u64, speed_bytes_per_sec: f64) -> String {
        if speed_bytes_per_sec <= 0.0 || remaining_bytes == 0 {
            return "--".to_string();
        }

        let seconds = (remaining_bytes as f64 / speed_bytes_per_sec) as u64;

        if seconds < 60 {
            format!("{}s", seconds)
        } else if seconds < 3600 {
            format!("{}m {}s", seconds / 60, seconds % 60)
        } else if seconds < 86400 {
            format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
        } else {
            format!("{}d {}h", seconds / 86400, (seconds % 86400) / 3600)
        }
    }

    #[test]
    fn test_eta_seconds() {
        assert_eq!(format_eta(100, 10.0), "10s");
        assert_eq!(format_eta(50, 10.0), "5s");
        assert_eq!(format_eta(59, 1.0), "59s");
    }

    #[test]
    fn test_eta_minutes() {
        assert_eq!(format_eta(120, 1.0), "2m 0s");
        assert_eq!(format_eta(90, 1.0), "1m 30s");
        assert_eq!(format_eta(3599, 1.0), "59m 59s");
    }

    #[test]
    fn test_eta_hours() {
        assert_eq!(format_eta(3600, 1.0), "1h 0m");
        assert_eq!(format_eta(7200, 1.0), "2h 0m");
        assert_eq!(format_eta(5400, 1.0), "1h 30m");
    }

    #[test]
    fn test_eta_days() {
        assert_eq!(format_eta(86400, 1.0), "1d 0h");
        assert_eq!(format_eta(172800, 1.0), "2d 0h");
        assert_eq!(format_eta(129600, 1.0), "1d 12h");
    }

    #[test]
    fn test_eta_zero_speed() {
        assert_eq!(format_eta(1000, 0.0), "--");
    }

    #[test]
    fn test_eta_zero_remaining() {
        assert_eq!(format_eta(0, 100.0), "--");
    }

    #[test]
    fn test_eta_negative_speed() {
        assert_eq!(format_eta(1000, -10.0), "--");
    }
}

// ============================================================================
// Size Formatting Tests
// ============================================================================

mod size_formatting_tests {
    fn format_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TiB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GiB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MiB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KiB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    fn format_speed(bytes_per_sec: f64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;

        if bytes_per_sec >= MB {
            format!("{:.2} MiB/s", bytes_per_sec / MB)
        } else if bytes_per_sec >= KB {
            format!("{:.2} KiB/s", bytes_per_sec / KB)
        } else {
            format!("{:.0} B/s", bytes_per_sec)
        }
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_kilobytes() {
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1536), "1.50 KiB");
        assert_eq!(format_size(10240), "10.00 KiB");
    }

    #[test]
    fn test_format_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size(1024 * 1024 + 512 * 1024), "1.50 MiB");
        assert_eq!(format_size(100 * 1024 * 1024), "100.00 MiB");
    }

    #[test]
    fn test_format_gigabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(
            format_size(4 * 1024 * 1024 * 1024 + 512 * 1024 * 1024),
            "4.50 GiB"
        );
    }

    #[test]
    fn test_format_terabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024 * 1024), "1.00 TiB");
    }

    #[test]
    fn test_format_speed_bps() {
        assert_eq!(format_speed(100.0), "100 B/s");
        assert_eq!(format_speed(1023.0), "1023 B/s");
    }

    #[test]
    fn test_format_speed_kbps() {
        assert_eq!(format_speed(1024.0), "1.00 KiB/s");
        assert_eq!(format_speed(5120.0), "5.00 KiB/s");
    }

    #[test]
    fn test_format_speed_mbps() {
        assert_eq!(format_speed(1024.0 * 1024.0), "1.00 MiB/s");
        assert_eq!(format_speed(10.0 * 1024.0 * 1024.0), "10.00 MiB/s");
    }
}

// ============================================================================
// Native Torrent Tests (feature-gated)
// ============================================================================

#[cfg(feature = "torrent-native")]
mod native_torrent_tests {
    #[test]
    fn test_native_feature_enabled() {
        // This test only compiles when torrent-native feature is enabled
        assert!(true);
    }

    #[test]
    fn test_output_dir_creation() {
        use std::fs;
        use tempfile::TempDir;

        let temp = TempDir::new().unwrap();
        let output_dir = temp.path().join("torrent_output");

        // Simulate directory creation
        fs::create_dir_all(&output_dir).unwrap();
        assert!(output_dir.exists());
        assert!(output_dir.is_dir());
    }

    #[test]
    fn test_file_info_structure() {
        // Simulating the file info structure used in native torrent
        #[derive(Debug, Clone)]
        struct FileInfo {
            name: String,
            size: u64,
        }

        let files = vec![
            FileInfo {
                name: "video.mp4".to_string(),
                size: 1024 * 1024 * 100,
            },
            FileInfo {
                name: "readme.txt".to_string(),
                size: 1024,
            },
            FileInfo {
                name: "data/file.bin".to_string(),
                size: 1024 * 1024 * 50,
            },
        ];

        assert_eq!(files.len(), 3);
        assert_eq!(files[0].name, "video.mp4");

        let total_size: u64 = files.iter().map(|f| f.size).sum();
        assert_eq!(total_size, 1024 * 1024 * 150 + 1024);
    }

    #[test]
    fn test_download_stats_computation() {
        struct DownloadStats {
            total_bytes: u64,
            downloaded_bytes: u64,
            upload_bytes: u64,
            peers: u32,
            seeds: u32,
        }

        impl DownloadStats {
            fn progress_percent(&self) -> f64 {
                if self.total_bytes == 0 {
                    0.0
                } else {
                    (self.downloaded_bytes as f64 / self.total_bytes as f64) * 100.0
                }
            }
        }

        let stats = DownloadStats {
            total_bytes: 1000,
            downloaded_bytes: 500,
            upload_bytes: 100,
            peers: 10,
            seeds: 5,
        };

        assert!((stats.progress_percent() - 50.0).abs() < 0.01);
        assert_eq!(stats.upload_bytes, 100);
        assert_eq!(stats.peers, 10);
        assert_eq!(stats.seeds, 5);
    }
}

// ============================================================================
// Torrent File JSON Output Tests
// ============================================================================

mod json_output_tests {
    use serde_json::json;

    #[test]
    fn test_files_json_format() {
        let files = vec![
            json!({"name": "file1.txt", "size": 1024}),
            json!({"name": "file2.mp4", "size": 1048576}),
        ];

        let json_str = serde_json::to_string(&files).unwrap();

        assert!(json_str.contains("file1.txt"));
        assert!(json_str.contains("1024"));
        assert!(json_str.contains("file2.mp4"));
        assert!(json_str.contains("1048576"));
    }

    #[test]
    fn test_file_progress_json_format() {
        let progress = vec![
            json!({"idx": 0, "downloaded": 512, "pct": 50.0}),
            json!({"idx": 1, "downloaded": 1048576, "pct": 100.0}),
        ];

        let json_str = serde_json::to_string(&progress).unwrap();

        // Should be parseable as array
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["idx"], 0);
        assert_eq!(parsed[0]["pct"], 50.0);
        assert_eq!(parsed[1]["pct"], 100.0);
    }

    #[test]
    fn test_progress_output_line_format() {
        let progress = 45.5;
        let downloaded = "45.5 MiB";
        let total = "100.0 MiB";
        let speed = "5.2 MiB/s";
        let eta = "10m 30s";

        let line = format!(
            "PROGRESS: {:.1}% ({}/{}) SPEED: {} ETA: {}",
            progress, downloaded, total, speed, eta
        );

        assert!(line.starts_with("PROGRESS:"));
        assert!(line.contains("45.5%"));
        assert!(line.contains("SPEED:"));
        assert!(line.contains("ETA:"));
    }
}
