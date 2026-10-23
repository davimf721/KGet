//! Saved credentials in the operating system's credential store.
//!
//! Passwords are kept in the system keychain, never in `config.json`, the
//! download history, logs or JSONL events:
//!
//! - macOS: Keychain (login keychain)
//! - Windows: Credential Manager
//! - Linux/BSD: Secret Service (GNOME Keyring, KWallet…) over D-Bus
//!
//! Every secret is stored under the service name `kget` and a *target* such as
//! `sftp://alice@example.com`, `ftp://bob@files.example.com:2121` or
//! `socks5://proxyuser@127.0.0.1:1080`. SSH key passphrases use
//! `ssh-key:///home/alice/.ssh/id_ed25519`.
//!
//! The platform stores are compiled in with the `keychain` cargo feature
//! (enabled by default). Without it, or when no keychain is reachable (a
//! headless Linux box without a Secret Service, a locked keychain…), KGet
//! behaves exactly as before: credentials come from the URL or a prompt and
//! are not remembered.
//!
//! Set `KGET_KEYCHAIN=off` to disable the system keychain, or
//! `KGET_KEYCHAIN=mock` to use a throw-away in-memory store (for tests).
//!
//! # Example
//!
//! ```rust,no_run
//! use kget::credentials::{self, Target};
//!
//! let target = Target::from_url("sftp://alice@example.com/data.bin").unwrap();
//! credentials::store(&target, "s3cret")?;
//! assert_eq!(credentials::get(&target)?.as_deref(), Some("s3cret"));
//! credentials::delete(&target)?;
//! # Ok::<(), kget::credentials::CredentialError>(())
//! ```

use crate::config::ProxyConfig;
use std::collections::BTreeSet;
use std::fmt;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Service name of every KGet entry in the credential store.
pub const SERVICE: &str = "kget";

/// Reserved entry holding the list of saved targets (no secrets), so they
/// can be listed on every platform.
const INDEX_USER: &str = "kget:index";

// ────────────────────────────────────────────────────────────
// Errors
// ────────────────────────────────────────────────────────────

/// Credential store failure. Messages never contain secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    /// No credential store is available (feature disabled, no Secret
    /// Service, `KGET_KEYCHAIN=off`…).
    Unavailable(String),
    /// The store rejected the operation (locked, access denied…).
    Backend(String),
    /// The target is not a valid `scheme://user@host[:port]`.
    InvalidTarget(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::Unavailable(e) => write!(f, "system keychain unavailable: {e}"),
            CredentialError::Backend(e) => write!(f, "system keychain error: {e}"),
            CredentialError::InvalidTarget(e) => write!(f, "invalid credential target: {e}"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// Map a keyring error without ever formatting secret data (some variants
/// carry raw bytes or whole entries).
fn backend_error(e: keyring_core::Error) -> CredentialError {
    use keyring_core::Error as E;
    match e {
        E::NoDefaultStore => CredentialError::Unavailable("no credential store".into()),
        E::PlatformFailure(p) | E::NoStorageAccess(p) => CredentialError::Backend(p.to_string()),
        E::BadEncoding(_) | E::BadDataFormat(..) => {
            CredentialError::Backend("stored secret is not valid text".into())
        }
        E::Ambiguous(items) => CredentialError::Backend(format!(
            "{} matching entries; remove the duplicates",
            items.len()
        )),
        E::TooLong(name, len) => {
            CredentialError::Backend(format!("'{name}' is longer than {len} characters"))
        }
        E::Invalid(attr, reason) => CredentialError::Backend(format!("{attr}: {reason}")),
        E::NotSupportedByStore(s) | E::BadStoreFormat(s) => CredentialError::Backend(s),
        E::NoEntry => CredentialError::Backend("no matching credential".into()),
        _ => CredentialError::Backend("unknown error".into()),
    }
}

// ────────────────────────────────────────────────────────────
// Targets
// ────────────────────────────────────────────────────────────

/// What a secret belongs to: `scheme://user@host[:port]` (default ports are
/// omitted) or `ssh-key://<path>` for a private key passphrase.
///
/// A target never contains a password or a path on the server.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Target(String);

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "ftp" => Some(21),
        "sftp" | "ssh" => Some(22),
        "http" | "webdav" => Some(80),
        "https" | "webdavs" => Some(443),
        _ => None,
    }
}

