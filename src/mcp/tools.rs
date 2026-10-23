//! MCP tools: definitions (name, description, JSON Schema) and handlers.

use super::Output;
use crate::OverwritePolicy;
use crate::checksum::{ChecksumAlgorithm, compute_checksum};
use crate::daemon::protocol::{RpcError, SubmitParams, SubscribeParams, codes};
use crate::daemon::{Backend, PathPolicy};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

/// Default and maximum time `download` waits with `wait: true`.
const DEFAULT_WAIT_SECS: u64 = 600;
const MAX_WAIT_SECS: u64 = 3600;

/// Everything a tool call needs.
pub struct ToolContext {
    /// Daemon client or in-process service
    pub backend: Arc<dyn Backend>,
    /// Where files may be written / read
    pub policy: PathPolicy,
    /// Progress notifications, when the client asked for them
    pub progress: Option<Progress>,
    /// Set when the client cancels the request
    pub cancel: Arc<AtomicBool>,
    /// Set when the session ends (stdin closed): stop waiting, still answer
    pub closing: Arc<AtomicBool>,
}

impl ToolContext {
    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.closing.load(Ordering::SeqCst)
    }
}

/// Sends `notifications/progress` for one request. Progress only ever
/// increases, as the spec requires.
pub struct Progress {
    out: Output,
    token: Value,
    last: std::sync::Mutex<f64>,
}

impl Progress {
    pub(crate) fn new(out: Output, token: Value) -> Self {
        Self {
            out,
            token,
            last: std::sync::Mutex::new(-1.0),
        }
    }

    fn report(&self, progress: f64, total: Option<f64>, message: &str) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if progress <= *last {
            return;
        }
        *last = progress;
        let mut params = json!({
            "progressToken": self.token,
            "progress": progress,
            "message": message,
        });
        if let Some(t) = total {
            params["total"] = json!(t);
        }
        self.out.send(&crate::daemon::protocol::notification(
            "notifications/progress",
            params,
        ));
    }
}

fn id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "integer", "minimum": 1, "description": "Job id returned by `download` or `list_downloads`." }
        },
        "required": ["id"],
        "additionalProperties": false
    })
}

