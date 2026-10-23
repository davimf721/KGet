//! `kget native-host install|uninstall`: register the host with browsers.
//!
//! A browser starts a native-messaging host from a manifest named
//! `com.kget.native_host.json`. Its `path` must be an executable that takes
//! no extra arguments, so the manifest points to a small launcher script
//! (`kget-native-host` / `kget-native-host.bat`) that runs
//! `<absolute path of kget> native-host "$@"`.
//!
//! | OS | Browser | Manifest |
//! |----|---------|----------|
//! | macOS | Chrome | `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/` |
//! | macOS | Chromium | `~/Library/Application Support/Chromium/NativeMessagingHosts/` |
//! | macOS | Edge | `~/Library/Application Support/Microsoft Edge/NativeMessagingHosts/` |
//! | macOS | Brave | `~/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/` |
//! | macOS | Firefox | `~/Library/Application Support/Mozilla/NativeMessagingHosts/` |
//! | Linux | Chrome / Chromium / Edge / Brave | `~/.config/{google-chrome,chromium,microsoft-edge,BraveSoftware/Brave-Browser}/NativeMessagingHosts/` |
//! | Linux | Firefox | `~/.mozilla/native-messaging-hosts/` |
//! | Windows | all | `%APPDATA%\kget\native-host\<browser>\` plus a `HKCU\Software\…\NativeMessagingHosts\com.kget.native_host` registry key (written with `reg.exe`) |
//!
//! The launcher and `allowed_origins.json` (the extension origins the host
//! accepts, see [`super::check_caller`]) live in `<config dir>/native-host/`.
//! `KGET_NATIVE_HOST_DIR` replaces every location: manifests go to
//! `$KGET_NATIVE_HOST_DIR/<browser>/`, the launcher next to them, and the
//! registry is not touched.

use serde_json::{Value, json};
use std::io;
use std::path::{Path, PathBuf};

/// Name of the native-messaging host.
pub const HOST_NAME: &str = "com.kget.native_host";

/// ID of the extension in `browser-extension/` when loaded unpacked in a
/// Chromium browser (derived from the `key` in its `manifest.json`).
pub const DEFAULT_CHROME_EXTENSION_ID: &str = "eccghfaajdkmbbbccabhfnhhbllfmlkg";

/// ID of the extension in Firefox (`browser_specific_settings.gecko.id`).
pub const DEFAULT_FIREFOX_EXTENSION_ID: &str = "kget@davimf721.github.io";

/// File listing the origins the host accepts.
pub const ALLOWLIST_FILE: &str = "allowed_origins.json";

/// Environment variable that redirects every install location (tests).
pub const DIR_ENV: &str = "KGET_NATIVE_HOST_DIR";

/// A browser the host can be registered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Browser {
    Chrome,
    Chromium,
    Edge,
    Brave,
    Firefox,
}

impl Browser {
    /// Every supported browser.
    pub const ALL: [Browser; 5] = [
        Browser::Chrome,
        Browser::Chromium,
        Browser::Edge,
        Browser::Brave,
        Browser::Firefox,
    ];

    /// Lowercase name (`chrome`, `firefox`…), also the directory name under
    /// `KGET_NATIVE_HOST_DIR`.
    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "chrome",
            Browser::Chromium => "chromium",
            Browser::Edge => "edge",
            Browser::Brave => "brave",
            Browser::Firefox => "firefox",
        }
    }

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Browser::Chrome => "Chrome",
            Browser::Chromium => "Chromium",
            Browser::Edge => "Edge",
            Browser::Brave => "Brave",
            Browser::Firefox => "Firefox",
        }
    }

    /// Parse `chrome`, `chromium`, `edge`, `brave`, `firefox` or `all`.
    pub fn parse_list(s: &str) -> Option<Vec<Browser>> {
        let s = s.trim().to_ascii_lowercase();
        if s == "all" {
            return Some(Self::ALL.to_vec());
        }
        Self::ALL.iter().find(|b| b.name() == s).map(|b| vec![*b])
    }

    /// Firefox manifests use `allowed_extensions` instead of `allowed_origins`.
    pub fn is_firefox(self) -> bool {
        self == Browser::Firefox
    }

    /// `HKCU` registry key read by this browser on Windows.
    pub fn registry_key(self) -> String {
        let vendor = match self {
            Browser::Chrome => r"Google\Chrome",
            Browser::Chromium => "Chromium",
            Browser::Edge => r"Microsoft\Edge",
            Browser::Brave => r"BraveSoftware\Brave-Browser",
            Browser::Firefox => "Mozilla",
        };
        format!(r"HKCU\Software\{vendor}\NativeMessagingHosts\{HOST_NAME}")
    }
}

