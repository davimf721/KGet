//! `kget --rules suggest`: propose rules from the local download history.
//!
//! The default is plain heuristics that run entirely on this machine:
//! domains you download from often (and the folder you usually save them
//! to) and extensions that no built-in category covers.
//!
//! With `--ai` and `ANTHROPIC_API_KEY` set, KGet can also ask Claude. Only
//! aggregated counts are sent ([`UsageStats`]: domains, extensions and
//! categories with how often they occur — never URLs, file names or paths);
//! they are printed first and sending needs a "yes" on a terminal. The answer
//! is validated with [`Rule::validate`] like any user rule. Suggestions are
//! never applied automatically: `--apply` or a confirmation is required.

use super::{CommandOptions, Rule, RuleAction, RuleMatch, add_rule};
use crate::queue::{DownloadHistory, EntryStatus, HistoryEntry, HistoryQuery};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Model used for `--ai` suggestions.
pub const AI_MODEL: &str = "claude-haiku-5-5";

/// Default Claude API base URL (`KGET_ANTHROPIC_BASE_URL` overrides it).
pub const DEFAULT_API_BASE: &str = "https://api.anthropic.com";

/// Downloads needed before a domain or extension is worth a rule.
const MIN_COUNT: usize = 3;

/// Maximum number of suggestions of each kind.
const MAX_SUGGESTIONS: usize = 8;

/// A proposed rule and why it was proposed.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// The rule (valid, not yet added)
    pub rule: Rule,
    /// Human-readable reason
    pub reason: String,
}

/// Heuristic suggestions from completed history `entries`.
///
/// Domains and extensions already handled by one of `existing` rules are
/// skipped; `root` is the organize root used to turn the folders downloads
/// were saved to into relative rule folders.
pub fn heuristic(entries: &[HistoryEntry], existing: &[Rule], root: &Path) -> Vec<Suggestion> {
    let done: Vec<&HistoryEntry> = entries
        .iter()
        .filter(|e| e.status == EntryStatus::Completed)
        .collect();
    let covered_domain = |d: &str| {
        existing
            .iter()
            .any(|r| r.when.domains.iter().any(|x| super::domain_matches(d, x)))
    };
    let covered_ext = |x: &str| {
        existing
            .iter()
            .any(|r| r.when.extensions.iter().any(|e| e.eq_ignore_ascii_case(x)))
    };
    let root_abs = absolute(root);

    let mut out = Vec::new();

    // Domains: where their files usually go.
    let mut by_domain: BTreeMap<String, Vec<&HistoryEntry>> = BTreeMap::new();
    for e in &done {
        if let Some(d) = e.domain() {
            by_domain.entry(d).or_default().push(e);
        }
    }
    let mut domains: Vec<(String, Vec<&HistoryEntry>)> = by_domain
        .into_iter()
        .filter(|(d, v)| v.len() >= MIN_COUNT && !covered_domain(d))
        .collect();
    domains.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (domain, list) in domains.into_iter().take(MAX_SUGGESTIONS) {
        let mut dirs: HashMap<&str, usize> = HashMap::new();
        for e in &list {
            *dirs.entry(e.output_dir.as_str()).or_default() += 1;
        }
        let (top_dir, top_n) = dirs
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
            .unwrap_or(("", 0));
        // A folder under the root that most of these downloads went to.
        let usual = absolute(Path::new(top_dir))
            .strip_prefix(&root_abs)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .filter(|p| !p.is_empty() && !p.contains(['{', '}']))
            .filter(|_| top_n * 10 >= list.len() * 6);
        let (folder, reason) = match usual {
            Some(f) => (
                f.clone(),
                format!(
                    "{top_n} of {} downloads from {domain} were saved to {f}",
                    list.len()
                ),
            ),
            None => (
                format!("Sites/{}", crate::utils::sanitize_filename(&domain)),
                format!("{} downloads from {domain}", list.len()),
            ),
        };
        push_valid(
            &mut out,
            Rule {
                name: format!("site-{domain}"),
                when: RuleMatch {
                    domains: vec![domain.clone()],
                    ..Default::default()
                },
                action: RuleAction {
                    folder: Some(folder),
                    ..Default::default()
                },
                ..Default::default()
            },
            reason,
        );
    }

    // Extensions no category knows about.
    let mut exts: BTreeMap<String, usize> = BTreeMap::new();
    for e in &done {
        if let Some(x) = e.extension() {
            *exts.entry(x).or_default() += 1;
        }
    }
    let mut exts: Vec<(String, usize)> = exts
        .into_iter()
        .filter(|(x, n)| {
            *n >= MIN_COUNT
                && !covered_ext(x)
                && x.chars().all(|c| c.is_ascii_alphanumeric())
                && super::CATEGORIES.iter().all(|c| {
                    !crate::queue::file_type_extensions(c).is_some_and(|l| l.contains(&x.as_str()))
                })
        })
        .collect();
    exts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (ext, n) in exts.into_iter().take(MAX_SUGGESTIONS) {
        push_valid(
            &mut out,
            Rule {
                name: format!("ext-{ext}"),
                when: RuleMatch {
                    extensions: vec![ext.clone()],
                    ..Default::default()
                },
                action: RuleAction {
                    folder: Some(ext.to_uppercase()),
                    tags: vec![ext.clone()],
                    ..Default::default()
                },
                ..Default::default()
            },
            format!("{n} .{ext} downloads that no built-in category covers"),
        );
    }
    out
}