/// Tool definitions for `tools/list`.
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "download",
            "title": "Download a file",
            "description": "Download a URL into the allowed download directory. Supports http(s), ftp(s), sftp, webdav(s), magnet links, .torrent and Metalink URLs, video pages (YouTube etc., via yt-dlp), Hugging Face repositories/files (hf://org/repo[/file]) and Ollama models (ollama://name:tag, saved as plain files). With wait=true (default) the call returns when the download finishes, with the saved path; with wait=false it returns a job id immediately — poll it with `download_status`. Existing files are never overwritten unless overwrite='overwrite': a free name like 'file (1).zip' is chosen. Give `checksum` to verify integrity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "What to download: http(s)://, ftp(s)://, sftp://, webdav(s)://, magnet:?… or a video page URL." },
                    "filename": { "type": "string", "description": "File name to save as (a single name, no slashes). Default: taken from the URL. Not allowed for torrents, Metalink and video." },
                    "dir": { "type": "string", "description": "Sub-directory of the allowed download directory (created if missing), e.g. 'datasets'. Default: the download directory itself." },
                    "checksum": { "type": "string", "description": "Expected checksum as '<algorithm>:<hex>' (sha256, sha512, sha1, md5, blake3), e.g. 'sha256:9f86d08…'. The download fails if it does not match." },
                    "checksum_url": { "type": "string", "description": "URL of a checksum file (SHA256SUMS, file.iso.sha256) to verify against." },
                    "headers": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Extra HTTP request headers, e.g. {\"Referer\": \"https://example.com/\"}." },
                    "turbo": { "type": "boolean", "description": "Use several parallel connections (faster for large HTTP files on servers that support ranges). Default false." },
                    "extract": { "type": "boolean", "description": "Extract the archive (.zip, .tar.gz, .7z…) after downloading. Default false." },
                    "overwrite": { "type": "string", "enum": ["rename", "overwrite", "skip"], "description": "If the file already exists: 'rename' (default, save as 'name (1).ext'), 'overwrite', or 'skip' (keep the existing file)." },
                    "wait": { "type": "boolean", "description": "Wait for the download to finish (default true). Use false for large files." },
                    "timeout_secs": { "type": "integer", "minimum": 1, "maximum": MAX_WAIT_SECS, "description": "With wait=true, stop waiting after this many seconds (default 600); the download keeps running." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags recorded in the download history (letters, digits, '-', '_'), e.g. [\"dataset\", \"q3\"]." },
                    "organize": { "type": "boolean", "description": "Apply the user's smart rules: the matching rule picks a sub-folder of `dir` (or of the download directory) and may add tags. Default: the user's setting." },
                    "include": { "type": "array", "items": { "type": "string" }, "description": "hf:// only: download just the files matching one of these globs, e.g. [\"*.safetensors\", \"config.json\"]." },
                    "exclude": { "type": "array", "items": { "type": "string" }, "description": "hf:// only: skip files matching any of these globs, e.g. [\"*.bin\"]." },
                    "revision": { "type": "string", "description": "hf:// only: branch, tag or commit to download (default 'main')." },
                    "select": { "type": "string", "description": "Torrents only: file numbers to download, 1-based as listed in the torrent, e.g. '1,3-5'." },
                    "select_glob": { "type": "array", "items": { "type": "string" }, "description": "Torrents only: download just the files whose path matches one of these patterns, e.g. [\"*.mkv\"]." },
                    "seed_ratio": { "type": "number", "exclusiveMinimum": 0, "description": "Torrents only: keep seeding after the download until this upload/size ratio." },
                    "seed_time_secs": { "type": "integer", "minimum": 0, "description": "Torrents only: keep seeding after the download for this many seconds." }
                },
                "required": ["url"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": true }
        }),
        json!({
            "name": "download_status",
            "title": "Download status",
            "description": "Get the state (queued, running, verifying, paused, cancelled, completed, failed), progress, saved path and error of one download job.",
            "inputSchema": id_schema(),
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "list_downloads",
            "title": "List downloads",
            "description": "List current download jobs (newest last) with their state and progress.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "state": { "type": "string", "enum": ["queued", "running", "verifying", "paused", "cancelled", "completed", "failed"], "description": "Only jobs in this state." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Maximum number of jobs (most recent), default 20." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "cancel_download",
            "title": "Cancel a download",
            "description": "Cancel a queued or running download. The partial file is kept, so `resume_download` can continue it.",
            "inputSchema": id_schema(),
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "pause_download",
            "title": "Pause a download",
            "description": "Pause a queued or running download; `resume_download` continues it from where it stopped.",
            "inputSchema": id_schema(),
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "resume_download",
            "title": "Resume a download",
            "description": "Resume a paused, cancelled or failed download (continues from the partial file when the server supports it). Returns immediately; poll `download_status`.",
            "inputSchema": id_schema(),
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": true }
        }),
        json!({
            "name": "search_history",
            "title": "Search download history",
            "description": "Search the persistent history of past downloads (also those made outside this session), newest first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Text contained in the URL or file name." },
                    "status": { "type": "string", "enum": ["completed", "failed", "cancelled", "handed_off"], "description": "Only entries with this outcome." },
                    "domain": { "type": "string", "description": "Only downloads from this host (subdomains included), e.g. 'github.com'." },
                    "file_type": { "type": "string", "description": "Category (video, audio, image, archive, document, disk, package) or extensions like 'iso,zip'." },
                    "since": { "type": "string", "description": "Started on/after: 'YYYY-MM-DD', 'today', '7d', '12h' (UTC)." },
                    "until": { "type": "string", "description": "Started on/before: 'YYYY-MM-DD' (UTC, inclusive)." },
                    "sort": { "type": "string", "enum": ["newest", "oldest", "largest", "smallest", "name", "fastest"], "description": "Order, default newest." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Maximum entries, default 20." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "verify_checksum",
            "title": "Verify a file's checksum",
            "description": "Compute the checksum of a local file inside the allowed download directory and, if `expected` is given, report whether it matches.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path, relative to the download directory (or absolute inside it)." },
                    "expected": { "type": "string", "description": "Expected checksum: '<algorithm>:<hex>' or plain hex (the algorithm is guessed from the length)." },
                    "algorithm": { "type": "string", "enum": ["sha256", "sha512", "sha1", "md5", "blake3"], "description": "Algorithm when `expected` does not name one; default sha256." }
                },
                "required": ["path"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
        json!({
            "name": "get_config",
            "title": "Show KGet configuration",
            "description": "Show where downloads are saved (the allowed directory), whether a background daemon is used, and KGet's settings (proxy without credentials, speed limit, connections).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        }),
    ]
}

/// Run tool `name`. Protocol errors (unknown tool, malformed arguments) are
/// `Err`; failures of the operation are results with `isError: true`.
pub fn call(ctx: &ToolContext, name: &str, args: &Value) -> Result<Value, RpcError> {
    if !args.is_object() {
        return Err(RpcError::invalid_params("arguments must be an object"));
    }
    let outcome = match name {
        "download" => download(ctx, args),
        "download_status" => job_id(args).and_then(|id| {
            let job = ctx.backend.call("get", json!({ "id": id }))?;
            Ok(job_result(&job))
        }),
        "list_downloads" => list(ctx, args),
        "cancel_download" | "pause_download" | "resume_download" => job_id(args).and_then(|id| {
            let method = name.trim_end_matches("_download");
            let job = ctx.backend.call(method, json!({ "id": id }))?;
            Ok(job_result(&job))
        }),
        "search_history" => history(ctx, args),
        "verify_checksum" => verify(ctx, args),
        "get_config" => config(ctx),
        other => {
            return Err(RpcError::invalid_params(format!("Unknown tool '{other}'")));
        }
    };
    Ok(match outcome {
        Ok(r) => r.into_value(),
        Err(e) => ToolResult::error(&e.message).into_value(),
    })
}

/// A tool result: a short text for the model plus the structured data.
struct ToolResult {
    summary: String,
    data: Value,
    is_error: bool,
}

impl ToolResult {
    fn ok(summary: impl Into<String>, data: Value) -> Self {
        Self {
            summary: summary.into(),
            data,
            is_error: false,
        }
    }

    fn error(message: &str) -> Self {
        Self {
            summary: format!("Error: {message}"),
            data: json!({ "error": message }),
            is_error: true,
        }
    }

    fn into_value(self) -> Value {
        let text = format!(
            "{}\n{}",
            self.summary,
            serde_json::to_string_pretty(&self.data).unwrap_or_default()
        );
        json!({
            "content": [{ "type": "text", "text": text }],
            "structuredContent": self.data,
            "isError": self.is_error,
        })
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

fn job_id(args: &Value) -> Result<u64, RpcError> {
    args.get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::invalid_params("'id' (a positive integer) is required"))
}

/// The fields of a job snapshot an agent needs.
fn job_view(job: &Value) -> Value {
    let p = &job["progress"];
    json!({
        "id": job["id"],
        "state": job["state"],
        "url": job["job"]["url"],
        "path": job["path"].as_str().or(job["job"]["output"].as_str()),
        "bytes": p["bytes"],
        "total_bytes": p["total"],
        "percent": p["percent"],
        "speed_bps": p["speed_bps"],
        "eta_secs": p["eta_secs"],
        "error": job["error"],
    })
}

fn job_result(job: &Value) -> ToolResult {
    let view = job_view(job);
    let state = view["state"].as_str().unwrap_or("unknown");
    let mut summary = format!("Job {} is {state}", view["id"]);
    if let Some(p) = view["path"].as_str() {
        summary.push_str(&format!(" ({p})"));
    }
    if let Some(e) = view["error"].as_str() {
        summary.push_str(&format!(": {e}"));
    }
    summary.push('.');
    ToolResult {
        is_error: state == "failed",
        summary,
        data: view,
    }
}

/// An array-of-strings argument (a single string is accepted too).
fn str_list_arg(args: &Value, key: &str) -> Result<Vec<String>, RpcError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| RpcError::invalid_params(format!("'{key}' must be strings")))
            })
            .collect(),
        Some(_) => Err(RpcError::invalid_params(format!(
            "'{key}' must be an array of strings"
        ))),
    }
}