/// Operating system whose layout is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    /// The OS this binary runs on (other Unixes use the Linux layout).
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Where the host's files go.
#[derive(Debug, Clone)]
pub struct Layout {
    /// OS conventions to follow
    pub os: Os,
    /// Home directory
    pub home: PathBuf,
    /// `~/.config` on Linux (`$XDG_CONFIG_HOME`)
    pub config_home: PathBuf,
    /// Directory of the launcher and the allowlist
    pub support_dir: PathBuf,
    /// Set from `KGET_NATIVE_HOST_DIR`: manifests go to
    /// `<override>/<browser>/` and the registry is left alone
    pub override_dir: Option<PathBuf>,
}

impl Layout {
    /// The layout for this user, honouring `KGET_NATIVE_HOST_DIR`.
    pub fn resolve() -> io::Result<Self> {
        let os = Os::current();
        if let Some(dir) = std::env::var_os(DIR_ENV).filter(|d| !d.is_empty()) {
            let dir = std::path::absolute(PathBuf::from(dir))?;
            return Ok(Self {
                os,
                home: dir.clone(),
                config_home: dir.clone(),
                support_dir: dir.clone(),
                override_dir: Some(dir),
            });
        }
        let home = dirs::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No home directory"))?;
        let config_home = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        Ok(Self {
            os,
            support_dir: crate::config::config_dir()?.join("native-host"),
            home,
            config_home,
            override_dir: None,
        })
    }

    /// Directory of `browser`'s manifest.
    pub fn manifest_dir(&self, browser: Browser) -> PathBuf {
        if let Some(dir) = &self.override_dir {
            return dir.join(browser.name());
        }
        match self.os {
            Os::MacOs => {
                let support = self.home.join("Library").join("Application Support");
                let vendor = match browser {
                    Browser::Chrome => "Google/Chrome",
                    Browser::Chromium => "Chromium",
                    Browser::Edge => "Microsoft Edge",
                    Browser::Brave => "BraveSoftware/Brave-Browser",
                    Browser::Firefox => "Mozilla",
                };
                support.join(vendor).join("NativeMessagingHosts")
            }
            Os::Linux => match browser {
                Browser::Firefox => self.home.join(".mozilla").join("native-messaging-hosts"),
                _ => {
                    let vendor = match browser {
                        Browser::Chrome => "google-chrome",
                        Browser::Chromium => "chromium",
                        Browser::Edge => "microsoft-edge",
                        _ => "BraveSoftware/Brave-Browser",
                    };
                    self.config_home.join(vendor).join("NativeMessagingHosts")
                }
            },
            Os::Windows => self.support_dir.join(browser.name()),
        }
    }

    /// Full path of `browser`'s manifest.
    pub fn manifest_path(&self, browser: Browser) -> PathBuf {
        self.manifest_dir(browser).join(format!("{HOST_NAME}.json"))
    }

    /// Path of the launcher script.
    pub fn launcher_path(&self) -> PathBuf {
        let name = if self.os == Os::Windows {
            "kget-native-host.bat"
        } else {
            "kget-native-host"
        };
        self.support_dir.join(name)
    }

    /// Path of the allowlist.
    pub fn allowlist_path(&self) -> PathBuf {
        self.support_dir.join(ALLOWLIST_FILE)
    }

    /// Whether the browser seems installed (its profile directory exists).
    /// Always true on Windows and with an override.
    fn browser_present(&self, browser: Browser) -> bool {
        if self.override_dir.is_some() || self.os == Os::Windows {
            return true;
        }
        self.manifest_dir(browser)
            .parent()
            .is_some_and(|vendor| vendor.is_dir())
    }

    /// Whether to write registry keys.
    fn uses_registry(&self) -> bool {
        self.os == Os::Windows && self.override_dir.is_none()
    }
}

/// Whether `id` looks like a Chromium extension ID (32 letters `a`–`p`).
pub fn is_chrome_extension_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Whether `id` looks like a Firefox add-on ID (`name@domain` or `{uuid}`).
pub fn is_firefox_extension_id(id: &str) -> bool {
    if id.len() > 80 {
        return false;
    }
    if let Some(uuid) = id.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        return uuid.len() == 36 && uuid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    }
    match id.split_once('@') {
        Some((name, domain)) => {
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
                && !domain.is_empty()
                && domain
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        }
        None => false,
    }
}

/// `chrome-extension://<id>/`
pub fn chrome_origin(id: &str) -> String {
    format!("chrome-extension://{id}/")
}

