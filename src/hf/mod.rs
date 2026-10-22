//! Hugging Face Hub downloads (`hf://`).
//!
//! Supported forms:
//!
//! - `hf://org/repo` — the whole repository
//! - `hf://org/repo/path/to/file.safetensors` — one file (or a folder prefix)
//! - `hf://org/repo@revision[/path]` — a branch, tag or commit
//!   (percent-encode slashes: `@refs%2Fpr%2F1`)
//! - `hf://datasets/org/name[/…]` and `hf://spaces/org/name[/…]`
//! - `https://huggingface.co/org/repo/resolve/<rev>/<path>`, `/blob/` and
//!   `/tree/` URLs (also under `/datasets/` and `/spaces/`)
//!
//! The file list comes from the Hub API (`/api/models/<repo>/revision/<rev>`,
//! falling back to the tree API), each file is fetched with the turbo engine
//! from `…/resolve/<commit>/<path>` and LFS files are verified against their
//! SHA-256. `--include` / `--exclude` take `fnmatch`-style globs, like
//! `huggingface_hub`'s `allow_patterns` / `ignore_patterns`.
//!
//! Files go to `./<repo-name>/` by default, or with `--hf-cache` into the
//! standard hub cache (`$HF_HUB_CACHE`, `$HF_HOME/hub` or
//! `~/.cache/huggingface/hub`: `blobs/`, `snapshots/<commit>/` symlinks and
//! `refs/`) so `transformers` and `huggingface_hub` reuse them.
//!
//! # Authentication
//!
//! The token comes from `HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`, the token file
//! written by `hf auth login` (`$HF_TOKEN_PATH` or `$HF_HOME/token`), then the
//! KGet keychain entry `hf://token@huggingface.co`. It is only sent to the Hub
//! endpoint: redirects to another host (the CDN) drop the `Authorization`
//! header. Tokens are never printed or logged.
//!
//! The endpoint is `$KGET_HF_ENDPOINT`, `$HF_ENDPOINT` or
//! `https://huggingface.co`.
//!
//! ```rust
//! use kget::hf::{HfRef, RepoType};
//!
//! let r = HfRef::parse("hf://datasets/org/name@v2/data/train.parquet").unwrap();
//! assert_eq!(r.repo_type, RepoType::Dataset);
//! assert_eq!(r.repo, "org/name");
//! assert_eq!(r.revision.as_deref(), Some("v2"));
//! assert_eq!(r.path.as_deref(), Some("data/train.parquet"));
//! ```

use crate::error::KgetError;
use crate::fetch::{self, FetchItem, ModelDownload, TransferOptions};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

/// Hub used when neither `KGET_HF_ENDPOINT` nor `HF_ENDPOINT` is set.
pub const DEFAULT_ENDPOINT: &str = "https://huggingface.co";

/// Revision used when the URL and `--revision` name none.
pub const DEFAULT_REVISION: &str = "main";

// ============================================================================
// URL parsing
// ============================================================================

/// Kind of Hub repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepoType {
    /// Model repository (the default)
    #[default]
    Model,
    /// Dataset repository (`hf://datasets/…`)
    Dataset,
    /// Space (`hf://spaces/…`)
    Space,
}

impl RepoType {
    /// Plural name used in API paths and cache folders (`models`, …).
    pub fn plural(&self) -> &'static str {
        match self {
            RepoType::Model => "models",
            RepoType::Dataset => "datasets",
            RepoType::Space => "spaces",
        }
    }

    /// Prefix of the repository in web/resolve URLs (`""`, `"datasets/"`, …).
    fn url_prefix(&self) -> &'static str {
        match self {
            RepoType::Model => "",
            RepoType::Dataset => "datasets/",
            RepoType::Space => "spaces/",
        }
    }

    fn from_segment(s: &str) -> Option<RepoType> {
        match s {
            "models" => Some(RepoType::Model),
            "datasets" => Some(RepoType::Dataset),
            "spaces" => Some(RepoType::Space),
            _ => None,
        }
    }
}

/// A parsed `hf://` (or Hub `https://`) reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfRef {
    /// Repository kind
    pub repo_type: RepoType,
    /// `org/name`
    pub repo: String,
    /// Branch, tag or commit given in the URL
    pub revision: Option<String>,
    /// File or folder inside the repository (`None` = whole repository)
    pub path: Option<String>,
}

fn invalid(url: &str, why: &str) -> KgetError {
    KgetError::Protocol(format!("Invalid Hugging Face URL '{url}': {why}"))
}

