//! Smart rules: auto-organize downloads into folders, tag them and tune
//! per-download settings.
//!
//! Rules live in `config.json` (`"rules": [...]`) next to the rest of the
//! configuration, so they are written atomically together with it, follow
//! `KGET_CONFIG_DIR`, and every frontend (CLI, REPL, GUI, macOS app) reads a
//! single file. They are applied only when organizing is enabled
//! (`config.rules_enabled`, or `--organize` for one run). The rules are tried
//! **in order** and the **first match wins**; the built-in rules (Videos,
//! Music, Images, …) come after the user's rules unless
//! `config.builtin_rules` is `false`.
//!
//! Within a rule every condition that is set must match (AND); a list matches
//! any of its values (OR). An explicit output path (`-O`) always takes
//! precedence over a rule's folder.
//!
//! ```json
//! {
//!   "name": "Linux ISOs",
//!   "match": { "domains": ["ubuntu.com", "debian.org"], "extensions": ["iso"] },
//!   "action": {
//!     "folder": "ISOs/{domain}/{date:%Y-%m}",
//!     "tags": ["linux"],
//!     "require_checksum": true,
//!     "connections": 8
//!   }
//! }
//! ```
//!
//! Folder templates may use `{category}`, `{domain}`, `{ext}`, `{protocol}`
//! and `{date:<strftime>}`. Substituted values are sanitized into single path
//! components, and `..`, absolute paths and drive prefixes are rejected, so a
//! rendered folder can never escape the download root.
//!
//! # Example
//!
//! ```rust
//! use kget::protocol::Protocol;
//! use kget::rules::{self, Facts};
//!
//! let rules = rules::builtin_rules();
//! let mut facts = Facts::from_url("https://cdn.example.com/talk.mp4", Protocol::Http);
//! let rule = rules::first_match(&rules, &mut facts, &mut || None).unwrap();
//! assert_eq!(rule.action.folder.as_deref(), Some("Videos"));
//! ```

pub mod suggest;

use crate::protocol::Protocol;
use crate::queue::file_type_extensions;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// File type categories understood by the `categories` condition and the
/// `{category}` placeholder (same names as `kget --history --type`).
pub const CATEGORIES: &[&str] = &[
    "video", "audio", "image", "archive", "document", "disk", "package",
];

// ============================================================================
// Rule format
// ============================================================================

/// One smart rule: conditions (`match`) and what to do (`action`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    /// Unique, human-readable name (`builtin:` is reserved)
    pub name: String,
    /// Disabled rules are skipped
    pub enabled: bool,
    /// Conditions; all that are set must match
    #[serde(rename = "match")]
    pub when: RuleMatch,
    /// Effects of the rule when it matches
    pub action: RuleAction,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            when: RuleMatch::default(),
            action: RuleAction::default(),
        }
    }
}

/// Conditions of a [`Rule`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleMatch {
    /// Host names; each also matches its subdomains (`example.com` matches
    /// `dl.example.com`). A leading `www.` is ignored.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    /// Regular expression searched in the whole URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_regex: Option<String>,
    /// Glob matched against the whole URL (`*` any text, `?` one character;
    /// case-insensitive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_glob: Option<String>,
    /// File extensions without the dot (`iso`, `tar.gz`)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    /// File categories (see [`CATEGORIES`]); from the extension, or the
    /// response `Content-Type` when the URL has none
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// MIME types from the response `Content-Type` (`video/*`,
    /// `application/pdf`); needs the server to report it
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mime: Vec<String>,
    /// Minimum size in bytes (needs the server to report the size)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<u64>,
    /// Maximum size in bytes (needs the server to report the size)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u64>,
    /// Protocols (`http`, `https`, `ftp`, `sftp`, `webdav`, `torrent`, `video`…)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub protocols: Vec<String>,
}

/// Effects of a matching [`Rule`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleAction {
    /// Destination folder template, relative to the download root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Tags recorded in the history
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Extract the archive after the download
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract: Option<bool>,
    /// Refuse to download without a checksum (`--sha256`, `--checksum` or
    /// `--checksum-url`)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub require_checksum: bool,
    /// Parallel connections for this download (1–32)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections: Option<usize>,
    /// Speed limit in bytes/second; the stricter of this and the configured
    /// limit applies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_limit: Option<u64>,
}

impl Rule {
    /// Whether the rule is one of [`builtin_rules`].
    pub fn is_builtin(&self) -> bool {
        self.name.starts_with("builtin:")
    }