/// The native-messaging manifest for `browser`.
pub fn manifest_json(browser: Browser, launcher: &Path, ids: &[String]) -> Value {
    let mut m = json!({
        "name": HOST_NAME,
        "description": "KGet download manager",
        "path": launcher.to_string_lossy(),
        "type": "stdio",
    });
    if browser.is_firefox() {
        let ids: Vec<&String> = ids.iter().filter(|i| is_firefox_extension_id(i)).collect();
        m["allowed_extensions"] = json!(ids);
    } else {
        let origins: Vec<String> = ids
            .iter()
            .filter(|i| is_chrome_extension_id(i))
            .map(|i| chrome_origin(i))
            .collect();
        m["allowed_origins"] = json!(origins);
    }
    m
}

/// The launcher script running `exe native-host` with the browser's
/// arguments.
pub fn launcher_script(os: Os, exe: &Path) -> String {
    let exe = exe.to_string_lossy();
    if os == Os::Windows {
        // `%` must be doubled in a batch file; paths cannot contain `"`.
        format!(
            "@echo off\r\nrem KGet native messaging launcher (kget native-host install)\r\n\"{}\" native-host %*\r\n",
            exe.replace('%', "%%")
        )
    } else {
        format!(
            "#!/bin/sh\n# KGet native messaging launcher (kget native-host install)\nexec '{}' native-host \"$@\"\n",
            exe.replace('\'', r"'\''")
        )
    }
}

/// The origins in the allowlist at `layout`, or the default extension IDs
/// when there is none.
pub fn load_allowlist(layout: &Layout) -> Vec<String> {
    std::fs::read(layout.allowlist_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_else(default_origins)
}

/// Origins of the extension in `browser-extension/`.
pub fn default_origins() -> Vec<String> {
    vec![
        chrome_origin(DEFAULT_CHROME_EXTENSION_ID),
        DEFAULT_FIREFOX_EXTENSION_ID.to_string(),
    ]
}

/// What to register.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Browsers to register with
    pub browsers: Vec<Browser>,
    /// Extra extension IDs (Chromium IDs and/or Firefox add-on IDs); the
    /// default IDs are always included
    pub extension_ids: Vec<String>,
    /// The kget executable the launcher runs
    pub exe: PathBuf,
    /// Skip browsers that do not seem installed (`--browser all`)
    pub only_present: bool,
}

/// Write the launcher, the allowlist and the manifests. Returns a line per
/// file or key written or skipped.
pub fn install(opts: &InstallOptions, layout: &Layout) -> io::Result<Vec<String>> {
    for id in &opts.extension_ids {
        if !is_chrome_extension_id(id) && !is_firefox_extension_id(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "'{id}' is neither a Chromium extension ID (32 letters a-p) nor a Firefox add-on ID (name@domain or {{uuid}})"
                ),
            ));
        }
    }
    let mut ids = vec![
        DEFAULT_CHROME_EXTENSION_ID.to_string(),
        DEFAULT_FIREFOX_EXTENSION_ID.to_string(),
    ];
    for id in &opts.extension_ids {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }

    let mut report = Vec::new();
    std::fs::create_dir_all(&layout.support_dir)?;
    let launcher = layout.launcher_path();
    crate::config::write_atomic(
        &launcher,
        launcher_script(layout.os, &opts.exe).as_bytes(),
        false,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))?;
    }
    report.push(format!("launcher: {}", launcher.display()));

    // Merge with origins allowed by earlier installs (other browsers or IDs).
    let mut origins = std::fs::read(layout.allowlist_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default();
    for id in &ids {
        let origin = if is_chrome_extension_id(id) {
            chrome_origin(id)
        } else {
            id.clone()
        };
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }
    let list = serde_json::to_vec_pretty(&origins).map_err(io::Error::other)?;
    crate::config::write_atomic(&layout.allowlist_path(), &list, false)?;
    report.push(format!(
        "allowed origins: {} ({})",
        origins.join(", "),
        layout.allowlist_path().display()
    ));

    for &browser in &opts.browsers {
        if opts.only_present && !layout.browser_present(browser) {
            report.push(format!("{}: skipped (not installed)", browser.label()));
            continue;
        }
        let path = layout.manifest_path(browser);
        let manifest = manifest_json(browser, &launcher, &ids);
        let text = serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?;
        crate::config::write_atomic(&path, &text, false)?;
        report.push(format!("{}: {}", browser.label(), path.display()));
        if layout.uses_registry() {
            let key = browser.registry_key();
            let path_str = path.to_string_lossy().into_owned();
            run_reg(&["add", &key, "/ve", "/t", "REG_SZ", "/d", &path_str, "/f"])?;
            report.push(format!("{}: registry {key}", browser.label()));
        }
    }
    Ok(report)
}

