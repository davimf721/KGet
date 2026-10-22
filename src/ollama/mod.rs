//! Ollama registry downloads (`ollama://`).
//!
//! Supported forms:
//!
//! - `ollama://llama3.2` / `ollama://llama3.2:3b` — official models
//!   (`registry.ollama.ai/library/…`, tag `latest` by default)
//! - `ollama://namespace/model:tag` — a user's model on the default registry
//! - `ollama://registry.example.com/namespace/model:tag` — another registry
//!
//! The manifest comes from `/v2/<namespace>/<model>/manifests/<tag>`; every
//! layer (and the config blob) is downloaded from `/v2/…/blobs/<digest>` with
//! the turbo engine and verified against its SHA-256 digest.
//!
//! By default the files are written into the Ollama models directory
//! (`$OLLAMA_MODELS` or `~/.ollama/models`: `blobs/sha256-<hex>` and
//! `manifests/<registry>/<namespace>/<model>/<tag>`), so `ollama run` finds
//! the model without downloading it again. With `--output <dir>` they are
//! saved as plain files instead (`model.gguf`, `template`, `params.json`,
//! `config.json`, `manifest.json`, …).
//!
//! `$KGET_OLLAMA_REGISTRY` replaces the base URL of the default registry
//! (`https://registry.ollama.ai`), e.g. for a mirror or tests.
//!
//! ```rust
//! use kget::ollama::OllamaRef;
//!
//! let r = OllamaRef::parse("ollama://llama3.2:3b").unwrap();
//! assert_eq!(r.registry, "registry.ollama.ai");
//! assert_eq!(r.namespace, "library");
//! assert_eq!(r.model, "llama3.2");
//! assert_eq!(r.tag, "3b");
//! ```

use crate::error::KgetError;
use crate::fetch::{self, FetchItem, ModelDownload, TransferOptions};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Registry of the official models.
pub const DEFAULT_REGISTRY: &str = "registry.ollama.ai";

/// Namespace of the official models.
pub const DEFAULT_NAMESPACE: &str = "library";

/// Tag used when the URL names none.
pub const DEFAULT_TAG: &str = "latest";

const MANIFEST_ACCEPT: &str = "application/vnd.docker.distribution.manifest.v2+json";

/// A parsed `ollama://` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaRef {
    /// Registry host (`registry.ollama.ai`)
    pub registry: String,
    /// Namespace (`library` for official models)
    pub namespace: String,
    /// Model name
    pub model: String,
    /// Tag (`latest` by default)
    pub tag: String,
}

fn valid_part(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl OllamaRef {
    /// Parse `ollama://[registry/][namespace/]model[:tag]`.
    pub fn parse(url: &str) -> Result<OllamaRef, KgetError> {
        let trimmed = url.trim();
        let invalid = |why: &str| {
            KgetError::Protocol(format!(
                "Invalid Ollama URL '{trimmed}': {why} (expected ollama://[namespace/]model[:tag])"
            ))
        };
        let rest = trimmed
            .get(..9)
            .filter(|s| s.eq_ignore_ascii_case("ollama://"))
            .map(|_| &trimmed[9..])
            .ok_or_else(|| invalid("not an ollama:// URL"))?;
        let rest = rest.trim_end_matches('/');
        let (name, tag) = match rest.rsplit_once(':') {
            // A ':' in the last path segment is the tag; otherwise it is a port.
            Some((n, t)) if !t.contains('/') => (n, t),
            _ => (rest, DEFAULT_TAG),
        };
        let segs: Vec<&str> = name.split('/').collect();
        let (registry, namespace, model) = match segs.as_slice() {
            [m] => (DEFAULT_REGISTRY, DEFAULT_NAMESPACE, *m),
            [ns, m] => (DEFAULT_REGISTRY, *ns, *m),
            [reg, ns, m] => (*reg, *ns, *m),
            _ => return Err(invalid("too many path segments")),
        };
        let reg_ok = !registry.is_empty()
            && registry
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'));
        if !reg_ok || !valid_part(namespace) || !valid_part(model) || !valid_part(tag) {
            return Err(invalid("bad name"));
        }
        Ok(OllamaRef {
            registry: registry.to_ascii_lowercase(),
            namespace: namespace.to_string(),
            model: model.to_string(),
            tag: tag.to_string(),
        })
    }

    /// `namespace/model:tag`, or `model:tag` for official models.
    pub fn display_name(&self) -> String {
        if self.namespace == DEFAULT_NAMESPACE && self.registry == DEFAULT_REGISTRY {
            format!("{}:{}", self.model, self.tag)
        } else {
            format!("{}/{}:{}", self.namespace, self.model, self.tag)
        }
    }

    /// Path of the manifest inside the models directory.
    pub fn manifest_path(&self, models_dir: &Path) -> PathBuf {
        models_dir
            .join("manifests")
            .join(&self.registry)
            .join(&self.namespace)
            .join(&self.model)
            .join(&self.tag)
    }
}

/// Whether `url` is an `ollama://` reference.
pub fn is_ollama_url(url: &str) -> bool {
    url.trim().to_ascii_lowercase().starts_with("ollama://")
}

/// Ollama models directory: `$OLLAMA_MODELS` or `~/.ollama/models`.
pub fn models_dir() -> PathBuf {
    std::env::var("OLLAMA_MODELS")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|p| PathBuf::from(crate::utils::expand_tilde(p.trim())))
        .or_else(|| dirs::home_dir().map(|h| h.join(".ollama").join("models")))
        .unwrap_or_else(|| PathBuf::from(".ollama/models"))
}

