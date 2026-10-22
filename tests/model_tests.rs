//! AI model sources: `hf://` (Hugging Face Hub) and `ollama://` against fake
//! Hub and registry APIs (wiremock). Never touches the real network.
//!
//! Output goes to `std::env::temp_dir()` so symlinks work even when the
//! repository lives on a filesystem without them.

use assert_cmd::Command;
use kget::KgetError;
use kget::fetch::TransferOptions;
use kget::hf::{self, HfAuth, HfOptions};
use kget::ollama::{self, OllamaOptions};
use sha2::Digest;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(data))
}

fn tmp() -> TempDir {
    tempfile::Builder::new()
        .prefix("kget-model-test-")
        .tempdir_in(std::env::temp_dir())
        .unwrap()
}

/// A file of the fake repository: (path, content, stored with LFS).
struct RepoFile {
    path: &'static str,
    body: Vec<u8>,
    lfs: bool,
}

fn repo_files() -> Vec<RepoFile> {
    vec![
        RepoFile {
            path: "config.json",
            body: br#"{"model_type":"llama"}"#.to_vec(),
            lfs: false,
        },
        RepoFile {
            path: "model-q4.gguf",
            body: vec![4u8; 3000],
            lfs: true,
        },
        RepoFile {
            path: "model-fp32.gguf",
            body: vec![32u8; 5000],
            lfs: true,
        },
        RepoFile {
            path: "sub/tokenizer.json",
            body: br#"{"tokens":[]}"#.to_vec(),
            lfs: false,
        },
    ]
}

fn blob_id(f: &RepoFile) -> String {
    // Any 40-hex id works for the fake Hub.
    hex::encode(&sha2::Sha256::digest(f.path.as_bytes())[..20])
}

fn siblings(files: &[RepoFile], lfs_sha_override: Option<&str>) -> serde_json::Value {
    serde_json::Value::Array(
        files
            .iter()
            .map(|f| {
                let mut v = serde_json::json!({
                    "rfilename": f.path,
                    "size": f.body.len(),
                    "blobId": blob_id(f),
                });
                if f.lfs {
                    v["lfs"] = serde_json::json!({
                        "sha256": lfs_sha_override.map(str::to_string).unwrap_or_else(|| sha256_hex(&f.body)),
                        "size": f.body.len(),
                        "pointerSize": 130,
                    });
                }
                v
            })
            .collect(),
    )
}

/// Mount the revision API and every file's resolve URL.
async fn mount_hub(
    server: &MockServer,
    repo: &str,
    files: &[RepoFile],
    lfs_override: Option<&str>,
) {
    Mock::given(method("GET"))
        .and(path(format!("/api/models/{repo}/revision/main")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": repo,
            "sha": COMMIT,
            "siblings": siblings(files, lfs_override),
        })))
        .mount(server)
        .await;
    for f in files {
        Mock::given(method("GET"))
            .and(path(format!("/{repo}/resolve/{COMMIT}/{}", f.path)))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(f.body.clone()))
            .mount(server)
            .await;
    }
}

fn hf_opts(server: &MockServer, out: &Path) -> HfOptions {
    HfOptions {
        endpoint: Some(server.uri()),
        auth: HfAuth::Anonymous,
        output: Some(out.to_string_lossy().into_owned()),
        ..Default::default()
    }
}

fn transfer() -> TransferOptions {
    TransferOptions {
        retries: 1,
        timeout: 10,
        ..Default::default()
    }
}

/// Run blocking library code off the async test thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.unwrap()
}

fn requests_to(reqs: &[Request], p: &str) -> usize {
    reqs.iter().filter(|r| r.url.path() == p).count()
}

// ============================================================================
// Hugging Face
// ============================================================================