    /// Check names, patterns, categories, sizes, tags and the folder template.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("rule needs a name".into());
        }
        if name.starts_with('#') || name.chars().any(char::is_control) {
            return Err(format!("invalid rule name '{name}'"));
        }
        let m = &self.when;
        if let Some(re) = &m.url_regex {
            regex::Regex::new(re).map_err(|e| format!("rule '{name}': invalid url_regex: {e}"))?;
        }
        if let Some(g) = &m.url_glob {
            glob_regex(g).map_err(|e| format!("rule '{name}': invalid url_glob: {e}"))?;
        }
        for c in &m.categories {
            if category_key(c).is_none() {
                return Err(format!(
                    "rule '{name}': unknown category '{c}' (expected {})",
                    CATEGORIES.join(", ")
                ));
            }
        }
        if let (Some(lo), Some(hi)) = (m.min_size, m.max_size)
            && lo > hi
        {
            return Err(format!("rule '{name}': min_size is larger than max_size"));
        }
        for d in &m.domains {
            if normalise_domain(d).is_empty() {
                return Err(format!("rule '{name}': empty domain"));
            }
        }
        let a = &self.action;
        if let Some(n) = a.connections
            && !(1..=32).contains(&n)
        {
            return Err(format!("rule '{name}': connections must be 1–32"));
        }
        for t in &a.tags {
            normalise_tag(t).ok_or_else(|| format!("rule '{name}': invalid tag '{t}'"))?;
        }
        if let Some(folder) = &a.folder {
            validate_template(folder).map_err(|e| format!("rule '{name}': {e}"))?;
        }
        Ok(())
    }

    /// Conditions that need only the URL.
    fn matches_url(&self, f: &Facts) -> bool {
        let m = &self.when;
        if !m.domains.is_empty() {
            let Some(host) = f.domain.as_deref() else {
                return false;
            };
            if !m.domains.iter().any(|d| domain_matches(host, d)) {
                return false;
            }
        }
        if let Some(re) = &m.url_regex
            && !regex::Regex::new(re).is_ok_and(|r| r.is_match(&f.url))
        {
            return false;
        }
        if let Some(g) = &m.url_glob
            && !glob_regex(g).is_ok_and(|r| r.is_match(&f.url))
        {
            return false;
        }
        if !m.extensions.is_empty() {
            let name = f.filename.to_ascii_lowercase();
            if !m.extensions.iter().any(|e| {
                let e = e.trim().trim_start_matches('.').to_ascii_lowercase();
                !e.is_empty() && name.ends_with(&format!(".{e}"))
            }) {
                return false;
            }
        }
        if !m.protocols.is_empty() {
            let scheme = f.url.split("://").next().unwrap_or("").to_ascii_lowercase();
            if !m.protocols.iter().any(|p| {
                let p = p.trim().to_ascii_lowercase();
                p == f.protocol.as_str() || (f.url.contains("://") && p == scheme)
            }) {
                return false;
            }
        }
        true
    }

    /// Whether the remaining conditions need the server's answer.
    fn needs_remote(&self, f: &Facts) -> bool {
        let m = &self.when;
        !m.mime.is_empty()
            || m.min_size.is_some()
            || m.max_size.is_some()
            || (!m.categories.is_empty() && f.extension_category().is_none())
    }

    /// Conditions that may need the server's answer (unknown = no match).
    fn matches_remote(&self, f: &Facts) -> bool {
        let m = &self.when;
        if !m.categories.is_empty() {
            let Some(cat) = f.category() else {
                return false;
            };
            if !m.categories.iter().any(|c| category_key(c) == Some(cat)) {
                return false;
            }
        }
        if !m.mime.is_empty() {
            let Some(mime) = f.mime.as_deref() else {
                return false;
            };
            if !m.mime.iter().any(|p| mime_matches(mime, p)) {
                return false;
            }
        }
        if m.min_size.is_some() || m.max_size.is_some() {
            let Some(size) = f.size else {
                return false;
            };
            if m.min_size.is_some_and(|lo| size < lo) || m.max_size.is_some_and(|hi| size > hi) {
                return false;
            }
        }
        true
    }

    /// One-line summary of the conditions (`kget --rules list`).
    pub fn describe_match(&self) -> String {
        let m = &self.when;
        let mut parts = Vec::new();
        let list = |k: &str, v: &[String]| format!("{k}={}", v.join(","));
        if !m.domains.is_empty() {
            parts.push(list("domain", &m.domains));
        }
        if let Some(r) = &m.url_regex {
            parts.push(format!("regex={r}"));
        }
        if let Some(g) = &m.url_glob {
            parts.push(format!("glob={g}"));
        }
        if !m.extensions.is_empty() {
            parts.push(list("ext", &m.extensions));
        }
        if !m.categories.is_empty() {
            parts.push(list("category", &m.categories));
        }
        if !m.mime.is_empty() {
            parts.push(list("mime", &m.mime));
        }
        if let Some(n) = m.min_size {
            parts.push(format!("min-size={n}"));
        }
        if let Some(n) = m.max_size {
            parts.push(format!("max-size={n}"));
        }
        if !m.protocols.is_empty() {
            parts.push(list("protocol", &m.protocols));
        }
        if parts.is_empty() {
            "(any download)".into()
        } else {
            parts.join(" ")
        }
    }

    /// One-line summary of the action (`kget --rules list`).
    pub fn describe_action(&self) -> String {
        let a = &self.action;
        let mut parts = Vec::new();
        if let Some(f) = &a.folder {
            parts.push(format!("→ {f}"));
        }
        if !a.tags.is_empty() {
            parts.push(format!("tags={}", a.tags.join(",")));
        }
        if a.extract == Some(true) {
            parts.push("extract".into());
        }
        if a.require_checksum {
            parts.push("checksum-required".into());
        }
        if let Some(n) = a.connections {
            parts.push(format!("connections={n}"));
        }
        if let Some(n) = a.speed_limit {
            parts.push(format!("limit={n}"));
        }
        parts.join(" ")
    }
}