fn download(ctx: &ToolContext, args: &Value) -> Result<ToolResult, RpcError> {
    let url = str_arg(args, "url").ok_or_else(|| RpcError::invalid_params("'url' is required"))?;
    // Resolve against the MCP root first; the daemon checks again against its own.
    let dir = ctx.policy.resolve_dir(str_arg(args, "dir"))?;
    let filename = str_arg(args, "filename").map(str::to_string);
    if let Some(name) = &filename {
        ctx.policy.check_file_name(name)?;
    }
    let overwrite = match str_arg(args, "overwrite") {
        None | Some("rename") => OverwritePolicy::Rename,
        Some("overwrite") => OverwritePolicy::Overwrite,
        Some("skip") => OverwritePolicy::Skip,
        Some(other) => {
            return Err(RpcError::invalid_params(format!(
                "overwrite must be rename, overwrite or skip, not '{other}'"
            )));
        }
    };
    let headers = match args.get("headers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(m)) => m
            .iter()
            .map(|(k, v)| {
                v.as_str()
                    .map(|v| (k.clone(), v.to_string()))
                    .ok_or_else(|| RpcError::invalid_params("header values must be strings"))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(RpcError::invalid_params("headers must be an object")),
    };
    let params = SubmitParams {
        url: url.trim().to_string(),
        dir: Some(dir.to_string_lossy().into_owned()),
        filename,
        advanced: args.get("turbo").and_then(Value::as_bool).unwrap_or(false),
        checksum: str_arg(args, "checksum").map(str::to_string),
        checksum_url: str_arg(args, "checksum_url").map(str::to_string),
        headers,
        overwrite: Some(overwrite),
        extract: args
            .get("extract")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        tags: str_list_arg(args, "tags")?,
        organize: args.get("organize").and_then(Value::as_bool),
        include: str_list_arg(args, "include")?,
        exclude: str_list_arg(args, "exclude")?,
        revision: str_arg(args, "revision").map(str::to_string),
        select: str_arg(args, "select").map(str::to_string),
        select_glob: str_list_arg(args, "select_glob")?,
        seed_ratio: args.get("seed_ratio").and_then(Value::as_f64),
        seed_time_secs: args.get("seed_time_secs").and_then(Value::as_u64),
        ..SubmitParams::default()
    };
    let wait = args.get("wait").and_then(Value::as_bool).unwrap_or(true);
    let timeout = args
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_WAIT_SECS)
        .clamp(1, MAX_WAIT_SECS);

    // Subscribe before submitting so no event is missed.
    let events = if wait {
        ctx.backend.subscribe(SubscribeParams::default()).ok()
    } else {
        None
    };
    let job = ctx.backend.call(
        "submit",
        serde_json::to_value(&params).map_err(RpcError::internal)?,
    )?;
    let id = job["id"].as_u64().unwrap_or(0);
    if !wait {
        let mut r = job_result(&job);
        r.summary = format!(
            "Started job {id}; it saves to {}. Check it with download_status.",
            job_view(&job)["path"]
                .as_str()
                .unwrap_or("the download directory")
        );
        return Ok(r);
    }

    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut finished = false;
    while !ctx.stopped() && Instant::now() < deadline {
        let event = match &events {
            Some(rx) => match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(ev) => Some(ev),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(500));
                    None
                }
            },
            None => {
                std::thread::sleep(Duration::from_millis(500));
                None
            }
        };
        match event {
            Some(ev) if ev["id"].as_u64() == Some(id) => {
                if ev["type"] == "download"
                    && ev["event"]["event"] == "progress"
                    && let Some(p) = &ctx.progress
                {
                    let e = &ev["event"];
                    let bytes = e["bytes"].as_u64().unwrap_or(0);
                    let msg = match e["percent"].as_f64() {
                        Some(pct) => format!("{pct:.1}% of {url}"),
                        None => format!("{bytes} bytes of {url}"),
                    };
                    p.report(bytes as f64, e["total"].as_u64().map(|t| t as f64), &msg);
                }
                if ev["type"] == "removed"
                    || ev["type"] == "state"
                        && matches!(
                            ev["state"].as_str(),
                            Some("completed" | "failed" | "cancelled" | "paused")
                        )
                {
                    finished = true;
                    break;
                }
            }
            Some(_) => {}
            None => {
                // Also poll, in case events are unavailable.
                let job = ctx.backend.call("get", json!({ "id": id }))?;
                if matches!(
                    job["state"].as_str(),
                    Some("completed" | "failed" | "cancelled" | "paused")
                ) {
                    finished = true;
                    break;
                }
            }
        }
    }
    let job = ctx.backend.call("get", json!({ "id": id }))?;
    let mut r = job_result(&job);
    if !finished {
        r.summary = format!(
            "Job {id} is still {} after {timeout}s; it keeps running. Check it with download_status.",
            r.data["state"].as_str().unwrap_or("running")
        );
    } else if r.data["state"] == "completed" {
        r.summary = format!(
            "Downloaded {} ({} bytes).",
            r.data["path"].as_str().unwrap_or("the file"),
            r.data["bytes"]
        );
    }
    Ok(r)
}

