//! Runs every fuzz target (`kget::fuzz_api`) over the checked-in seed corpus
//! (`fuzz/seeds/<target>/`) and the inputs that once crashed it
//! (`fuzz/regressions/<target>/`), so the parsers stay exercised by a plain
//! `cargo test` without nightly or libFuzzer.

use std::path::Path;

fn run(target: &str, f: fn(&[u8])) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz");
    let mut count = 0;
    for kind in ["seeds", "regressions"] {
        let Ok(entries) = std::fs::read_dir(root.join(kind).join(target)) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            if std::panic::catch_unwind(|| f(&data)).is_err() {
                panic!("{target} panicked on {}", path.display());
            }
            count += 1;
        }
    }
    assert!(count > 0, "no inputs for {target}");
    // A few generic edge cases for every target.
    for data in [&b""[..], b"\0", b"\xff\xfe", &[b'a'; 300]] {
        f(data);
    }
}

#[test]
fn metalink() {
    run("metalink", kget::fuzz_api::metalink);
}

#[test]
fn magnet() {
    run("magnet", kget::fuzz_api::magnet);
}

#[test]
fn content_disposition() {
    run("content_disposition", kget::fuzz_api::content_disposition);
}

#[test]
fn filename() {
    run("filename", kget::fuzz_api::filename);
}

#[test]
fn url() {
    run("url", kget::fuzz_api::url);
}

#[test]
fn resume_state() {
    run("resume_state", kget::fuzz_api::resume_state);
}

#[test]
fn checksum_sidecar() {
    run("checksum_sidecar", kget::fuzz_api::checksum_sidecar);
}

#[test]
fn config() {
    run("config", kget::fuzz_api::config);
}

#[test]
fn jsonl() {
    run("jsonl", kget::fuzz_api::jsonl);
}

#[test]
fn date_bound() {
    run("date_bound", kget::fuzz_api::date_bound);
}

#[test]
fn checksum_spec() {
    run("checksum_spec", kget::fuzz_api::checksum_spec);
}

#[test]
fn url_list() {
    run("url_list", kget::fuzz_api::url_list);
}