fn decode(s: &str) -> String {
    urlencoding::decode(s)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.to_string())
}

fn valid_repo_part(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl HfRef {
    /// Parse `hf://…` or a Hub `https://…/resolve|blob|tree/…` URL.
    pub fn parse(url: &str) -> Result<HfRef, KgetError> {
        let trimmed = url.trim();
        let no_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
        if no_query.len() >= 5 && no_query[..5].eq_ignore_ascii_case("hf://") {
            return Self::parse_hf(trimmed, &no_query[5..]);
        }
        Self::parse_web(trimmed)
    }

    fn parse_hf(url: &str, rest: &str) -> Result<HfRef, KgetError> {
        let mut segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        let repo_type = match segs.first().and_then(|s| RepoType::from_segment(s)) {
            Some(t) if segs.len() > 2 => {
                segs.remove(0);
                t
            }
            _ => RepoType::Model,
        };
        if segs.len() < 2 {
            return Err(invalid(url, "expected hf://org/repo[@revision][/path]"));
        }
        let org = segs[0];
        let (name, revision) = match segs[1].split_once('@') {
            Some((n, r)) if !r.is_empty() => (n, Some(decode(r))),
            Some(_) => return Err(invalid(url, "empty revision after '@'")),
            None => (segs[1], None),
        };
        if !valid_repo_part(org) || !valid_repo_part(name) {
            return Err(invalid(url, "bad repository name"));
        }
        let path = segs[2..]
            .iter()
            .map(|s| decode(s))
            .collect::<Vec<_>>()
            .join("/");
        Ok(HfRef {
            repo_type,
            repo: format!("{org}/{name}"),
            revision,
            path: (!path.is_empty()).then_some(path),
        })
    }

    fn parse_web(url: &str) -> Result<HfRef, KgetError> {
        let parsed = url::Url::parse(url).map_err(|e| invalid(url, &e.to_string()))?;
        if !matches!(parsed.scheme(), "http" | "https") || !is_hub_host(&parsed) {
            return Err(invalid(url, "not a Hugging Face Hub URL"));
        }
        let mut segs: Vec<&str> = parsed
            .path_segments()
            .map(|s| s.filter(|p| !p.is_empty()).collect())
            .unwrap_or_default();
        let repo_type = match segs.first().and_then(|s| RepoType::from_segment(s)) {
            Some(t) => {
                segs.remove(0);
                t
            }
            None => RepoType::Model,
        };
        if segs.len() < 4 || !matches!(segs[2], "resolve" | "blob" | "tree") {
            return Err(invalid(
                url,
                "expected https://huggingface.co/org/repo/resolve/<revision>/<path>",
            ));
        }
        if !valid_repo_part(segs[0]) || !valid_repo_part(segs[1]) {
            return Err(invalid(url, "bad repository name"));
        }
        let path = segs[4..]
            .iter()
            .map(|s| decode(s))
            .collect::<Vec<_>>()
            .join("/");
        Ok(HfRef {
            repo_type,
            repo: format!("{}/{}", segs[0], segs[1]),
            revision: Some(decode(segs[3])),
            path: (!path.is_empty()).then_some(path),
        })
    }

    /// Repository name without the organisation (default output folder).
    pub fn name(&self) -> &str {
        self.repo.rsplit('/').next().unwrap_or(&self.repo)
    }

    /// Folder of the repository in the hub cache (`models--org--repo`).
    pub fn cache_folder(&self) -> String {
        format!(
            "{}--{}",
            self.repo_type.plural(),
            self.repo.replace('/', "--")
        )
    }
}

fn is_hub_host(url: &url::Url) -> bool {
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    if matches!(
        host.as_str(),
        "huggingface.co" | "www.huggingface.co" | "hf.co"
    ) {
        return true;
    }
    // A mirror set with KGET_HF_ENDPOINT / HF_ENDPOINT.
    url::Url::parse(&endpoint()).is_ok_and(|e| {
        e.host_str().map(str::to_ascii_lowercase).as_deref() == Some(host.as_str())
            && e.port_or_known_default() == url.port_or_known_default()
    })
}

/// Whether `url` is a Hugging Face reference: `hf://…`, or a Hub
/// `https://…/resolve/…`, `/blob/…` or `/tree/…` URL.
pub fn is_hf_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("hf://") {
        return true;
    }
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && (lower.contains("/resolve/") || lower.contains("/blob/") || lower.contains("/tree/"))
        && HfRef::parse(url).is_ok()
}