impl Target {
    /// Target for a URL with a user name (`sftp://alice:pw@host/path` →
    /// `sftp://alice@host`). `None` when the URL has no user.
    pub fn from_url(raw: &str) -> Option<Target> {
        let url = url::Url::parse(raw.trim()).ok()?;
        if url.username().is_empty() {
            return None;
        }
        let scheme = url.scheme().to_ascii_lowercase();
        let host = url.host_str()?.to_ascii_lowercase();
        let port = url
            .port()
            .filter(|p| default_port(&scheme) != Some(*p))
            .map(|p| format!(":{p}"))
            .unwrap_or_default();
        Some(Target(format!(
            "{scheme}://{}@{host}{port}",
            url.username()
        )))
    }

    /// Target for the passphrase of an SSH private key.
    pub fn ssh_key(path: &Path) -> Target {
        let path = path.to_string_lossy().replace('\\', "/");
        let path = path.trim_start_matches('/');
        Target(format!("ssh-key:///{path}"))
    }

    /// Parse a target typed by the user: `sftp://alice@host`, a full URL
    /// (path and password are dropped) or `ssh-key:///path/to/key`.
    pub fn parse(raw: &str) -> Result<Target, CredentialError> {
        let raw = raw.trim();
        if let Some(path) = raw.strip_prefix("ssh-key://") {
            if path.trim_matches('/').is_empty() {
                return Err(CredentialError::InvalidTarget(
                    "expected ssh-key:///path/to/key".into(),
                ));
            }
            return Ok(Target::ssh_key(Path::new(path)));
        }
        if !raw.contains("://") {
            return Err(CredentialError::InvalidTarget(format!(
                "'{raw}' (expected scheme://user@host[:port])"
            )));
        }
        Target::from_url(raw).ok_or_else(|| {
            CredentialError::InvalidTarget(format!(
                "'{}' (expected scheme://user@host[:port])",
                crate::queue::redact_url(raw)
            ))
        })
    }

    /// The target string as stored in the keychain.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ────────────────────────────────────────────────────────────
// Keychain
// ────────────────────────────────────────────────────────────

/// Handle to a credential store (the system keychain or an in-memory mock).
#[derive(Clone)]
pub struct Keychain {
    store: Arc<keyring_core::CredentialStore>,
}

impl fmt::Debug for Keychain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keychain")
            .field("backend", &self.backend_name())
            .finish()
    }
}

/// Serialises read-modify-write updates of the target index.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

impl Keychain {
    /// The operating system's credential store (opened once per process).
    ///
    /// Honours `KGET_KEYCHAIN=off` (disabled) and `KGET_KEYCHAIN=mock`
    /// (in-memory store, nothing is persisted).
    pub fn system() -> Result<Keychain, CredentialError> {
        static SYSTEM: OnceLock<Result<Keychain, CredentialError>> = OnceLock::new();
        SYSTEM
            .get_or_init(|| {
                match std::env::var("KGET_KEYCHAIN")
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "off" | "0" | "false" | "no" | "none" => Err(CredentialError::Unavailable(
                        "disabled by KGET_KEYCHAIN".into(),
                    )),
                    "mock" => Ok(Keychain::mock()),
                    _ => open_platform_store(),
                }
            })
            .clone()
    }

    /// A new, empty in-memory store. Nothing touches the real keychain.
    pub fn mock() -> Keychain {
        // keyring-core's mock `Store::new` always returns `Ok`.
        #[allow(clippy::expect_used)]
        let store: Arc<keyring_core::CredentialStore> =
            keyring_core::mock::Store::new().expect("the mock store cannot fail");
        Keychain { store }
    }

    /// Wrap any keyring-core credential store.
    pub fn from_store(store: Arc<keyring_core::CredentialStore>) -> Keychain {
        Keychain { store }
    }

    /// Human-readable name of the backend.
    pub fn backend_name(&self) -> String {
        let vendor = self.store.vendor();
        if vendor.starts_with("Mock") {
            "in-memory (mock)".into()
        } else {
            vendor
        }
    }

    fn entry(&self, user: &str) -> Result<keyring_core::Entry, CredentialError> {
        self.store.build(SERVICE, user, None).map_err(backend_error)
    }

    /// The secret saved for `target`, if any.
    pub fn get(&self, target: &Target) -> Result<Option<String>, CredentialError> {
        match self.entry(target.as_str())?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(backend_error(e)),
        }
    }

    /// Save (or replace) the secret for `target`.
    pub fn store(&self, target: &Target, secret: &str) -> Result<(), CredentialError> {
        self.entry(target.as_str())?
            .set_password(secret)
            .map_err(backend_error)?;
        self.update_index(|set| {
            set.insert(target.as_str().to_string());
        })
    }

    /// Remove the secret for `target`. Returns `false` if none was saved.
    pub fn delete(&self, target: &Target) -> Result<bool, CredentialError> {
        let existed = match self.entry(target.as_str())?.delete_credential() {
            Ok(()) => true,
            Err(keyring_core::Error::NoEntry) => false,
            Err(e) => return Err(backend_error(e)),
        };
        self.update_index(|set| {
            set.remove(target.as_str());
        })?;
        Ok(existed)
    }

    /// Targets saved by KGet, sorted.
    pub fn list(&self) -> Result<Vec<Target>, CredentialError> {
        Ok(self.read_index()?.into_iter().map(Target).collect())
    }

    fn read_index(&self) -> Result<BTreeSet<String>, CredentialError> {
        match self.entry(INDEX_USER)?.get_password() {
            Ok(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
            Err(keyring_core::Error::NoEntry) => Ok(BTreeSet::new()),
            Err(e) => Err(backend_error(e)),
        }
    }

    fn update_index(&self, f: impl FnOnce(&mut BTreeSet<String>)) -> Result<(), CredentialError> {
        let _guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut set = self.read_index()?;
        let before = set.clone();
        f(&mut set);
        if set == before {
            return Ok(());
        }
        let entry = self.entry(INDEX_USER)?;
        if set.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(e) => Err(backend_error(e)),
            }
        } else {
            let json = serde_json::to_string(&set).unwrap_or_else(|_| "[]".into());
            entry.set_password(&json).map_err(backend_error)
        }
    }
}

