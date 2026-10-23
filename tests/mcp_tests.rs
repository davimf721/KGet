//! `kget mcp` tests: MCP sessions driven through stdin/stdout of the built
//! binary, with an in-process job manager (no daemon) and a wiremock server.

use assert_cmd::Command as AssertCommand;
use serde_json::{Value, json};
use sha2::Digest;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BIN: &str = env!("CARGO_BIN_EXE_kget");

/// Isolated config/runtime dirs (no daemon is ever found) and a root dir.
struct Env {
    _tmp: TempDir,
    config: std::path::PathBuf,
    runtime: std::path::PathBuf,
    root: std::path::PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::Builder::new()
            .prefix("km")
            .tempdir_in(std::env::temp_dir())
            .unwrap();
        let base = tmp.path().to_path_buf();
        let e = Self {
            config: base.join("c"),
            runtime: base.join("r"),
            root: base.join("root"),
            _tmp: tmp,
        };
        std::fs::create_dir_all(&e.root).unwrap();
        e
    }

    fn apply(&self, cmd: &mut Command) {
        cmd.env("KGET_CONFIG_DIR", &self.config)
            .env("KGET_RUNTIME_DIR", &self.runtime)
            .env("KGET_KEYCHAIN", "off")
            .args(["mcp", "--no-daemon", "--root"])
            .arg(&self.root);
    }
}

fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},
        "clientInfo":{"name":"kget-tests","version":"0"}}})
}

/// Run a whole session at once (stdin closed at the end) and return every
/// output line as JSON.
fn one_shot(env: &Env, messages: &[Value]) -> Vec<Value> {
    let input: String = messages.iter().map(|m| format!("{m}\n")).collect();
    let mut cmd = Command::new(BIN);
    env.apply(&mut cmd);
    let out = AssertCommand::from_std(cmd)
        .write_stdin(input)
        .timeout(Duration::from_secs(60))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("stdout carries only JSON-RPC"))
        .collect()
}

#[test]
fn initialize_and_list_tools() {
    let env = Env::new();
    let out = one_shot(
        &env,
        &[
            initialize(),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":4,"method":"resources/list"}),
        ],
    );
    assert_eq!(out.len(), 4, "notifications get no answer: {out:?}");
    let init = &out[0]["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["serverInfo"]["name"], "kget");
    assert!(init["capabilities"]["tools"].is_object());

    let tools = out[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "download",
        "download_status",
        "list_downloads",
        "cancel_download",
        "pause_download",
        "resume_download",
        "search_history",
        "verify_checksum",
        "get_config",
    ] {
        assert!(names.contains(&expected), "{expected} in {names:?}");
    }
    // Every schema is a closed JSON Schema object whose required keys exist.
    for t in tools {
        let s = &t["inputSchema"];
        assert_eq!(s["type"], "object");
        assert_eq!(s["additionalProperties"], false);
        for r in s["required"].as_array().into_iter().flatten() {
            assert!(s["properties"][r.as_str().unwrap()].is_object());
        }
        assert!(t["description"].as_str().unwrap().len() > 20);
    }
    assert_eq!(out[2]["result"], json!({}));
    assert_eq!(out[3]["error"]["code"], -32601);
}

/// A live session: send messages and read answers as they come.
struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    notifications: Vec<Value>,
}

