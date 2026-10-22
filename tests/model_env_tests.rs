//! Builder API with `hf://` and `ollama://` URLs. The endpoints come from
//! process-wide environment variables (`KGET_HF_ENDPOINT`,
//! `KGET_OLLAMA_REGISTRY`), so this binary runs a single test.

use kget::DownloadEvent;
use sha2::Digest;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const COMMIT: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: this binary runs a single test; the variables are set before
    // any download thread starts.
    unsafe { std::env::set_var(key, value) };
}

#[tokio::test(flavor = "multi_thread")]
async fn builder_downloads_model_urls() {
    let server = MockServer::start().await;
    let weights = vec![5u8; 1500];
    let readme = b"# tiny".to_vec();
    let sha = hex::encode(sha2::Sha256::digest(&weights));
    Mock::given(method("GET"))
        .and(path("/api/models/org/tiny/revision/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": COMMIT,
            "siblings": [
                {"rfilename": "README.md", "size": readme.len(), "blobId": "1".repeat(40)},
                {"rfilename": "w.safetensors", "size": weights.len(), "blobId": "2".repeat(40),
                 "lfs": {"sha256": sha, "size": weights.len()}}
            ]
        })))
        .mount(&server)
        .await;
    for (name, body) in [("README.md", &readme), ("w.safetensors", &weights)] {
        Mock::given(method("GET"))
            .and(path(format!("/org/tiny/resolve/{COMMIT}/{name}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
    }
    let digest = format!("sha256:{sha}");
    let config = b"{}".to_vec();
    let config_digest = format!("sha256:{}", hex::encode(sha2::Sha256::digest(&config)));
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "config": {"mediaType": "application/vnd.docker.container.image.v1+json", "digest": config_digest, "size": config.len()},
        "layers": [{"mediaType": "application/vnd.ollama.image.model", "digest": digest, "size": weights.len()}]
    });
    Mock::given(method("GET"))
        .and(path("/v2/library/tiny/manifests/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(manifest))
        .mount(&server)
        .await;
    for (d, body) in [(&digest, &weights), (&config_digest, &config)] {
        Mock::given(method("GET"))
            .and(path(format!("/v2/library/tiny/blobs/{d}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
    }

    let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    set_env("KGET_CONFIG_DIR", dir.path().join("cfg"));
    set_env("KGET_KEYCHAIN", "off");
    set_env("KGET_HF_ENDPOINT", server.uri());
    set_env("HF_HOME", dir.path().join("hf-home"));
    set_env("KGET_OLLAMA_REGISTRY", server.uri());
    set_env("OLLAMA_MODELS", dir.path().join("ollama"));
    for var in [
        "HF_TOKEN",
        "HUGGING_FACE_HUB_TOKEN",
        "HF_HUB_CACHE",
        "HF_TOKEN_PATH",
    ] {
        // SAFETY: see set_env.
        unsafe { std::env::remove_var(var) };
    }

    let out = dir.path().join("tiny");
    let o = out.to_string_lossy().into_owned();
    let result = tokio::task::spawn_blocking(move || {
        kget::builder("hf://org/tiny")
            .output(o)
            .include("*.safetensors")
            .quiet(true)
            .download()
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.path, out.to_string_lossy());
    assert_eq!(result.bytes_downloaded, weights.len() as u64);
    assert_eq!(std::fs::read(out.join("w.safetensors")).unwrap(), weights);
    assert!(!out.join("README.md").exists());

    // A Hub resolve URL on the configured endpoint is detected as hf too.
    let url = format!("{}/org/tiny/resolve/main/README.md", server.uri());
    assert_eq!(kget::protocol::detect(&url), kget::Protocol::HuggingFace);

    // spawn(): progress/status events, then Completed.
    let (handle, events) = kget::builder("ollama://tiny").spawn();
    let events: Vec<DownloadEvent> = tokio::task::spawn_blocking(move || events.iter().collect())
        .await
        .unwrap();
    let r = handle.join().unwrap().unwrap();
    assert!(r.path.ends_with("ollama"));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DownloadEvent::Status(m) if m.contains("Ollama")))
    );
    assert!(matches!(
        events.last(),
        Some(DownloadEvent::Completed { .. })
    ));
    assert!(
        dir.path()
            .join("ollama/manifests/registry.ollama.ai/library/tiny/latest")
            .is_file()
    );
}