#[cfg(all(feature = "keychain", target_os = "macos"))]
fn open_platform_store() -> Result<Keychain, CredentialError> {
    let store: Arc<keyring_core::CredentialStore> =
        apple_native_keyring_store::keychain::Store::new().map_err(backend_error)?;
    Ok(Keychain { store })
}

#[cfg(all(feature = "keychain", target_os = "windows"))]
fn open_platform_store() -> Result<Keychain, CredentialError> {
    let store: Arc<keyring_core::CredentialStore> =
        windows_native_keyring_store::Store::new().map_err(backend_error)?;
    Ok(Keychain { store })
}

#[cfg(all(
    feature = "keychain",
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
fn open_platform_store() -> Result<Keychain, CredentialError> {
    let store: Arc<keyring_core::CredentialStore> = zbus_secret_service_keyring_store::Store::new()
        .map_err(|e| {
            CredentialError::Unavailable(format!(
                "no Secret Service (GNOME Keyring/KWallet) on the session D-Bus ({})",
                backend_error(e)
            ))
        })?;
    Ok(Keychain { store })
}

#[cfg(not(all(
    feature = "keychain",
    any(
        target_os = "macos",
        target_os = "windows",
        all(unix, not(any(target_os = "ios", target_os = "android")))
    )
)))]
fn open_platform_store() -> Result<Keychain, CredentialError> {
    Err(CredentialError::Unavailable(
        "KGet was built without the `keychain` feature".into(),
    ))
}

/// Whether KGet was built with a platform keychain backend.
pub fn compiled_in() -> bool {
    cfg!(feature = "keychain")
}

/// One-line description of the system keychain for `--doctor`.
pub fn status() -> String {
    match Keychain::system() {
        Ok(k) => format!("available ({})", k.backend_name()),
        Err(e) => e.to_string(),
    }
}

/// Save a secret in the system keychain.
pub fn store(target: &Target, secret: &str) -> Result<(), CredentialError> {
    Keychain::system()?.store(target, secret)
}

/// Read a secret from the system keychain.
pub fn get(target: &Target) -> Result<Option<String>, CredentialError> {
    Keychain::system()?.get(target)
}

/// Delete a secret from the system keychain.
pub fn delete(target: &Target) -> Result<bool, CredentialError> {
    Keychain::system()?.delete(target)
}

/// List the targets KGet saved in the system keychain.
pub fn list() -> Result<Vec<Target>, CredentialError> {
    Keychain::system()?.list()
}

// ────────────────────────────────────────────────────────────
// Proxy password
// ────────────────────────────────────────────────────────────

/// Keychain target of an authenticated proxy (`http://user@proxy:3128`).
pub fn proxy_target(proxy: &ProxyConfig) -> Option<Target> {
    let user = proxy.username.as_deref().filter(|u| !u.is_empty())?;
    let raw = proxy.url.as_deref()?.trim();
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        let scheme = match proxy.proxy_type {
            crate::config::ProxyType::Http => "http",
            crate::config::ProxyType::Https => "https",
            crate::config::ProxyType::Socks5 => "socks5",
        };
        format!("{scheme}://{raw}")
    };
    let mut url = url::Url::parse(&with_scheme).ok()?;
    url.set_username(user).ok()?;
    let _ = url.set_password(None);
    Target::from_url(url.as_str())
}