// ============================================================================
// Environment: endpoint, cache, token
// ============================================================================

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Hub endpoint: `$KGET_HF_ENDPOINT`, `$HF_ENDPOINT` or [`DEFAULT_ENDPOINT`].
pub fn endpoint() -> String {
    env_nonempty("KGET_HF_ENDPOINT")
        .or_else(|| env_nonempty("HF_ENDPOINT"))
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn home_cache() -> PathBuf {
    env_nonempty("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"))
}

/// `$HF_HOME`, or `~/.cache/huggingface`.
pub fn hf_home() -> PathBuf {
    env_nonempty("HF_HOME")
        .map(|p| PathBuf::from(crate::utils::expand_tilde(&p)))
        .unwrap_or_else(|| home_cache().join("huggingface"))
}

/// Hub cache: `$HF_HUB_CACHE`, `$HF_HOME/hub` or `~/.cache/huggingface/hub`.
pub fn hub_cache_dir() -> PathBuf {
    env_nonempty("HF_HUB_CACHE")
        .or_else(|| env_nonempty("HUGGINGFACE_HUB_CACHE"))
        .map(|p| PathBuf::from(crate::utils::expand_tilde(&p)))
        .unwrap_or_else(|| hf_home().join("hub"))
}

/// Token file written by `hf auth login`: `$HF_TOKEN_PATH` or `$HF_HOME/token`.
pub fn token_path() -> PathBuf {
    env_nonempty("HF_TOKEN_PATH")
        .map(|p| PathBuf::from(crate::utils::expand_tilde(&p)))
        .unwrap_or_else(|| hf_home().join("token"))
}

/// Where a Hugging Face token was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    /// An environment variable (`HF_TOKEN`, `HUGGING_FACE_HUB_TOKEN`)
    Env(&'static str),
    /// The token file
    File(PathBuf),
    /// The KGet keychain entry
    Keychain(String),
}

impl fmt::Display for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenSource::Env(v) => write!(f, "${v}"),
            TokenSource::File(p) => write!(f, "{}", p.display()),
            TokenSource::Keychain(t) => write!(f, "system keychain ({t})"),
        }
    }
}

/// Keychain target of the Hugging Face token for `endpoint`
/// (`hf://token@huggingface.co`).
pub fn keychain_target(endpoint: &str) -> Option<crate::credentials::Target> {
    let host = url::Url::parse(endpoint)
        .ok()?
        .host_str()?
        .to_ascii_lowercase();
    crate::credentials::Target::from_url(&format!("hf://token@{host}"))
}

/// The Hugging Face token and where it came from (see the module docs for
/// the lookup order). The token itself must never be printed.
pub fn find_token(endpoint: &str) -> Option<(String, TokenSource)> {
    for var in ["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN"] {
        if let Some(t) = env_nonempty(var) {
            return Some((t, TokenSource::Env(var)));
        }
    }
    let path = token_path();
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim();
        if !t.is_empty() {
            return Some((t.to_string(), TokenSource::File(path)));
        }
    }
    let target = keychain_target(endpoint)?;
    match crate::credentials::get(&target) {
        Ok(Some(t)) if !t.trim().is_empty() => Some((
            t.trim().to_string(),
            TokenSource::Keychain(target.to_string()),
        )),
        _ => None,
    }
}

/// One-line authentication status for `--doctor` (never the token).
pub fn auth_status() -> String {
    let ep = endpoint();
    match find_token(&ep) {
        Some((_, source)) => format!("token from {source} (endpoint {ep})"),
        None => format!(
            "no token (public repositories only; set HF_TOKEN or run `hf auth login`) (endpoint {ep})"
        ),
    }
}

// ============================================================================
// Hub API
// ============================================================================

/// One file of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFile {
    /// Path inside the repository
    pub path: String,
    /// Size in bytes (the real size for LFS files)
    pub size: Option<u64>,
    /// Git blob id
    pub blob_id: Option<String>,
    /// SHA-256 of an LFS file
    pub lfs_sha256: Option<String>,
}

impl RepoFile {
    /// Name of the file in the hub cache `blobs/` folder: the LFS SHA-256,
    /// or the git blob id for regular files (what `huggingface_hub` uses).
    pub fn etag(&self) -> Option<&str> {
        self.lfs_sha256.as_deref().or(self.blob_id.as_deref())
    }
}