/// Base URL of the registry of `r` (`$KGET_OLLAMA_REGISTRY` for the default one).
pub fn registry_url(r: &OllamaRef) -> String {
    if r.registry == DEFAULT_REGISTRY
        && let Ok(v) = std::env::var("KGET_OLLAMA_REGISTRY")
        && !v.trim().is_empty()
    {
        return v.trim().trim_end_matches('/').to_string();
    }
    format!("https://{}", r.registry)
}

/// One blob referenced by a manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    /// Media type (`application/vnd.ollama.image.model`, …)
    #[serde(default)]
    pub media_type: String,
    /// `sha256:<hex>`
    pub digest: String,
    /// Size in bytes
    #[serde(default)]
    pub size: u64,
}

/// An image manifest (Docker distribution v2 schema).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    /// Model configuration blob
    pub config: Layer,
    /// Model weights, template, parameters, license…
    #[serde(default)]
    pub layers: Vec<Layer>,
}

impl Layer {
    /// Hex SHA-256 of a valid `sha256:<64 hex>` digest.
    pub fn sha256(&self) -> Option<&str> {
        self.digest
            .strip_prefix("sha256:")
            .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
    }

    /// Name of the blob in the models directory (`sha256-<hex>`).
    pub fn blob_name(&self) -> Option<String> {
        self.sha256()
            .map(|h| format!("sha256-{}", h.to_ascii_lowercase()))
    }

    /// File name in the plain `--output` layout.
    fn plain_name(&self) -> String {
        let kind = self
            .media_type
            .strip_prefix("application/vnd.ollama.image.")
            .unwrap_or("");
        match kind {
            "model" => "model.gguf".into(),
            "projector" => "projector.gguf".into(),
            "adapter" => "adapter.gguf".into(),
            "params" => "params.json".into(),
            "messages" => "messages.json".into(),
            "template" | "system" | "license" => kind.into(),
            _ if self.media_type.contains("container.image") => "config.json".into(),
            _ => self
                .sha256()
                .map(|h| format!("blob-{}", &h[..12]))
                .unwrap_or_else(|| "blob".into()),
        }
    }
}

/// Options of an Ollama download.
#[derive(Debug, Clone, Default)]
pub struct OllamaOptions {
    /// Base URL of the registry (`None` = [`registry_url`])
    pub registry_url: Option<String>,
    /// Save plain files into this directory instead of the models directory
    pub output: Option<String>,
    /// Models directory (`None` = [`models_dir()`])
    pub models_dir: Option<PathBuf>,
}

/// Fetch and parse the manifest of `r`; returns it with its raw bytes.
pub fn fetch_manifest(
    r: &OllamaRef,
    base: &str,
    transfer: &TransferOptions,
) -> Result<(Manifest, Vec<u8>), KgetError> {
    let client = crate::http::build_client(
        &transfer.proxy,
        &crate::http::ClientOptions::with_timeout_secs(transfer.timeout),
    )?;
    let url = format!("{base}/v2/{}/{}/manifests/{}", r.namespace, r.model, r.tag);
    let req = crate::http::apply_headers(client.get(&url), &transfer.headers)
        .header(reqwest::header::ACCEPT, MANIFEST_ACCEPT);
    let resp = req.send()?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err(KgetError::NotFound(format!(
            "model '{}' in {}",
            r.display_name(),
            r.registry
        )));
    }
    if !status.is_success() {
        return Err(KgetError::from_status(status.as_u16(), url));
    }
    let raw = resp.bytes()?.to_vec();
    let manifest: Manifest = serde_json::from_slice(&raw)
        .map_err(|e| KgetError::Protocol(format!("Invalid Ollama manifest: {e}")))?;
    Ok((manifest, raw))
}