/// Remove the manifests (and registry keys) of `browsers`; with every
/// browser, also the launcher and the allowlist.
pub fn uninstall(browsers: &[Browser], layout: &Layout) -> io::Result<Vec<String>> {
    let mut report = Vec::new();
    for &browser in browsers {
        let path = layout.manifest_path(browser);
        match std::fs::remove_file(&path) {
            Ok(()) => report.push(format!("{}: removed {}", browser.label(), path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if layout.uses_registry() {
            let key = browser.registry_key();
            if run_reg(&["delete", &key, "/f"]).is_ok() {
                report.push(format!("{}: removed registry {key}", browser.label()));
            }
        }
    }
    if Browser::ALL.iter().all(|b| browsers.contains(b)) {
        for path in [layout.launcher_path(), layout.allowlist_path()] {
            if std::fs::remove_file(&path).is_ok() {
                report.push(format!("removed {}", path.display()));
            }
        }
    }
    if report.is_empty() {
        report.push("nothing to remove".into());
    }
    Ok(report)
}

/// Run `reg.exe` with `args`, failing on a non-zero exit status.
fn run_reg(args: &[&str]) -> io::Result<()> {
    let out = std::process::Command::new("reg")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "reg {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(os: Os) -> Layout {
        Layout {
            os,
            home: PathBuf::from("/home/u"),
            config_home: PathBuf::from("/home/u/.config"),
            support_dir: PathBuf::from("/cfg/kget/native-host"),
            override_dir: None,
        }
    }

    #[test]
    fn manifest_locations_per_os() {
        let mac = layout(Os::MacOs);
        assert_eq!(
            mac.manifest_path(Browser::Chrome),
            Path::new(
                "/home/u/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.kget.native_host.json"
            )
        );
        assert_eq!(
            mac.manifest_dir(Browser::Firefox),
            Path::new("/home/u/Library/Application Support/Mozilla/NativeMessagingHosts")
        );
        assert_eq!(
            mac.manifest_dir(Browser::Brave),
            Path::new(
                "/home/u/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts"
            )
        );
        let linux = layout(Os::Linux);
        assert_eq!(
            linux.manifest_dir(Browser::Edge),
            Path::new("/home/u/.config/microsoft-edge/NativeMessagingHosts")
        );
        assert_eq!(
            linux.manifest_dir(Browser::Firefox),
            Path::new("/home/u/.mozilla/native-messaging-hosts")
        );
        let win = layout(Os::Windows);
        assert_eq!(
            win.manifest_dir(Browser::Chromium),
            Path::new("/cfg/kget/native-host/chromium")
        );
        assert!(win.launcher_path().ends_with("kget-native-host.bat"));
        assert_eq!(
            Browser::Edge.registry_key(),
            r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.kget.native_host"
        );
    }

    #[test]
    fn extension_id_formats() {
        assert!(is_chrome_extension_id(DEFAULT_CHROME_EXTENSION_ID));
        assert!(!is_chrome_extension_id("eccghfaajdkmbbbccabhfnhhbllfmlkz"));
        assert!(!is_chrome_extension_id("short"));
        assert!(is_firefox_extension_id(DEFAULT_FIREFOX_EXTENSION_ID));
        assert!(is_firefox_extension_id(
            "{12345678-1234-1234-1234-123456789abc}"
        ));
        assert!(!is_firefox_extension_id("no-at-sign"));
        assert!(!is_firefox_extension_id("a@b/c"));
    }

    #[test]
    fn manifests_per_browser_family() {
        let ids = vec![
            DEFAULT_CHROME_EXTENSION_ID.to_string(),
            DEFAULT_FIREFOX_EXTENSION_ID.to_string(),
        ];
        let c = manifest_json(Browser::Chrome, Path::new("/x/kget-native-host"), &ids);
        assert_eq!(c["name"], HOST_NAME);
        assert_eq!(c["type"], "stdio");
        assert_eq!(
            c["allowed_origins"],
            json!([format!("chrome-extension://{DEFAULT_CHROME_EXTENSION_ID}/")])
        );
        assert!(c.get("allowed_extensions").is_none());
        let f = manifest_json(Browser::Firefox, Path::new("/x/kget-native-host"), &ids);
        assert_eq!(
            f["allowed_extensions"],
            json!([DEFAULT_FIREFOX_EXTENSION_ID])
        );
        assert!(f.get("allowed_origins").is_none());
    }

    #[test]
    fn launcher_quotes_the_path() {
        let sh = launcher_script(Os::Linux, Path::new("/opt/it's here/kget"));
        assert!(sh.contains(r#"exec '/opt/it'\''s here/kget' native-host "$@""#));
        let bat = launcher_script(Os::Windows, Path::new(r"C:\100%\kget.exe"));
        assert!(bat.contains(r#""C:\100%%\kget.exe" native-host %*"#));
    }
}
