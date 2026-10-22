//! SQLite download history: migration from `history.json`, queries,
//! concurrency and corruption handling.
//!
//! Every test opens its own database in a temporary directory through
//! `HistoryStore::open`, so nothing touches the user's config dir.

use kget::queue::{
    EntryStatus, HistoryEntry, HistoryQuery, HistorySort, HistoryStore, MAX_ENTRIES,
    SCHEMA_VERSION, parse_date_bound,
};
use std::path::Path;

fn entry(url: &str, status: EntryStatus, created_at: u64, bytes: Option<u64>) -> HistoryEntry {
    let mut e = HistoryEntry::new(url, "/tmp", None);
    e.status = status;
    e.created_at = created_at;
    e.finished_at = Some(created_at + 5);
    e.bytes_total = bytes;
    e
}

fn ids(v: &[HistoryEntry]) -> Vec<String> {
    v.iter().map(|e| e.filename.clone()).collect()
}

fn raw_bytes(dir: &Path) -> Vec<u8> {
    let mut all = Vec::new();
    for name in ["history.db", "history.db-wal"] {
        if let Ok(b) = std::fs::read(dir.join(name)) {
            all.extend(b);
        }
    }
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn migrates_history_json_once_and_keeps_backup() {
    let dir = tempfile::tempdir().unwrap();
    // Format written by KGet 1.7 (no protocol/duration/speed/checksum fields)
    let json = r#"[
      {"id":"a1","url":"https://example.com/one.iso","filename":"one.iso","output_dir":"/d",
       "status":"completed","bytes_total":100,"sha256":null,"expected_sha256":null,
       "created_at":1000,"finished_at":1010,"error":null},
      {"id":"a2","url":"sftp://bob:pa55w0rd@host.example.org/two.zip","filename":"two.zip",
       "output_dir":"/d","status":"failed","bytes_total":null,"sha256":null,
       "expected_sha256":null,"created_at":2000,"finished_at":null,"error":"boom"},
      {"id":"a3","url":"magnet:?xt=urn:btih:abc","filename":"abc","output_dir":"/d",
       "status":"handed_off","bytes_total":null,"sha256":null,"expected_sha256":null,
       "created_at":3000,"finished_at":3000,"error":null}
    ]"#;
    std::fs::write(dir.path().join("history.json"), json).unwrap();
    std::fs::write(dir.path().join("history.lock"), "").unwrap();

    let store = HistoryStore::open(dir.path().join("history.db")).unwrap();
    let all = store.all().unwrap();
    assert_eq!(ids(&all), ["one.iso", "two.zip", "abc"], "order is kept");
    assert_eq!(all[0].bytes_total, Some(100));
    assert_eq!(all[1].status, EntryStatus::Failed);
    assert_eq!(all[1].error.as_deref(), Some("boom"));
    assert_eq!(all[2].status, EntryStatus::HandedOff);
    assert_eq!(all[1].url, "sftp://host.example.org/two.zip");
    assert!(!contains(&raw_bytes(dir.path()), "pa55w0rd"));
    assert!(!dir.path().join("history.json").exists());
    assert!(dir.path().join("history.json.bak").exists());
    assert!(!dir.path().join("history.lock").exists());
    drop(store);

    // Reopening does not import again
    let store = HistoryStore::open(dir.path().join("history.db")).unwrap();
    assert_eq!(store.all().unwrap().len(), 3);
    drop(store);

    // An older kget writing history.json after the upgrade: merged, no duplicates
    let json2 = r#"[
      {"id":"a1","url":"https://example.com/one.iso","filename":"one.iso","output_dir":"/d",
       "status":"completed","bytes_total":100,"sha256":null,"expected_sha256":null,
       "created_at":1000,"finished_at":1010,"error":null},
      {"id":"b1","url":"https://example.com/new.bin","filename":"new.bin","output_dir":"/d",
       "status":"completed","bytes_total":1,"sha256":null,"expected_sha256":null,
       "created_at":4000,"finished_at":4000,"error":null}
    ]"#;
    std::fs::write(dir.path().join("history.json"), json2).unwrap();
    let store = HistoryStore::open(dir.path().join("history.db")).unwrap();
    assert_eq!(
        ids(&store.all().unwrap()),
        ["one.iso", "two.zip", "abc", "new.bin"]
    );
}