/// The built-in organizing rules: Videos, Music, Images, Archives,
/// Documents, Disk Images and Software folders under the download root.
pub fn builtin_rules() -> Vec<Rule> {
    [
        ("videos", "video", "Videos"),
        ("music", "audio", "Music"),
        ("images", "image", "Images"),
        ("archives", "archive", "Archives"),
        ("documents", "document", "Documents"),
        ("disk-images", "disk", "Disk Images"),
        ("software", "package", "Software"),
    ]
    .into_iter()
    .map(|(name, category, folder)| Rule {
        name: format!("builtin:{name}"),
        enabled: true,
        when: RuleMatch {
            categories: vec![category.into()],
            ..Default::default()
        },
        action: RuleAction {
            folder: Some(folder.into()),
            ..Default::default()
        },
    })
    .collect()
}

/// The rules tried for a download: the user's rules, then (unless disabled)
/// the built-in ones.
pub fn active_rules(config: &crate::Config) -> Vec<Rule> {
    let mut rules = config.rules.clone();
    if config.builtin_rules {
        rules.extend(builtin_rules());
    }
    rules
}

/// Root folder for rule destinations: `config.organize_dir`, or the current
/// directory.
pub fn organize_root(config: &crate::Config) -> PathBuf {
    match config
        .organize_dir
        .as_deref()
        .filter(|d| !d.trim().is_empty())
    {
        Some(d) => PathBuf::from(crate::utils::expand_tilde(d)),
        None => PathBuf::from("."),
    }
}

// ============================================================================
// Matching
// ============================================================================

/// What is known about a download when the rules are evaluated.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    /// The URL (credentials removed)
    pub url: String,
    /// Lowercase host without `www.`
    pub domain: Option<String>,
    /// File name from the URL
    pub filename: String,
    /// Protocol used for the download
    pub protocol: Protocol,
    /// MIME type from the response `Content-Type`, if probed
    pub mime: Option<String>,
    /// Size in bytes reported by the server, if probed
    pub size: Option<u64>,
    /// Whether [`first_match`] already asked the server
    pub probed: bool,
}

/// MIME type and size reported by the server (`HEAD`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteInfo {
    /// `Content-Type` without parameters, lowercase
    pub mime: Option<String>,
    /// `Content-Length`
    pub size: Option<u64>,
}

impl Facts {
    /// Facts derived from the URL alone.
    pub fn from_url(url: &str, protocol: Protocol) -> Self {
        let url = crate::queue::redact_url(url);
        let domain = url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(normalise_domain))
            .filter(|h| !h.is_empty());
        Self {
            filename: crate::utils::get_filename_from_url_or_default(&url, ""),
            url,
            domain,
            protocol,
            mime: None,
            size: None,
            probed: false,
        }
    }

    /// Lowercase extension of the file name, without the dot.
    pub fn extension(&self) -> Option<String> {
        Path::new(&self.filename)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .filter(|e| !e.is_empty())
    }

    /// Category from the file extension.
    pub fn extension_category(&self) -> Option<&'static str> {
        let ext = self.extension()?;
        CATEGORIES
            .iter()
            .copied()
            .find(|c| file_type_extensions(c).is_some_and(|exts| exts.contains(&ext.as_str())))
    }

    /// Category from the extension, else from the MIME type.
    pub fn category(&self) -> Option<&'static str> {
        self.extension_category()
            .or_else(|| self.mime.as_deref().and_then(mime_category))
    }
}