fn push_valid(out: &mut Vec<Suggestion>, rule: Rule, reason: String) {
    if rule.validate().is_ok() {
        out.push(Suggestion { rule, reason });
    }
}

fn absolute(p: &Path) -> std::path::PathBuf {
    let p = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    // Resolve `.`/`..` lexically (the folders may no longer exist).
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// AI-assisted suggestions (opt-in)
// ============================================================================

/// Aggregated, anonymized usage counts: the only data `--ai` sends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UsageStats {
    /// Number of completed downloads considered
    pub downloads: usize,
    /// Most frequent domains and how often they occur
    pub domains: Vec<(String, usize)>,
    /// Most frequent file extensions and how often they occur
    pub extensions: Vec<(String, usize)>,
    /// File categories and how often they occur
    pub categories: Vec<(String, usize)>,
}

impl UsageStats {
    /// Count domains, extensions and categories of completed `entries`
    /// (at most 25 of each, most frequent first).
    pub fn from_history(entries: &[HistoryEntry]) -> Self {
        let mut domains: HashMap<String, usize> = HashMap::new();
        let mut exts: HashMap<String, usize> = HashMap::new();
        let mut cats: HashMap<String, usize> = HashMap::new();
        let mut n = 0;
        for e in entries
            .iter()
            .filter(|e| e.status == EntryStatus::Completed)
        {
            n += 1;
            if let Some(d) = e.domain() {
                *domains.entry(d).or_default() += 1;
            }
            if let Some(x) = e.extension().filter(|x| x.len() <= 10) {
                *exts.entry(x).or_default() += 1;
            }
            let f = super::Facts::from_url(&e.url, crate::protocol::Protocol::Http);
            let f = super::Facts {
                filename: e.filename.clone(),
                ..f
            };
            if let Some(c) = f.category() {
                *cats.entry(c.to_string()).or_default() += 1;
            }
        }
        let top = |m: HashMap<String, usize>| {
            let mut v: Vec<(String, usize)> = m.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            v.truncate(25);
            v
        };
        Self {
            downloads: n,
            domains: top(domains),
            extensions: top(exts),
            categories: top(cats),
        }
    }
}

const AI_PROMPT: &str = "You help organize a download manager. Below are aggregated \
counts from the user's download history (no URLs or paths). Propose at most 8 rules \
that sort downloads into folders. Reply with ONLY a JSON array; each element is \
{\"name\": string, \"match\": {\"domains\"?: [string], \"extensions\"?: [string], \
\"categories\"?: [\"video\"|\"audio\"|\"image\"|\"archive\"|\"document\"|\"disk\"|\"package\"]}, \
\"action\": {\"folder\": string, \"tags\"?: [string]}}. Folders are relative paths \
without '..'; they may use the placeholders {category}, {domain}, {ext} and {date:%Y-%m}. \
Do not repeat what plain category folders (Videos, Music, Images, Archives, Documents, \
Disk Images, Software) already do.\n\nStatistics:\n";

/// Ask Claude for rules based on `stats` and return the valid ones (plus how
/// many were rejected by the validator).
///
/// `base_url` is normally [`DEFAULT_API_BASE`]; only `stats` is sent.
pub fn ai_suggest(
    stats: &UsageStats,
    api_key: &str,
    base_url: &str,
    proxy: &crate::ProxyConfig,
) -> Result<(Vec<Suggestion>, usize), BoxError> {
    let client =
        crate::http::build_client(proxy, &crate::http::ClientOptions::with_timeout_secs(120))?;
    let body = serde_json::json!({
        "model": AI_MODEL,
        "max_tokens": 4096,
        "output_config": { "effort": "low" },
        "messages": [{
            "role": "user",
            "content": format!("{AI_PROMPT}{}", serde_json::to_string_pretty(stats)?),
        }],
    });
    let resp = client
        .post(format!("{}/v1/messages", base_url.trim_end_matches('/')))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        let msg = json["error"]["message"]
            .as_str()
            .unwrap_or("request failed");
        return Err(format!("Claude API error (HTTP {status}): {msg}").into());
    }
    if json["stop_reason"] == "refusal" {
        return Err("Claude declined to suggest rules".into());
    }
    let text: String = json["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect()
        })
        .unwrap_or_default();
    Ok(parse_ai_rules(&text))
}

