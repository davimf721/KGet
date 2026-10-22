//! Job manager tests: submit, concurrency limit, cancel, pause/resume and
//! event fan-out, against a wiremock HTTP server.
//!
//! History recording is disabled so the tests never touch the user's
//! config directory.

use kget::events::JsonEvent;
use kget::jobs::{DownloadState, JobError, JobEvent, JobManager};
use kget::runner::DownloadJob;
use std::sync::mpsc::Receiver;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WAIT: Option<Duration> = Some(Duration::from_secs(20));

fn manager(max: usize) -> JobManager {
    let m = JobManager::with_concurrency(kget::Config::default(), max);
    m.set_record_history(false);
    m
}

fn job(server: &MockServer, name: &str, dir: &TempDir) -> DownloadJob {
    let mut job = DownloadJob::new(format!("{}/{name}", server.uri()));
    job.output = Some(dir.path().join(name).to_string_lossy().into_owned());
    job.retries = 1;
    job
}

async fn mount(server: &MockServer, name: &str, body: &[u8], delay_ms: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/{name}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.to_vec())
                .set_delay(Duration::from_millis(delay_ms)),
        )
        .mount(server)
        .await;
}

/// Collect events until `pred` matches one (or the timeout elapses).
fn collect_until(rx: &Receiver<JobEvent>, pred: impl Fn(&JobEvent) -> bool) -> Vec<JobEvent> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.recv_timeout(Duration::from_secs(20)) {
        let done = pred(&ev);
        out.push(ev);
        if done {
            break;
        }
    }
    out
}

