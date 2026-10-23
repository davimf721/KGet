//! Terminal side of `kget daemon …` and `kget queue …`.
//!
//! The binary only parses arguments; these functions talk to the daemon and
//! print tables, progress lines or (`--json`) machine-readable output. They
//! return the process exit code.

use super::DaemonPaths;
use super::client::DaemonClient;
use super::protocol::{RpcError, SubmitParams, SubscribeParams};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Connect to the daemon, starting it when `autostart` is set.
pub fn client(autostart: bool) -> Result<DaemonClient, BoxError> {
    let result = if autostart {
        DaemonClient::ensure_running()
    } else {
        DaemonClient::connect_default()
    };
    result.map_err(|e| {
        if autostart {
            e.message.into()
        } else {
            format!("{} (start it with `kget daemon`)", e.message).into()
        }
    })
}

/// `kget daemon` without `--foreground`: start it in the background.
pub fn start_background(extra_args: &[String]) -> Result<i32, BoxError> {
    let paths = DaemonPaths::resolve()?;
    if let Ok(c) = DaemonClient::connect(&paths) {
        let v = c.call("version", Value::Null)?;
        println!("The KGet daemon is already running (pid {}).", v["pid"]);
        return Ok(0);
    }
    let exe = std::env::current_exe()?;
    let c = DaemonClient::ensure_running_with(&paths, &exe, extra_args)?;
    let v = c.call("version", Value::Null)?;
    println!(
        "KGet daemon started (pid {}). Downloads go to {}.\nLog: {}",
        v["pid"],
        v["download_dir"].as_str().unwrap_or("?"),
        paths.log().display()
    );
    Ok(0)
}

/// `kget daemon status`.
pub fn status(json_out: bool) -> Result<i32, BoxError> {
    let paths = DaemonPaths::resolve()?;
    match DaemonClient::connect(&paths) {
        Ok(c) => {
            let v = c.call("version", Value::Null)?;
            let jobs = c.list()?;
            if json_out {
                println!(
                    "{}",
                    json!({"running": true, "daemon": v, "jobs": jobs.len()})
                );
            } else {
                println!(
                    "KGet daemon {} running (pid {}, protocol {}, {} transport)",
                    v["kget"].as_str().unwrap_or("?"),
                    v["pid"],
                    v["protocol"],
                    v["transport"].as_str().unwrap_or("?")
                );
                println!(
                    "Download dir: {}",
                    v["download_dir"].as_str().unwrap_or("?")
                );
                println!("Jobs:         {}", jobs.len());
                println!("Log:          {}", paths.log().display());
            }
            Ok(0)
        }
        Err(e) => {
            if json_out {
                println!("{}", json!({"running": false, "error": e.message}));
            } else {
                println!("The KGet daemon is not running.");
            }
            Ok(3)
        }
    }
}