/// Extract the JSON array of rules from a model reply and keep only rules
/// that pass validation (named `ai-…`). Returns them and the rejected count.
pub fn parse_ai_rules(text: &str) -> (Vec<Suggestion>, usize) {
    let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) else {
        return (Vec::new(), 0);
    };
    let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&text[start..=end.max(start)])
    else {
        return (Vec::new(), 0);
    };
    let mut out: Vec<Suggestion> = Vec::new();
    let mut rejected = 0;
    for item in items {
        let Ok(mut rule) = serde_json::from_value::<Rule>(item) else {
            rejected += 1;
            continue;
        };
        let base = crate::utils::sanitize_filename(rule.name.trim().trim_start_matches("builtin:"));
        rule.name = if base.starts_with("ai-") {
            base
        } else {
            format!("ai-{base}")
        };
        rule.enabled = true;
        // Only folders and tags are accepted from the model.
        rule.action = RuleAction {
            folder: rule.action.folder,
            tags: super::normalise_tags(rule.action.tags.iter().map(String::as_str))
                .unwrap_or_default(),
            ..Default::default()
        };
        if rule.validate().is_err() || out.iter().any(|s| s.rule.name == rule.name) {
            rejected += 1;
            continue;
        }
        out.push(Suggestion {
            rule,
            reason: "suggested by Claude".into(),
        });
    }
    (out, rejected)
}

fn tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn confirm(question: &str) -> bool {
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok()
        && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// `kget --rules suggest [--ai] [--apply]`.
pub fn command(config: &mut crate::Config, opts: CommandOptions) -> Result<(), BoxError> {
    let entries = DownloadHistory::query(
        &HistoryQuery::new()
            .status(EntryStatus::Completed)
            .limit(5_000),
    )?;
    let mut suggestions = heuristic(&entries, &config.rules, &super::organize_root(config));

    if opts.ai {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .ok_or("--ai needs the ANTHROPIC_API_KEY environment variable")?;
        if !tty() {
            return Err(
                "--ai asks for confirmation before sending anything; run it in a terminal".into(),
            );
        }
        let stats = UsageStats::from_history(&entries);
        let base = std::env::var("KGET_ANTHROPIC_BASE_URL")
            .ok()
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_API_BASE.into());
        println!("Only these aggregated counts will be sent to {base} (model {AI_MODEL}):");
        println!("{}", serde_json::to_string_pretty(&stats)?);
        if confirm("Send them?") {
            let (ai, rejected) = ai_suggest(&stats, &key, &base, &config.proxy)?;
            if rejected > 0 {
                println!("Ignored {rejected} invalid rule(s) from Claude.");
            }
            for s in ai {
                if !suggestions.iter().any(|x| x.rule.name == s.rule.name) {
                    suggestions.push(s);
                }
            }
        } else {
            println!("Nothing was sent.");
        }
    }

    suggestions.retain(|s| {
        !config
            .rules
            .iter()
            .any(|r| r.name.eq_ignore_ascii_case(&s.rule.name))
    });
    if suggestions.is_empty() {
        println!(
            "No suggestions yet (they need a few completed downloads from the same site or of the same type)."
        );
        return Ok(());
    }
    println!("Suggested rules:");
    for (i, s) in suggestions.iter().enumerate() {
        println!(
            "{:>3}  {:<24} {}  {}\n       {}\n       {}",
            format!("#{}", i + 1),
            s.rule.name,
            s.rule.describe_match(),
            s.rule.describe_action(),
            s.reason,
            serde_json::to_string(&s.rule)?
        );
    }
    let apply =
        opts.apply || (tty() && confirm(&format!("Add these {} rule(s)?", suggestions.len())));
    if !apply {
        println!(
            "Nothing changed. Re-run with --apply to add them, or add some with `kget --rules add`."
        );
        return Ok(());
    }
    let mut added = 0;
    for s in suggestions {
        if add_rule(config, s.rule, None).is_ok() {
            added += 1;
        }
    }
    crate::cli::save_config(config)?;
    println!("Added {added} rule(s).");
    if !config.rules_enabled {
        println!("Organizing is off: use --organize, or `kget --rules enable`.");
    }
    Ok(())
}
