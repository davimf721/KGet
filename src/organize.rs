//! Glue between [`run_job`](crate::runner::run_job) and the smart rules /
//! duplicate detection: kept out of `runner.rs` so the dispatcher stays small.

use crate::checksum::{ChecksumAlgorithm, compute_checksum};
use crate::dedupe::{self, DuplicatePolicy};
use crate::events::JsonEvent;
use crate::protocol::Protocol;
use crate::runner::{DownloadJob, Reporter, RunContext};
use crate::{OverwritePolicy, utils};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// `dir` with a trailing separator, so it is used as a directory.
fn as_dir(dir: &Path) -> String {
    let s = dir.to_string_lossy();
    if s.ends_with('/') || s.ends_with(std::path::MAIN_SEPARATOR) {
        s.into_owned()
    } else {
        format!("{s}{}", std::path::MAIN_SEPARATOR)
    }
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Whether the smart rules apply to `job` (`--organize`/`--no-organize`,
/// else `config.rules_enabled`).
pub fn organizing(job: &DownloadJob, config: &crate::Config) -> bool {
    job.organize.unwrap_or(config.rules_enabled)
}

/// Apply the smart rules to `job` — destination (unless `output` is set),
/// extraction, checksum requirement, connections, speed limit — and return
/// the tags to record (`--tag` plus the rule's).
pub(crate) fn apply_rules(
    job: &mut DownloadJob,
    ctx: &mut RunContext,
    reporter: &Reporter,
    proto: Protocol,
) -> Result<Vec<String>, BoxError> {
    let mut tags = crate::rules::normalise_tags(job.tags.iter().map(String::as_str))?;
    // Ollama models and the hub cache must land where those tools look for them.
    if proto == Protocol::Ollama || (proto == Protocol::HuggingFace && job.hf_cache) {
        return Ok(tags);
    }
    let base_dir = job.base_dir.as_deref().map(utils::expand_tilde);
    let fallback = |job: &mut DownloadJob| {
        if job.output.is_none()
            && let Some(b) = &base_dir
        {
            job.output = Some(as_dir(Path::new(b)));
        }
    };
    if !organizing(job, &ctx.config) {
        fallback(job);
        return Ok(tags);
    }
    let rules = crate::rules::active_rules(&ctx.config);
    let (url, headers, proxy) = (
        job.url.clone(),
        job.headers.clone(),
        ctx.config.proxy.clone(),
    );
    let plan = crate::rules::plan(&rules, &url, proto, &mut || {
        (proto == Protocol::Http)
            .then(|| crate::rules::probe_http(&url, &headers, &proxy))
            .flatten()
    })?;
    let Some(name) = plan.rule else {
        fallback(job);
        return Ok(tags);
    };
    let action = plan.action;
    if action.require_checksum
        && job.sha256.is_none()
        && job.checksum.is_none()
        && job.checksum_url.is_none()
    {
        return Err(format!(
            "Rule '{name}' requires a checksum: pass --sha256, --checksum or --checksum-url"
        )
        .into());
    }
    for t in &action.tags {
        if !tags.contains(t) {
            tags.push(t.clone());
        }
    }
    if action.extract == Some(true) {
        job.extract = true;
    }
    if let Some(n) = action.connections {
        ctx.config.optimization.max_connections = n;
    }
    if let Some(limit) = action.speed_limit {
        let current = ctx.config.optimization.speed_limit;
        ctx.config.optimization.speed_limit = Some(current.map_or(limit, |c| c.min(limit)));
    }
    let mut output = String::new();
    match (&job.output, &plan.folder) {
        // An explicit output always wins over the rule's folder.
        (None, Some(folder)) => {
            let root = base_dir
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| crate::rules::organize_root(&ctx.config));
            let dir = root.join(folder);
            std::fs::create_dir_all(&dir)?;
            output = as_dir(&dir);
            job.output = Some(output.clone());
        }
        _ => fallback(job),
    }
    if !reporter.quiet && reporter.events.is_none() && !output.is_empty() {
        println!("Rule '{name}' → {output}");
    }
    reporter.emit(&JsonEvent::Rule {
        name,
        output,
        tags: tags.clone(),
    });
    Ok(tags)
}

/// Per-URL folder for batch downloads without `-O` when organizing (URL
/// facts only, no network): `output_dir/<rule folder>`.
pub(crate) fn batch_dir(
    url: &str,
    template: &DownloadJob,
    config: &crate::Config,
    output_dir: &Path,
) -> PathBuf {
    let mut job = template.clone();
    job.url = url.to_string();
    if template.output.is_some() || !organizing(&job, config) {
        return output_dir.to_path_buf();
    }
    let rules = crate::rules::active_rules(config);
    match crate::rules::plan(&rules, url, job.protocol(), &mut || None) {
        Ok(crate::rules::Plan {
            folder: Some(f), ..
        }) => output_dir.join(f),
        _ => output_dir.to_path_buf(),
    }
}

/// A download replaced by duplicate handling.
pub(crate) enum Preempted {
    /// Kept the existing file (its path)
    Skipped(PathBuf),
    /// Linked/copied the existing file to the destination (its SHA-256, if known)
    Linked(PathBuf, Option<String>),
}

/// Serializes duplicate prompts (batch downloads run in parallel).
static PROMPT_LOCK: Mutex<()> = Mutex::new(());

/// Look for an earlier completed download of the same URL and apply the
/// duplicate policy.
pub(crate) fn check_duplicate(
    job: &DownloadJob,
    ctx: &RunContext,
    reporter: &Reporter,
    proto: Protocol,
) -> Result<Option<Preempted>, BoxError> {
    if job.resume
        || !matches!(
            proto,
            Protocol::Http | Protocol::Ftp | Protocol::Sftp | Protocol::WebDav
        )
    {
        return Ok(None);
    }
    let interactive = ctx.credential_prompts
        && reporter.events.is_none()
        && std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal();
    let policy = match job.duplicates {
        Some(DuplicatePolicy::Ask) if !interactive => DuplicatePolicy::Download,
        Some(p) => p,
        None if interactive => DuplicatePolicy::Ask,
        None => DuplicatePolicy::Download,
    };
    // Scripts keep the old behaviour without even reading the history.
    if policy == DuplicatePolicy::Download {
        return Ok(None);
    }
    let Some(dup) = dedupe::find_previous(&job.url) else {
        return Ok(None);
    };
    let dest = absolute(Path::new(&utils::resolve_output_path(
        job.output.clone(),
        &job.url,
        "download",
    )));
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let same_place = canon(&dest) == canon(&dup.path);
    let decision = if policy == DuplicatePolicy::Ask {
        ask_duplicate(&dup, same_place)
    } else {
        policy
    };
    let emit = |action: &str| {
        reporter.emit(&JsonEvent::Duplicate {
            kind: "url".into(),
            action: action.into(),
            path: dup.path.to_string_lossy().into_owned(),
            existing_id: Some(dup.entry.id.clone()),
            sha256: dup.entry.sha256.clone(),
        })
    };
    let say = |msg: String| {
        if !reporter.quiet && reporter.events.is_none() {
            println!("{msg}");
        }
    };
    match decision {
        DuplicatePolicy::Link
            if !same_place && !(dest.exists() && job.overwrite == OverwritePolicy::Skip) =>
        {
            let hard = dedupe::link_or_copy(&dup.path, &dest)?;
            emit("link");
            say(format!(
                "Already downloaded: {} {} {}",
                dup.path.display(),
                if hard { "hard-linked to" } else { "copied to" },
                dest.display()
            ));
            Ok(Some(Preempted::Linked(dest, dup.entry.sha256.clone())))
        }
        DuplicatePolicy::Skip | DuplicatePolicy::Link => {
            emit("skip");
            say(format!(
                "Already downloaded: {} (use --duplicates download to fetch it again)",
                dup.path.display()
            ));
            Ok(Some(Preempted::Skipped(dup.path.clone())))
        }
        DuplicatePolicy::Download | DuplicatePolicy::Ask => {
            emit("download");
            Ok(None)
        }
    }
}

/// Ask on the terminal what to do with an already-downloaded URL (default: skip).
fn ask_duplicate(dup: &dedupe::Duplicate, same_place: bool) -> DuplicatePolicy {
    use std::io::{BufRead, Write};
    let _guard = PROMPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let size = dup
        .entry
        .bytes_total
        .map(|b| humansize::format_size(b, humansize::BINARY))
        .unwrap_or_default();
    eprint!(
        "{} was already downloaded on {}: {} ({size}).\n[s]kip, [d]ownload again{}? [S/d{}] ",
        dup.entry.filename,
        dup.entry.created_at_display(),
        dup.path.display(),
        if same_place { "" } else { ", [l]ink it here" },
        if same_place { "" } else { "/l" },
    );
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    match line.trim().to_ascii_lowercase().as_str() {
        "d" | "download" => DuplicatePolicy::Download,
        "l" | "link" if !same_place => DuplicatePolicy::Link,
        _ => DuplicatePolicy::Skip,
    }
}

/// After a download: find identical content in the history (by SHA-256) and,
/// with `--dedupe`, replace the new file by a hard link. Returns the file's
/// SHA-256 when known (verified, or computed for `--dedupe`).
pub(crate) fn dedupe_after(
    job: &DownloadJob,
    reporter: &Reporter,
    path: &Path,
    verified_sha256: Option<String>,
) -> Option<String> {
    if !path.is_file() {
        return verified_sha256;
    }
    let sha = verified_sha256.or_else(|| {
        job.dedupe
            .then(|| compute_checksum(path, &ChecksumAlgorithm::Sha256).ok())
            .flatten()
    })?;
    if let Some(dup) = dedupe::find_same_content(&sha, path) {
        let action = if job.dedupe {
            match dedupe::replace_with_hardlink(&dup.path, path) {
                Ok(()) => "linked",
                Err(e) => {
                    eprintln!(
                        "Warning: --dedupe: cannot hard-link {}: {e}",
                        path.display()
                    );
                    "kept"
                }
            }
        } else {
            "kept"
        };
        if !reporter.quiet && reporter.events.is_none() {
            if action == "linked" {
                println!(
                    "Identical to {}: replaced with a hard link.",
                    dup.path.display()
                );
            } else {
                println!(
                    "Identical to {} (use --dedupe to replace it with a hard link).",
                    dup.path.display()
                );
            }
        }
        reporter.emit(&JsonEvent::Duplicate {
            kind: "content".into(),
            action: action.into(),
            path: dup.path.to_string_lossy().into_owned(),
            existing_id: Some(dup.entry.id),
            sha256: Some(sha.clone()),
        });
    }
    Some(sha)
}
