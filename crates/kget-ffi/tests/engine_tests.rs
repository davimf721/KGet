//! `KgetEngine` end to end against a wiremock server: submit, listener
//! events, snapshots, errors and history.

use kget_ffi::{
    DownloadRequest, EngineError, HistoryFilter, HistoryStatus, JobEventRecord, JobState,
    KgetEngine, KgetEventListener, OverwriteMode,
};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TIMEOUT: Duration = Duration::from_secs(30);

/// The config dir is process-wide, so every test shares one.
fn config_dir() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().unwrap()).path()
}

fn engine() -> Arc<KgetEngine> {
    KgetEngine::new(Some(config_dir().to_string_lossy().into_owned())).unwrap()
}

struct Collector(Mutex<Sender<JobEventRecord>>);

impl KgetEventListener for Collector {
    fn on_event(&self, event: JobEventRecord) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

fn listen(engine: &KgetEngine) -> Receiver<JobEventRecord> {
    let (tx, rx) = mpsc::channel();
    engine.set_listener(Arc::new(Collector(Mutex::new(tx))));
    rx
}

/// Events of job `id` up to (and including) its first finished state.
fn events_until_finished(rx: &Receiver<JobEventRecord>, id: u64) -> Vec<JobEventRecord> {
    let mut out = Vec::new();
    loop {
        let ev = rx
            .recv_timeout(TIMEOUT)
            .expect("job did not finish in time");
        if ev.id() != id {
            continue;
        }
        let done = matches!(
            ev,
            JobEventRecord::StateChanged { state, .. } if kget_ffi::types::job_state_is_finished(state)
        );
        out.push(ev);
        if done {
            return out;
        }
    }
}

fn request(url: String, output: &Path) -> DownloadRequest {
    DownloadRequest {
        output: Some(output.to_string_lossy().into_owned()),
        overwrite: Some(OverwriteMode::Overwrite),
        retries: Some(1),
        timeout_secs: Some(10),
        ..DownloadRequest::new(url)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn submit_delivers_events_and_saves_the_file() {
    let server = MockServer::start().await;
    let body = b"hello from kget-ffi".repeat(64);
    Mock::given(method("GET"))
        .and(path("/ffi-ok.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("ffi-ok.bin");
    let url = format!("{}/ffi-ok.bin", server.uri());

    let engine = engine();
    let rx = listen(&engine);
    let mut req = request(url.clone(), &out);
    req.checksum = Some(format!("sha256:{}", sha256_hex(&body)));
    let id = engine.submit(req).unwrap();

    let events = tokio::task::spawn_blocking(move || events_until_finished(&rx, id))
        .await
        .unwrap();

    assert!(matches!(
        events.first(),
        Some(JobEventRecord::StateChanged {
            state: JobState::Queued,
            ..
        })
    ));
    assert!(events.iter().any(|e| matches!(
        e,
        JobEventRecord::StateChanged {
            state: JobState::Running,
            ..
        }
    )));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, JobEventRecord::Started { .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, JobEventRecord::Verified { ok: true, .. }))
    );
    assert!(
        events.iter().any(
            |e| matches!(e, JobEventRecord::Saved { bytes, .. } if *bytes == body.len() as u64)
        )
    );
    assert!(matches!(
        events.last(),
        Some(JobEventRecord::StateChanged {
            state: JobState::Completed,
            error: None,
            ..
        })
    ));
    assert_eq!(std::fs::read(&out).unwrap(), body);

    let snap = engine.get(id).unwrap();
    assert_eq!(snap.state, JobState::Completed);
    assert_eq!(snap.url, url);
    assert_eq!(snap.progress.bytes, body.len() as u64);
    assert!(snap.finished_at.is_some());
    assert!(engine.list().iter().any(|s| s.id == id));

    // Recorded in the history of the engine's config dir.
    let items = engine
        .history(HistoryFilter {
            search: Some("ffi-ok.bin".into()),
            ..HistoryFilter::default()
        })
        .unwrap();
    assert!(
        items
            .iter()
            .any(|i| i.status == HistoryStatus::Completed && i.checksum_ok == Some(true))
    );

    engine.remove(id).unwrap();
    assert!(engine.get(id).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_are_reported() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing.bin"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let out_dir = tempfile::tempdir().unwrap();

    let engine = engine();
    let rx = listen(&engine);
    let id = engine
        .submit(request(
            format!("{}/missing.bin", server.uri()),
            &out_dir.path().join("missing.bin"),
        ))
        .unwrap();
    let events = tokio::task::spawn_blocking(move || events_until_finished(&rx, id))
        .await
        .unwrap();

    assert!(
        events
            .iter()
            .any(|e| matches!(e, JobEventRecord::Error { .. }))
    );
    match events.last() {
        Some(JobEventRecord::StateChanged {
            state: JobState::Failed,
            error: Some(msg),
            ..
        }) => assert!(!msg.is_empty()),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(engine.get(id).unwrap().state, JobState::Failed);
}

#[test]
fn engine_errors() {
    let engine = engine();
    assert_eq!(engine.cancel(999), Err(EngineError::NotFound { id: 999 }));
    assert_eq!(engine.resume(999), Err(EngineError::NotFound { id: 999 }));
    assert!(matches!(
        engine.submit(request(String::new(), Path::new("x"))),
        Err(EngineError::InvalidRequest { .. })
    ));

    engine.set_max_concurrent(0);
    assert_eq!(engine.max_concurrent(), 1, "at least one slot");
    engine.set_max_concurrent(4);
    assert_eq!(engine.max_concurrent(), 4);
    assert_eq!(kget_ffi::kget_version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn queued_jobs_can_be_paused_and_removed() {
    let engine = engine();
    engine.set_max_concurrent(1);
    let rx = listen(&engine);
    // Unroutable address: the first job holds the only slot.
    let out = tempfile::tempdir().unwrap();
    let first = engine
        .submit(request(
            "http://10.255.255.1/a".into(),
            &out.path().join("a"),
        ))
        .unwrap();
    let second = engine
        .submit(request(
            "http://10.255.255.1/b".into(),
            &out.path().join("b"),
        ))
        .unwrap();
    engine.pause(second).unwrap();
    assert_eq!(engine.get(second).unwrap().state, JobState::Paused);
    assert!(matches!(
        engine.pause(second),
        Err(EngineError::InvalidState {
            state: JobState::Paused,
            ..
        })
    ));
    engine.remove(second).unwrap();
    engine.cancel(first).unwrap();
    let events = events_until_finished(&rx, first);
    assert!(matches!(
        events.last(),
        Some(JobEventRecord::StateChanged {
            state: JobState::Cancelled,
            ..
        })
    ));

    // The listener saw the removal; clearing it stops delivery.
    engine.clear_listener();
}

fn sha256_hex(data: &[u8]) -> String {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), data).unwrap();
    kget::checksum::compute_checksum(file.path(), &kget::ChecksumAlgorithm::Sha256).unwrap()
}
