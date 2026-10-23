//! `kget daemon` tests: the real binary in the foreground, driven over its
//! socket with [`DaemonClient`] against a wiremock HTTP server.
//!
//! Every daemon gets its own config, runtime and download directories under
//! the system temp dir (Unix sockets cannot live on every filesystem), so the
//! tests never touch the developer's config or a running daemon.

use kget::daemon::DaemonPaths;
use kget::daemon::client::DaemonClient;
use kget::daemon::protocol::{SubmitParams, SubscribeParams, codes};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BIN: &str = env!("CARGO_BIN_EXE_kget");

/// Directories of one test daemon.
struct Dirs {
    _root: TempDir,
    config: PathBuf,
    runtime: PathBuf,
    downloads: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("kd")
            .tempdir_in(std::env::temp_dir())
            .unwrap();
        let base = root.path().to_path_buf();
        let d = Self {
            config: base.join("c"),
            runtime: base.join("r"),
            downloads: base.join("dl"),
            _root: root,
        };
        std::fs::create_dir_all(&d.config).unwrap();
        d
    }

    fn paths(&self) -> DaemonPaths {
        DaemonPaths::in_dirs(&self.config, &self.runtime)
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(BIN);
        cmd.env("KGET_CONFIG_DIR", &self.config)
            .env("KGET_RUNTIME_DIR", &self.runtime)
            .env("KGET_KEYCHAIN", "off")
            .env_remove("KGET_DAEMON_TRANSPORT");
        cmd
    }
}

/// A daemon process, killed on drop.
struct Daemon {
    child: Child,
}