/// `kget daemon stop`.
pub fn stop() -> Result<i32, BoxError> {
    let paths = DaemonPaths::resolve()?;
    let Ok(c) = DaemonClient::connect(&paths) else {
        println!("The KGet daemon is not running.");
        return Ok(0);
    };
    c.shutdown()?;
    // Wait for the socket to go away (jobs are paused and saved first).
    for _ in 0..100 {
        if DaemonClient::connect(&paths).is_err() && !paths.pid().exists() {
            println!("KGet daemon stopped; unfinished jobs resume on the next start.");
            return Ok(0);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("The daemon did not stop within 10 seconds".into())
}

/// `kget queue add`.
pub fn add(
    params: &SubmitParams,
    wait: bool,
    json_out: bool,
    autostart: bool,
) -> Result<i32, BoxError> {
    let c = client(autostart)?;
    // Subscribe before submitting so no event is missed.
    let events = if wait {
        Some(c.events(&SubscribeParams::default())?)
    } else {
        None
    };
    let job = c.submit(params)?;
    let id = job["id"].as_u64().unwrap_or(0);
    if json_out && !wait {
        println!("{job}");
    } else if !json_out {
        println!(
            "Queued job {id}: {}",
            job["job"]["output"].as_str().unwrap_or_default()
        );
    }
    match events {
        Some(rx) => follow(&c, id, rx, json_out),
        None => Ok(0),
    }
}

/// Print events of job `id` until it finishes; exit code 0 when completed.
fn follow(
    c: &DaemonClient,
    id: u64,
    rx: std::sync::mpsc::Receiver<Value>,
    json_out: bool,
) -> Result<i32, BoxError> {
    loop {
        let ev = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(ev) => ev,
            Err(RecvTimeoutError::Timeout) => {
                // Missed the final event (e.g. reconnect): ask directly.
                let job = c.get(id)?;
                if is_finished(&job["state"]) {
                    return Ok(finish(&job, json_out));
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return Err("Lost the daemon connection".into()),
        };
        if ev["id"].as_u64() != Some(id) {
            continue;
        }
        if json_out {
            println!("{ev}");
        } else if ev["type"] == "download" && ev["event"]["event"] == "progress" {
            print_progress(&ev["event"]);
        }
        if ev["type"] == "state" && is_finished(&ev["state"]) {
            if !json_out {
                eprintln!();
            }
            return Ok(finish(&c.get(id)?, json_out));
        }
        if ev["type"] == "removed" {
            return Ok(1);
        }
    }
}

fn finish(job: &Value, json_out: bool) -> i32 {
    if json_out {
        println!("{job}");
    } else {
        print_job_line(job);
        if let Some(err) = job["error"].as_str() {
            eprintln!("Error: {err}");
        }
    }
    if job["state"] == "completed" { 0 } else { 1 }
}

fn is_finished(state: &Value) -> bool {
    matches!(
        state.as_str(),
        Some("completed" | "failed" | "cancelled" | "paused")
    )
}

fn print_progress(p: &Value) {
    let bytes = p["bytes"].as_u64().unwrap_or(0);
    let size = humansize::format_size(bytes, humansize::BINARY);
    let speed = humansize::format_size(p["speed_bps"].as_u64().unwrap_or(0), humansize::BINARY);
    let pct = p["percent"]
        .as_f64()
        .map(|x| format!("{x:5.1}%"))
        .unwrap_or_else(|| "   ? ".into());
    eprint!("\r{pct}  {size:>10}  {speed:>10}/s   ");
    let _ = std::io::stderr().flush();
}

/// `kget queue list`.
pub fn list(json_out: bool) -> Result<i32, BoxError> {
    let jobs = client(false)?.list()?;
    if json_out {
        println!("{}", Value::Array(jobs));
    } else if jobs.is_empty() {
        println!("The queue is empty.");
    } else {
        println!(
            "{:>5}  {:<10} {:>7} {:>10}  File / URL",
            "ID", "State", "Done", "Size"
        );
        println!("{}", "-".repeat(80));
        for j in &jobs {
            print_job_line(j);
        }
    }
    Ok(0)
}

fn print_job_line(j: &Value) {
    let pct = j["progress"]["percent"]
        .as_f64()
        .map(|x| format!("{x:.1}%"))
        .unwrap_or_default();
    let size = j["progress"]["total"]
        .as_u64()
        .or(j["progress"]["bytes"].as_u64().filter(|b| *b > 0))
        .map(|b| humansize::format_size(b, humansize::BINARY))
        .unwrap_or_default();
    let target = j["path"]
        .as_str()
        .or(j["job"]["output"].as_str())
        .filter(|s| !s.is_empty())
        .or(j["job"]["url"].as_str())
        .unwrap_or_default();
    println!(
        "{:>5}  {:<10} {:>7} {:>10}  {}",
        j["id"],
        j["state"].as_str().unwrap_or("?"),
        pct,
        size,
        target
    );
}

/// `kget queue status <id>`.
pub fn job_status(id: u64, json_out: bool) -> Result<i32, BoxError> {
    let j = client(false)?.get(id)?;
    if json_out {
        println!("{j}");
    } else {
        println!("Job {id}");
        println!("  State:    {}", j["state"].as_str().unwrap_or("?"));
        println!("  URL:      {}", j["job"]["url"].as_str().unwrap_or(""));
        println!(
            "  Output:   {}",
            j["path"]
                .as_str()
                .or(j["job"]["output"].as_str())
                .unwrap_or("")
        );
        let p = &j["progress"];
        println!(
            "  Progress: {} of {}",
            humansize::format_size(p["bytes"].as_u64().unwrap_or(0), humansize::BINARY),
            p["total"]
                .as_u64()
                .map(|t| humansize::format_size(t, humansize::BINARY))
                .unwrap_or_else(|| "?".into())
        );
        if let Some(e) = j["error"].as_str() {
            println!("  Error:    {e}");
        }
    }
    Ok(0)
}

/// `kget queue cancel|pause|resume|rm <id>`.
pub fn job_action(method: &str, id: u64, json_out: bool) -> Result<i32, BoxError> {
    let j = client(false)?.call(method, json!({ "id": id }))?;
    if json_out {
        println!("{j}");
    } else {
        let done = match method {
            "cancel" => "Cancelled",
            "pause" => "Paused",
            "resume" => "Resumed",
            _ => "Removed",
        };
        println!("{done} job {id}.");
    }
    Ok(0)
}

/// `kget queue watch [ids…]`: print events until Ctrl+C (or until every
/// given job has finished).
pub fn watch(ids: &[u64], json_out: bool) -> Result<i32, BoxError> {
    let c = client(false)?;
    let rx = c.events(&SubscribeParams {
        ids: ids.to_vec(),
        progress: true,
    })?;
    let mut pending: std::collections::HashSet<u64> = ids.iter().copied().collect();
    for id in ids {
        if let Ok(j) = c.get(*id)
            && is_finished(&j["state"])
        {
            pending.remove(id);
        }
    }
    if !ids.is_empty() && pending.is_empty() {
        return Ok(0);
    }
    for ev in rx {
        if json_out {
            println!("{ev}");
        } else {
            match ev["type"].as_str() {
                Some("state") => println!(
                    "job {}: {}{}",
                    ev["id"],
                    ev["state"].as_str().unwrap_or("?"),
                    ev["error"]
                        .as_str()
                        .map(|e| format!(" ({e})"))
                        .unwrap_or_default()
                ),
                Some("removed") => println!("job {}: removed", ev["id"]),
                Some("download") if ev["event"]["event"] == "progress" => {
                    let p = &ev["event"];
                    println!(
                        "job {}: {} {}",
                        ev["id"],
                        p["percent"]
                            .as_f64()
                            .map(|x| format!("{x:.1}%"))
                            .unwrap_or_default(),
                        humansize::format_size(p["bytes"].as_u64().unwrap_or(0), humansize::BINARY)
                    )
                }
                _ => {}
            }
        }
        let _ = std::io::stdout().flush();
        if let Some(id) = ev["id"].as_u64()
            && (ev["type"] == "removed" || (ev["type"] == "state" && is_finished(&ev["state"])))
        {
            pending.remove(&id);
            if !ids.is_empty() && pending.is_empty() {
                return Ok(0);
            }
        }
    }
    Err(RpcError::internal("The daemon closed the connection").into())
}

/// `kget <url> --via-daemon`: submit, show progress, exit like a normal
/// download (0 on success).
pub fn download_via_daemon(params: &SubmitParams, quiet: bool) -> Result<i32, BoxError> {
    let c = client(true)?;
    let rx = c.events(&SubscribeParams::default())?;
    let job = c.submit(params)?;
    let id = job["id"].as_u64().unwrap_or(0);
    if quiet {
        drop(rx);
        loop {
            let v = c.call("wait", json!({"id": id, "timeout_secs": 3600}))?;
            if v["finished"] == true {
                return Ok(if v["job"]["state"] == "completed" {
                    0
                } else {
                    1
                });
            }
        }
    }
    follow(&c, id, rx, false)
}