/// Download `url` (see the module docs for the forms and layouts).
pub fn download(
    url: &str,
    opts: &OllamaOptions,
    transfer: &TransferOptions,
) -> Result<ModelDownload, KgetError> {
    let r = OllamaRef::parse(url)?;
    let base = opts
        .registry_url
        .clone()
        .unwrap_or_else(|| registry_url(&r))
        .trim_end_matches('/')
        .to_string();
    transfer.status(format!("Ollama: pulling manifest for {}", r.display_name()));
    let (manifest, raw) = fetch_manifest(&r, &base, transfer)?;

    let mut layers = vec![&manifest.config];
    layers.extend(manifest.layers.iter());
    let mut seen = HashSet::new();
    layers.retain(|l| seen.insert(l.digest.clone()));

    let plain = opts
        .output
        .as_deref()
        .map(|o| PathBuf::from(crate::utils::expand_tilde(o)));
    let root = match &plain {
        Some(dir) => absolute(dir),
        None => absolute(&opts.models_dir.clone().unwrap_or_else(models_dir)),
    };
    let mut names = HashSet::new();
    let mut items = Vec::with_capacity(layers.len());
    for layer in layers {
        let sha = layer.sha256().ok_or_else(|| {
            KgetError::Protocol(format!("Unsupported layer digest '{}'", layer.digest))
        })?;
        let blob = layer.blob_name().unwrap_or_default();
        let (name, dest) = if plain.is_some() {
            let mut name = layer.plain_name();
            let mut n = 1;
            while !names.insert(name.clone()) {
                name = format!("{}-{n}", layer.plain_name());
                n += 1;
            }
            (name.clone(), root.join(name))
        } else {
            (blob.clone(), root.join("blobs").join(&blob))
        };
        items.push(FetchItem {
            name,
            url: format!(
                "{base}/v2/{}/{}/blobs/{}",
                r.namespace, r.model, layer.digest
            ),
            dest,
            size: (layer.size > 0).then_some(layer.size),
            sha256: Some(sha.to_ascii_lowercase()),
        });
    }
    let total: u64 = items.iter().filter_map(|i| i.size).sum();
    transfer.status(format!(
        "{} blob(s), {}",
        items.len(),
        humansize::format_size(total, humansize::BINARY)
    ));

    let stats = fetch::fetch_all(&items, transfer)?;

    // The manifest goes last: Ollama only sees the model once every blob is there.
    if plain.is_some() {
        crate::config::write_atomic(&root.join("manifest.json"), &raw, false)?;
    } else {
        crate::config::write_atomic(&r.manifest_path(&root), &raw, false)?;
    }
    Ok(ModelDownload {
        path: root,
        single_file: false,
        files: stats.files,
        skipped: stats.skipped,
        bytes: stats.bytes,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ollama_urls() {
        let r = OllamaRef::parse("ollama://llama3.2").unwrap();
        assert_eq!(r.tag, "latest");
        assert_eq!(r.display_name(), "llama3.2:latest");

        let r = OllamaRef::parse("ollama://alice/tiny:q4_0").unwrap();
        assert_eq!(
            (
                r.registry.as_str(),
                r.namespace.as_str(),
                r.model.as_str(),
                r.tag.as_str()
            ),
            (DEFAULT_REGISTRY, "alice", "tiny", "q4_0")
        );

        let r = OllamaRef::parse("ollama://registry.example.com:5000/ns/m:v1").unwrap();
        assert_eq!(r.registry, "registry.example.com:5000");
        assert_eq!(
            r.manifest_path(Path::new("/m")),
            Path::new("/m/manifests/registry.example.com:5000/ns/m/v1")
        );

        assert!(OllamaRef::parse("ollama://").is_err());
        assert!(OllamaRef::parse("ollama://a/b/c/d").is_err());
        assert!(OllamaRef::parse("ollama://../x").is_err());
        assert!(OllamaRef::parse("ollama://m:../../x").is_err());
    }

    #[test]
    fn layer_names() {
        let l = Layer {
            media_type: "application/vnd.ollama.image.model".into(),
            digest: format!("sha256:{}", "a".repeat(64)),
            size: 1,
        };
        assert_eq!(l.plain_name(), "model.gguf");
        assert_eq!(l.blob_name().unwrap(), format!("sha256-{}", "a".repeat(64)));
        let bad = Layer {
            digest: "sha256:../../etc".into(),
            ..l
        };
        assert!(bad.blob_name().is_none());
    }
}
