//! Multi-algorithm file checksum computation and sidecar-file parsing.
//!
//! Supports SHA-256, SHA-512, SHA-1, MD5, and BLAKE3.
//! The [`parse_sidecar`] function understands the GNU `<hash>  <file>` format
//! (also with a single space), the BSD `ALG (file) = hash` format, and files
//! that contain only the hash (`file.iso.sha256`).

use crate::error::KgetError;
use sha2::Digest as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;

// ── Algorithm enum ────────────────────────────────────────────────────────────

/// Checksum algorithm used to verify a downloaded file.
///
/// Serialized as its lowercase name (`"sha256"`, `"md5"`, …).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgorithm {
    Sha256,
    Sha512,
    Sha1,
    Md5,
    Blake3,
}

impl ChecksumAlgorithm {
    /// Guess the algorithm from the hex-string length.
    ///
    /// - 32 chars → MD5
    /// - 40 chars → SHA-1
    /// - 64 chars → SHA-256  (BLAKE3 also produces 64 chars; SHA-256 is assumed)
    /// - 128 chars → SHA-512
    pub fn from_hex_len(hex: &str) -> Option<Self> {
        match hex.trim().len() {
            32 => Some(ChecksumAlgorithm::Md5),
            40 => Some(ChecksumAlgorithm::Sha1),
            64 => Some(ChecksumAlgorithm::Sha256),
            128 => Some(ChecksumAlgorithm::Sha512),
            _ => None,
        }
    }

    /// Parse an algorithm name (`sha256`, `sha-256`, `SHA512`, `md5`, `blake3`…).
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().replace('-', "").as_str() {
            "sha256" => Some(ChecksumAlgorithm::Sha256),
            "sha512" => Some(ChecksumAlgorithm::Sha512),
            "sha1" => Some(ChecksumAlgorithm::Sha1),
            "md5" => Some(ChecksumAlgorithm::Md5),
            "blake3" | "b3" => Some(ChecksumAlgorithm::Blake3),
            _ => None,
        }
    }

    /// Human-readable algorithm name.
    pub fn name(&self) -> &'static str {
        match self {
            ChecksumAlgorithm::Sha256 => "sha256",
            ChecksumAlgorithm::Sha512 => "sha512",
            ChecksumAlgorithm::Sha1 => "sha1",
            ChecksumAlgorithm::Md5 => "md5",
            ChecksumAlgorithm::Blake3 => "blake3",
        }
    }
}

// ── Computation ───────────────────────────────────────────────────────────────

const BUF_SIZE: usize = 1024 * 1024; // 1 MiB

/// Compute the checksum of a file using the specified algorithm.
///
/// Returns the lowercase hex-encoded digest.
pub fn compute_checksum(path: &Path, algorithm: &ChecksumAlgorithm) -> Result<String, KgetError> {
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; BUF_SIZE];

    match algorithm {
        ChecksumAlgorithm::Sha256 => {
            let mut h = sha2::Sha256::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                sha2::Digest::update(&mut h, &buf[..n]);
            }
            Ok(hex::encode(sha2::Digest::finalize(h)))
        }
        ChecksumAlgorithm::Sha512 => {
            let mut h = sha2::Sha512::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                sha2::Digest::update(&mut h, &buf[..n]);
            }
            Ok(hex::encode(sha2::Digest::finalize(h)))
        }
        ChecksumAlgorithm::Sha1 => {
            let mut h = sha1::Sha1::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                sha1::Digest::update(&mut h, &buf[..n]);
            }
            Ok(hex::encode(sha1::Digest::finalize(h)))
        }
        ChecksumAlgorithm::Md5 => {
            let mut h = md5::Md5::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                md5::Digest::update(&mut h, &buf[..n]);
            }
            Ok(hex::encode(md5::Digest::finalize(h)))
        }
        ChecksumAlgorithm::Blake3 => {
            let mut h = blake3::Hasher::new();
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            Ok(h.finalize().to_hex().to_string())
        }
    }
}

// ── Sidecar parsing ───────────────────────────────────────────────────────────