#[test]
fn corrupt_json_is_preserved_not_imported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("history.json"), "[{broken").unwrap();
    let store = HistoryStore::open(dir.path().join("history.db")).unwrap();
    assert!(store.all().unwrap().is_empty());
    assert!(dir.path().join("history.json.corrupt").exists());
}

#[test]
fn corrupt_database_is_moved_aside() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("history.db");
    std::fs::write(&db, vec![0x42u8; 8192]).unwrap();
    let mut store = HistoryStore::open(&db).unwrap();
    store
        .insert(&entry(
            "https://example.com/a",
            EntryStatus::Completed,
            1,
            None,
        ))
        .unwrap();
    assert_eq!(store.all().unwrap().len(), 1);
    assert!(dir.path().join("history.db.corrupt").exists());
}

#[test]
fn schema_version_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("history.db");
    drop(HistoryStore::open(&db).unwrap());
    let conn = rusqlite_version(&db);
    assert_eq!(conn, SCHEMA_VERSION);
}

/// Read `PRAGMA user_version` straight from the file header (bytes 60..64).
fn rusqlite_version(db: &Path) -> i32 {
    let bytes = std::fs::read(db).unwrap();
    i32::from_be_bytes(bytes[60..64].try_into().unwrap())
}

#[test]
fn extra_columns_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = HistoryStore::open(dir.path().join("history.db")).unwrap();
    let mut e = entry(
        "ftp://files.example.com/pub/a.tar.gz",
        EntryStatus::Completed,
        10,
        Some(4096),
    );
    e.duration_ms = Some(2000);
    e.avg_speed = Some(2048);
    e.checksum_ok = Some(false);
    e.sha256 = Some("ab".into());
    store.insert(&e).unwrap();
    let got = store.get(&e.id[..8]).unwrap().expect("found by id prefix");
    assert_eq!(got.protocol.as_deref(), Some("ftp"));
    assert_eq!(got.duration_ms, Some(2000));
    assert_eq!(got.avg_speed, Some(2048));
    assert_eq!(got.checksum_ok, Some(false));
    assert_eq!(got.sha256.as_deref(), Some("ab"));
    assert_eq!(got.domain().as_deref(), Some("files.example.com"));
    assert_eq!(got.extension().as_deref(), Some("gz"));
    assert!(store.get("zzzz").unwrap().is_none());
}

fn sample_store(dir: &Path) -> HistoryStore {
    let mut store = HistoryStore::open(dir.join("history.db")).unwrap();
    let day = 86_400;
    let rows = [
        (
            "https://example.com/Ubuntu.iso",
            EntryStatus::Completed,
            10 * day,
            Some(5000),
        ),
        (
            "https://dl.example.com/movie.MKV",
            EntryStatus::Failed,
            11 * day,
            None,
        ),
        (
            "https://notexample.com/song.mp3",
            EntryStatus::Completed,
            12 * day,
            Some(10),
        ),
        (
            "https://www.other.org/backup.tar.gz",
            EntryStatus::Cancelled,
            13 * day,
            Some(700),
        ),
        (
            "magnet:?xt=urn:btih:abcdef&dn=linux",
            EntryStatus::HandedOff,
            14 * day,
            None,
        ),
        (
            "https://example.com/report.pdf",
            EntryStatus::Failed,
            15 * day,
            Some(20),
        ),
    ];
    for (url, status, at, bytes) in rows {
        store.insert(&entry(url, status, at, bytes)).unwrap();
    }
    store
}