#[tokio::test(flavor = "multi_thread")]
async fn hf_lists_repo_and_applies_include_exclude() {
    let server = MockServer::start().await;
    let files = repo_files();
    mount_hub(&server, "org/repo", &files, None).await;
    let dir = tmp();
    let out = dir.path().join("repo");

    let mut opts = hf_opts(&server, &out);
    opts.include = vec!["*.gguf".into(), "*.json".into()];
    opts.exclude = vec!["*fp32*".into()];
    let done = blocking(move || hf::download("hf://org/repo", &opts, &transfer()))
        .await
        .unwrap();

    assert!(!done.single_file);
    assert_eq!(done.files, 3);
    assert_eq!(done.path, out);
    assert_eq!(
        std::fs::read(out.join("model-q4.gguf")).unwrap(),
        files[1].body
    );
    assert_eq!(
        std::fs::read(out.join("config.json")).unwrap(),
        files[0].body
    );
    assert_eq!(
        std::fs::read(out.join("sub/tokenizer.json")).unwrap(),
        files[3].body
    );
    assert!(
        !out.join("model-fp32.gguf").exists(),
        "excluded file downloaded"
    );
    // No resume leftovers.
    assert!(!out.join("model-q4.gguf.part").exists());
    assert!(!out.join("model-q4.gguf.kget").exists());

    // The listing helper gives the same selection.
    let uri = server.uri();
    let (listing, selected) = blocking(move || {
        let opts = HfOptions {
            endpoint: Some(uri),
            auth: HfAuth::Anonymous,
            exclude: vec!["*.gguf".into()],
            ..Default::default()
        };
        hf::list("hf://org/repo", &opts, &transfer())
    })
    .await
    .unwrap();
    assert_eq!(listing.commit, COMMIT);
    let names: Vec<_> = selected.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(names, ["config.json", "sub/tokenizer.json"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_single_file_and_folder_prefix() {
    let server = MockServer::start().await;
    let files = repo_files();
    mount_hub(&server, "org/repo", &files, None).await;
    let dir = tmp();

    // A single file lands under its own name in the output directory.
    let mut opts = hf_opts(&server, dir.path());
    opts.output = Some(format!("{}/", dir.path().display()));
    let done = blocking(move || hf::download("hf://org/repo/model-q4.gguf", &opts, &transfer()))
        .await
        .unwrap();
    assert!(done.single_file);
    assert_eq!(done.path, dir.path().join("model-q4.gguf"));
    assert_eq!(std::fs::read(&done.path).unwrap(), files[1].body);

    // A folder prefix keeps the repository paths.
    let out = dir.path().join("prefix");
    let opts = hf_opts(&server, &out);
    let done = blocking(move || hf::download("hf://org/repo/sub", &opts, &transfer()))
        .await
        .unwrap();
    assert_eq!(done.files, 1);
    assert!(out.join("sub/tokenizer.json").is_file());

    // A missing path is a clear not-found error.
    let opts = hf_opts(&server, &out);
    let err = blocking(move || hf::download("hf://org/repo/nope.bin", &opts, &transfer()))
        .await
        .unwrap_err();
    assert!(matches!(err, KgetError::NotFound(_)), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_web_resolve_url_is_rewritten() {
    let server = MockServer::start().await;
    let files = repo_files();
    mount_hub(&server, "org/repo", &files, None).await;
    let dir = tmp();
    let opts = HfOptions {
        endpoint: Some(server.uri()),
        auth: HfAuth::Anonymous,
        output: Some(dir.path().join("cfg.json").to_string_lossy().into_owned()),
        ..Default::default()
    };
    // /blob/ URLs are pages: KGet downloads the file through /resolve/<commit>/.
    let url = "https://huggingface.co/org/repo/blob/main/config.json?download=true";
    assert_eq!(kget::protocol::detect(url), kget::Protocol::HuggingFace);
    let done = blocking(move || hf::download(url, &opts, &transfer()))
        .await
        .unwrap();
    assert_eq!(std::fs::read(done.path).unwrap(), files[0].body);
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_lfs_sha256_mismatch_fails() {
    let server = MockServer::start().await;
    let files = repo_files();
    let wrong = "f".repeat(64);
    mount_hub(&server, "org/repo", &files, Some(&wrong)).await;
    let dir = tmp();
    let out = dir.path().join("repo");
    let mut opts = hf_opts(&server, &out);
    opts.include = vec!["model-q4.gguf".into()];
    let err = blocking(move || hf::download("hf://org/repo", &opts, &transfer()))
        .await
        .unwrap_err();
    match err {
        KgetError::ChecksumMismatch { expected, got, .. } => {
            assert_eq!(expected, wrong);
            assert_eq!(got, sha256_hex(&files[1].body));
        }
        other => panic!("expected a checksum mismatch, got {other}"),
    }
    assert!(!out.join("model-q4.gguf").exists(), "corrupt file kept");
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_cache_layout_blobs_snapshots_refs() {
    let server = MockServer::start().await;
    let files = repo_files();
    mount_hub(&server, "org/repo", &files, None).await;
    let dir = tmp();
    let cache = dir.path().join("hub");
    let mut opts = hf_opts(&server, &cache);
    opts.hf_cache = true;
    opts.exclude = vec!["*fp32*".into()];
    let done = blocking(move || hf::download("hf://org/repo", &opts, &transfer()))
        .await
        .unwrap();

    let repo_dir = cache.join("models--org--repo");
    let snapshot = repo_dir.join("snapshots").join(COMMIT);
    assert_eq!(done.path, snapshot);
    assert_eq!(
        std::fs::read_to_string(repo_dir.join("refs/main")).unwrap(),
        COMMIT
    );
    // LFS files are stored under their SHA-256, regular files under the blob id.
    let lfs_blob = repo_dir.join("blobs").join(sha256_hex(&files[1].body));
    assert_eq!(std::fs::read(&lfs_blob).unwrap(), files[1].body);
    assert!(repo_dir.join("blobs").join(blob_id(&files[0])).is_file());

    for (rel, body) in [
        ("model-q4.gguf", &files[1].body),
        ("config.json", &files[0].body),
        ("sub/tokenizer.json", &files[3].body),
    ] {
        let link = snapshot.join(rel);
        assert_eq!(&std::fs::read(&link).unwrap(), body, "{rel}");
        #[cfg(unix)]
        {
            let meta = std::fs::symlink_metadata(&link).unwrap();
            assert!(meta.file_type().is_symlink(), "{rel} is not a symlink");
            let target = std::fs::read_link(&link).unwrap();
            assert!(
                target.is_relative() && target.starts_with(".."),
                "{target:?}"
            );
        }
        #[cfg(windows)]
        assert!(std::fs::symlink_metadata(&link).unwrap().is_file());
    }
    assert!(!snapshot.join("model-fp32.gguf").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_auth_header_sent_to_hub_and_dropped_on_cross_host_redirect() {
    let hub = MockServer::start().await;
    let cdn = MockServer::start().await;
    let body = vec![7u8; 2048];
    let sha = sha256_hex(&body);
    let token = "hf_secret_test_token";
    let bearer = format!("Bearer {token}");

    Mock::given(method("GET"))
        .and(path("/api/models/org/gated/revision/main"))
        .and(header("authorization", bearer.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": COMMIT,
            "siblings": [{
                "rfilename": "weights.safetensors",
                "size": body.len(),
                "blobId": "a".repeat(40),
                "lfs": {"sha256": sha, "size": body.len()}
            }]
        })))
        .mount(&hub)
        .await;
    // Without the token the Hub refuses (gated repository).
    Mock::given(method("GET"))
        .and(path("/api/models/org/gated/revision/main"))
        .respond_with(ResponseTemplate::new(401).insert_header("x-error-code", "GatedRepo"))
        .with_priority(10)
        .mount(&hub)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/org/gated/resolve/{COMMIT}/weights.safetensors"
        )))
        .and(header("authorization", bearer.as_str()))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/cdn/weights", cdn.uri()).as_str()),
        )
        .mount(&hub)
        .await;
    Mock::given(method("GET"))
        .and(path("/cdn/weights"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&cdn)
        .await;

    let dir = tmp();
    let mut opts = hf_opts(&hub, &dir.path().join("gated"));
    opts.auth = HfAuth::Token(token.into());
    let done = blocking(move || hf::download("hf://org/gated", &opts, &transfer()))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(done.path.join("weights.safetensors")).unwrap(),
        body
    );

    let cdn_reqs = cdn.received_requests().await.unwrap();
    assert!(!cdn_reqs.is_empty());
    assert!(
        cdn_reqs
            .iter()
            .all(|r| !r.headers.contains_key("authorization")),
        "Authorization leaked to the CDN"
    );
    let hub_reqs = hub.received_requests().await.unwrap();
    assert!(
        hub_reqs
            .iter()
            .filter(|r| r.method.as_str() == "GET")
            .all(|r| r
                .headers
                .get("authorization")
                .is_some_and(|v| v == bearer.as_str()))
    );

    // Anonymous: a clear message about the gated repository, never the token.
    let mut opts = hf_opts(&hub, &dir.path().join("anon"));
    opts.auth = HfAuth::Anonymous;
    let err = blocking(move || hf::download("hf://org/gated", &opts, &transfer()))
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, KgetError::Http { status: 401, .. }), "{msg}");
    assert!(msg.contains("gated") && msg.contains("HF_TOKEN"), "{msg}");
    assert!(!msg.contains(token));
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_resumes_partial_multi_file_download() {
    let server = MockServer::start().await;
    let files = repo_files();
    let repo = "org/resume";
    Mock::given(method("GET"))
        .and(path(format!("/api/models/{repo}/revision/main")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": COMMIT,
            "siblings": siblings(&files, None),
        })))
        .mount(&server)
        .await;
    // The fp32 file fails on the first run only.
    let broken = format!("/{repo}/resolve/{COMMIT}/model-fp32.gguf");
    Mock::given(method("GET"))
        .and(path(broken.as_str()))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    for f in &files {
        Mock::given(method("GET"))
            .and(path(format!("/{repo}/resolve/{COMMIT}/{}", f.path)))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(f.body.clone()))
            .mount(&server)
            .await;
    }

    let dir = tmp();
    let out = dir.path().join("resume");
    let mut t = transfer();
    t.parallel_files = 1;
    let opts = hf_opts(&server, &out);
    let (o, tt) = (opts.clone(), t.clone());
    let first = blocking(move || hf::download("hf://org/resume", &o, &tt)).await;
    assert!(first.is_err(), "first run should fail");
    assert!(!out.join("model-fp32.gguf").exists());

    let before = server.received_requests().await.unwrap();
    let q4 = format!("/{repo}/resolve/{COMMIT}/model-q4.gguf");
    let q4_before = requests_to(&before, &q4);
    let done = blocking(move || hf::download("hf://org/resume", &opts, &t))
        .await
        .unwrap();
    assert_eq!(done.files, 4);
    assert!(done.skipped >= 1, "complete files were downloaded again");
    for f in &files {
        assert_eq!(
            std::fs::read(out.join(f.path)).unwrap(),
            f.body,
            "{}",
            f.path
        );
    }
    // Files finished by the first run are not requested again.
    assert!(q4_before > 0, "first run did not reach model-q4.gguf");
    let after = server.received_requests().await.unwrap();
    assert_eq!(requests_to(&after, &q4), q4_before);
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_tree_api_fallback_for_datasets() {
    let server = MockServer::start().await;
    let body = b"a,b\n1,2\n".to_vec();
    Mock::given(method("GET"))
        .and(path("/api/datasets/org/data/revision/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": COMMIT,
            "siblings": [{"rfilename": "train.csv"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/datasets/org/data/tree/{COMMIT}")))
        .and(query_param("recursive", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"type": "directory", "path": "data", "oid": "b".repeat(40), "size": 0},
            {"type": "file", "path": "data/train.csv", "oid": "c".repeat(40), "size": body.len()}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/datasets/org/data/resolve/{COMMIT}/data/train.csv"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    let dir = tmp();
    let opts = hf_opts(&server, &dir.path().join("data"));
    let done = blocking(move || hf::download("hf://datasets/org/data@v1", &opts, &transfer()))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(done.path.join("data/train.csv")).unwrap(),
        body
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hf_rejects_path_traversal_from_listing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/models/org/evil/revision/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": COMMIT,
            "siblings": [{"rfilename": "../../escape.txt", "size": 1, "blobId": "d".repeat(40)}]
        })))
        .mount(&server)
        .await;
    let dir = tmp();
    let opts = hf_opts(&server, &dir.path().join("evil"));
    let err = blocking(move || hf::download("hf://org/evil", &opts, &transfer()))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("unsafe"), "{err}");
}