/// Files of a repository at a resolved commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoListing {
    /// Commit the revision resolved to
    pub commit: String,
    /// Every file in the repository
    pub files: Vec<RepoFile>,
}

#[derive(Deserialize)]
struct ApiLfs {
    sha256: Option<String>,
    oid: Option<String>,
    size: Option<u64>,
}

#[derive(Deserialize)]
struct ApiSibling {
    rfilename: String,
    size: Option<u64>,
    #[serde(rename = "blobId")]
    blob_id: Option<String>,
    lfs: Option<ApiLfs>,
}

#[derive(Deserialize)]
struct ApiRevision {
    sha: Option<String>,
    siblings: Option<Vec<ApiSibling>>,
}

#[derive(Deserialize)]
struct ApiTreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    size: Option<u64>,
    oid: Option<String>,
    lfs: Option<ApiLfs>,
}

fn lfs_sha(lfs: &Option<ApiLfs>) -> Option<String> {
    lfs.as_ref()
        .and_then(|l| l.sha256.clone().or_else(|| l.oid.clone()))
        .map(|s| s.trim_start_matches("sha256:").to_ascii_lowercase())
        .filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Blocking client for the Hub API.
pub struct HfClient {
    endpoint: String,
    token: Option<String>,
    client: Client,
}

impl fmt::Debug for HfClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HfClient")
            .field("endpoint", &self.endpoint)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl HfClient {
    /// Client for `endpoint` (e.g. `https://huggingface.co`), sending `token`
    /// as a bearer token to that host only.
    pub fn new(
        endpoint: &str,
        token: Option<String>,
        proxy: &crate::config::ProxyConfig,
        timeout_secs: u64,
    ) -> Result<HfClient, KgetError> {
        Ok(HfClient {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            token,
            client: crate::http::build_client(
                proxy,
                &crate::http::ClientOptions::with_timeout_secs(timeout_secs),
            )?,
        })
    }

    /// The endpoint this client talks to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// `Authorization` header for requests to the endpoint, if a token is set.
    pub fn auth_headers(&self) -> Vec<(String, String)> {
        self.token
            .iter()
            .map(|t| ("Authorization".to_string(), format!("Bearer {t}")))
            .collect()
    }

    /// Download URL of `path` at `revision`.
    pub fn resolve_url(&self, r: &HfRef, revision: &str, path: &str) -> String {
        let path = path
            .split('/')
            .map(|s| urlencoding::encode(s).into_owned())
            .collect::<Vec<_>>()
            .join("/");
        format!(
            "{}/{}{}/resolve/{}/{}",
            self.endpoint,
            r.repo_type.url_prefix(),
            r.repo,
            urlencoding::encode(revision),
            path
        )
    }

    fn get(
        &self,
        url: &str,
        r: &HfRef,
        revision: &str,
    ) -> Result<reqwest::blocking::Response, KgetError> {
        let req = crate::http::apply_headers(self.client.get(url), &self.auth_headers());
        let resp = req.send()?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        Err(self.api_error(&resp, r, revision))
    }

    /// Readable error for a failed Hub request (gated, private, missing…).
    fn api_error(
        &self,
        resp: &reqwest::blocking::Response,
        r: &HfRef,
        revision: &str,
    ) -> KgetError {
        let status = resp.status().as_u16();
        let header = |n: &str| {
            resp.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let code = header("x-error-code").unwrap_or_default();
        let message = header("x-error-message");
        auth_error(self, status, &code, message.as_deref(), r, revision)
    }

    /// Resolve `revision` to a commit and list every file of the repository.
    pub fn list(&self, r: &HfRef, revision: &str) -> Result<RepoListing, KgetError> {
        let url = format!(
            "{}/api/{}/{}/revision/{}?blobs=true",
            self.endpoint,
            r.repo_type.plural(),
            r.repo,
            urlencoding::encode(revision)
        );
        let info: ApiRevision = self
            .get(&url, r, revision)?
            .json()
            .map_err(|e| KgetError::Protocol(format!("Unexpected Hub API answer: {e}")))?;
        let commit = info
            .sha
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| revision.to_string());
        let siblings = info.siblings.unwrap_or_default();
        // `blobs=true` gives sizes and LFS hashes; without them use the tree API.
        if !siblings.is_empty() && siblings.iter().all(|s| s.size.is_some()) {
            let files = siblings
                .into_iter()
                .map(|s| RepoFile {
                    lfs_sha256: lfs_sha(&s.lfs),
                    size: s.lfs.as_ref().and_then(|l| l.size).or(s.size),
                    path: s.rfilename,
                    blob_id: s.blob_id,
                })
                .collect();
            return Ok(RepoListing { commit, files });
        }
        let files = self.tree(r, &commit)?;
        Ok(RepoListing { commit, files })
    }

    /// Every file at `commit` from the paginated tree API.
    fn tree(&self, r: &HfRef, commit: &str) -> Result<Vec<RepoFile>, KgetError> {
        let mut url = Some(format!(
            "{}/api/{}/{}/tree/{}?recursive=true",
            self.endpoint,
            r.repo_type.plural(),
            r.repo,
            urlencoding::encode(commit)
        ));
        let mut files = Vec::new();
        let mut pages = 0;
        while let Some(u) = url.take() {
            pages += 1;
            if pages > 10_000 {
                return Err(KgetError::Protocol("Hub tree listing does not end".into()));
            }
            let resp = self.get(&u, r, commit)?;
            url = resp
                .headers()
                .get(reqwest::header::LINK)
                .and_then(|v| v.to_str().ok())
                .and_then(next_link)
                .map(|next| absolutize(&self.endpoint, &next))
                // Never follow a pagination link to another host with the token.
                .filter(|next| same_origin(&self.endpoint, next));
            let entries: Vec<ApiTreeEntry> = resp
                .json()
                .map_err(|e| KgetError::Protocol(format!("Unexpected Hub API answer: {e}")))?;
            files.extend(
                entries
                    .into_iter()
                    .filter(|e| e.kind == "file")
                    .map(|e| RepoFile {
                        lfs_sha256: lfs_sha(&e.lfs),
                        size: e.lfs.as_ref().and_then(|l| l.size).or(e.size),
                        path: e.path,
                        blob_id: e.oid,
                    }),
            );
        }
        Ok(files)
    }
}

/// `next` URL of an RFC 8288 `Link` header.
fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let (url, params) = part.split_once(';')?;
        params
            .split(';')
            .any(|p| p.trim().trim_start_matches("rel=").trim_matches('"') == "next")
            .then(|| {
                url.trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string()
            })
    })
}