#[test]
fn query_filters() {
    let dir = tempfile::tempdir().unwrap();
    let store = sample_store(dir.path());
    let q = |q: HistoryQuery| ids(&store.query(&q).unwrap());

    // default: everything, newest first
    assert_eq!(
        q(HistoryQuery::new()),
        [
            "report.pdf",
            "download",
            "backup.tar.gz",
            "song.mp3",
            "movie.MKV",
            "Ubuntu.iso"
        ]
    );
    // status (OR between statuses)
    assert_eq!(
        q(HistoryQuery::new().status(EntryStatus::Failed)),
        ["report.pdf", "movie.MKV"]
    );
    assert_eq!(
        q(HistoryQuery::new()
            .status(EntryStatus::Cancelled)
            .status(EntryStatus::HandedOff)),
        ["download", "backup.tar.gz"]
    );
    // domain matches subdomains but not look-alikes; www. is ignored
    assert_eq!(
        q(HistoryQuery::new().domain("Example.com")),
        ["report.pdf", "movie.MKV", "Ubuntu.iso"]
    );
    assert_eq!(
        q(HistoryQuery::new().domain("https://www.other.org/")),
        ["backup.tar.gz"]
    );
    // extensions and categories (case-insensitive, compound extensions)
    assert_eq!(q(HistoryQuery::new().extension(".ISO")), ["Ubuntu.iso"]);
    assert_eq!(
        q(HistoryQuery::new().extension("tar.gz")),
        ["backup.tar.gz"]
    );
    assert_eq!(q(HistoryQuery::new().file_type("video")), ["movie.MKV"]);
    assert_eq!(
        q(HistoryQuery::new().file_type("iso,mp3")),
        ["song.mp3", "Ubuntu.iso"]
    );
    // protocol
    assert_eq!(q(HistoryQuery::new().protocol("torrent")), ["download"]);
    // date range: since inclusive, until exclusive
    let day = 86_400;
    assert_eq!(
        q(HistoryQuery::new().since(12 * day).until(14 * day)),
        ["backup.tar.gz", "song.mp3"]
    );
    // text search on url and filename, case-insensitive
    assert_eq!(q(HistoryQuery::new().search("UBUNTU")), ["Ubuntu.iso"]);
    assert_eq!(q(HistoryQuery::new().search("btih")), ["download"]);
    // combined filters
    assert_eq!(
        q(HistoryQuery::new()
            .status(EntryStatus::Failed)
            .domain("example.com")
            .since(14 * day)),
        ["report.pdf"]
    );
    // sorting
    assert_eq!(
        q(HistoryQuery::new().sort(HistorySort::Largest).limit(3)),
        ["Ubuntu.iso", "backup.tar.gz", "report.pdf"]
    );
    assert_eq!(
        q(HistoryQuery::new().sort(HistorySort::Name).limit(2)),
        ["backup.tar.gz", "download"]
    );
    assert_eq!(
        q(HistoryQuery::new().sort(HistorySort::Oldest).limit(1)),
        ["Ubuntu.iso"]
    );
    // paging
    assert_eq!(
        q(HistoryQuery::new().offset(1).limit(2)),
        ["download", "backup.tar.gz"]
    );
    assert_eq!(q(HistoryQuery::new().offset(5)), ["Ubuntu.iso"]);
    // count ignores paging
    assert_eq!(
        store
            .count(&HistoryQuery::new().domain("example.com").limit(1))
            .unwrap(),
        3
    );
    // SQL wildcards in user input are plain text
    assert!(q(HistoryQuery::new().search("%")).is_empty());
    assert!(q(HistoryQuery::new().domain("%")).is_empty());
}