/// The first enabled rule matching `facts`.
///
/// `probe` is called at most once, and only when a rule whose URL conditions
/// match also needs the MIME type or size (or a category the URL alone does
/// not reveal); return `None` when the server cannot be asked. The answer is
/// stored in `facts`.
pub fn first_match<'a>(
    rules: &'a [Rule],
    facts: &mut Facts,
    probe: &mut dyn FnMut() -> Option<RemoteInfo>,
) -> Option<&'a Rule> {
    rules.iter().filter(|r| r.enabled).find(|rule| {
        if !rule.matches_url(facts) {
            return false;
        }
        if rule.needs_remote(facts) && !facts.probed {
            facts.probed = true;
            if let Some(info) = probe() {
                facts.mime = info.mime;
                facts.size = info.size;
            }
        }
        rule.matches_remote(facts)
    })
}

/// Ask an HTTP server for the MIME type and size with a `HEAD` request
/// (short timeout, best-effort).
pub fn probe_http(
    url: &str,
    headers: &[(String, String)],
    proxy: &crate::ProxyConfig,
) -> Option<RemoteInfo> {
    let client =
        crate::http::build_client(proxy, &crate::http::ClientOptions::with_timeout_secs(10))
            .ok()?;
    let resp = crate::http::apply_headers(client.head(url), headers)
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty());
    let size = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    Some(RemoteInfo { mime, size })
}

fn category_key(c: &str) -> Option<&'static str> {
    let exts = file_type_extensions(c)?;
    CATEGORIES
        .iter()
        .copied()
        .find(|k| file_type_extensions(k) == Some(exts))
}

fn mime_category(mime: &str) -> Option<&'static str> {
    let (top, sub) = mime.split_once('/')?;
    Some(match top {
        "video" => "video",
        "audio" => "audio",
        "image" => "image",
        "text" => "document",
        _ => match sub {
            "pdf" | "msword" | "rtf" | "epub+zip" => "document",
            s if s.starts_with("vnd.openxmlformats") || s.starts_with("vnd.oasis") => "document",
            "zip" | "gzip" | "x-gzip" | "x-tar" | "x-bzip2" | "x-xz" | "zstd"
            | "x-7z-compressed" | "vnd.rar" | "x-rar-compressed" => "archive",
            "x-iso9660-image" | "x-apple-diskimage" => "disk",
            "vnd.debian.binary-package"
            | "x-rpm"
            | "x-msdownload"
            | "x-msi"
            | "vnd.android.package-archive" => "package",
            _ => return None,
        },
    })
}

fn mime_matches(mime: &str, pattern: &str) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    match pattern.strip_suffix("/*") {
        Some(top) => mime.split('/').next() == Some(top),
        None => pattern == "*" || pattern == "*/*" || mime == pattern,
    }
}

/// `https://www.Example.com/x` / `Example.COM.` → `example.com`.
fn normalise_domain(s: &str) -> String {
    let s = s.trim();
    let host = url::Url::parse(s)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| s.to_string());
    host.trim_end_matches('.')
        .trim_start_matches("*.")
        .trim_start_matches("www.")
        .to_ascii_lowercase()
}

fn domain_matches(host: &str, rule_domain: &str) -> bool {
    let d = normalise_domain(rule_domain);
    !d.is_empty() && (host == d || host.ends_with(&format!(".{d}")))
}

/// Compile a URL glob (`*`, `?`) into an anchored, case-insensitive regex.
fn glob_regex(glob: &str) -> Result<regex::Regex, regex::Error> {
    let mut re = String::from("(?i)^");
    for c in glob.chars() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    regex::Regex::new(&re)
}

/// Lowercase, trimmed tag; `None` if empty or containing a comma or control
/// character (tags are stored comma-separated).
pub fn normalise_tag(tag: &str) -> Option<String> {
    let t = tag.trim().to_lowercase();
    (!t.is_empty() && t.len() <= 64 && !t.contains(',') && !t.chars().any(char::is_control))
        .then_some(t)
}