fn absolutize(endpoint: &str, link: &str) -> String {
    match url::Url::parse(endpoint).and_then(|base| base.join(link)) {
        Ok(u) => u.to_string(),
        Err(_) => link.to_string(),
    }
}

fn same_origin(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(a), Ok(b)) => {
            a.scheme() == b.scheme()
                && a.host_str() == b.host_str()
                && a.port_or_known_default() == b.port_or_known_default()
        }
        _ => false,
    }
}

/// Error for an HTTP failure from the Hub, with a hint for gated, private
/// and missing repositories. Never includes the token.
fn auth_error(
    client: &HfClient,
    status: u16,
    code: &str,
    message: Option<&str>,
    r: &HfRef,
    revision: &str,
) -> KgetError {
    let page = format!("{}/{}{}", client.endpoint, r.repo_type.url_prefix(), r.repo);
    let has_token = client.token.is_some();
    let detail = match (status, code) {
        (401, _) | (403, _)
            if code == "GatedRepo" || message.is_some_and(|m| m.contains("gated")) =>
        {
            if has_token {
                format!(
                    "{page} is a gated repository: accept its terms on that page with the account of your token"
                )
            } else {
                format!(
                    "{page} is a gated repository: accept its terms on that page, then set HF_TOKEN (or run `hf auth login`)"
                )
            }
        }
        (401, _) if has_token => {
            format!("{page}: the Hugging Face token was rejected or cannot access this repository")
        }
        (401, _) => format!(
            "{page} needs authentication (private or gated): set HF_TOKEN or run `hf auth login`"
        ),
        (403, _) => format!(
            "{page}: access denied{}",
            message.map(|m| format!(" ({m})")).unwrap_or_default()
        ),
        (404, "RevisionNotFound") => {
            return KgetError::NotFound(format!("revision '{revision}' of {page}"));
        }
        (404, "EntryNotFound") => return KgetError::NotFound(format!("file in {page}")),
        (404, _) if has_token => return KgetError::NotFound(page),
        (404, _) => {
            return KgetError::NotFound(format!(
                "{page} (if the repository is private, set HF_TOKEN)"
            ));
        }
        _ => return KgetError::from_status(status, page),
    };
    KgetError::Http {
        status,
        url: detail,
    }
}