impl Session {
    fn start(env: &Env) -> Self {
        let mut cmd = Command::new(BIN);
        env.apply(&mut cmd);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let v: Value = serde_json::from_str(&line).expect("JSON on stdout");
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        let mut s = Self {
            child,
            stdin,
            lines,
            notifications: Vec::new(),
        };
        assert!(s.request(initialize())["result"].is_object());
        s.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        s
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Send a request and wait for its answer, keeping notifications.
    fn request(&mut self, msg: Value) -> Value {
        let id = msg["id"].clone();
        self.send(msg);
        loop {
            let v = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("answer");
            if v["id"] == id && v.get("method").is_none() {
                return v;
            }
            self.notifications.push(v);
        }
    }

    fn tool(&mut self, id: u64, name: &str, args: Value) -> Value {
        self.request(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":name,"arguments":args,"_meta":{"progressToken":format!("p{id}")}}}))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_status_verify_and_errors() {
    let server = MockServer::start().await;
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    Mock::given(method("GET"))
        .and(path("/model.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    let url = format!("{}/model.bin", server.uri());
    let sha = hex::encode(sha2::Sha256::digest(&body));

    tokio::task::spawn_blocking(move || {
        let env = Env::new();
        let mut s = Session::start(&env);

        let r = s.tool(
            10,
            "download",
            json!({"url": url, "dir": "models", "checksum": format!("sha256:{sha}")}),
        );
        let res = &r["result"];
        assert_eq!(res["isError"], false, "{r}");
        let data = &res["structuredContent"];
        assert_eq!(data["state"], "completed");
        let saved = std::path::PathBuf::from(data["path"].as_str().unwrap());
        assert!(saved.starts_with(env.root.canonicalize().unwrap().join("models")));
        assert_eq!(std::fs::read(&saved).unwrap(), body);
        assert_eq!(res["content"][0]["type"], "text");
        // Progress notifications, if any, belong to this request and grow.
        let mut last = -1.0;
        for n in &s.notifications {
            assert_eq!(n["method"], "notifications/progress");
            assert_eq!(n["params"]["progressToken"], "p10");
            let p = n["params"]["progress"].as_f64().unwrap();
            assert!(p > last);
            last = p;
        }

        let id = data["id"].as_u64().unwrap();
        let st = s.tool(11, "download_status", json!({"id": id}));
        assert_eq!(st["result"]["structuredContent"]["state"], "completed");
        let list = s.tool(12, "list_downloads", json!({}));
        assert_eq!(
            list["result"]["structuredContent"]["jobs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Same name again: a free name is chosen, nothing is overwritten.
        let again = s.tool(
            13,
            "download",
            json!({"url": url, "dir": "models", "wait": false}),
        );
        let p = again["result"]["structuredContent"]["path"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(p.ends_with("model (1).bin"), "{p}");

        let v = s.tool(
            14,
            "verify_checksum",
            json!({"path": "models/model.bin", "expected": sha}),
        );
        assert_eq!(v["result"]["structuredContent"]["matches"], true, "{v}");
        let v = s.tool(
            15,
            "verify_checksum",
            json!({"path": "models/model.bin", "expected": "md5:00000000000000000000000000000000"}),
        );
        assert_eq!(v["result"]["isError"], true);
        assert_eq!(v["result"]["structuredContent"]["matches"], false);

        // Writes and reads outside the root are tool errors.
        for (n, args) in [
            (16, json!({"url": url, "dir": "../escape"})),
            (17, json!({"url": url, "filename": "../../escape.bin"})),
            (18, json!({"url": url, "dir": "/"})),
        ] {
            let r = s.tool(n, "download", args);
            assert_eq!(r["result"]["isError"], true, "{r}");
        }
        let r = s.tool(19, "verify_checksum", json!({"path": "/etc/hosts"}));
        assert_eq!(r["result"]["isError"], true);
        assert!(!env.root.parent().unwrap().join("escape").exists());
        assert!(
            !env.root
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("escape.bin")
                .exists()
        );

        // Unknown tools and bad ids.
        assert_eq!(s.tool(20, "rm_rf", json!({}))["error"]["code"], -32602);
        assert_eq!(
            s.tool(21, "cancel_download", json!({"id": 999}))["result"]["isError"],
            true
        );

        let cfg = s.tool(22, "get_config", json!({}));
        let c = &cfg["result"]["structuredContent"];
        assert_eq!(c["backend"], "in-process");
        assert_eq!(c["allowed_root"], json!(env.root.canonicalize().unwrap()));
        assert!(c["config"]["proxy"].get("password").is_none());
    })
    .await
    .unwrap();
}

#[test]
fn parse_errors_keep_the_session_alive() {
    let env = Env::new();
    let input = [
        initialize(),
        json!("not an object"),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ];
    let mut cmd = Command::new(BIN);
    env.apply(&mut cmd);
    let text: String = format!("{}\n{{broken json\n{}\n{}\n", input[0], input[1], input[2]);
    let out = AssertCommand::from_std(cmd)
        .write_stdin(text)
        .timeout(Duration::from_secs(60))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let lines: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[1]["error"]["code"], -32700);
    assert_eq!(lines[2]["error"]["code"], -32600);
    assert!(lines[3]["result"]["tools"].is_array());
}