// ============================================================================
// Ollama
// ============================================================================

struct FakeModel {
    manifest: Vec<u8>,
    blobs: Vec<(String, Vec<u8>)>,
}

fn fake_model(corrupt: bool) -> FakeModel {
    let weights = vec![9u8; 4096];
    let template = b"{{ .Prompt }}".to_vec();
    let config = br#"{"model_format":"gguf"}"#.to_vec();
    let digest = |b: &[u8]| format!("sha256:{}", sha256_hex(b));
    let weights_digest = if corrupt {
        format!("sha256:{}", "e".repeat(64))
    } else {
        digest(&weights)
    };
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {"mediaType": "application/vnd.docker.container.image.v1+json", "digest": digest(&config), "size": config.len()},
        "layers": [
            {"mediaType": "application/vnd.ollama.image.model", "digest": weights_digest, "size": weights.len()},
            {"mediaType": "application/vnd.ollama.image.template", "digest": digest(&template), "size": template.len()}
        ]
    });
    FakeModel {
        manifest: serde_json::to_vec(&manifest).unwrap(),
        blobs: vec![
            (digest(&config), config),
            (weights_digest, weights),
            (digest(&template), template),
        ],
    }
}

async fn mount_registry(server: &MockServer, ns: &str, model: &str, tag: &str, m: &FakeModel) {
    Mock::given(method("GET"))
        .and(path(format!("/v2/{ns}/{model}/manifests/{tag}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(m.manifest.clone()))
        .mount(server)
        .await;
    for (digest, body) in &m.blobs {
        Mock::given(method("GET"))
            .and(path(format!("/v2/{ns}/{model}/blobs/{digest}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(server)
            .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_writes_models_dir_layout() {
    let server = MockServer::start().await;
    let m = fake_model(false);
    mount_registry(&server, "library", "tiny", "3b", &m).await;
    let dir = tmp();
    let models = dir.path().join("models");
    let opts = OllamaOptions {
        registry_url: Some(server.uri()),
        models_dir: Some(models.clone()),
        ..Default::default()
    };
    let done = blocking(move || ollama::download("ollama://tiny:3b", &opts, &transfer()))
        .await
        .unwrap();
    assert_eq!(done.path, models);
    assert_eq!(done.files, 3);
    let manifest = models.join("manifests/registry.ollama.ai/library/tiny/3b");
    assert_eq!(std::fs::read(&manifest).unwrap(), m.manifest);
    for (digest, body) in &m.blobs {
        let blob = models.join("blobs").join(digest.replace(':', "-"));
        assert_eq!(&std::fs::read(&blob).unwrap(), body, "{digest}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_plain_output_and_digest_mismatch() {
    let server = MockServer::start().await;
    let m = fake_model(false);
    mount_registry(&server, "alice", "tiny", "latest", &m).await;
    let dir = tmp();
    let out = dir.path().join("plain");
    let opts = OllamaOptions {
        registry_url: Some(server.uri()),
        output: Some(out.to_string_lossy().into_owned()),
        ..Default::default()
    };
    blocking(move || ollama::download("ollama://alice/tiny", &opts, &transfer()))
        .await
        .unwrap();
    assert_eq!(std::fs::read(out.join("model.gguf")).unwrap(), m.blobs[1].1);
    assert_eq!(std::fs::read(out.join("template")).unwrap(), m.blobs[2].1);
    assert_eq!(
        std::fs::read(out.join("config.json")).unwrap(),
        m.blobs[0].1
    );
    assert_eq!(
        std::fs::read(out.join("manifest.json")).unwrap(),
        m.manifest
    );

    // A blob that does not match its digest fails and no manifest is written.
    let bad = fake_model(true);
    mount_registry(&server, "library", "bad", "latest", &bad).await;
    let models = dir.path().join("models");
    let opts = OllamaOptions {
        registry_url: Some(server.uri()),
        models_dir: Some(models.clone()),
        ..Default::default()
    };
    let err = blocking(move || ollama::download("ollama://bad", &opts, &transfer()))
        .await
        .unwrap_err();
    assert!(matches!(err, KgetError::ChecksumMismatch { .. }), "{err}");
    assert!(
        !models
            .join("manifests/registry.ollama.ai/library/bad/latest")
            .exists()
    );

    // Unknown model: not found.
    let opts = OllamaOptions {
        registry_url: Some(server.uri()),
        models_dir: Some(models),
        ..Default::default()
    };
    let err = blocking(move || ollama::download("ollama://missing:1b", &opts, &transfer()))
        .await
        .unwrap_err();
    assert!(matches!(err, KgetError::NotFound(_)), "{err}");
}

// ============================================================================
// CLI wiring: environment variables, flags, JSONL, doctor
// ============================================================================

fn kget(cfg: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("kget"));
    cmd.env("KGET_CONFIG_DIR", cfg)
        .env("KGET_KEYCHAIN", "off")
        .env_remove("HF_TOKEN")
        .env_remove("HUGGING_FACE_HUB_TOKEN")
        .env_remove("HF_ENDPOINT")
        .env_remove("HF_HUB_CACHE")
        .env_remove("HF_TOKEN_PATH");
    cmd
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_hf_cache_with_env_endpoint_token_and_jsonl() {
    let server = MockServer::start().await;
    let files = repo_files();
    let token = "hf_cli_token_123";
    Mock::given(method("GET"))
        .and(path("/api/models/org/repo/revision/main"))
        .and(header("authorization", format!("Bearer {token}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": COMMIT,
            "siblings": siblings(&files, None),
        })))
        .mount(&server)
        .await;
    for f in &files {
        Mock::given(method("GET"))
            .and(path(format!("/org/repo/resolve/{COMMIT}/{}", f.path)))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(f.body.clone()))
            .mount(&server)
            .await;
    }
    let dir = tmp();
    let hf_home = dir.path().join("hf-home");
    let cfg = dir.path().join("cfg");
    let uri = server.uri();
    let (home, c) = (hf_home.clone(), cfg.clone());
    let out = blocking(move || {
        kget(&c)
            .env("KGET_HF_ENDPOINT", &uri)
            .env("HF_HOME", &home)
            .env("HF_TOKEN", token)
            .args([
                "hf://org/repo",
                "--hf-cache",
                "--include",
                "*.json",
                "--jsonl",
                "-j",
                "2",
            ])
            .output()
            .unwrap()
    })
    .await;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stdout.contains(token), "token printed");
    let events: Vec<serde_json::Value> = stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let kinds: Vec<&str> = events.iter().filter_map(|e| e["event"].as_str()).collect();
    assert_eq!(kinds.first(), Some(&"started"));
    assert_eq!(events[0]["protocol"], "hf");
    assert!(kinds.contains(&"files") && kinds.contains(&"file_progress"));
    assert_eq!(kinds.last(), Some(&"completed"));
    let files_ev = events.iter().find(|e| e["event"] == "files").unwrap();
    assert_eq!(files_ev["files"].as_array().unwrap().len(), 2);

    let snapshot: PathBuf = hf_home.join("hub/models--org--repo/snapshots").join(COMMIT);
    assert_eq!(
        std::fs::read(snapshot.join("config.json")).unwrap(),
        files[0].body
    );
    assert!(snapshot.join("sub/tokenizer.json").exists());
    assert!(!snapshot.join("model-q4.gguf").exists());

    // The history records the download as "hf".
    let c = cfg.clone();
    let hist = blocking(move || {
        kget(&c)
            .args(["--history", "--json", "--protocol", "hf"])
            .output()
            .unwrap()
    })
    .await;
    assert!(String::from_utf8_lossy(&hist.stdout).contains("hf://org/repo"));

    // --doctor reports the token's source, never the token.
    let (home, c) = (hf_home.clone(), cfg.clone());
    let doctor = blocking(move || {
        kget(&c)
            .env("HF_HOME", &home)
            .env("HF_TOKEN", token)
            .arg("--doctor")
            .output()
            .unwrap()
    })
    .await;
    let text = String::from_utf8_lossy(&doctor.stdout);
    assert!(text.contains("HF auth:  token from $HF_TOKEN"), "{text}");
    assert!(!text.contains(token));
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_ollama_uses_env_registry_and_models_dir() {
    let server = MockServer::start().await;
    let m = fake_model(false);
    mount_registry(&server, "library", "tiny", "latest", &m).await;
    let dir = tmp();
    let models = dir.path().join("ollama-models");
    let cfg = dir.path().join("cfg");
    let uri = server.uri();
    let mm = models.clone();
    let out = blocking(move || {
        kget(&cfg)
            .env("KGET_OLLAMA_REGISTRY", &uri)
            .env("OLLAMA_MODELS", &mm)
            .args(["ollama://tiny", "-q"])
            .output()
            .unwrap()
    })
    .await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(models.join("manifests/registry.ollama.ai/library/tiny/latest")).unwrap(),
        m.manifest
    );
    assert!(
        models
            .join("blobs")
            .join(m.blobs[1].0.replace(':', "-"))
            .is_file()
    );
}