/// Normalise, validate and de-duplicate tags (invalid ones are an error).
pub fn normalise_tags<'a>(tags: impl IntoIterator<Item = &'a str>) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let n = normalise_tag(t).ok_or_else(|| format!("invalid tag '{t}'"))?;
        if !out.contains(&n) {
            out.push(n);
        }
    }
    Ok(out)
}

// ============================================================================
// Folder templates
// ============================================================================

/// Render a folder template into a relative path that stays inside the
/// download root.
///
/// ```rust
/// use kget::protocol::Protocol;
/// use kget::rules::{Facts, render_folder};
///
/// let f = Facts::from_url("https://dl.example.com/a/b.iso", Protocol::Http);
/// let p = render_folder("ISOs/{domain}/{ext}", &f).unwrap();
/// assert_eq!(p, std::path::Path::new("ISOs/dl.example.com/iso"));
/// assert!(render_folder("../outside", &f).is_err());
/// assert!(render_folder("/etc", &f).is_err());
/// ```
pub fn render_folder(template: &str, facts: &Facts) -> Result<PathBuf, String> {
    render_with(template, facts, chrono::Local::now())
}

fn render_with(
    template: &str,
    facts: &Facts,
    now: chrono::DateTime<chrono::Local>,
) -> Result<PathBuf, String> {
    let t = template.trim();
    if t.is_empty() {
        return Err("empty folder template".into());
    }
    if t.starts_with(['/', '\\', '~']) || Path::new(t).has_root() {
        return Err(format!(
            "folder '{t}' must be relative to the download root"
        ));
    }
    let mut out = String::new();
    let mut rest = t;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("unclosed '{{' in folder '{t}'"))?;
        let key = &after[..end];
        out.push_str(&placeholder(key, facts, now)?);
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err(format!("unexpected '}}' in folder '{t}'"));
    }
    out.push_str(rest);

    let mut path = PathBuf::new();
    for part in out.split(['/', '\\']).map(str::trim) {
        if part.is_empty() {
            continue;
        }
        if part.contains(':') {
            return Err(format!("folder component '{part}' may not contain ':'"));
        }
        crate::download::validate_filename(part)
            .map_err(|e| format!("folder component '{part}': {e}"))?;
        path.push(part);
    }
    if path.as_os_str().is_empty() {
        return Err(format!("folder '{t}' renders to an empty path"));
    }
    // Belt and braces: only plain components may remain.
    if !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("folder '{t}' escapes the download root"));
    }
    Ok(path)
}

fn placeholder(
    key: &str,
    f: &Facts,
    now: chrono::DateTime<chrono::Local>,
) -> Result<String, String> {
    let raw = match key.trim() {
        "category" => f.category().unwrap_or("other").to_string(),
        "domain" => f.domain.clone().unwrap_or_else(|| "unknown".into()),
        "ext" => f.extension().unwrap_or_else(|| "none".into()),
        "protocol" => f.protocol.as_str().to_string(),
        k => match k.strip_prefix("date:") {
            Some(fmt) => {
                use chrono::format::{Item, StrftimeItems};
                if fmt.is_empty() || StrftimeItems::new(fmt).any(|i| matches!(i, Item::Error)) {
                    return Err(format!("invalid date format '{fmt}'"));
                }
                now.format(fmt).to_string()
            }
            None => {
                return Err(format!(
                    "unknown placeholder '{{{k}}}' (use {{category}}, {{domain}}, {{ext}}, \
                     {{protocol}} or {{date:%Y-%m}})"
                ));
            }
        },
    };
    // A value is always one path component: separators become '_'.
    Ok(crate::utils::sanitize_filename(&raw))
}

/// Check that a folder template is well-formed and cannot escape the root.
pub fn validate_template(template: &str) -> Result<(), String> {
    let sample = Facts::from_url("https://example.com/file.bin", Protocol::Http);
    render_folder(template, &sample).map(|_| ())
}

// ============================================================================
// Planning a download
// ============================================================================

/// The outcome of applying the rules to one download.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Name of the matching rule (`None` = no rule matched)
    pub rule: Option<String>,
    /// Rendered destination folder (relative to the root), if the rule has one
    pub folder: Option<PathBuf>,
    /// The matching rule's action
    pub action: RuleAction,
    /// The facts used (after probing)
    pub facts: Option<Facts>,
}