/// Scan a checksum sidecar file and return the `(algorithm, hash)` pair
/// matching `filename`.
///
/// Understands two line formats:
///
/// - **GNU**: `<hash>  <filename>` or `<hash> *<filename>` (binary mode marker)
/// - **BSD**: `SHA256 (<filename>) = <hash>`
///
/// `filename` is matched as a suffix so both bare names and paths work.
pub fn parse_sidecar(content: &str, filename: &str) -> Option<(ChecksumAlgorithm, String)> {
    // ── Hash-only file: "<hash>" (e.g. ubuntu.iso.sha256) ────────────────────
    let meaningful: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if meaningful.len() == 1
        && meaningful[0].chars().all(|c| c.is_ascii_hexdigit())
        && let Some(algo) = ChecksumAlgorithm::from_hex_len(meaningful[0])
    {
        return Some((algo, meaningful[0].to_lowercase()));
    }

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // ── GNU format: "<hash>  <file>", "<hash> *<file>" or "<hash> <file>" ─
        let gnu = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .or_else(|| {
                line.split_once(' ')
                    .filter(|(h, _)| h.chars().all(|c| c.is_ascii_hexdigit()))
            });

        if let Some((hash_part, file_part)) = gnu {
            let hash = hash_part.trim();
            let file = file_part.trim().trim_start_matches('*').trim();
            if matches_filename(file, filename)
                && let Some(algo) = ChecksumAlgorithm::from_hex_len(hash)
            {
                return Some((algo, hash.to_lowercase()));
            }
        }

        // ── BSD format: "SHA256 (<file>) = <hash>" ────────────────────────────
        if let Some(eq_pos) = line.find(" = ") {
            let hash = line[eq_pos + 3..].trim().to_lowercase();
            let prefix = &line[..eq_pos];
            if let (Some(lp), Some(rp)) = (prefix.find('('), prefix.rfind(')')) {
                let file = prefix[lp + 1..rp].trim();
                if matches_filename(file, filename) {
                    let algo_str = prefix[..lp].trim().to_uppercase();
                    let algo = match algo_str.as_str() {
                        "SHA256" => Some(ChecksumAlgorithm::Sha256),
                        "SHA512" => Some(ChecksumAlgorithm::Sha512),
                        "SHA1" => Some(ChecksumAlgorithm::Sha1),
                        "MD5" => Some(ChecksumAlgorithm::Md5),
                        "BLAKE3" => Some(ChecksumAlgorithm::Blake3),
                        _ => None,
                    };
                    if let Some(algo) = algo {
                        return Some((algo, hash));
                    }
                }
            }
        }
    }
    None
}

/// Match file names flexibly — either exact or suffix match.
fn matches_filename(candidate: &str, target: &str) -> bool {
    let c = candidate.trim_start_matches("./");
    let t = target.trim_start_matches("./");
    c == t
        || c.ends_with(&format!("/{t}"))
        || t.ends_with(&format!("/{c}"))
        || std::path::Path::new(c).file_name() == std::path::Path::new(t).file_name()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gnu_format() {
        let sidecar = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  empty.bin\n\
                       abc123def456abc123def456abc123def456abc123def456abc123def456abc1  file.iso\n";
        let (algo, hash) = parse_sidecar(sidecar, "file.iso").unwrap();
        assert_eq!(algo, ChecksumAlgorithm::Sha256);
        assert_eq!(
            hash,
            "abc123def456abc123def456abc123def456abc123def456abc123def456abc1"
        );
    }

    #[test]
    fn parse_bsd_format() {
        let sidecar = "SHA256 (ubuntu.iso) = deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef\n";
        let (algo, hash) = parse_sidecar(sidecar, "ubuntu.iso").unwrap();
        assert_eq!(algo, ChecksumAlgorithm::Sha256);
        assert_eq!(
            hash,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        );
    }

    #[test]
    fn parse_md5_by_length() {
        let sidecar = "d41d8cd98f00b204e9800998ecf8427e  empty.bin\n";
        let (algo, _) = parse_sidecar(sidecar, "empty.bin").unwrap();
        assert_eq!(algo, ChecksumAlgorithm::Md5);
    }

    #[test]
    fn parse_hash_only_sidecar() {
        let h = "a".repeat(64);
        let (algo, hash) = parse_sidecar(&format!("{h}\n"), "anything.iso").unwrap();
        assert_eq!(algo, ChecksumAlgorithm::Sha256);
        assert_eq!(hash, h);
    }

    #[test]
    fn parse_gnu_single_space() {
        let sidecar = format!("{} file.iso\n", "b".repeat(128));
        let (algo, _) = parse_sidecar(&sidecar, "file.iso").unwrap();
        assert_eq!(algo, ChecksumAlgorithm::Sha512);
    }

    #[test]
    fn algo_parse_names() {
        assert_eq!(
            ChecksumAlgorithm::parse("SHA-256"),
            Some(ChecksumAlgorithm::Sha256)
        );
        assert_eq!(
            ChecksumAlgorithm::parse("blake3"),
            Some(ChecksumAlgorithm::Blake3)
        );
        assert_eq!(ChecksumAlgorithm::parse("crc"), None);
    }

    #[test]
    fn algo_from_hex_len() {
        assert_eq!(
            ChecksumAlgorithm::from_hex_len(&"a".repeat(32)),
            Some(ChecksumAlgorithm::Md5)
        );
        assert_eq!(
            ChecksumAlgorithm::from_hex_len(&"a".repeat(40)),
            Some(ChecksumAlgorithm::Sha1)
        );
        assert_eq!(
            ChecksumAlgorithm::from_hex_len(&"a".repeat(64)),
            Some(ChecksumAlgorithm::Sha256)
        );
        assert_eq!(
            ChecksumAlgorithm::from_hex_len(&"a".repeat(128)),
            Some(ChecksumAlgorithm::Sha512)
        );
        assert_eq!(ChecksumAlgorithm::from_hex_len(&"a".repeat(10)), None);
    }
}