fn list(ctx: &ToolContext, args: &Value) -> Result<ToolResult, RpcError> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 200);
    let mut params = json!({ "limit": limit });
    if let Some(s) = str_arg(args, "state") {
        params["states"] = json!([s]);
    }
    let v = ctx.backend.call("list", params)?;
    let jobs: Vec<Value> = v["jobs"]
        .as_array()
        .map(|a| a.iter().map(job_view).collect())
        .unwrap_or_default();
    Ok(ToolResult::ok(
        format!("{} download job(s).", jobs.len()),
        json!({ "jobs": jobs }),
    ))
}

fn history(ctx: &ToolContext, args: &Value) -> Result<ToolResult, RpcError> {
    let mut params = json!({
        "limit": args.get("limit").and_then(Value::as_u64).unwrap_or(20).clamp(1, 100),
    });
    for (from, to) in [
        ("query", "search"),
        ("status", "status"),
        ("domain", "domain"),
        ("file_type", "file_type"),
        ("since", "since"),
        ("until", "until"),
        ("sort", "sort"),
    ] {
        if let Some(v) = str_arg(args, from) {
            params[to] = json!(v);
        }
    }
    let v = ctx.backend.call("history.query", params)?;
    let entries: Vec<Value> = v["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| {
                    json!({
                        "url": e["url"],
                        "filename": e["filename"],
                        "dir": e["output_dir"],
                        "status": e["status"],
                        "bytes": e["bytes_total"],
                        "sha256": e["sha256"],
                        "created_at": e["created_at"],
                        "error": e["error"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ToolResult::ok(
        format!(
            "{} of {} matching history entries.",
            entries.len(),
            v["total"]
        ),
        json!({ "entries": entries, "total": v["total"] }),
    ))
}

fn verify(ctx: &ToolContext, args: &Value) -> Result<ToolResult, RpcError> {
    let path =
        str_arg(args, "path").ok_or_else(|| RpcError::invalid_params("'path' is required"))?;
    let path = ctx.policy.resolve_existing(path)?;
    if !path.is_file() {
        return Err(RpcError::invalid_params(format!(
            "{} is not a file",
            path.display()
        )));
    }
    let expected = str_arg(args, "expected").map(str::trim);
    let (algorithm, expected_hex) = match expected {
        Some(spec) if spec.contains(':') => {
            let (alg, hex) = crate::runner::parse_checksum_spec(spec)
                .map_err(|e| RpcError::invalid_params(e.to_string()))?;
            (alg, Some(hex))
        }
        Some(hex) => {
            let alg = match str_arg(args, "algorithm") {
                Some(a) => parse_algorithm(a)?,
                None => ChecksumAlgorithm::from_hex_len(hex).ok_or_else(|| {
                    RpcError::invalid_params("cannot guess the algorithm from the checksum length")
                })?,
            };
            (alg, Some(hex.to_ascii_lowercase()))
        }
        None => (
            parse_algorithm(str_arg(args, "algorithm").unwrap_or("sha256"))?,
            None,
        ),
    };
    let hash =
        compute_checksum(&path, &algorithm).map_err(|e| RpcError::internal(e.to_string()))?;
    let matches = expected_hex.as_ref().map(|e| e.eq_ignore_ascii_case(&hash));
    let summary = match matches {
        Some(true) => format!("Checksum OK ({}).", algorithm.name()),
        Some(false) => format!("Checksum MISMATCH ({}): got {hash}.", algorithm.name()),
        None => format!("{} = {hash}", algorithm.name()),
    };
    Ok(ToolResult {
        is_error: matches == Some(false),
        summary,
        data: json!({
            "path": path,
            "algorithm": algorithm.name(),
            "hash": hash,
            "expected": expected_hex,
            "matches": matches,
        }),
    })
}

fn parse_algorithm(name: &str) -> Result<ChecksumAlgorithm, RpcError> {
    ChecksumAlgorithm::parse(name)
        .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, format!("unknown algorithm '{name}'")))
}

fn config(ctx: &ToolContext) -> Result<ToolResult, RpcError> {
    let mut v = ctx.backend.call("config.get", Value::Null)?;
    v["backend"] = json!(ctx.backend.mode());
    v["allowed_root"] = json!(ctx.policy.root());
    Ok(ToolResult::ok(
        format!(
            "Downloads are restricted to {} ({} backend).",
            ctx.policy.root().display(),
            ctx.backend.mode()
        ),
        v,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_are_valid_schemas() {
        let defs = definitions();
        let mut names = std::collections::HashSet::new();
        for d in &defs {
            let name = d["name"].as_str().unwrap();
            assert!(names.insert(name), "duplicate tool {name}");
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{name}"
            );
            assert!(d["description"].as_str().unwrap().len() > 20);
            let schema = &d["inputSchema"];
            assert_eq!(schema["type"], "object", "{name}");
            assert!(schema["properties"].is_object(), "{name}");
            for req in schema["required"].as_array().into_iter().flatten() {
                assert!(
                    schema["properties"].get(req.as_str().unwrap()).is_some(),
                    "{name}: required {req} not in properties"
                );
            }
            for (prop, s) in schema["properties"].as_object().unwrap() {
                assert!(s["type"].is_string(), "{name}.{prop} has no type");
                assert!(
                    s["description"].is_string(),
                    "{name}.{prop} has no description"
                );
            }
        }
        assert_eq!(defs.len(), 9);
    }
}