/// Evaluate `rules` for `url` (probing the server with `probe` if needed).
pub fn plan(
    rules: &[Rule],
    url: &str,
    protocol: Protocol,
    probe: &mut dyn FnMut() -> Option<RemoteInfo>,
) -> Result<Plan, String> {
    let mut facts = Facts::from_url(url, protocol);
    let Some(rule) = first_match(rules, &mut facts, probe) else {
        return Ok(Plan {
            facts: Some(facts),
            ..Default::default()
        });
    };
    let folder = match &rule.action.folder {
        Some(t) => Some(render_folder(t, &facts)?),
        None => None,
    };
    Ok(Plan {
        rule: Some(rule.name.clone()),
        folder,
        action: rule.action.clone(),
        facts: Some(facts),
    })
}

// ============================================================================
// `kget --rules` command
// ============================================================================

/// Usage of `kget --rules` / the interactive `rules` command.
pub const COMMAND_USAGE: &str = "\
rules list                      Rules in order (first match wins), built-ins last
rules test <url>                Which rule matches and where the file would go (no download)
rules add key=value…            Add a rule (or one JSON object), e.g.
                                  rules add name=ISOs ext=iso folder=ISOs/{domain} tag=linux
                                keys: name domain regex glob ext category mime protocol
                                      min-size max-size folder tag extract checksum
                                      connections limit position enabled
rules remove <name|#n>          Delete a rule
rules enable|disable [name|#n]  Organizing on/off (no name), one rule, or 'builtins'
rules suggest [--ai] [--apply]  Propose rules from your download history";

/// Options of `kget --rules` that are separate flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandOptions {
    /// `--ai`: also ask Claude (needs `ANTHROPIC_API_KEY`, a terminal and
    /// confirmation)
    pub ai: bool,
    /// `--apply`: add the suggested rules without asking
    pub apply: bool,
}

/// Run a `kget --rules` command; `config` is saved when changed.
pub fn command(
    args: &[&str],
    config: &mut crate::Config,
    opts: CommandOptions,
) -> Result<(), BoxError> {
    let usage = || -> BoxError { format!("usage:\n{COMMAND_USAGE}").into() };
    match args {
        [] | ["list" | "ls"] => {
            print!("{}", list_text(config));
            Ok(())
        }
        ["test", url] => {
            print!("{}", test_text(config, url, true)?);
            Ok(())
        }
        ["add", spec @ ..] if !spec.is_empty() => {
            let (rule, position) = parse_rule_spec(spec)?;
            let name = rule.name.clone();
            add_rule(config, rule, position)?;
            crate::cli::save_config(config)?;
            println!("Added rule '{name}'.");
            if !config.rules_enabled {
                println!("Organizing is off: use --organize, or `kget --rules enable`.");
            }
            Ok(())
        }
        ["remove" | "rm" | "delete", reference] => {
            let i = find_rule(config, reference)?;
            let removed = config.rules.remove(i);
            crate::cli::save_config(config)?;
            println!("Removed rule '{}'.", removed.name);
            Ok(())
        }
        [verb @ ("enable" | "disable" | "on" | "off"), target @ ..] if target.len() <= 1 => {
            let on = matches!(*verb, "enable" | "on");
            let msg = match target.first() {
                None => {
                    config.rules_enabled = on;
                    format!("Organizing {}.", if on { "enabled" } else { "disabled" })
                }
                Some(t)
                    if t.eq_ignore_ascii_case("builtins") || t.eq_ignore_ascii_case("builtin") =>
                {
                    config.builtin_rules = on;
                    format!(
                        "Built-in rules {}.",
                        if on { "enabled" } else { "disabled" }
                    )
                }
                Some(reference) => {
                    let i = find_rule(config, reference)?;
                    config.rules[i].enabled = on;
                    format!(
                        "Rule '{}' {}.",
                        config.rules[i].name,
                        if on { "enabled" } else { "disabled" }
                    )
                }
            };
            crate::cli::save_config(config)?;
            println!("{msg}");
            Ok(())
        }
        ["suggest"] => suggest::command(config, opts),
        _ => Err(usage()),
    }
}

/// `kget --rules list` output.
pub fn list_text(config: &crate::Config) -> String {
    let mut out = format!(
        "Organizing: {} · built-in rules: {} · root: {}\n",
        if config.rules_enabled {
            "on"
        } else {
            "off (use --organize or `kget --rules enable`)"
        },
        if config.builtin_rules { "on" } else { "off" },
        organize_root(config).display()
    );
    let rules = active_rules(config);
    if rules.is_empty() {
        out.push_str("No rules.\n");
        return out;
    }
    for (i, r) in rules.iter().enumerate() {
        let flag = if r.enabled { ' ' } else { '-' };
        out.push_str(&format!(
            "{:>3}{flag} {:<22} {}  {}\n",
            format!("#{}", i + 1),
            r.name,
            r.describe_match(),
            r.describe_action()
        ));
    }
    out
}