fn states(events: &[JobEvent], id: u64) -> Vec<DownloadState> {
    events
        .iter()
        .filter_map(|e| match e {
            JobEvent::State { id: i, state, .. } if *i == id => Some(*state),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn submit_downloads_and_reports_events() {
    let server = MockServer::start().await;
    let body = b"job manager payload".to_vec();
    mount(&server, "a.bin", &body, 0).await;
    let dir = TempDir::new().unwrap();
    let job = job(&server, "a.bin", &dir);
    let out = dir.path().join("a.bin");

    let (info, events) = tokio::task::spawn_blocking(move || {
        let m = manager(2);
        let rx = m.subscribe();
        let id = m.submit(job);
        assert_eq!(id, 1, "ids are assigned by the manager");
        let info = m.wait(id, WAIT).expect("job finishes");
        let events = collect_until(
            &rx,
            |e| matches!(e, JobEvent::State { state, .. } if state.is_finished()),
        );
        (info, events)
    })
    .await
    .unwrap();

    assert_eq!(info.state, DownloadState::Completed, "{:?}", info.error);
    assert_eq!(std::fs::read(&out).unwrap(), body);
    assert_eq!(info.path.as_deref(), Some(out.to_string_lossy().as_ref()));
    assert_eq!(
        states(&events, info.id),
        vec![
            DownloadState::Queued,
            DownloadState::Running,
            DownloadState::Completed
        ]
    );
    let download: Vec<&JsonEvent> = events
        .iter()
        .filter_map(|e| match e {
            JobEvent::Download { event, .. } => Some(event),
            _ => None,
        })
        .collect();
    assert!(matches!(download.first(), Some(JsonEvent::Started { .. })));
    assert!(
        download
            .iter()
            .any(|e| matches!(e, JsonEvent::Saved { bytes, .. } if *bytes == body.len() as u64))
    );
    assert!(matches!(download.last(), Some(JsonEvent::Completed)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrency_limit_is_respected() {
    let server = MockServer::start().await;
    for i in 0..5 {
        mount(&server, &format!("f{i}.bin"), b"data", 300).await;
    }
    let dir = TempDir::new().unwrap();
    let jobs: Vec<DownloadJob> = (0..5)
        .map(|i| job(&server, &format!("f{i}.bin"), &dir))
        .collect();

    let (max_seen, infos) = tokio::task::spawn_blocking(move || {
        let m = manager(2);
        let rx = m.subscribe();
        let ids: Vec<u64> = jobs.into_iter().map(|j| m.submit(j)).collect();
        assert!(m.running() <= 2);
        assert!(m.wait_idle(WAIT), "all jobs finish");

        // Replay the state events: never more than 2 running at once.
        let mut running = std::collections::HashSet::new();
        let mut max_seen = 0;
        while let Ok(ev) = rx.try_recv() {
            if let JobEvent::State { id, state, .. } = ev {
                if state.is_active() {
                    running.insert(id);
                } else {
                    running.remove(&id);
                }
                max_seen = max_seen.max(running.len());
            }
        }
        let infos: Vec<_> = ids.iter().map(|id| m.get(*id).unwrap()).collect();
        (max_seen, infos)
    })
    .await
    .unwrap();

    assert_eq!(max_seen, 2, "two slots are used, never more");
    assert!(infos.iter().all(|i| i.state == DownloadState::Completed));
    let mut ids: Vec<u64> = infos.iter().map(|i| i.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), 5, "unique ids");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_queued_and_running_jobs() {
    let server = MockServer::start().await;
    mount(&server, "slow.bin", b"slow", 800).await;
    mount(&server, "next.bin", b"next", 0).await;
    let dir = TempDir::new().unwrap();
    let slow = job(&server, "slow.bin", &dir);
    let next = job(&server, "next.bin", &dir);

    let (slow_info, next_info, errors) = tokio::task::spawn_blocking(move || {
        let m = manager(1);
        let a = m.submit(slow);
        let b = m.submit(next);
        assert_eq!(m.get(b).unwrap().state, DownloadState::Queued);

        // Queued: cancelled immediately and never started.
        m.cancel(b).unwrap();
        assert_eq!(m.get(b).unwrap().state, DownloadState::Cancelled);

        // Running: stops when the downloader notices the request.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(m.get(a).unwrap().state, DownloadState::Running);
        m.cancel(a).unwrap();
        let slow_info = m.wait(a, WAIT).unwrap();

        let errors = (
            m.cancel(a),
            m.remove(a).map(|i| i.id),
            m.cancel(999),
            m.get(a).is_none(),
        );
        (slow_info, m.get(b).unwrap(), errors)
    })
    .await
    .unwrap();

    assert_eq!(slow_info.state, DownloadState::Cancelled);
    assert_eq!(next_info.started_at, None);
    assert!(!dir.path().join("next.bin").exists());
    let (cancel_again, removed, unknown, gone) = errors;
    assert!(matches!(
        cancel_again,
        Err(JobError::InvalidState {
            state: DownloadState::Cancelled,
            ..
        })
    ));
    assert_eq!(removed, Ok(slow_info.id));
    assert_eq!(unknown, Err(JobError::NotFound(999)));
    assert!(gone);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_and_resume_complete_the_job() {
    let server = MockServer::start().await;
    // First request is slow (paused while waiting), the retry is immediate.
    Mock::given(method("GET"))
        .and(path("/p.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(b"paused payload".to_vec())
                .set_delay(Duration::from_millis(800)),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount(&server, "p.bin", b"paused payload", 0).await;
    let dir = TempDir::new().unwrap();
    let job = job(&server, "p.bin", &dir);
    let out = dir.path().join("p.bin");

    let (paused, done, resume_err) = tokio::task::spawn_blocking(move || {
        let m = manager(1);
        let id = m.submit(job);
        std::thread::sleep(Duration::from_millis(100));
        m.pause(id).unwrap();
        let paused = m.wait(id, WAIT).unwrap();
        m.resume(id).unwrap();
        let done = m.wait(id, WAIT).unwrap();
        (paused, done, m.resume(id))
    })
    .await
    .unwrap();

    assert_eq!(paused.state, DownloadState::Paused);
    assert_eq!(done.state, DownloadState::Completed, "{:?}", done.error);
    assert!(done.job.resume, "resumed jobs continue their .part file");
    assert_eq!(std::fs::read(out).unwrap(), b"paused payload");
    assert!(matches!(resume_err, Err(JobError::InvalidState { .. })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_fan_out_to_every_subscriber() {
    let server = MockServer::start().await;
    mount(&server, "ok.bin", b"ok", 0).await;
    Mock::given(method("GET"))
        .and(path("/missing.bin"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let dir = TempDir::new().unwrap();
    let ok = job(&server, "ok.bin", &dir);
    let missing = job(&server, "missing.bin", &dir);

    let (first, second, failed) = tokio::task::spawn_blocking(move || {
        let m = manager(2);
        let rx1 = m.subscribe();
        let rx2 = m.subscribe();
        // A dropped subscriber is skipped without affecting the others.
        drop(m.subscribe());
        let a = m.submit(ok);
        let b = m.submit(missing);
        assert!(m.wait_idle(WAIT));
        let drain = |rx: &Receiver<JobEvent>| -> Vec<String> {
            std::iter::from_fn(|| rx.try_recv().ok())
                .map(|e| serde_json::to_string(&e).unwrap())
                .collect()
        };
        let (e1, e2) = (drain(&rx1), drain(&rx2));
        assert_eq!(m.get(a).unwrap().state, DownloadState::Completed);
        (e1, e2, m.get(b).unwrap())
    })
    .await
    .unwrap();

    assert!(!first.is_empty());
    assert_eq!(first, second, "every subscriber sees the same events");
    assert_eq!(failed.state, DownloadState::Failed);
    assert!(failed.error.as_deref().unwrap_or("").contains("404"));
    assert!(
        first
            .iter()
            .any(|e| e.contains(r#""type":"state""#) && e.contains(r#""state":"failed""#))
    );
}

/// A `phase: verifying` event moves the job to `Verifying`, whether the hash
/// is computed by the runner (`--checksum`) or by the engine (`--sha256`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checksum_phase_drives_verifying_state() {
    use kget::checksum::ChecksumAlgorithm;
    use kget::events::Phase;
    use sha2::Digest;

    let server = MockServer::start().await;
    let body = b"verify me please".to_vec();
    mount(&server, "c.bin", &body, 0).await;
    mount(&server, "s.bin", &body, 0).await;
    let dir = TempDir::new().unwrap();
    let hash = hex::encode(sha2::Sha256::digest(&body));

    let mut by_checksum = job(&server, "c.bin", &dir);
    by_checksum.checksum = Some((ChecksumAlgorithm::Sha256, hash.clone()));
    let mut by_sha256 = job(&server, "s.bin", &dir);
    by_sha256.sha256 = Some(hash);

    for job in [by_checksum, by_sha256] {
        let (info, events) = tokio::task::spawn_blocking(move || {
            let m = manager(1);
            let rx = m.subscribe();
            let id = m.submit(job);
            let info = m.wait(id, WAIT).expect("job finishes");
            let events = collect_until(
                &rx,
                |e| matches!(e, JobEvent::State { state, .. } if state.is_finished()),
            );
            (info, events)
        })
        .await
        .unwrap();

        assert_eq!(info.state, DownloadState::Completed, "{:?}", info.error);
        assert_eq!(
            states(&events, info.id),
            vec![
                DownloadState::Queued,
                DownloadState::Running,
                DownloadState::Verifying,
                DownloadState::Completed
            ]
        );
        // The phase event precedes the state change it causes.
        let phase_at = events.iter().position(|e| {
            matches!(
                e,
                JobEvent::Download {
                    event: JsonEvent::Phase {
                        phase: Phase::Verifying,
                        ..
                    },
                    ..
                }
            )
        });
        let state_at = events.iter().position(
            |e| matches!(e, JobEvent::State { state, .. } if *state == DownloadState::Verifying),
        );
        assert!(phase_at.is_some() && phase_at < state_at, "{events:?}");
    }
}