// ============================================================================
// Download
// ============================================================================

/// Which token to send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum HfAuth {
    /// Look the token up with [`find_token`] (default)
    #[default]
    Auto,
    /// Use this token
    Token(String),
    /// Send no token
    Anonymous,
}

/// Options of a Hugging Face download.
#[derive(Debug, Clone, Default)]
pub struct HfOptions {
    /// Hub endpoint (`None` = [`endpoint()`])
    pub endpoint: Option<String>,
    /// Token to send
    pub auth: HfAuth,
    /// Branch, tag or commit; overrides the URL's `@revision`
    pub revision: Option<String>,
    /// Only download files matching one of these globs
    pub include: Vec<String>,
    /// Skip files matching any of these globs
    pub exclude: Vec<String>,
    /// Plain layout: target directory (or file for a single file).
    /// Hub cache layout: cache directory (`None` = [`hub_cache_dir()`]).
    pub output: Option<String>,
    /// Write into the hub cache layout instead of a plain directory
    pub hf_cache: bool,
    /// Expected SHA-256 of a single-file download (LFS files are always
    /// verified against the Hub's hash)
    pub expected_sha256: Option<String>,
}

/// Files of `url` that a download would fetch, after the path and
/// `--include`/`--exclude` filters, and whether it is a single file.
fn select<'a>(
    r: &HfRef,
    listing: &'a RepoListing,
    opts: &HfOptions,
) -> Result<(Vec<&'a RepoFile>, bool), KgetError> {
    if let Some(p) = r.path.as_deref() {
        let p = p.trim_end_matches('/');
        if let Some(f) = listing.files.iter().find(|f| f.path == p) {
            return Ok((vec![f], true));
        }
        let prefix = format!("{p}/");
        let files: Vec<&RepoFile> = listing
            .files
            .iter()
            .filter(|f| f.path.starts_with(&prefix))
            .filter(|f| fetch::filter_path(&f.path, &opts.include, &opts.exclude))
            .collect();
        if files.is_empty() {
            return Err(KgetError::NotFound(format!("'{p}' in {}", r.repo)));
        }
        return Ok((files, false));
    }
    let files: Vec<&RepoFile> = listing
        .files
        .iter()
        .filter(|f| fetch::filter_path(&f.path, &opts.include, &opts.exclude))
        .collect();
    if files.is_empty() {
        return Err(KgetError::Other(format!(
            "No file of {} matches the --include/--exclude filters",
            r.repo
        )));
    }
    Ok((files, false))
}

/// List the files `url` refers to (after filters), without downloading.
pub fn list(
    url: &str,
    opts: &HfOptions,
    transfer: &TransferOptions,
) -> Result<(RepoListing, Vec<RepoFile>), KgetError> {
    let r = HfRef::parse(url)?;
    let client = client_for(opts, transfer)?;
    let revision = revision_for(&r, opts);
    let listing = client.list(&r, &revision)?;
    let (files, _) = select(&r, &listing, opts)?;
    let files = files.into_iter().cloned().collect();
    Ok((listing, files))
}

fn revision_for(r: &HfRef, opts: &HfOptions) -> String {
    opts.revision
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| r.revision.clone())
        .unwrap_or_else(|| DEFAULT_REVISION.to_string())
}

fn client_for(opts: &HfOptions, transfer: &TransferOptions) -> Result<HfClient, KgetError> {
    let ep = opts
        .endpoint
        .clone()
        .unwrap_or_else(endpoint)
        .trim_end_matches('/')
        .to_string();
    let token = match &opts.auth {
        HfAuth::Auto => find_token(&ep).map(|(t, _)| t),
        HfAuth::Token(t) => Some(t.clone()),
        HfAuth::Anonymous => None,
    };
    HfClient::new(&ep, token, &transfer.proxy, transfer.timeout)
}