/// `kget --rules test <url>` output; `probe` allows a `HEAD` request.
pub fn test_text(config: &crate::Config, url: &str, probe: bool) -> Result<String, BoxError> {
    let proto = crate::protocol::detect(url);
    let rules = active_rules(config);
    let plan = plan(&rules, url, proto, &mut || {
        (probe && proto == Protocol::Http)
            .then(|| probe_http(url, &[], &config.proxy))
            .flatten()
    })?;
    let facts = plan
        .facts
        .clone()
        .unwrap_or_else(|| Facts::from_url(url, proto));
    let mut out = format!("URL:       {}\n", facts.url);
    let mut known = vec![
        format!("protocol={}", facts.protocol),
        format!("domain={}", facts.domain.as_deref().unwrap_or("-")),
        format!("ext={}", facts.extension().unwrap_or_else(|| "-".into())),
        format!("category={}", facts.category().unwrap_or("-")),
    ];
    if let Some(m) = &facts.mime {
        known.push(format!("mime={m}"));
    }
    if let Some(s) = facts.size {
        known.push(format!("size={s}"));
    }
    out.push_str(&format!("Facts:     {}\n", known.join(" ")));
    match &plan.rule {
        Some(name) => {
            let pos = rules.iter().position(|r| &r.name == name).unwrap_or(0) + 1;
            out.push_str(&format!("Rule:      {name} (#{pos})\n"));
        }
        None => out.push_str("Rule:      none (default location)\n"),
    }
    let root = organize_root(config);
    let dir = match &plan.folder {
        Some(f) => root.join(f),
        None => root,
    };
    let name = crate::utils::get_filename_from_url_or_default(url, "download");
    out.push_str(&format!("Output:    {}\n", dir.join(name).display()));
    let action = Rule {
        action: plan.action.clone(),
        ..Default::default()
    }
    .describe_action();
    if !action.is_empty() {
        out.push_str(&format!("Action:    {action}\n"));
    }
    if !config.rules_enabled {
        out.push_str("Organizing is off: pass --organize, or run `kget --rules enable`.\n");
    }
    Ok(out)
}

/// Insert `rule` (validated, unique name) at the 1-based `position`, or at
/// the end.
pub fn add_rule(
    config: &mut crate::Config,
    rule: Rule,
    position: Option<usize>,
) -> Result<(), BoxError> {
    rule.validate()?;
    if rule.is_builtin() {
        return Err("rule names starting with 'builtin:' are reserved".into());
    }
    if config
        .rules
        .iter()
        .any(|r| r.name.eq_ignore_ascii_case(&rule.name))
    {
        return Err(format!("a rule named '{}' already exists", rule.name).into());
    }
    let at = position
        .map(|p| p.saturating_sub(1).min(config.rules.len()))
        .unwrap_or(config.rules.len());
    config.rules.insert(at, rule);
    Ok(())
}

/// Index in `config.rules` of a rule given by name or `#n` (1-based).
fn find_rule(config: &crate::Config, reference: &str) -> Result<usize, BoxError> {
    let r = reference.trim();
    if let Ok(n) = r.trim_start_matches('#').parse::<usize>()
        && (1..=config.rules.len()).contains(&n)
    {
        return Ok(n - 1);
    }
    config
        .rules
        .iter()
        .position(|x| x.name.eq_ignore_ascii_case(r))
        .ok_or_else(|| {
            if r.starts_with("builtin:") {
                "built-in rules cannot be changed one by one; use `--rules disable builtins`".into()
            } else {
                format!("no rule '{r}' (see `kget --rules list`)").into()
            }
        })
}