impl Daemon {
    fn start(dirs: &Dirs, extra_env: &[(&str, &str)], extra_args: &[&str]) -> Self {
        let mut cmd = dirs.command();
        cmd.args(["daemon", "--foreground", "--download-dir"])
            .arg(&dirs.downloads)
            .args(extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().expect("spawn kget daemon");
        let d = Self { child };
        let deadline = Instant::now() + Duration::from_secs(20);
        while DaemonClient::connect(&dirs.paths()).is_err() {
            assert!(Instant::now() < deadline, "daemon did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        d
    }

    /// Ask it to stop and wait for the process to exit.
    fn shutdown(mut self, dirs: &Dirs) {
        DaemonClient::connect(&dirs.paths())
            .unwrap()
            .shutdown()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "daemon exit status {status}");
                return;
            }
            assert!(Instant::now() < deadline, "daemon did not stop");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn client(dirs: &Dirs) -> DaemonClient {
    DaemonClient::connect(&dirs.paths()).expect("connect to daemon")
}

async fn mount(server: &MockServer, name: &str, body: &[u8]) {
    Mock::given(method("GET"))
        .and(path(format!("/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.to_vec()))
        .mount(server)
        .await;
}

fn wait_state(c: &DaemonClient, id: u64, states: &[&str]) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let j = c.get(id).unwrap();
        if states.contains(&j["state"].as_str().unwrap_or("")) {
            return j;
        }
        assert!(
            Instant::now() < deadline,
            "job {id} stuck in {}",
            j["state"]
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn submit_streams_events_until_completed() {
    let server = MockServer::start().await;
    let body = vec![7u8; 200_000];
    mount(&server, "data.bin", &body).await;
    let url = format!("{}/data.bin", server.uri());

    tokio::task::spawn_blocking(move || {
        let dirs = Dirs::new();
        let daemon = Daemon::start(&dirs, &[], &[]);
        let c = client(&dirs);

        let v = c.call("version", Value::Null).unwrap();
        assert_eq!(v["protocol"], 1);
        // Unix sockets by default; Windows falls back to token-guarded TCP.
        let expected_transport = if cfg!(unix) { "unix" } else { "tcp" };
        assert_eq!(v["transport"], expected_transport);

        let events = c.events(&SubscribeParams::default()).unwrap();
        let mut params = SubmitParams::new(url);
        params.dir = Some("sub".into());
        let job = c.submit(&params).unwrap();
        let id = job["id"].as_u64().unwrap();
        assert_eq!(id, 1);

        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let Ok(ev) = events.recv_timeout(Duration::from_secs(1)) else {
                continue;
            };
            let done = ev["type"] == "state" && ev["state"] == "completed";
            seen.push(ev);
            if done {
                break;
            }
        }
        let kinds: Vec<String> = seen
            .iter()
            .map(|e| match e["type"].as_str().unwrap() {
                "state" => format!("state:{}", e["state"].as_str().unwrap()),
                "download" => format!("download:{}", e["event"]["event"].as_str().unwrap()),
                other => other.to_string(),
            })
            .collect();
        for expected in [
            "state:queued",
            "state:running",
            "download:started",
            "download:saved",
            "download:completed",
            "state:completed",
        ] {
            assert!(
                kinds.iter().any(|k| k == expected),
                "{expected} in {kinds:?}"
            );
        }

        let info = c.get(id).unwrap();
        let saved = PathBuf::from(info["path"].as_str().unwrap());
        assert!(saved.starts_with(dirs.downloads.canonicalize().unwrap().join("sub")));
        assert_eq!(std::fs::read(&saved).unwrap(), body);

        // The CLI client sees the same queue.
        let out = dirs
            .command()
            .args(["queue", "list", "--json"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let list: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(list[0]["state"], "completed");

        // remove forgets it.
        c.call("remove", json!({"id": id})).unwrap();
        assert_eq!(
            c.get(id).unwrap_err().code,
            codes::JOB_NOT_FOUND,
            "removed job"
        );
        daemon.shutdown(&dirs);
        assert!(!dirs.paths().socket().exists(), "socket removed on exit");
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[test]
fn socket_and_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dirs = Dirs::new();
    let _daemon = Daemon::start(&dirs, &[], &[]);
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let paths = dirs.paths();
    assert_eq!(mode(&paths.socket()), 0o600, "socket");
    assert_eq!(mode(&paths.runtime_dir), 0o700, "runtime dir");
    assert_eq!(mode(&paths.database()), 0o600, "jobs.db");
    assert_eq!(mode(&paths.log()), 0o600, "daemon.log");
    assert_eq!(mode(&paths.pid()), 0o600, "pid file");
}

#[test]
fn rejects_paths_outside_the_download_dir() {
    let dirs = Dirs::new();
    let _daemon = Daemon::start(&dirs, &[], &[]);
    let c = client(&dirs);
    let outside = tempfile::tempdir().unwrap();
    let cases: Vec<(Value, i64)> = vec![
        (
            json!({"url": "https://example.com/f", "dir": "../escape"}),
            codes::PATH_NOT_ALLOWED,
        ),
        (
            json!({"url": "https://example.com/f", "dir": "a/../../escape"}),
            codes::PATH_NOT_ALLOWED,
        ),
        (
            json!({"url": "https://example.com/f", "dir": outside.path()}),
            codes::PATH_NOT_ALLOWED,
        ),
        (
            json!({"url": "https://example.com/f", "filename": "../../x"}),
            codes::PATH_NOT_ALLOWED,
        ),
        (
            json!({"url": "https://example.com/f", "filename": "/etc/passwd"}),
            codes::PATH_NOT_ALLOWED,
        ),
        (json!({"url": "file:///etc/passwd"}), codes::INVALID_PARAMS),
        (
            json!({"url": "https://example.com/f", "bogus": true}),
            codes::INVALID_PARAMS,
        ),
        (json!({"dir": "x"}), codes::INVALID_PARAMS),
    ];
    for (params, code) in cases {
        let err = c.call("submit", params.clone()).unwrap_err();
        assert_eq!(err.code, code, "{params}: {err}");
    }
    assert!(c.list().unwrap().is_empty(), "nothing was queued");
    assert_eq!(
        std::fs::read_dir(outside.path()).unwrap().count(),
        0,
        "nothing written outside"
    );
    // The CLI reports the same error.
    let out = dirs
        .command()
        .args(["queue", "add", "https://example.com/f", "-O"])
        .arg(outside.path().join("f"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("outside the allowed directory"));
}

#[test]
fn raw_protocol_errors() {
    let dirs = Dirs::new();
    let _daemon = Daemon::start(&dirs, &[], &[]);
    #[cfg(unix)]
    {
        let stream = std::os::unix::net::UnixStream::connect(dirs.paths().socket()).unwrap();
        let mut w = stream.try_clone().unwrap();
        let mut r = BufReader::new(stream);
        let mut send = |line: &str| -> Value {
            // The daemon stops reading an oversized line, so writing may fail.
            let _ = w
                .write_all(line.as_bytes())
                .and_then(|_| w.write_all(b"\n"));
            let mut resp = String::new();
            r.read_line(&mut resp).unwrap();
            serde_json::from_str(&resp).unwrap()
        };
        assert_eq!(send("not json")["error"]["code"], codes::PARSE_ERROR);
        assert_eq!(
            send(r#"{"id":1,"method":"version"}"#)["error"]["code"],
            codes::INVALID_REQUEST
        );
        assert_eq!(
            send(r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#)["error"]["code"],
            codes::METHOD_NOT_FOUND
        );
        let batch = send(
            r#"[{"jsonrpc":"2.0","id":3,"method":"version"},{"jsonrpc":"2.0","id":4,"method":"list"}]"#,
        );
        assert_eq!(batch.as_array().unwrap().len(), 2);
        assert_eq!(batch[1]["result"]["jobs"], json!([]));
        // Oversized requests are refused and the connection closed.
        let huge = format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"version","params":"{}"}}"#,
            "x".repeat(2 << 20)
        );
        assert_eq!(send(&huge)["error"]["code"], codes::REQUEST_TOO_LARGE);
    }
}

#[test]
fn tcp_transport_requires_the_token() {
    let dirs = Dirs::new();
    let _daemon = Daemon::start(&dirs, &[("KGET_DAEMON_TRANSPORT", "tcp")], &[]);
    let endpoint_file = dirs.paths().tcp_endpoint();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&endpoint_file)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let ep: Value =
        serde_json::from_str(&std::fs::read_to_string(&endpoint_file).unwrap()).unwrap();
    let port = ep["port"].as_u64().unwrap() as u16;

    let exchange = |lines: &[String]| -> Vec<Value> {
        let s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        let mut out = Vec::new();
        for l in lines {
            if w.write_all(format!("{l}\n").as_bytes()).is_err() {
                break;
            }
            let mut resp = String::new();
            if r.read_line(&mut resp).unwrap_or(0) == 0 {
                break;
            }
            out.push(serde_json::from_str(&resp).unwrap());
        }
        out
    };
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"list"}"#.to_string();
    // No auth: refused and disconnected.
    let r = exchange(&[list.clone(), list.clone()]);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["error"]["code"], codes::UNAUTHORIZED);
    // Wrong token: refused.
    let bad = r#"{"jsonrpc":"2.0","id":1,"method":"auth","params":{"token":"0000"}}"#.to_string();
    let r = exchange(&[bad, list.clone()]);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["error"]["code"], codes::UNAUTHORIZED);
    // Right token: works.
    let good =
        json!({"jsonrpc":"2.0","id":1,"method":"auth","params":{"token": ep["token"]}}).to_string();
    let r = exchange(&[good, list]);
    assert_eq!(r.len(), 2);
    assert_eq!(r[1]["result"]["jobs"], json!([]));
    // And the client library authenticates by itself.
    assert_eq!(
        client(&dirs).call("version", Value::Null).unwrap()["transport"],
        "tcp"
    );
}

#[test]
fn second_instance_refuses_to_start() {
    let dirs = Dirs::new();
    let _daemon = Daemon::start(&dirs, &[], &[]);
    let out = dirs
        .command()
        .args(["daemon", "--foreground", "--download-dir"])
        .arg(&dirs.downloads)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("already running"));
    // The first one still answers.
    client(&dirs).call("version", Value::Null).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_survives_a_restart() {
    let server = MockServer::start().await;
    let body = b"resumed after restart".to_vec();
    // The first request hangs (the daemon is stopped meanwhile); later ones answer.
    Mock::given(method("GET"))
        .and(path("/slow.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.clone())
                .set_delay(Duration::from_secs(60)),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    mount(&server, "slow.bin", &body).await;
    mount(&server, "later.bin", b"later").await;
    let uri = server.uri();

    tokio::task::spawn_blocking(move || {
        let dirs = Dirs::new();
        let daemon = Daemon::start(&dirs, &[], &["--max-concurrent", "1"]);
        let c = client(&dirs);
        let a = c
            .submit(&SubmitParams::new(format!("{uri}/slow.bin")))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let b = c
            .submit(&SubmitParams::new(format!("{uri}/later.bin")))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        wait_state(&c, a, &["running"]);
        c.call("pause", json!({"id": b})).unwrap();
        daemon.shutdown(&dirs);

        let daemon = Daemon::start(&dirs, &[], &["--max-concurrent", "1"]);
        let c = client(&dirs);
        let jobs = c.list().unwrap();
        assert_eq!(jobs.len(), 2, "{jobs:?}");
        // The interrupted job keeps its id and finishes; the paused one stays paused.
        let done = wait_state(&c, a, &["completed", "failed"]);
        assert_eq!(done["state"], "completed", "{done}");
        assert_eq!(std::fs::read(done["path"].as_str().unwrap()).unwrap(), body);
        assert_eq!(c.get(b).unwrap()["state"], "paused");
        c.call("resume", json!({"id": b})).unwrap();
        assert_eq!(
            wait_state(&c, b, &["completed", "failed"])["state"],
            "completed"
        );
        // New ids continue after the restored ones.
        let next = c
            .submit(&SubmitParams::new(format!("{uri}/later.bin")))
            .unwrap();
        assert_eq!(next["id"].as_u64().unwrap(), b + 1);
        assert!(
            next["job"]["output"]
                .as_str()
                .unwrap()
                .ends_with("later (1).bin"),
            "existing file is not overwritten: {next}"
        );
        daemon.shutdown(&dirs);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_uses_a_running_daemon() {
    let server = MockServer::start().await;
    mount(&server, "m.bin", b"via daemon").await;
    let url = format!("{}/m.bin", server.uri());

    tokio::task::spawn_blocking(move || {
        let dirs = Dirs::new();
        let _daemon = Daemon::start(&dirs, &[], &[]);
        let input = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"download","arguments":{"url": url, "dir": "agent"}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_config","arguments":{}}}),
        ]
        .iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>();
        let mut child = dirs
            .command()
            .args(["mcp", "--root"])
            .arg(&dirs.downloads)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // Keep stdin open until both answers arrived.
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut answers = std::collections::HashMap::new();
        while answers.len() < 3 {
            let msg: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            if let Some(id) = msg["id"].as_u64() {
                answers.insert(id, msg);
            }
        }
        drop(stdin);
        assert!(child.wait().unwrap().success());
        let dl = &answers[&2]["result"];
        assert_eq!(dl["isError"], false, "{dl}");
        assert_eq!(dl["structuredContent"]["state"], "completed");
        assert_eq!(answers[&3]["result"]["structuredContent"]["backend"], "daemon");
        // The job lives in the daemon.
        assert_eq!(client(&dirs).list().unwrap().len(), 1);
    })
    .await
    .unwrap();
}
