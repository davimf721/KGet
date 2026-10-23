//! `kget native-host` tests: the browser-extension bridge driven through the
//! built binary with native-messaging frames, against a real test daemon
//! and a wiremock server; manifest installation into `KGET_NATIVE_HOST_DIR`;
//! and the structure of `browser-extension/manifest.json`.
//!
//! Every test uses its own config, runtime and download directories under
//! the system temp dir (Unix sockets cannot live on every filesystem).

use kget::daemon::DaemonPaths;
use kget::daemon::client::DaemonClient;
use kget::native_host::framing;
use kget::native_host::install::{DEFAULT_CHROME_EXTENSION_ID, DEFAULT_FIREFOX_EXTENSION_ID};
use serde_json::{Value, json};
use sha2::Digest;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BIN: &str = env!("CARGO_BIN_EXE_kget");

/// Directories of one test.
struct Dirs {
    _root: TempDir,
    config: PathBuf,
    runtime: PathBuf,
    downloads: PathBuf,
    host_dir: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("kn")
            .tempdir_in(std::env::temp_dir())
            .unwrap();
        let base = root.path().to_path_buf();
        let d = Self {
            config: base.join("c"),
            runtime: base.join("r"),
            downloads: base.join("dl"),
            host_dir: base.join("nm"),
            _root: root,
        };
        std::fs::create_dir_all(&d.config).unwrap();
        std::fs::create_dir_all(&d.downloads).unwrap();
        d
    }

    fn paths(&self) -> DaemonPaths {
        DaemonPaths::in_dirs(&self.config, &self.runtime)
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(BIN);
        cmd.env("KGET_CONFIG_DIR", &self.config)
            .env("KGET_RUNTIME_DIR", &self.runtime)
            .env("KGET_NATIVE_HOST_DIR", &self.host_dir)
            .env("KGET_NATIVE_HOST_NO_LAUNCH", "1")
            .env("KGET_KEYCHAIN", "off")
            .env_remove("KGET_DAEMON_TRANSPORT");
        cmd
    }

    /// Make an auto-started daemon save into `downloads`.
    fn write_config(&self) {
        let config = json!({"daemon": {"download_dir": self.downloads}});
        std::fs::write(self.config.join("config.json"), config.to_string()).unwrap();
    }
}

/// A daemon process, killed on drop.
struct Daemon {
    child: Child,
}