/// Parse `rules add` arguments: one JSON object, or `key=value` pairs.
/// Returns the rule and the requested position.
pub fn parse_rule_spec(spec: &[&str]) -> Result<(Rule, Option<usize>), BoxError> {
    let joined = spec.join(" ");
    if joined.trim_start().starts_with('{') {
        let rule: Rule = serde_json::from_str(&joined)?;
        rule.validate()?;
        return Ok((rule, None));
    }
    let mut rule = Rule::default();
    let mut position = None;
    let list = |v: &str| -> Vec<String> {
        v.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    };
    for item in spec {
        let (k, v) = item
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, got '{item}'"))?;
        let k = k.trim().to_ascii_lowercase().replace('_', "-");
        let v = v.trim();
        let m = &mut rule.when;
        let a = &mut rule.action;
        match k.as_str() {
            "name" => rule.name = v.to_string(),
            "enabled" => rule.enabled = parse_bool(v)?,
            "domain" | "domains" => m.domains.extend(list(v)),
            "regex" | "url-regex" => m.url_regex = Some(v.to_string()),
            "glob" | "url-glob" => m.url_glob = Some(v.to_string()),
            "ext" | "extension" | "extensions" => m.extensions.extend(
                list(v)
                    .into_iter()
                    .map(|e| e.trim_start_matches('.').to_lowercase()),
            ),
            "category" | "categories" | "type" => m.categories.extend(list(v)),
            "mime" => m.mime.extend(list(v)),
            "protocol" | "protocols" => m.protocols.extend(list(v)),
            "min-size" => m.min_size = Some(parse_size(v)?),
            "max-size" => m.max_size = Some(parse_size(v)?),
            "folder" | "dir" => a.folder = Some(v.to_string()),
            "tag" | "tags" => a.tags.extend(list(v)),
            "extract" => a.extract = Some(parse_bool(v)?),
            "checksum" | "require-checksum" => a.require_checksum = parse_bool(v)?,
            "connections" => {
                a.connections = Some(
                    v.parse()
                        .map_err(|_| format!("connections: bad number '{v}'"))?,
                )
            }
            "limit" | "speed-limit" => a.speed_limit = Some(parse_size(v)?),
            "position" | "pos" => {
                position = Some(
                    v.parse()
                        .map_err(|_| format!("position: bad number '{v}'"))?,
                )
            }
            other => return Err(format!("unknown rule key '{other}'").into()),
        }
    }
    rule.action.tags = normalise_tags(rule.action.tags.iter().map(String::as_str))?;
    rule.validate()?;
    Ok((rule, position))
}

fn parse_bool(v: &str) -> Result<bool, String> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "required" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("expected true/false, got '{v}'")),
    }
}

/// Parse a byte count: `1048576`, `500K`, `100MB`, `2G`, `1.5GiB` (binary units).
pub fn parse_size(v: &str) -> Result<u64, String> {
    let t = v.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: f64 = num.parse().map_err(|_| format!("invalid size '{v}'"))?;
    let mult: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        "t" | "tb" | "tib" => 1 << 40,
        _ => return Err(format!("invalid size unit in '{v}'")),
    };
    Ok((n * mult as f64) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(url: &str) -> Facts {
        Facts::from_url(url, crate::protocol::detect(url))
    }

    #[test]
    fn date_placeholder_and_sanitizing() {
        let f = facts("https://user:pw@www.Example.com/a/clip.MP4");
        assert_eq!(f.domain.as_deref(), Some("example.com"));
        assert!(!f.url.contains("pw"));
        let now = chrono::Local::now();
        let p = render_with("{category}/{date:%Y}", &f, now).unwrap();
        assert_eq!(p, Path::new("video").join(now.format("%Y").to_string()));
        // A slash produced by a date format stays inside one component.
        let p = render_with("{date:%Y/%m}", &f, now).unwrap();
        assert_eq!(p.components().count(), 1);
        assert!(render_with("{date:%Q}", &f, now).is_err());
        assert!(render_with("{nope}", &f, now).is_err());
        assert!(render_with("a/../b", &f, now).is_err());
        assert!(render_with("C:/x", &f, now).is_err());
        assert!(render_with("~/x", &f, now).is_err());
        assert!(render_with("x/CON", &f, now).is_err());
    }

    #[test]
    fn mime_categories() {
        assert_eq!(mime_category("video/mp4"), Some("video"));
        assert_eq!(mime_category("application/pdf"), Some("document"));
        assert_eq!(
            mime_category("application/x-7z-compressed"),
            Some("archive")
        );
        assert_eq!(mime_category("application/octet-stream"), None);
        assert!(mime_matches("video/mp4", "video/*"));
        assert!(!mime_matches("audio/mp4", "video/*"));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("100").unwrap(), 100);
        assert_eq!(parse_size("2K").unwrap(), 2048);
        assert_eq!(parse_size("1.5GiB").unwrap(), 1_610_612_736);
        assert!(parse_size("5 parsecs").is_err());
    }
}
