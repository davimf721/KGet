//! Tests that change process-wide environment variables (`KGET_CONFIG_DIR`,
//! `KGET_YTDLP_PATH`, `PATH`). They live in their own test binary and run
//! sequentially inside a single test to avoid races.

use kget::queue::{DownloadHistory, EntryStatus, HistoryEntry};

fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: this binary runs a single test, so no other thread reads the
    // environment concurrently.
    unsafe { std::env::set_var(key, value) };
}

#[cfg(unix)]
fn fake_executable(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, "#!/bin/sh\necho fake\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn environment_dependent_behaviour() {
    let cfg = tempfile::tempdir().unwrap();
    set_env("KGET_CONFIG_DIR", cfg.path());

    // ── config dir override (P0-10) ──────────────────────────────────────────
    assert_eq!(kget::config::config_dir().unwrap(), cfg.path());
    let mut config = kget::Config::default();
    config.optimization.max_connections = 9;
    config.save().unwrap();
    assert!(cfg.path().join("config.json").exists());
    assert_eq!(
        kget::Config::load().unwrap().optimization.max_connections,
        9
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(cfg.path().join("config.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "config may hold proxy credentials");
    }
    std::fs::remove_file(cfg.path().join("config.json")).unwrap();

    // ── history: concurrent writers keep every entry (P1-7) ──────────────────
    let handles: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let e = HistoryEntry::new(&format!("https://example.com/{i}.bin"), "/tmp", None);
                DownloadHistory::append(e, EntryStatus::Completed, None).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let history = DownloadHistory::load();
    assert_eq!(history.entries().len(), 20);
    let mut ids: Vec<_> = history.entries().iter().map(|e| e.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 20, "ids are unique");

    // ── history: credentials are redacted ────────────────────────────────────
    let e = HistoryEntry::new("sftp://alice:s3cret@host/file", "/tmp", None);
    DownloadHistory::append(e, EntryStatus::Failed, Some("x".into())).unwrap();
    let raw = std::fs::read_to_string(cfg.path().join("history.json")).unwrap();
    assert!(!raw.contains("s3cret"));
    assert!(!raw.contains("alice"));

    // ── history: a corrupt file is preserved, not wiped ──────────────────────
    std::fs::write(cfg.path().join("history.json"), "[{broken").unwrap();
    let e = HistoryEntry::new("https://example.com/new.bin", "/tmp", None);
    DownloadHistory::append(e, EntryStatus::Completed, None).unwrap();
    assert!(cfg.path().join("history.json.corrupt").exists());
    assert_eq!(DownloadHistory::load().entries().len(), 1);

    // ── yt-dlp lookup (P0-5) ─────────────────────────────────────────────────
    #[cfg(unix)]
    {
        let bin = tempfile::tempdir().unwrap();
        let fake = fake_executable(bin.path(), "my-yt-dlp");
        set_env("KGET_YTDLP_PATH", &fake);
        assert_eq!(
            kget::ytdlp::locate_ytdlp().as_deref(),
            Some(fake.to_str().unwrap()),
            "KGET_YTDLP_PATH wins"
        );

        // Not in PATH, found in a PATH directory searched without `which`.
        unsafe { std::env::remove_var("KGET_YTDLP_PATH") };
        let in_path = fake_executable(bin.path(), "yt-dlp");
        set_env("PATH", bin.path());
        assert_eq!(
            kget::ytdlp::locate_ytdlp().as_deref(),
            Some(in_path.to_str().unwrap())
        );
        assert!(
            kget::ytdlp::ytdlp_search_dirs()
                .iter()
                .any(|d| d.ends_with("homebrew/bin"))
        );
        assert!(kget::ytdlp::ytdlp_not_found_message().contains("KGET_YTDLP_PATH"));
    }
}