/// Fill a missing proxy password from `keychain`. Returns `true` if one was found.
pub fn fill_proxy_password_from(proxy: &mut ProxyConfig, keychain: &Keychain) -> bool {
    if !proxy.enabled || proxy.password.is_some() {
        return false;
    }
    let Some(target) = proxy_target(proxy) else {
        return false;
    };
    match keychain.get(&target) {
        Ok(Some(secret)) => {
            proxy.password = Some(secret);
            true
        }
        _ => false,
    }
}

/// Fill a missing proxy password from the system keychain (silently does
/// nothing when it is unavailable).
pub fn fill_proxy_password(proxy: &mut ProxyConfig) -> bool {
    if !proxy.enabled || proxy.password.is_some() || proxy_target(proxy).is_none() {
        return false;
    }
    match Keychain::system() {
        Ok(k) => fill_proxy_password_from(proxy, &k),
        Err(_) => false,
    }
}

/// Save `proxy.password` in `keychain`; returns the target it was saved under.
pub fn store_proxy_password_in(
    proxy: &ProxyConfig,
    keychain: &Keychain,
) -> Result<Target, CredentialError> {
    let target = proxy_target(proxy).ok_or_else(|| {
        CredentialError::InvalidTarget("the proxy needs a URL and a user name".into())
    })?;
    let secret = proxy
        .password
        .as_deref()
        .ok_or_else(|| CredentialError::InvalidTarget("no proxy password given".into()))?;
    keychain.store(&target, secret)?;
    Ok(target)
}

/// Save `proxy.password` in the system keychain.
pub fn store_proxy_password(proxy: &ProxyConfig) -> Result<Target, CredentialError> {
    store_proxy_password_in(proxy, &Keychain::system()?)
}

// ────────────────────────────────────────────────────────────
// Resolver: keychain → prompt → offer to save
// ────────────────────────────────────────────────────────────

/// When credentials obtained during a download are saved to the keychain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SavePolicy {
    /// Never save.
    #[default]
    Never,
    /// Ask on the terminal after a prompted password was accepted.
    Ask,
    /// Save every accepted password, including one given in the URL
    /// (`--save-credentials`).
    Always,
}

/// Reads a secret; receives the prompt text, returns `None` when cancelled.
pub type PromptFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;
/// Answers a yes/no question.
pub type ConfirmFn = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Prompt,
    Url,
}

struct Pending {
    target: Target,
    secret: String,
    origin: Origin,
    accepted: bool,
}

enum Backend {
    System,
    Fixed(Keychain),
    Disabled,
}

/// Finds credentials for one download: the URL, then the keychain, then (on
/// a terminal) a prompt. Accepted secrets can be saved afterwards with
/// [`finish`](Self::finish).
///
/// Protocol downloaders take an `Arc<CredentialResolver>` through their
/// `with_credentials` method; without one they only use the URL.
pub struct CredentialResolver {
    backend: Backend,
    prompt: Option<PromptFn>,
    confirm: Option<ConfirmFn>,
    save: SavePolicy,
    quiet: bool,
    pending: Mutex<Vec<Pending>>,
}

impl fmt::Debug for CredentialResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialResolver")
            .field("interactive", &self.prompt.is_some())
            .field("save", &self.save)
            .finish()
    }
}

/// Prompts from parallel downloads must not interleave.
static PROMPT_LOCK: Mutex<()> = Mutex::new(());
/// "Keychain unavailable" is reported once per process.
static WARNED: AtomicBool = AtomicBool::new(false);

impl CredentialResolver {
    /// Use the system keychain, without prompting and without saving.
    pub fn system() -> Self {
        Self::with_backend(Backend::System)
    }

    /// Use the given store (e.g. [`Keychain::mock`] in tests).
    pub fn with_keychain(keychain: Keychain) -> Self {
        Self::with_backend(Backend::Fixed(keychain))
    }

    /// No keychain at all: only the URL and prompts.
    pub fn without_keychain() -> Self {
        Self::with_backend(Backend::Disabled)
    }