/// Download `url` (see the module docs for the forms and layouts).
pub fn download(
    url: &str,
    opts: &HfOptions,
    transfer: &TransferOptions,
) -> Result<ModelDownload, KgetError> {
    let r = HfRef::parse(url)?;
    let client = client_for(opts, transfer)?;
    let revision = revision_for(&r, opts);
    transfer.status(format!(
        "Hugging Face: listing {}{} @ {revision}",
        r.repo_type.url_prefix(),
        r.repo
    ));
    let listing = client.list(&r, &revision)?;
    let (files, single) = select(&r, &listing, opts)?;
    let total: u64 = files.iter().filter_map(|f| f.size).sum();
    transfer.status(format!(
        "Commit {}: {} file(s), {}",
        &listing.commit[..listing.commit.len().min(12)],
        files.len(),
        humansize::format_size(total, humansize::BINARY)
    ));

    let mut items = Vec::with_capacity(files.len());
    let target: PathBuf;
    let mut cache: Option<(PathBuf, Vec<(PathBuf, String)>)> = None;
    if opts.hf_cache {
        let root = opts
            .output
            .as_deref()
            .map(|o| PathBuf::from(crate::utils::expand_tilde(o)))
            .unwrap_or_else(hub_cache_dir);
        let repo_dir = root.join(r.cache_folder());
        let snapshot = repo_dir.join("snapshots").join(&listing.commit);
        let mut links = Vec::new();
        for f in &files {
            let rel = fetch::safe_relative_path(&f.path).ok_or_else(|| unsafe_path(&f.path))?;
            let etag = f
                .etag()
                .filter(|e| {
                    fetch::safe_relative_path(e).is_some_and(|p| p.components().count() == 1)
                })
                .ok_or_else(|| {
                    KgetError::Protocol(format!("The Hub gave no blob id for '{}'", f.path))
                })?;
            items.push(item(
                &client,
                &r,
                &listing.commit,
                f,
                repo_dir.join("blobs").join(etag),
                single,
                opts,
            ));
            links.push((rel, etag.to_string()));
        }
        target = if single {
            snapshot.join(&links[0].0)
        } else {
            snapshot.clone()
        };
        cache = Some((repo_dir, links));
    } else if single {
        let f = files[0];
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let name = crate::utils::sanitize_filename(name);
        target = match opts.output.as_deref().map(crate::utils::expand_tilde) {
            None => PathBuf::from(&name),
            Some(o)
                if o.ends_with('/')
                    || o.ends_with(std::path::MAIN_SEPARATOR)
                    || Path::new(&o).is_dir() =>
            {
                Path::new(&o).join(&name)
            }
            Some(o) => PathBuf::from(o),
        };
        items.push(item(
            &client,
            &r,
            &listing.commit,
            f,
            absolute(&target),
            true,
            opts,
        ));
    } else {
        target = opts
            .output
            .as_deref()
            .map(|o| PathBuf::from(crate::utils::expand_tilde(o)))
            .unwrap_or_else(|| PathBuf::from(r.name()));
        for f in &files {
            let rel = fetch::safe_relative_path(&f.path).ok_or_else(|| unsafe_path(&f.path))?;
            items.push(item(
                &client,
                &r,
                &listing.commit,
                f,
                absolute(&target.join(rel)),
                false,
                opts,
            ));
        }
    }

    let mut transfer = transfer.clone();
    transfer.headers.extend(client.auth_headers());
    let stats = fetch::fetch_all(&items, &transfer).map_err(|e| match e {
        KgetError::Http { status, .. } if status == 401 || status == 403 => {
            auth_error(&client, status, "", None, &r, &revision)
        }
        other => other,
    })?;

    if let Some((repo_dir, links)) = cache {
        write_snapshot(&repo_dir, &listing.commit, &revision, &links)?;
    }
    Ok(ModelDownload {
        path: absolute(&target),
        single_file: single,
        files: stats.files,
        skipped: stats.skipped,
        bytes: stats.bytes,
    })
}

fn unsafe_path(p: &str) -> KgetError {
    KgetError::Protocol(format!("Refusing unsafe file path from the Hub: '{p}'"))
}