#[test]
fn delete_and_update() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = sample_store(dir.path());
    let removed = store
        .delete(&HistoryQuery::new().status(EntryStatus::Failed))
        .unwrap();
    assert_eq!(removed, 2);
    assert_eq!(store.all().unwrap().len(), 4);

    // delete honours sort + limit: drop only the oldest entry
    let removed = store
        .delete(&HistoryQuery::new().sort(HistorySort::Oldest).limit(1))
        .unwrap();
    assert_eq!(removed, 1);
    assert_eq!(
        ids(&store.all().unwrap()),
        ["song.mp3", "backup.tar.gz", "download"]
    );

    // update: remove, modify in place, append
    store
        .update(|entries| {
            entries.retain(|e| e.filename != "download");
            entries[0].error = Some("edited".into());
            entries.push(HistoryEntry::new("https://example.com/z.bin", "/tmp", None));
        })
        .unwrap();
    let all = store.all().unwrap();
    assert_eq!(ids(&all), ["song.mp3", "backup.tar.gz", "z.bin"]);
    assert_eq!(all[0].error.as_deref(), Some("edited"));

    store.replace_all(&all[1..]).unwrap();
    assert_eq!(ids(&store.all().unwrap()), ["backup.tar.gz", "z.bin"]);
}

#[test]
fn keeps_only_the_newest_max_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = HistoryStore::open(dir.path().join("history.db")).unwrap();
    let entries: Vec<_> = (0..MAX_ENTRIES + 5)
        .map(|i| {
            let mut e = HistoryEntry::new(&format!("https://example.com/{i}"), "/tmp", None);
            e.id = format!("id{i}");
            e
        })
        .collect();
    store.replace_all(&entries).unwrap();
    let all = store.all().unwrap();
    assert_eq!(all.len(), MAX_ENTRIES);
    assert_eq!(all[0].id, "id5");
}

#[test]
fn concurrent_writers_from_many_threads() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("history.db");
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let db = db.clone();
            std::thread::spawn(move || {
                for i in 0..25 {
                    // A fresh connection per write, like separate processes
                    let mut store = HistoryStore::open(&db).unwrap();
                    let e = HistoryEntry::new(&format!("https://example.com/{t}/{i}"), "/", None);
                    store.insert(&e).unwrap();
                    // Readers never block writers in WAL mode
                    let _ = store.count(&HistoryQuery::new()).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let store = HistoryStore::open(&db).unwrap();
    let all = store.all().unwrap();
    assert_eq!(all.len(), 200);
    let mut ids: Vec<_> = all.iter().map(|e| e.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 200, "ids are unique");
}

#[test]
fn credentials_never_reach_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = HistoryStore::open(dir.path().join("history.db")).unwrap();
    let mut e = HistoryEntry::new("https://alice:s3cret@example.com/f", "/tmp", None);
    assert_eq!(e.url, "https://example.com/f");
    // Even if a caller puts credentials back, insert redacts them.
    e.url = "ftp://carol:t0ps3cret@example.com/g".into();
    store.insert(&e).unwrap();
    let raw = raw_bytes(dir.path());
    assert!(!contains(&raw, "s3cret"));
    assert!(!contains(&raw, "t0ps3cret"));
    assert!(!contains(&raw, "carol"));
}

#[test]
fn parsing_helpers() {
    assert_eq!(parse_date_bound("1970-01-02", false).unwrap(), 86_400);
    assert_eq!(parse_date_bound("1970-01-02", true).unwrap(), 2 * 86_400);
    assert_eq!(parse_date_bound("1970-01-01T00:01:05", false).unwrap(), 65);
    assert_eq!(
        parse_date_bound("1700000000", false).unwrap(),
        1_700_000_000
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let week = parse_date_bound("7d", false).unwrap();
    assert!(now - week >= 7 * 86_400 && now - week < 7 * 86_400 + 5);
    assert!(parse_date_bound("today", false).unwrap() <= now);
    assert!(parse_date_bound("yesterday", true).unwrap() <= now);
    assert!(parse_date_bound("next tuesday", false).is_err());

    assert_eq!("done".parse::<EntryStatus>(), Ok(EntryStatus::Completed));
    assert_eq!(
        "Canceled".parse::<EntryStatus>(),
        Ok(EntryStatus::Cancelled)
    );
    assert_eq!(
        "handed-off".parse::<EntryStatus>(),
        Ok(EntryStatus::HandedOff)
    );
    assert!("bogus".parse::<EntryStatus>().is_err());
    assert_eq!("size".parse::<HistorySort>(), Ok(HistorySort::Largest));
    assert!("bogus".parse::<HistorySort>().is_err());
}