    fn with_backend(backend: Backend) -> Self {
        Self {
            backend,
            prompt: None,
            confirm: None,
            save: SavePolicy::Never,
            quiet: false,
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Prompt on the terminal for missing passwords (only if stdin and
    /// stderr are terminals) and ask before saving them.
    pub fn interactive(mut self, enabled: bool) -> Self {
        if enabled && std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
            self.prompt = Some(Arc::new(tty_prompt));
            self.confirm = Some(Arc::new(tty_confirm));
            if self.save == SavePolicy::Never {
                self.save = SavePolicy::Ask;
            }
        }
        self
    }

    /// Custom secret prompt (GUI dialogs, tests).
    pub fn with_prompt(mut self, prompt: PromptFn) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// Custom yes/no question used by [`SavePolicy::Ask`].
    pub fn with_confirm(mut self, confirm: ConfirmFn) -> Self {
        self.confirm = Some(confirm);
        self
    }

    /// When accepted secrets are saved.
    pub fn save_policy(mut self, save: SavePolicy) -> Self {
        self.save = save;
        self
    }

    /// Do not print notices.
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Whether a prompt is available.
    pub fn can_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    fn keychain(&self) -> Option<Keychain> {
        match &self.backend {
            Backend::Fixed(k) => Some(k.clone()),
            Backend::Disabled => None,
            Backend::System => match Keychain::system() {
                Ok(k) => Some(k),
                Err(e) => {
                    if !self.quiet && compiled_in() && !WARNED.swap(true, Ordering::Relaxed) {
                        eprintln!("Note: {e}; saved credentials are not used.");
                    }
                    None
                }
            },
        }
    }

    /// The secret saved for `target` in the keychain.
    pub fn lookup(&self, target: &Target) -> Option<String> {
        match self.keychain()?.get(target) {
            Ok(secret) => secret,
            Err(e) => {
                if !self.quiet {
                    eprintln!("Note: could not read the saved credential for {target}: {e}");
                }
                None
            }
        }
    }

    /// Ask for the secret of `target` (`what` is e.g. "Password"); `None`
    /// when not interactive or cancelled.
    pub fn prompt(&self, target: &Target, what: &str) -> Option<String> {
        let prompt = self.prompt.as_ref()?;
        let secret = {
            let _guard = PROMPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            prompt(&format!("{what} for {target}: "))?
        };
        if secret.is_empty() {
            return None;
        }
        self.remember(target, &secret, Origin::Prompt);
        Some(secret)
    }

    /// Keychain first, then a prompt.
    pub fn resolve(&self, target: &Target, what: &str) -> Option<String> {
        self.lookup(target).or_else(|| self.prompt(target, what))
    }

    /// Record a secret that came with the URL (saved only with
    /// [`SavePolicy::Always`]).
    pub fn provided(&self, target: &Target, secret: &str) {
        self.remember(target, secret, Origin::Url);
    }

    fn remember(&self, target: &Target, secret: &str, origin: Origin) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|p| &p.target != target);
        pending.push(Pending {
            target: target.clone(),
            secret: secret.to_string(),
            origin,
            accepted: false,
        });
    }

    /// The server accepted the secret last recorded for `target`.
    pub fn accepted(&self, target: &Target) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        for p in pending.iter_mut().filter(|p| &p.target == target) {
            p.accepted = true;
        }
    }

    /// Save accepted secrets according to the [`SavePolicy`]; returns the
    /// targets that were saved. Call it once the download is over, so the
    /// question does not interrupt a progress bar.
    pub fn finish(&self) -> Vec<Target> {
        let pending: Vec<Pending> = {
            let mut guard = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *guard)
        };
        let mut saved = Vec::new();
        for p in pending.into_iter().filter(|p| p.accepted) {
            let wanted = match self.save {
                SavePolicy::Never => false,
                SavePolicy::Always => true,
                SavePolicy::Ask => {
                    p.origin == Origin::Prompt
                        && self.confirm.as_ref().is_some_and(|ask| {
                            let _guard = PROMPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                            ask(&format!(
                                "Save the password for {} in the system keychain? [y/N] ",
                                p.target
                            ))
                        })
                }
            };
            if !wanted {
                continue;
            }
            let Some(keychain) = self.keychain() else {
                if !self.quiet {
                    eprintln!("Credentials for {} were not saved.", p.target);
                }
                continue;
            };
            if keychain.get(&p.target).ok().flatten().as_deref() == Some(p.secret.as_str()) {
                continue;
            }
            match keychain.store(&p.target, &p.secret) {
                Ok(()) => {
                    if !self.quiet {
                        eprintln!("Saved credentials for {} in the system keychain.", p.target);
                    }
                    saved.push(p.target);
                }
                Err(e) => {
                    if !self.quiet {
                        eprintln!("Credentials for {} were not saved: {e}", p.target);
                    }
                }
            }
        }
        saved
    }
}

fn tty_prompt(text: &str) -> Option<String> {
    rpassword::prompt_password(text).ok()
}

fn tty_confirm(text: &str) -> bool {
    eprint!("{text}");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Read a secret for `kget --credentials add`: hidden prompt on a terminal,
/// otherwise the first line of stdin.
pub fn read_secret(prompt: &str) -> Option<String> {
    let secret = if std::io::stdin().is_terminal() {
        rpassword::prompt_password(prompt).ok()?
    } else {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).ok()?;
        line.trim_end_matches(['\r', '\n']).to_string()
    };
    (!secret.is_empty()).then_some(secret)
}

