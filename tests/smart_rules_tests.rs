//! Smart rules (matching, templates, config, `--rules`), history tags,
//! duplicate detection / `--dedupe`, and rule suggestions (heuristic and the
//! opt-in AI path against a fake Messages API — never the real network).

use assert_cmd::Command;
use kget::Config;
use kget::protocol::Protocol;
use kget::queue::{EntryStatus, HistoryEntry, HistoryQuery, HistoryStore, SCHEMA_VERSION};
use kget::rules::suggest::{self, UsageStats};
use kget::rules::{self, Facts, RemoteInfo, Rule, RuleAction, RuleMatch};
use std::path::Path;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn kget_in(dir: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("kget"));
    cmd.env("KGET_CONFIG_DIR", dir);
    cmd.env("KGET_KEYCHAIN", "off");
    cmd.env_remove("ANTHROPIC_API_KEY");
    cmd
}

fn rule(name: &str, when: RuleMatch, folder: &str) -> Rule {
    Rule {
        name: name.into(),
        when,
        action: RuleAction {
            folder: Some(folder.into()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn first<'a>(rules: &'a [Rule], url: &str) -> Option<&'a str> {
    let mut f = Facts::from_url(url, kget::protocol::detect(url));
    rules::first_match(rules, &mut f, &mut || None).map(|r| r.name.as_str())
}

// ============================================================================
// Matching and templates
// ============================================================================

#[test]
fn first_match_wins_in_order_and_user_rules_precede_builtins() {
    let user_rules = vec![
        rule(
            "talks",
            RuleMatch {
                domains: vec!["conf.example.org".into()],
                ..Default::default()
            },
            "Talks",
        ),
        rule(
            "example-video",
            RuleMatch {
                domains: vec!["example.org".into()],
                categories: vec!["video".into()],
                ..Default::default()
            },
            "Example",
        ),
        Rule {
            enabled: false,
            ..rule(
                "disabled",
                RuleMatch {
                    extensions: vec!["mp4".into()],
                    ..Default::default()
                },
                "Never",
            )
        },
    ];
    let mut config = Config {
        rules: user_rules,
        ..Default::default()
    };
    let active = rules::active_rules(&config);
    // Subdomain match on the first rule wins over the second.
    assert_eq!(
        first(&active, "https://conf.example.org/a.mp4"),
        Some("talks")
    );
    // A subdomain of example.org (www. ignored) with a video extension.
    assert_eq!(
        first(&active, "https://www.cdn.example.org/b.mkv"),
        Some("example-video")
    );
    // Disabled rule skipped; the built-in Videos rule comes after user rules.
    assert_eq!(
        first(&active, "https://other.net/c.mp4"),
        Some("builtin:videos")
    );
    assert_eq!(
        first(&active, "https://other.net/d.iso"),
        Some("builtin:disk-images")
    );
    assert_eq!(first(&active, "https://other.net/e.unknownext"), None);
    // "notexample.org" is not a subdomain of example.org.
    assert_eq!(
        first(&active, "https://notexample.org/c.txt"),
        Some("builtin:documents")
    );

    config.builtin_rules = false;
    assert_eq!(
        first(&rules::active_rules(&config), "https://other.net/c.mp4"),
        None
    );
}

#[test]
fn regex_glob_protocol_mime_and_size_conditions() {
    let rules = vec![
        rule(
            "regex",
            RuleMatch {
                url_regex: Some(r"/releases/v\d+".into()),
                ..Default::default()
            },
            "R",
        ),
        rule(
            "glob",
            RuleMatch {
                url_glob: Some("ftp://*.example.com/pub/*".into()),
                protocols: vec!["ftp".into()],
                ..Default::default()
            },
            "G",
        ),
        rule(
            "big-pdf",
            RuleMatch {
                mime: vec!["application/pdf".into()],
                min_size: Some(1_000_000),
                ..Default::default()
            },
            "BigPDF",
        ),
    ];
    assert_eq!(
        first(&rules, "https://x.io/releases/v12/app.bin"),
        Some("regex")
    );
    assert_eq!(
        first(&rules, "FTP://mirror.example.com/pub/f"),
        Some("glob")
    );
    // Without a probe, MIME/size are unknown: no match.
    assert_eq!(first(&rules, "https://x.io/doc"), None);

    // The probe runs once, only for the rule that needs it.
    let mut calls = 0;
    let mut facts = Facts::from_url("https://x.io/doc", Protocol::Http);
    let hit = rules::first_match(&rules, &mut facts, &mut || {
        calls += 1;
        Some(RemoteInfo {
            mime: Some("application/pdf".into()),
            size: Some(5_000_000),
        })
    });
    assert_eq!(hit.map(|r| r.name.as_str()), Some("big-pdf"));
    assert_eq!(calls, 1);
    // The MIME type also gives a category for extension-less URLs.
    assert_eq!(facts.category(), Some("document"));

    let mut calls = 0;
    let mut facts = Facts::from_url("https://x.io/releases/v1/a", Protocol::Http);
    rules::first_match(&rules, &mut facts, &mut || {
        calls += 1;
        None
    });
    assert_eq!(calls, 0, "no probe when an earlier rule matches on the URL");
}

#[test]
fn templates_render_and_never_escape_the_root() {
    let f = Facts::from_url("https://dl.Example.com/x/Movie.MKV?a=1", Protocol::Http);
    let p = rules::render_folder("{category}/{domain}/{ext}", &f).unwrap();
    assert_eq!(p, Path::new("video/dl.example.com/mkv"));
    let year = chrono::Local::now().format("%Y").to_string();
    assert_eq!(
        rules::render_folder("By date/{date:%Y}", &f).unwrap(),
        Path::new("By date").join(year)
    );
    for bad in [
        "../up",
        "a/../../b",
        "/abs",
        "\\abs",
        "~/home",
        "C:\\x",
        "x/./..",
        "{domain}/..",
        "{bogus}",
        "{date:%Q}",
        "unclosed{",
        "",
        "NUL",
    ] {
        assert!(rules::validate_template(bad).is_err(), "accepted {bad:?}");
    }
    // A hostile host name cannot inject separators.
    let f = Facts {
        domain: Some("../../etc".into()),
        ..f
    };
    let p = rules::render_folder("{domain}", &f).unwrap();
    assert_eq!(p.components().count(), 1);
    assert!(!p.to_string_lossy().contains('/'));
}

#[test]
fn validation_rejects_bad_rules() {
    let ok = rule("ok", RuleMatch::default(), "Folder");
    assert!(ok.validate().is_ok());
    let cases = [
        rule("", RuleMatch::default(), "F"),
        rule("x", RuleMatch::default(), "../F"),
        rule(
            "x",
            RuleMatch {
                url_regex: Some("(".into()),
                ..Default::default()
            },
            "F",
        ),
        rule(
            "x",
            RuleMatch {
                categories: vec!["spaceships".into()],
                ..Default::default()
            },
            "F",
        ),
        rule(
            "x",
            RuleMatch {
                min_size: Some(10),
                max_size: Some(1),
                ..Default::default()
            },
            "F",
        ),
        Rule {
            action: RuleAction {
                connections: Some(99),
                ..Default::default()
            },
            ..rule("x", RuleMatch::default(), "F")
        },
        Rule {
            action: RuleAction {
                tags: vec!["a,b".into()],
                ..Default::default()
            },
            ..rule("x", RuleMatch::default(), "F")
        },
    ];
    for r in cases {
        assert!(r.validate().is_err(), "accepted {r:?}");
    }
}

#[test]
fn config_round_trip_and_defaults() {
    let mut config = Config::default();
    assert!(!config.rules_enabled);
    assert!(config.builtin_rules);
    config.rules_enabled = true;
    config.organize_dir = Some("~/Downloads".into());
    let (r, pos) = rules::parse_rule_spec(&[
        "name=ISOs",
        "domain=ubuntu.com,debian.org",
        "ext=.iso",
        "min-size=100M",
        "folder=ISOs/{domain}/{date:%Y-%m}",
        "tag=Linux",
        "checksum=required",
        "connections=8",
        "limit=5M",
        "position=1",
    ])
    .unwrap();
    assert_eq!(pos, Some(1));
    assert_eq!(r.when.extensions, vec!["iso"]);
    assert_eq!(r.when.min_size, Some(100 << 20));
    assert_eq!(r.action.tags, vec!["linux"]);
    assert!(r.action.require_checksum);
    rules::add_rule(&mut config, r.clone(), pos).unwrap();
    assert!(
        rules::add_rule(&mut config, r, None).is_err(),
        "duplicate name"
    );

    let json = serde_json::to_string_pretty(&config).unwrap();
    assert!(json.contains("\"match\""));
    let back: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(back.rules, config.rules);
    assert!(back.rules_enabled);
    assert_eq!(back.organize_dir.as_deref(), Some("~/Downloads"));

    // Old config files (no rule fields) keep loading with defaults.
    let old: Config = serde_json::from_str(r#"{"ytdlp_path":null}"#).unwrap();
    assert!(old.builtin_rules && !old.rules_enabled && old.rules.is_empty());

    // JSON form of `rules add`.
    let (j, _) = rules::parse_rule_spec(&[
        r#"{"name":"j","match":{"extensions":["epub"]},"action":{"folder":"Books"}}"#,
    ])
    .unwrap();
    assert_eq!(j.action.folder.as_deref(), Some("Books"));
    assert!(rules::parse_rule_spec(&["name=x", "folder=../../etc"]).is_err());
    assert!(rules::parse_rule_spec(&["name=builtin:x"]).is_ok());
    assert!(rules::add_rule(&mut config, j.clone(), None).is_ok());
}

// ============================================================================
// `kget --rules`
// ============================================================================

#[test]
fn rules_cli_add_list_test_enable_remove() {
    let dir = tempfile::tempdir().unwrap();
    let out = kget_in(dir.path())
        .args(["--rules", "test", "https://cdn.example.net/films/movie.mkv"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Rule:      builtin:videos"), "{text}");
    assert!(
        text.contains(&format!("Videos{}movie.mkv", std::path::MAIN_SEPARATOR)),
        "{text}"
    );
    assert!(text.contains("category=video"), "{text}");
    assert!(text.contains("Organizing is off"), "{text}");

    kget_in(dir.path())
        .args([
            "--rules",
            "add",
            "name=films",
            "domain=example.net",
            "folder=Films/{domain}",
            "tag=cinema",
        ])
        .assert()
        .success();
    kget_in(dir.path())
        .args(["--rules", "enable"])
        .assert()
        .success();
    let text = String::from_utf8(
        kget_in(dir.path())
            .args(["--rules", "test", "https://cdn.example.net/films/movie.mkv"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert!(text.contains("Rule:      films (#1)"), "{text}");
    assert!(text.contains("cdn.example.net"), "{text}");
    assert!(text.contains("tags=cinema"), "{text}");
    assert!(!text.contains("Organizing is off"), "{text}");

    let list = String::from_utf8(
        kget_in(dir.path())
            .args(["--rules", "list"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert!(list.starts_with("Organizing: on"), "{list}");
    assert!(list.contains("#1  films"), "{list}");
    assert!(list.contains("builtin:software"), "{list}");

    // Invalid template rejected; built-ins cannot be removed one by one.
    kget_in(dir.path())
        .args(["--rules", "add", "name=bad", "folder=../x"])
        .assert()
        .failure();
    kget_in(dir.path())
        .args(["--rules", "remove", "builtin:videos"])
        .assert()
        .failure();
    kget_in(dir.path())
        .args(["--rules", "disable", "films"])
        .assert()
        .success();
    let saved: Config =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("config.json")).unwrap())
            .unwrap();
    assert!(saved.rules_enabled);
    assert!(!saved.rules[0].enabled);
    kget_in(dir.path())
        .args(["--rules", "remove", "#1"])
        .assert()
        .success();
    let saved: Config =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("config.json")).unwrap())
            .unwrap();
    assert!(saved.rules.is_empty());
}

// ============================================================================
// History tags
// ============================================================================

#[test]
fn v1_database_gains_tags_column() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("history.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE history (
                seq INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE, url TEXT NOT NULL,
                filename TEXT NOT NULL, output_dir TEXT NOT NULL, status TEXT NOT NULL,
                domain TEXT NOT NULL DEFAULT '', protocol TEXT, bytes_total INTEGER,
                sha256 TEXT, expected_sha256 TEXT, checksum_ok INTEGER,
                created_at INTEGER NOT NULL, finished_at INTEGER, duration_ms INTEGER,
                avg_speed INTEGER, error TEXT);
             INSERT INTO history (id, url, filename, output_dir, status, domain, created_at)
             VALUES ('old1', 'https://a.com/x.iso', 'x.iso', '/tmp', 'completed', 'a.com', 5);
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let mut store = HistoryStore::open(&db).unwrap();
    let all = store.all().unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].tags.is_empty());
    let version: i32 = rusqlite::Connection::open(&db)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);

    let mut e = HistoryEntry::new("https://b.com/y.mkv", "/tmp", None);
    e.tags = vec!["Movies".into(), "family".into(), "movies".into()];
    store.insert(&e).unwrap();
    let mut e2 = HistoryEntry::new("https://b.com/z.mkv", "/tmp", None);
    e2.tags = vec!["movies-old".into()];
    store.insert(&e2).unwrap();

    let hits = store.query(&HistoryQuery::new().tag("movies")).unwrap();
    assert_eq!(hits.len(), 1, "exact tag only, not a prefix");
    assert_eq!(hits[0].tags, vec!["movies", "family"]);
    assert_eq!(
        store
            .query(&HistoryQuery::new().tag("MOVIES").tag("family"))
            .unwrap()
            .len(),
        1
    );
    assert!(
        store
            .query(&HistoryQuery::new().tag("movies").tag("nope"))
            .unwrap()
            .is_empty()
    );
    let json = serde_json::to_value(&hits[0]).unwrap();
    assert_eq!(json["tags"], serde_json::json!(["movies", "family"]));
}

// ============================================================================
// Downloads: rules, tags, duplicates, dedupe (wiremock + the binary)
// ============================================================================

async fn serve(server: &MockServer, name: &str, body: &[u8]) {
    Mock::given(method("GET"))
        .and(path(format!("/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .mount(server)
        .await;
}

fn gets(requests: &[wiremock::Request], name: &str) -> usize {
    requests
        .iter()
        .filter(|r| r.method.as_str() == "GET" && r.url.path() == format!("/{name}"))
        .count()
}

fn jsonl_events(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn organize_tags_and_explicit_output_override() {
    let server = MockServer::start().await;
    serve(&server, "clip.mp4", b"video bytes").await;
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let url = format!("{}/clip.mp4", server.uri());

    let out = kget_in(cfg.path())
        .current_dir(work.path())
        .args(["--organize", "--tag", "Demo", "--jsonl", &url])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(work.path().join("Videos/clip.mp4").is_file());
    let events = jsonl_events(&out.stdout);
    let rule = events.iter().find(|e| e["event"] == "rule").unwrap();
    assert_eq!(rule["name"], "builtin:videos");
    assert_eq!(rule["tags"], serde_json::json!(["demo"]));

    // -O always wins over the rule folder.
    kget_in(cfg.path())
        .current_dir(work.path())
        .args(["--organize", "-q", "-O", "mine.mp4", &url])
        .assert()
        .success();
    assert!(work.path().join("mine.mp4").is_file());

    let hist = kget_in(cfg.path())
        .args(["--history", "--json", "--tag", "demo"])
        .output()
        .unwrap();
    let list: Vec<serde_json::Value> = serde_json::from_slice(&hist.stdout).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["tags"], serde_json::json!(["demo"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rule_requiring_a_checksum_refuses_to_download() {
    let server = MockServer::start().await;
    serve(&server, "img.iso", b"iso").await;
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    kget_in(cfg.path())
        .args(["--rules", "add", "name=isos", "ext=iso", "checksum=true"])
        .assert()
        .success();
    let out = kget_in(cfg.path())
        .current_dir(work.path())
        .args([
            "--organize",
            "--jsonl",
            &format!("{}/img.iso", server.uri()),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let events = jsonl_events(&out.stdout);
    assert!(events.iter().any(|e| {
        e["event"] == "error"
            && e["message"]
                .as_str()
                .unwrap_or("")
                .contains("requires a checksum")
    }));
    assert_eq!(
        gets(&server.received_requests().await.unwrap(), "img.iso"),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_policies() {
    let server = MockServer::start().await;
    serve(&server, "data.bin", b"0123456789").await;
    let cfg = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let url = format!("{}/data.bin", server.uri());
    let run = |extra: &[&str]| {
        let mut args = vec!["--jsonl"];
        args.extend_from_slice(extra);
        args.push(&url);
        kget_in(cfg.path())
            .current_dir(work.path())
            .args(&args)
            .output()
            .unwrap()
    };

    // First download; not a terminal, so the default policy is "download".
    assert!(run(&[]).status.success());
    assert!(run(&[]).status.success());
    assert_eq!(
        gets(&server.received_requests().await.unwrap(), "data.bin"),
        2
    );

    // skip: no request, a duplicate event pointing at the existing file.
    let out = run(&["--duplicates", "skip"]);
    assert!(out.status.success());
    let ev = jsonl_events(&out.stdout);
    let dup = ev.iter().find(|e| e["event"] == "duplicate").unwrap();
    assert_eq!(dup["kind"], "url");
    assert_eq!(dup["action"], "skip");
    assert_eq!(dup["v"], 1);
    assert!(dup["path"].as_str().unwrap().ends_with("data.bin"));
    assert!(ev.iter().any(|e| e["event"] == "completed"));
    assert_eq!(
        gets(&server.received_requests().await.unwrap(), "data.bin"),
        2
    );

    // ask without a terminal falls back to downloading.
    assert!(run(&["--duplicates", "ask"]).status.success());
    assert_eq!(
        gets(&server.received_requests().await.unwrap(), "data.bin"),
        3
    );

    // link: the existing file appears at the new destination, no request.
    let out = run(&["--duplicates", "link", "-O", "copy/"]);
    assert!(out.status.success());
    let ev = jsonl_events(&out.stdout);
    assert!(
        ev.iter()
            .any(|e| e["event"] == "duplicate" && e["action"] == "link")
    );
    let linked = work.path().join("copy/data.bin");
    assert_eq!(std::fs::read(&linked).unwrap(), b"0123456789");
    assert_eq!(
        gets(&server.received_requests().await.unwrap(), "data.bin"),
        3
    );

    // A changed file is no longer a valid duplicate.
    std::fs::write(work.path().join("data.bin"), b"short").unwrap();
    std::fs::remove_file(&linked).unwrap();
    assert!(run(&["--duplicates", "skip"]).status.success());
    assert_eq!(
        gets(&server.received_requests().await.unwrap(), "data.bin"),
        4
    );

    // Credentials and fragments do not hide a duplicate.
    let with_creds = url.replace("http://", "http://me:pw@") + "#frag";
    let out = kget_in(cfg.path())
        .current_dir(work.path())
        .args(["--jsonl", "--duplicates", "skip", &with_creds])
        .output()
        .unwrap();
    assert!(
        jsonl_events(&out.stdout)
            .iter()
            .any(|e| e["event"] == "duplicate" && e["action"] == "skip")
    );
    assert_eq!(
        gets(&server.received_requests().await.unwrap(), "data.bin"),
        4
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dedupe_replaces_identical_content_with_a_hardlink() {
    use std::os::unix::fs::MetadataExt;
    let server = MockServer::start().await;
    let body = b"same bytes from two mirrors".to_vec();
    serve(&server, "a.bin", &body).await;
    serve(&server, "b.bin", &body).await;
    let cfg = tempfile::tempdir().unwrap();
    // temp_dir is on the internal disk (exFAT has no hard links).
    let work = tempfile::tempdir().unwrap();
    for name in ["a.bin", "b.bin"] {
        let out = kget_in(cfg.path())
            .current_dir(work.path())
            .args(["--dedupe", "--jsonl", &format!("{}/{name}", server.uri())])
            .output()
            .unwrap();
        assert!(out.status.success());
        if name == "b.bin" {
            let ev = jsonl_events(&out.stdout);
            let dup = ev.iter().find(|e| e["event"] == "duplicate").unwrap();
            assert_eq!(dup["kind"], "content");
            assert_eq!(dup["action"], "linked");
            assert!(dup["path"].as_str().unwrap().ends_with("a.bin"));
        }
    }
    let a = std::fs::metadata(work.path().join("a.bin")).unwrap();
    let b = std::fs::metadata(work.path().join("b.bin")).unwrap();
    assert_eq!(a.ino(), b.ino());
    assert_eq!(std::fs::read(work.path().join("b.bin")).unwrap(), body);

    // The SHA-256 is recorded, so `--history --json` can show it.
    let hist = kget_in(cfg.path())
        .args(["--history", "--json"])
        .output()
        .unwrap();
    let list: Vec<serde_json::Value> = serde_json::from_slice(&hist.stdout).unwrap();
    assert!(
        list.iter()
            .all(|e| e["sha256"].as_str().is_some_and(|h| h.len() == 64))
    );
}

#[test]
fn hardlink_helpers() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    std::fs::write(&a, b"x").unwrap();
    std::fs::write(&b, b"x").unwrap();
    kget::dedupe::replace_with_hardlink(&a, &b).unwrap();
    assert_eq!(std::fs::read(&b).unwrap(), b"x");
    let c = dir.path().join("sub/c");
    kget::dedupe::link_or_copy(&a, &c).unwrap();
    assert_eq!(std::fs::read(&c).unwrap(), b"x");
}

// ============================================================================
// Suggestions
// ============================================================================

fn done(url: &str, dir: &str) -> HistoryEntry {
    let mut e = HistoryEntry::new(url, dir, None);
    e.status = EntryStatus::Completed;
    e
}

#[test]
fn heuristic_suggestions() {
    let root = tempfile::tempdir().unwrap();
    let saved = root.path().join("Work/Reports");
    let saved = saved.to_string_lossy();
    let mut entries = Vec::new();
    for i in 0..4 {
        entries.push(done(
            &format!("https://reports.corp.example/r{i}.pdf"),
            &saved,
        ));
    }
    for i in 0..3 {
        entries.push(done(
            &format!("https://site{i}.net/model{i}.stl"),
            "/elsewhere",
        ));
    }
    // Too few to matter.
    entries.push(done("https://rare.org/a.bin", "/x"));
    entries.push(done("https://rare.org/b.bin", "/x"));
    let mut failed = done("https://reports.corp.example/f.pdf", "/x");
    failed.status = EntryStatus::Failed;
    entries.push(failed);

    let s = suggest::heuristic(&entries, &[], root.path());
    let names: Vec<&str> = s.iter().map(|x| x.rule.name.as_str()).collect();
    assert!(names.contains(&"site-reports.corp.example"), "{names:?}");
    assert!(names.contains(&"ext-stl"), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("rare")), "{names:?}");
    let site = s
        .iter()
        .find(|x| x.rule.name == "site-reports.corp.example")
        .unwrap();
    assert_eq!(site.rule.action.folder.as_deref(), Some("Work/Reports"));
    assert!(site.reason.contains("4 of 4"));
    assert!(s.iter().all(|x| x.rule.validate().is_ok()));

    // Already covered by a user rule: not suggested again.
    let existing = vec![rule(
        "mine",
        RuleMatch {
            extensions: vec!["stl".into()],
            ..Default::default()
        },
        "3D",
    )];
    let s = suggest::heuristic(&entries, &existing, root.path());
    assert!(!s.iter().any(|x| x.rule.name == "ext-stl"));

    let stats = UsageStats::from_history(&entries);
    assert_eq!(stats.downloads, 9);
    assert_eq!(stats.domains[0], ("reports.corp.example".to_string(), 4));
    let json = serde_json::to_string(&stats).unwrap();
    assert!(!json.contains("http"), "no URLs in the stats");
    assert!(
        !json.contains("r0.pdf") && !json.contains("Reports"),
        "no names or paths"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ai_suggestions_are_validated_and_only_stats_are_sent() {
    let server = MockServer::start().await;
    let reply = r#"Here you go:
[
 {"name":"3d-models","match":{"extensions":["stl"]},"action":{"folder":"3D/{domain}","tags":["3D"],"connections":32,"require_checksum":true}},
 {"name":"escape","match":{"domains":["x.com"]},"action":{"folder":"../../etc"}},
 {"name":"bad-category","match":{"categories":["warp"]},"action":{"folder":"W"}},
 "not a rule"
]"#;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": suggest::AI_MODEL,
            "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": reply}
            ]
        })))
        .mount(&server)
        .await;

    let entries = vec![
        done("https://secret.example/private/plan.stl", "/home/me/Secret"),
        done(
            "https://secret.example/private/plan2.stl",
            "/home/me/Secret",
        ),
    ];
    let stats = UsageStats::from_history(&entries);
    let uri = server.uri();
    let (rules, rejected) = tokio::task::spawn_blocking(move || {
        suggest::ai_suggest(&stats, "test-key", &uri, &kget::ProxyConfig::default())
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(rejected, 3);
    assert_eq!(rules.len(), 1);
    let r = &rules[0].rule;
    assert_eq!(r.name, "ai-3d-models");
    assert_eq!(r.action.folder.as_deref(), Some("3D/{domain}"));
    assert_eq!(r.action.tags, vec!["3d"]);
    // Only folders and tags are taken from the model.
    assert_eq!(r.action.connections, None);
    assert!(!r.action.require_checksum);

    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(body["model"], "claude-haiku-5-5");
    let sent = body.to_string();
    assert!(sent.contains("secret.example"));
    assert!(!sent.contains("plan.stl") && !sent.contains("/private") && !sent.contains("Secret"));

    // API errors are reported, not swallowed.
    let bad = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "authentication_error", "message": "invalid x-api-key"}
        })))
        .mount(&bad)
        .await;
    let uri = bad.uri();
    let err = tokio::task::spawn_blocking(move || {
        suggest::ai_suggest(
            &UsageStats::default(),
            "k",
            &uri,
            &kget::ProxyConfig::default(),
        )
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(err.to_string().contains("invalid x-api-key"));
}

#[test]
fn ai_flag_needs_a_key_and_never_runs_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    kget_in(dir.path())
        .args(["--rules", "suggest", "--ai"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("ANTHROPIC_API_KEY"));
    kget_in(dir.path())
        .env("ANTHROPIC_API_KEY", "x")
        .env("KGET_ANTHROPIC_BASE_URL", "http://127.0.0.1:9")
        .args(["--rules", "suggest", "--ai"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("terminal"));
    kget_in(dir.path())
        .args(["--rules", "suggest"])
        .assert()
        .success()
        .stdout(predicates::str::contains("No suggestions"));
}