fn item(
    client: &HfClient,
    r: &HfRef,
    commit: &str,
    f: &RepoFile,
    dest: PathBuf,
    single: bool,
    opts: &HfOptions,
) -> FetchItem {
    let user_sha = single
        .then(|| {
            opts.expected_sha256
                .as_ref()
                .map(|s| s.trim().to_ascii_lowercase())
        })
        .flatten();
    FetchItem {
        name: f.path.clone(),
        url: client.resolve_url(r, commit, &f.path),
        dest,
        size: f.size,
        sha256: f.lfs_sha256.clone().or(user_sha),
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

/// Create `snapshots/<commit>/<file>` links to `blobs/<etag>` (copies where
/// symlinks are unavailable, e.g. Windows without developer mode) and record
/// `refs/<revision>`.
fn write_snapshot(
    repo_dir: &Path,
    commit: &str,
    revision: &str,
    links: &[(PathBuf, String)],
) -> Result<(), KgetError> {
    let snapshot = repo_dir.join("snapshots").join(commit);
    for (rel, etag) in links {
        let link = snapshot.join(rel);
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let blob = repo_dir.join("blobs").join(etag);
        if std::fs::symlink_metadata(&link).is_ok() {
            std::fs::remove_file(&link)?;
        }
        // Relative target, like huggingface_hub: ../../blobs/<etag> (+1 per folder).
        let depth = rel.components().count();
        let mut target = PathBuf::new();
        for _ in 0..=depth {
            target.push("..");
        }
        target.push("blobs");
        target.push(etag);
        if symlink(&target, &link).is_err() {
            std::fs::copy(&blob, &link)?;
        }
    }
    if revision != commit {
        let rel = fetch::safe_relative_path(revision)
            .ok_or_else(|| KgetError::Protocol(format!("Invalid revision name '{revision}'")))?;
        crate::config::write_atomic(&repo_dir.join("refs").join(rel), commit.as_bytes(), false)?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "symlinks are not used on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hf_urls() {
        let r = HfRef::parse("hf://meta-llama/Llama-3.2-1B").unwrap();
        assert_eq!(r.repo_type, RepoType::Model);
        assert_eq!(r.repo, "meta-llama/Llama-3.2-1B");
        assert_eq!(r.revision, None);
        assert_eq!(r.path, None);
        assert_eq!(r.cache_folder(), "models--meta-llama--Llama-3.2-1B");

        let r = HfRef::parse("hf://org/repo@refs%2Fpr%2F1/sub/model.safetensors").unwrap();
        assert_eq!(r.revision.as_deref(), Some("refs/pr/1"));
        assert_eq!(r.path.as_deref(), Some("sub/model.safetensors"));

        let r = HfRef::parse("hf://spaces/org/demo").unwrap();
        assert_eq!(r.repo_type, RepoType::Space);
        assert_eq!(r.cache_folder(), "spaces--org--demo");

        // A model repo literally named "datasets/x" is not ambiguous: two
        // segments after the type are required.
        let r = HfRef::parse("hf://datasets/name").unwrap();
        assert_eq!(r.repo_type, RepoType::Model);
        assert_eq!(r.repo, "datasets/name");

        assert!(HfRef::parse("hf://only-one").is_err());
        assert!(HfRef::parse("hf://org/repo@").is_err());
        assert!(HfRef::parse("hf://../repo").is_err());
    }

    #[test]
    fn parses_web_urls() {
        let r = HfRef::parse(
            "https://huggingface.co/TheBloke/Mistral-7B-GGUF/resolve/main/mistral.Q4_K_M.gguf?download=true",
        )
        .unwrap();
        assert_eq!(r.repo, "TheBloke/Mistral-7B-GGUF");
        assert_eq!(r.revision.as_deref(), Some("main"));
        assert_eq!(r.path.as_deref(), Some("mistral.Q4_K_M.gguf"));

        let r = HfRef::parse("https://huggingface.co/datasets/org/data/blob/v1/a/b.csv").unwrap();
        assert_eq!(r.repo_type, RepoType::Dataset);
        assert_eq!(r.path.as_deref(), Some("a/b.csv"));

        assert!(is_hf_url(
            "https://huggingface.co/org/repo/resolve/main/x.bin"
        ));
        assert!(is_hf_url("HF://org/repo"));
        assert!(!is_hf_url("https://huggingface.co/org/repo"));
        assert!(!is_hf_url(
            "https://example.com/org/repo/resolve/main/x.bin"
        ));
    }

    #[test]
    fn link_header_and_resolve_urls() {
        assert_eq!(
            next_link(r#"<https://huggingface.co/api/x?cursor=abc>; rel="next""#).as_deref(),
            Some("https://huggingface.co/api/x?cursor=abc")
        );
        assert_eq!(next_link(r#"<https://a/b>; rel="prev""#), None);
        let c = HfClient::new("https://hub.example/", None, &Default::default(), 10).unwrap();
        let r = HfRef::parse("hf://datasets/org/data").unwrap();
        assert_eq!(
            c.resolve_url(&r, "refs/pr/1", "dir/a b.txt"),
            "https://hub.example/datasets/org/data/resolve/refs%2Fpr%2F1/dir/a%20b.txt"
        );
        assert!(c.auth_headers().is_empty());
        assert!(!format!("{c:?}").contains("Bearer"));
    }
}