/// Usage of the credential management commands (CLI and REPL).
pub const COMMAND_USAGE: &str = "\
credentials list                 Saved targets (passwords are never shown)
credentials add <target>         Save a password (prompted; read from stdin when piped)
credentials delete <target>      Remove a saved password
credentials status               Which keychain is used

<target> is scheme://user@host[:port] (sftp, ftp, webdav, webdavs, or a proxy
URL such as http://user@proxy:3128), ssh-key:///path/to/key for a key passphrase,
or hf://token@huggingface.co for a Hugging Face access token.";

/// Run a credential management command (`list`, `add`, `delete`, `status`)
/// against the system keychain and return the text to print.
pub fn command(args: &[&str]) -> Result<String, CredentialError> {
    command_with(args, Keychain::system(), read_secret)
}

fn command_with(
    args: &[&str],
    keychain: Result<Keychain, CredentialError>,
    read: impl Fn(&str) -> Option<String>,
) -> Result<String, CredentialError> {
    let usage = || CredentialError::InvalidTarget(format!("usage:\n{COMMAND_USAGE}"));
    match args {
        [] | ["status"] => Ok(match keychain {
            Ok(k) => format!("System keychain: {}", k.backend_name()),
            Err(e) => format!("System keychain: {e}"),
        }),
        ["list" | "ls"] => {
            let keychain = keychain?;
            let targets = keychain.list()?;
            if targets.is_empty() {
                return Ok(format!(
                    "No saved credentials ({}).",
                    keychain.backend_name()
                ));
            }
            let mut out = format!("Saved credentials ({}):", keychain.backend_name());
            for t in targets {
                out.push_str("\n  ");
                out.push_str(t.as_str());
            }
            Ok(out)
        }
        ["delete" | "remove" | "rm", target] => {
            let target = Target::parse(target)?;
            if keychain?.delete(&target)? {
                Ok(format!("Deleted the saved password for {target}."))
            } else {
                Ok(format!("No saved password for {target}."))
            }
        }
        ["add" | "set" | "save", target] => {
            let target = Target::parse(target)?;
            let keychain = keychain?;
            let secret = read(&format!("Password for {target}: ")).ok_or_else(|| {
                CredentialError::InvalidTarget("no password given; nothing saved".into())
            })?;
            keychain.store(&target, &secret)?;
            Ok(format!(
                "Saved the password for {target} in the system keychain."
            ))
        }
        _ => Err(usage()),
    }
}