impl Daemon {
    fn start(dirs: &Dirs) -> Self {
        let child = dirs
            .command()
            .args(["daemon", "--foreground", "--download-dir"])
            .arg(&dirs.downloads)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn kget daemon");
        let d = Self { child };
        let deadline = Instant::now() + Duration::from_secs(20);
        while DaemonClient::connect(&dirs.paths()).is_err() {
            assert!(Instant::now() < deadline, "daemon did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        d
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A `kget native-host` process with framed stdin/stdout.
struct Host {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    next_id: u64,
    /// Pushed messages (events) seen while waiting for replies
    pushed: Vec<Value>,
}

impl Host {
    fn start(dirs: &Dirs, caller: &[&str]) -> Self {
        let mut child = dirs
            .command()
            .arg("native-host")
            .args(caller)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn kget native-host");
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Some(msg)) = framing::read_message(&mut stdout, framing::MAX_OUTGOING) {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
        Self {
            stdin: child.stdin.take(),
            child,
            rx,
            next_id: 1,
            pushed: Vec::new(),
        }
    }

    fn send(&mut self, msg: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin
            .write_all(&framing::encode_message(msg, framing::MAX_INCOMING).unwrap())
            .unwrap();
        stdin.flush().unwrap();
    }

    /// Send a request and return its reply.
    fn request(&mut self, mut msg: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        msg["id"] = json!(id);
        self.send(&msg);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let reply = self.rx.recv_timeout(left).expect("reply from the host");
            if reply["id"] == json!(id) {
                return reply;
            }
            self.pushed.push(reply);
        }
    }

    /// Wait for a pushed event matching `pred`.
    fn wait_event(&mut self, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.pushed.iter().position(&pred) {
            return self.pushed.remove(i);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = self.rx.recv_timeout(left).expect("event from the host");
            if pred(&msg) {
                return msg;
            }
            self.pushed.push(msg);
        }
    }

    /// Close stdin (the browser closed the port) and return the exit code.
    fn close(mut self) -> i32 {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            assert!(Instant::now() < deadline, "host did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn chrome_caller() -> String {
    format!("chrome-extension://{DEFAULT_CHROME_EXTENSION_ID}/")
}

fn completed(job: u64) -> impl Fn(&Value) -> bool {
    move |m: &Value| {
        m["type"] == "event"
            && m["event"]["id"] == json!(job)
            && m["event"]["type"] == "state"
            && matches!(m["event"]["state"].as_str(), Some("completed" | "failed"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bridge_to_the_daemon() {
    let server = MockServer::start().await;
    let body = vec![3u8; 150_000];
    Mock::given(method("GET"))
        .and(path("/file.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    // Only answers when the opted-in cookie arrives.
    Mock::given(method("GET"))
        .and(path("/private.bin"))
        .and(header("cookie", "sid=42"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"secret".to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/private.bin"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let base = server.uri();

    tokio::task::spawn_blocking(move || {
        let dirs = Dirs::new();
        let _daemon = Daemon::start(&dirs);
        let mut host = Host::start(&dirs, &[&chrome_caller()]);

        let r = host.request(json!({"type": "ping"}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["result"]["protocol"], 1);

        let r = host.request(json!({"type": "version"}));
        assert_eq!(r["result"]["daemon"]["protocol"], 1, "{r}");
        let root = dirs.downloads.canonicalize().unwrap();
        assert_eq!(
            Path::new(r["result"]["daemon"]["download_dir"].as_str().unwrap()),
            root
        );

        let r = host.request(json!({"type": "subscribe"}));
        assert_eq!(r["result"]["subscribed"], true, "{r}");

        // A path is reduced to its file name; credentials are dropped
        // without the opt-in.
        let r = host.request(json!({
            "type": "download",
            "url": format!("{base}/file.bin"),
            "filename": "../../outside/renamed.bin",
            "referrer": "https://example.com/page",
            "headers": {"Cookie": "a=1", "Authorization": "Bearer t", "X-From": "ext"},
            "cookies": "sid=1",
        }));
        assert_eq!(r["ok"], true, "{r}");
        let job = r["result"]["id"].as_u64().unwrap();
        let names: Vec<String> = r["result"]["job"]["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h[0].as_str().unwrap().to_ascii_lowercase())
            .collect();
        assert!(names.contains(&"x-from".into()), "{names:?}");
        assert!(names.contains(&"referer".into()), "{names:?}");
        assert!(!names.contains(&"cookie".into()), "{names:?}");
        assert!(!names.contains(&"authorization".into()), "{names:?}");

        let ev = host.wait_event(completed(job));
        assert_eq!(ev["event"]["state"], "completed", "{ev}");
        let saved = root.join("renamed.bin");
        assert_eq!(std::fs::read(&saved).unwrap(), body);

        let r = host.request(json!({"type": "list"}));
        assert!(
            r["result"]["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|j| j["id"] == json!(job))
        );

        // open / reveal resolve the daemon's path (launching is disabled).
        let r = host.request(json!({"type": "reveal", "job": job}));
        assert_eq!(Path::new(r["result"]["path"].as_str().unwrap()), saved);
        assert_eq!(r["result"]["launched"], false);
        let r = host.request(json!({"type": "open", "job": job}));
        assert_eq!(r["ok"], true, "{r}");

        // With send_cookies the cookie reaches the server.
        let r = host.request(json!({
            "type": "download",
            "url": format!("{base}/private.bin"),
            "send_cookies": true,
            "cookies": "sid=42",
        }));
        let job2 = r["result"]["id"].as_u64().unwrap();
        let ev = host.wait_event(completed(job2));
        assert_eq!(ev["event"]["state"], "completed", "{ev}");
        assert_eq!(std::fs::read(root.join("private.bin")).unwrap(), b"secret");

        // Errors keep the session alive.
        let r = host.request(json!({"type": "download", "url": "file:///etc/passwd"}));
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"]["code"], -32602);
        let r = host.request(json!({"type": "pause", "job": 999}));
        assert_eq!(r["error"]["code"], -32001, "{r}");
        let r = host.request(json!({"type": "remove", "job": job}));
        assert_eq!(r["ok"], true, "{r}");
        host.wait_event(|m| m["type"] == "event" && m["event"]["type"] == "removed");

        assert_eq!(host.close(), 0);
    })
    .await
    .unwrap();
}

#[test]
fn host_starts_the_daemon() {
    let dirs = Dirs::new();
    dirs.write_config();
    let mut host = Host::start(
        &dirs,
        &["/x/com.kget.native_host.json", DEFAULT_FIREFOX_EXTENSION_ID],
    );
    let r = host.request(json!({"type": "version"}));
    assert_eq!(r["ok"], true, "{r}");
    let pid = r["result"]["daemon"]["pid"].as_u64().unwrap();
    assert!(pid > 0);
    assert_eq!(host.close(), 0);
    // The daemon outlives the host (the browser closing the port).
    let c = DaemonClient::connect(&dirs.paths()).expect("daemon still running");
    c.shutdown().unwrap();
}

#[test]
fn unknown_callers_are_refused() {
    let dirs = Dirs::new();
    for caller in [
        vec!["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"],
        vec!["/x/manifest.json", "evil@example.com"],
        vec![],
    ] {
        let out = dirs
            .command()
            .arg("native-host")
            .args(&caller)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{caller:?}");
        assert!(out.stdout.is_empty(), "nothing reaches the extension");
    }
    // An ID added at install time is accepted.
    let id = "abcdefghijklmnopabcdefghijklmnop";
    let st = dirs
        .command()
        .args([
            "native-host",
            "install",
            "--browser",
            "chrome",
            "--extension-id",
            id,
        ])
        .output()
        .unwrap();
    assert!(st.status.success());
    let host = Host::start(
        &dirs,
        &[&format!("chrome-extension://{id}/"), "--parent-window=0"],
    );
    assert_eq!(host.close(), 0);
}

#[test]
fn malformed_frames() {
    let dirs = Dirs::new();
    // Invalid JSON gets an error reply and the session continues.
    let mut host = Host::start(&dirs, &[&chrome_caller()]);
    let stdin = host.stdin.as_mut().unwrap();
    stdin.write_all(&3u32.to_ne_bytes()).unwrap();
    stdin.write_all(b"abc").unwrap();
    stdin.flush().unwrap();
    let err = host.rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(err["ok"], false);
    assert_eq!(err["error"]["code"], -32700);
    let r = host.request(json!({"type": "ping"}));
    assert_eq!(r["ok"], true);
    assert_eq!(host.close(), 0);

    // An oversized length ends the session.
    let mut host = Host::start(&dirs, &[&chrome_caller()]);
    let stdin = host.stdin.as_mut().unwrap();
    stdin
        .write_all(&((framing::MAX_INCOMING + 1) as u32).to_ne_bytes())
        .unwrap();
    stdin.flush().unwrap();
    let err = host.rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(err["error"]["code"], -32005);
    assert_eq!(host.close(), 1);

    // A truncated frame ends it too.
    let mut host = Host::start(&dirs, &[&chrome_caller()]);
    host.stdin
        .as_mut()
        .unwrap()
        .write_all(&[10, 0, 0, 0, b'{'])
        .unwrap();
    assert_eq!(host.close(), 1);
}

#[test]
fn install_writes_manifests_per_browser() {
    let dirs = Dirs::new();
    let out = dirs
        .command()
        .args(["native-host", "install", "--browser", "all"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let report = String::from_utf8(out.stdout).unwrap();
    let launcher = dirs.host_dir.join(if cfg!(windows) {
        "kget-native-host.bat"
    } else {
        "kget-native-host"
    });
    assert!(launcher.is_file());
    assert!(report.contains(&launcher.display().to_string()), "{report}");
    let script = std::fs::read_to_string(&launcher).unwrap();
    assert!(script.contains("native-host"));
    assert!(script.contains(Path::new(BIN).file_name().unwrap().to_str().unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&launcher).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "launcher is executable");
    }

    for browser in ["chrome", "chromium", "edge", "brave", "firefox"] {
        let path = dirs
            .host_dir
            .join(browser)
            .join("com.kget.native_host.json");
        assert!(report.contains(&path.display().to_string()), "{report}");
        let m: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(m["name"], "com.kget.native_host");
        assert_eq!(m["type"], "stdio");
        assert_eq!(Path::new(m["path"].as_str().unwrap()), launcher);
        if browser == "firefox" {
            assert_eq!(
                m["allowed_extensions"],
                json!([DEFAULT_FIREFOX_EXTENSION_ID])
            );
            assert!(m.get("allowed_origins").is_none());
        } else {
            assert_eq!(m["allowed_origins"], json!([chrome_caller()]));
            assert!(m.get("allowed_extensions").is_none());
        }
    }

    // A second install for one browser keeps the origins of the first.
    let ff = "{12345678-1234-1234-1234-123456789abc}";
    let out = dirs
        .command()
        .args([
            "native-host",
            "install",
            "--browser",
            "firefox",
            "--extension-id",
            ff,
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let allow: Vec<String> =
        serde_json::from_slice(&std::fs::read(dirs.host_dir.join("allowed_origins.json")).unwrap())
            .unwrap();
    assert!(allow.contains(&chrome_caller()));
    assert!(allow.contains(&ff.to_string()));
    let m: Value = serde_json::from_slice(
        &std::fs::read(dirs.host_dir.join("firefox/com.kget.native_host.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        m["allowed_extensions"],
        json!([DEFAULT_FIREFOX_EXTENSION_ID, ff])
    );

    // Bad input is rejected.
    for args in [
        vec!["native-host", "install", "--browser", "netscape"],
        vec!["native-host", "install", "--extension-id", "not an id"],
    ] {
        let out = dirs.command().args(&args).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
    }

    // Uninstalling one browser leaves the others; all removes everything.
    let out = dirs
        .command()
        .args(["native-host", "uninstall", "--browser", "edge"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        !dirs
            .host_dir
            .join("edge/com.kget.native_host.json")
            .exists()
    );
    assert!(
        dirs.host_dir
            .join("chrome/com.kget.native_host.json")
            .exists()
    );
    assert!(launcher.exists());
    let out = dirs
        .command()
        .args(["native-host", "uninstall"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        !dirs
            .host_dir
            .join("chrome/com.kget.native_host.json")
            .exists()
    );
    assert!(!launcher.exists());
    assert!(!dirs.host_dir.join("allowed_origins.json").exists());
}

/// Standard base64 (the manifest `key`).
fn base64_decode(s: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|&c| c != b'=') {
        let v = ALPHABET.iter().position(|&a| a == c).expect("base64") as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

#[test]
fn extension_manifest_is_valid() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("browser-extension");
    let m: Value = serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap())
        .expect("manifest.json is JSON");
    assert_eq!(m["manifest_version"], 3);
    let strings = |v: &Value| -> Vec<String> {
        let mut s: Vec<String> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        s.sort();
        s
    };
    // Minimal permissions; the rest are requested at runtime.
    assert_eq!(
        strings(&m["permissions"]),
        ["contextMenus", "nativeMessaging", "storage"]
    );
    assert_eq!(
        strings(&m["optional_permissions"]),
        ["cookies", "downloads"]
    );
    assert_eq!(strings(&m["optional_host_permissions"]), ["<all_urls>"]);
    assert!(m.get("host_permissions").is_none());
    assert_eq!(
        m["browser_specific_settings"]["gecko"]["id"],
        DEFAULT_FIREFOX_EXTENSION_ID
    );
    assert_eq!(m["background"]["service_worker"], "background.js");
    assert_eq!(
        m["background"]["scripts"],
        json!(["lib.js", "background.js"])
    );

    // The key pins the unpacked Chromium ID that `install` allows by default.
    let der = base64_decode(m["key"].as_str().unwrap());
    let digest = sha2::Sha256::digest(&der);
    let id: String = hex::encode(&digest[..16])
        .chars()
        .map(|c| (b'a' + c.to_digit(16).unwrap() as u8) as char)
        .collect();
    assert_eq!(id, DEFAULT_CHROME_EXTENSION_ID);

    // Every referenced file exists.
    let mut files = vec![
        m["action"]["default_popup"].as_str().unwrap().to_string(),
        m["options_ui"]["page"].as_str().unwrap().to_string(),
        "background.js".to_string(),
        "lib.js".to_string(),
    ];
    for v in m["icons"].as_object().unwrap().values() {
        files.push(v.as_str().unwrap().to_string());
    }
    for v in m["action"]["default_icon"].as_object().unwrap().values() {
        files.push(v.as_str().unwrap().to_string());
    }
    for f in files {
        assert!(dir.join(&f).is_file(), "{f} is missing");
    }
}

/// The extension's JS unit tests (`node --test`), when node is installed.
#[test]
fn extension_js_unit_tests() {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("browser-extension/tests/lib.test.js");
    let Ok(out) = Command::new("node").arg("--test").arg(&tests).output() else {
        eprintln!("node not found; skipping the extension's JS tests");
        return;
    };
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