/// Whether an OpenSSH/PEM private key file is passphrase-protected.
pub fn ssh_key_is_encrypted(path: &Path) -> bool {
    // "openssh-key-v1\0" + cipher "none", base64-encoded.
    const OPENSSH_UNENCRYPTED: &str = "b3BlbnNzaC1rZXktdjEAAAAABG5vbmU";
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    if text.contains("ENCRYPTED") {
        return true;
    }
    if text.contains("BEGIN OPENSSH PRIVATE KEY") {
        let body: String = text
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<Vec<_>>()
            .concat();
        return !body.starts_with(OPENSSH_UNENCRYPTED);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProxyType;

    fn t(s: &str) -> Target {
        Target::parse(s).unwrap()
    }

    #[test]
    fn targets_from_urls() {
        assert_eq!(
            Target::from_url("sftp://alice:pw@Example.COM/data/x.bin")
                .unwrap()
                .as_str(),
            "sftp://alice@example.com"
        );
        assert_eq!(
            Target::from_url("sftp://alice@host:22/x").unwrap().as_str(),
            "sftp://alice@host"
        );
        assert_eq!(
            Target::from_url("ftp://bob@host:2121/x").unwrap().as_str(),
            "ftp://bob@host:2121"
        );
        assert_eq!(
            Target::from_url("webdavs://u%40corp@dav.example.com/f")
                .unwrap()
                .as_str(),
            "webdavs://u%40corp@dav.example.com"
        );
        assert!(Target::from_url("ftp://host/file").is_none());
        assert!(Target::from_url("not a url").is_none());
    }

    #[test]
    fn parse_rejects_bad_targets_without_leaking() {
        assert!(Target::parse("host").is_err());
        let err = Target::parse("sftp://host/x").unwrap_err().to_string();
        assert!(err.contains("sftp://host"));
        assert_eq!(
            t("ssh-key:///home/a/.ssh/id_ed25519").as_str(),
            "ssh-key:///home/a/.ssh/id_ed25519"
        );
        assert!(Target::parse("ssh-key://").is_err());
        // A password typed by mistake is never part of the target.
        assert_eq!(t("ftp://bob:hunter2@host").as_str(), "ftp://bob@host");
    }

    #[test]
    fn mock_store_round_trip_and_list() {
        let k = Keychain::mock();
        let a = t("sftp://alice@host");
        let b = t("ftp://bob@files:2121");
        assert_eq!(k.get(&a).unwrap(), None);
        k.store(&a, "one").unwrap();
        k.store(&b, "two").unwrap();
        k.store(&a, "uno").unwrap();
        assert_eq!(k.get(&a).unwrap().as_deref(), Some("uno"));
        assert_eq!(k.list().unwrap(), vec![b.clone(), a.clone()]);
        assert!(k.delete(&a).unwrap());
        assert!(!k.delete(&a).unwrap());
        assert_eq!(k.list().unwrap(), vec![b.clone()]);
        assert!(k.delete(&b).unwrap());
        assert!(k.list().unwrap().is_empty());
    }

    #[test]
    fn mock_stores_are_isolated() {
        let a = t("sftp://alice@host");
        let k1 = Keychain::mock();
        k1.store(&a, "x").unwrap();
        assert_eq!(Keychain::mock().get(&a).unwrap(), None);
        assert_eq!(Keychain::mock().backend_name(), "in-memory (mock)");
    }

    #[test]
    fn backend_errors_are_reported() {
        let k = Keychain::mock();
        let a = t("sftp://alice@host");
        k.store(&a, "x").unwrap();
        let entry = k.entry(a.as_str()).unwrap();
        let mock: &keyring_core::mock::Cred = entry.as_any().downcast_ref().unwrap();
        mock.set_error(keyring_core::Error::BadEncoding(b"secret-bytes".to_vec()));
        let err = k.get(&a).unwrap_err().to_string();
        assert!(!err.contains("secret-bytes"), "{err}");
        // The error is consumed; the value is intact.
        assert_eq!(k.get(&a).unwrap().as_deref(), Some("x"));
    }

    #[test]
    fn proxy_password_round_trip() {
        let k = Keychain::mock();
        let mut proxy = ProxyConfig {
            enabled: true,
            url: Some("proxy.local:3128".into()),
            username: Some("pu".into()),
            password: Some("pp".into()),
            proxy_type: ProxyType::Http,
        };
        let target = store_proxy_password_in(&proxy, &k).unwrap();
        assert_eq!(target.as_str(), "http://pu@proxy.local:3128");

        proxy.password = None;
        assert!(fill_proxy_password_from(&mut proxy, &k));
        assert_eq!(proxy.password.as_deref(), Some("pp"));

        let socks = ProxyConfig {
            enabled: true,
            url: Some("socks5://10.0.0.1:1080".into()),
            username: Some("s".into()),
            password: None,
            proxy_type: ProxyType::Socks5,
        };
        assert_eq!(
            proxy_target(&socks).unwrap().as_str(),
            "socks5://s@10.0.0.1:1080"
        );
        let mut disabled = ProxyConfig {
            enabled: false,
            ..proxy.clone()
        };
        disabled.password = None;
        assert!(!fill_proxy_password_from(&mut disabled, &k));
        assert!(proxy_target(&ProxyConfig::default()).is_none());
    }

    fn answering(secret: &'static str) -> PromptFn {
        Arc::new(move |_| Some(secret.to_string()))
    }

    #[test]
    fn resolver_prefers_keychain_over_prompt() {
        let k = Keychain::mock();
        let a = t("ftp://bob@host");
        k.store(&a, "saved").unwrap();
        let r = CredentialResolver::with_keychain(k).with_prompt(Arc::new(|_| {
            panic!("must not prompt when the keychain has the secret")
        }));
        assert_eq!(r.resolve(&a, "Password").as_deref(), Some("saved"));
        assert!(r.finish().is_empty());
    }

    #[test]
    fn resolver_asks_before_saving_prompted_secret() {
        let k = Keychain::mock();
        let a = t("sftp://alice@host");
        let asked = Arc::new(AtomicBool::new(false));
        let flag = asked.clone();
        let r = CredentialResolver::with_keychain(k.clone())
            .with_prompt(answering("typed"))
            .with_confirm(Arc::new(move |q| {
                assert!(q.contains("sftp://alice@host") && !q.contains("typed"));
                flag.store(true, Ordering::SeqCst);
                true
            }))
            .save_policy(SavePolicy::Ask)
            .quiet(true);
        assert_eq!(r.resolve(&a, "Password").as_deref(), Some("typed"));
        r.accepted(&a);
        assert_eq!(r.finish(), vec![a.clone()]);
        assert!(asked.load(Ordering::SeqCst));
        assert_eq!(k.get(&a).unwrap().as_deref(), Some("typed"));
    }

    #[test]
    fn resolver_does_not_save_rejected_or_declined_secrets() {
        let k = Keychain::mock();
        let a = t("sftp://alice@host");
        let r = CredentialResolver::with_keychain(k.clone())
            .with_prompt(answering("wrong"))
            .with_confirm(Arc::new(|_| true))
            .save_policy(SavePolicy::Ask)
            .quiet(true);
        r.prompt(&a, "Password");
        // Never accepted by the server.
        assert!(r.finish().is_empty());

        let r = CredentialResolver::with_keychain(k.clone())
            .with_prompt(answering("right"))
            .with_confirm(Arc::new(|_| false))
            .save_policy(SavePolicy::Ask)
            .quiet(true);
        r.prompt(&a, "Password");
        r.accepted(&a);
        assert!(r.finish().is_empty());
        assert_eq!(k.get(&a).unwrap(), None);
    }

    #[test]
    fn url_secrets_saved_only_with_always() {
        let k = Keychain::mock();
        let a = t("webdav://u@dav");
        let r = CredentialResolver::with_keychain(k.clone())
            .with_confirm(Arc::new(|_| true))
            .save_policy(SavePolicy::Ask)
            .quiet(true);
        r.provided(&a, "fromurl");
        r.accepted(&a);
        assert!(r.finish().is_empty());

        let r = CredentialResolver::with_keychain(k.clone())
            .save_policy(SavePolicy::Always)
            .quiet(true);
        r.provided(&a, "fromurl");
        r.accepted(&a);
        assert_eq!(r.finish(), vec![a.clone()]);
        assert_eq!(k.get(&a).unwrap().as_deref(), Some("fromurl"));
    }

    #[test]
    fn non_interactive_resolver_never_prompts() {
        let r = CredentialResolver::without_keychain().quiet(true);
        let a = t("ftp://bob@host");
        assert!(!r.can_prompt());
        assert_eq!(r.resolve(&a, "Password"), None);
        r.provided(&a, "x");
        r.accepted(&a);
        assert!(r.finish().is_empty());
    }

    #[test]
    fn management_commands() {
        let k = Keychain::mock();
        let run = |args: &[&str]| command_with(args, Ok(k.clone()), |_| Some("pw".into()));
        assert!(run(&["list"]).unwrap().starts_with("No saved credentials"));
        let out = run(&["add", "sftp://alice:typo@host/path"]).unwrap();
        assert!(out.contains("sftp://alice@host") && !out.contains("typo"));
        assert_eq!(
            k.get(&t("sftp://alice@host")).unwrap().as_deref(),
            Some("pw")
        );
        let listed = run(&["list"]).unwrap();
        assert!(listed.contains("sftp://alice@host") && !listed.contains("pw\n"));
        assert!(
            run(&["delete", "sftp://alice@host"])
                .unwrap()
                .starts_with("Deleted")
        );
        assert!(
            run(&["delete", "sftp://alice@host"])
                .unwrap()
                .starts_with("No saved")
        );
        assert!(run(&["frobnicate"]).is_err());
        assert!(run(&["delete", "nonsense"]).is_err());
        assert!(run(&["status"]).unwrap().contains("mock"));

        let none = command_with(&["add", "ftp://b@h"], Ok(k.clone()), |_| None);
        assert!(none.is_err());
        let off = CredentialError::Unavailable("off".into());
        assert!(command_with(&["list"], Err(off.clone()), |_| None).is_err());
        assert!(
            command_with(&["status"], Err(off), |_| None)
                .unwrap()
                .contains("unavailable")
        );
    }

    #[test]
    fn detects_encrypted_ssh_keys() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain");
        std::fs::write(
            &plain,
            "-----BEGIN OPENSSH PRIVATE KEY-----\n\
             b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
             -----END OPENSSH PRIVATE KEY-----\n",
        )
        .unwrap();
        let enc = dir.path().join("enc");
        std::fs::write(
            &enc,
            "-----BEGIN OPENSSH PRIVATE KEY-----\n\
             b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABD\n\
             -----END OPENSSH PRIVATE KEY-----\n",
        )
        .unwrap();
        let pem = dir.path().join("pem");
        std::fs::write(
            &pem,
            "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nabc\n-----END RSA PRIVATE KEY-----\n",
        )
        .unwrap();
        assert!(!ssh_key_is_encrypted(&plain));
        assert!(ssh_key_is_encrypted(&enc));
        assert!(ssh_key_is_encrypted(&pem));
        assert!(!ssh_key_is_encrypted(&dir.path().join("missing")));
    }
}
