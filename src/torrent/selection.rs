//! Torrent file selection (`--select`, `--select-glob`) and seeding limits
//! (`--seed-ratio`, `--seed-time`).
//!
//! Everything here is backend-independent and pure, so it is unit tested
//! without a network or a torrent engine.
//!
//! # Example
//!
//! ```rust
//! use kget::torrent::FileSelection;
//!
//! let mut sel = FileSelection::default();
//! sel.add_spec("1,3-4").unwrap(); // 1-based, as shown by `--list-files`
//! sel.globs.push("*.nfo".into());
//!
//! let names = ["a.mkv", "b.mkv", "c.mkv", "d.mkv", "info.nfo"];
//! assert_eq!(sel.resolve(&names).unwrap(), Some(vec![0, 2, 3, 4]));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

// ============================================================================
// File selection
// ============================================================================

/// Which files of a torrent to download. Empty = every file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileSelection {
    /// 0-based file indices (the CLI takes 1-based numbers, see [`parse_select`])
    pub indices: Vec<usize>,
    /// Shell-style patterns matched against the path inside the torrent
    /// (`*` matches any run of characters including `/`, `?` one character;
    /// case-insensitive)
    pub globs: Vec<String>,
}

impl FileSelection {
    /// No selection: download every file.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty() && self.globs.is_empty()
    }

    /// Add the files of a `--select` spec (`"1,3-5"`, 1-based).
    pub fn add_spec(&mut self, spec: &str) -> Result<(), String> {
        for i in parse_select(spec)? {
            if !self.indices.contains(&i) {
                self.indices.push(i);
            }
        }
        Ok(())
    }

    /// The 0-based indices to download among `names` (the torrent's files in
    /// order), or `None` when there is no selection (every file).
    ///
    /// Fails when an index is out of range or nothing matches, so a typo does
    /// not silently download nothing (or everything).
    pub fn resolve<S: AsRef<str>>(&self, names: &[S]) -> Result<Option<Vec<usize>>, String> {
        if self.is_empty() {
            return Ok(None);
        }
        let mut out = BTreeSet::new();
        for &i in &self.indices {
            if i >= names.len() {
                return Err(format!(
                    "--select: file {} does not exist (the torrent has {} file{})",
                    i + 1,
                    names.len(),
                    if names.len() == 1 { "" } else { "s" }
                ));
            }
            out.insert(i);
        }
        for pattern in &self.globs {
            let mut matched = false;
            for (i, name) in names.iter().enumerate() {
                if glob_match(pattern, name.as_ref()) {
                    matched = true;
                    out.insert(i);
                }
            }
            if !matched {
                return Err(format!("--select-glob: no file matches '{pattern}'"));
            }
        }
        Ok(Some(out.into_iter().collect()))
    }
}

/// Parse a `--select` spec: comma-separated 1-based numbers and inclusive
/// ranges (`"1,3-5"`). Returns sorted, de-duplicated 0-based indices.
pub fn parse_select(spec: &str) -> Result<Vec<usize>, String> {
    let mut out = BTreeSet::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let parse = |s: &str| -> Result<usize, String> {
            match s.trim().parse::<usize>() {
                Ok(0) => Err("--select: file numbers start at 1".to_string()),
                Ok(n) => Ok(n - 1),
                Err(_) => Err(format!("--select: '{s}' is not a file number")),
            }
        };
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (parse(a)?, parse(b)?);
                if a > b {
                    return Err(format!("--select: range '{part}' is reversed"));
                }
                if b - a > 1_000_000 {
                    return Err(format!("--select: range '{part}' is too large"));
                }
                out.extend(a..=b);
            }
            None => {
                out.insert(parse(part)?);
            }
        }
    }
    if out.is_empty() {
        return Err("--select: no file numbers given".into());
    }
    Ok(out.into_iter().collect())
}

/// Case-insensitive shell-style match: `*` = any run of characters
/// (including `/`), `?` = exactly one character. A pattern without `/` also
/// matches the file's base name, so `"*.mkv"` and `"ep?.mkv"` both select
/// `Show/ep1.mkv`.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let full: Vec<char> = name.to_lowercase().replace('\\', "/").chars().collect();
    if wildcard(&p, &full) {
        return true;
    }
    if !pattern.contains('/')
        && let Some(pos) = full.iter().rposition(|c| *c == '/')
    {
        return wildcard(&p, &full[pos + 1..]);
    }
    false
}

/// Iterative wildcard matching with single-star backtracking.
fn wildcard(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Per-job torrent options ([`DownloadJob::torrent`](crate::runner::DownloadJob::torrent)).
///
/// The seeding limits override `config.torrent.seed_ratio` /
/// `seed_time_secs` for this job.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentJobOptions {
    /// Files to download (`--select`, `--select-glob`); empty = all
    pub select: FileSelection,
    /// Seed until this ratio (`--seed-ratio`)
    pub seed_ratio: Option<f64>,
    /// Seed for this many seconds (`--seed-time`)
    pub seed_time_secs: Option<u64>,
}

impl TorrentJobOptions {
    /// `true` when nothing differs from the defaults.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Effective seeding limits: the job's, else the configuration's.
    pub fn seed_limits(&self, config: &crate::config::TorrentConfig) -> SeedLimits {
        SeedLimits::new(
            self.seed_ratio.or(config.seed_ratio),
            self.seed_time_secs.or(config.seed_time_secs),
        )
    }
}

// ============================================================================
// Seeding limits
// ============================================================================

/// When to stop seeding after the download finished.
///
/// With no limit KGet stops as soon as the data is complete (it never seeds
/// by default). With both limits it stops at whichever comes first.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SeedLimits {
    /// Stop once `uploaded / size >= ratio`
    pub ratio: Option<f64>,
    /// Stop after seeding this long
    pub time: Option<Duration>,
}

/// Why seeding stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedStop {
    /// The ratio limit was reached
    Ratio,
    /// The time limit was reached
    Time,
}

impl SeedLimits {
    /// Limits from the job (CLI flags) with the configuration as fallback.
    pub fn new(ratio: Option<f64>, time_secs: Option<u64>) -> Self {
        Self {
            ratio: ratio.filter(|r| r.is_finite() && *r > 0.0),
            time: time_secs.filter(|t| *t > 0).map(Duration::from_secs),
        }
    }

    /// `true` when the torrent should seed after downloading.
    pub fn seeds(&self) -> bool {
        self.ratio.is_some() || self.time.is_some()
    }

    /// Whether seeding should stop now, given the bytes uploaded, the size of
    /// the downloaded data and how long it has been seeding.
    pub fn should_stop(&self, uploaded: u64, size: u64, seeding: Duration) -> Option<SeedStop> {
        if !self.seeds() {
            return Some(SeedStop::Ratio);
        }
        if let Some(r) = self.ratio
            && ratio(uploaded, size) >= r
        {
            return Some(SeedStop::Ratio);
        }
        if let Some(t) = self.time
            && seeding >= t
        {
            return Some(SeedStop::Time);
        }
        None
    }
}

/// Share ratio: bytes uploaded divided by the size of the data (0 when the
/// size is unknown).
pub fn ratio(uploaded: u64, size: u64) -> f64 {
    if size == 0 {
        0.0
    } else {
        uploaded as f64 / size as f64
    }
}

/// Parse a `--seed-ratio` value (`1.5`; must be positive).
pub fn parse_seed_ratio(s: &str) -> Result<f64, BoxError> {
    let r: f64 = s
        .trim()
        .parse()
        .map_err(|_| format!("--seed-ratio: '{s}' is not a number"))?;
    if !r.is_finite() || r <= 0.0 {
        return Err(format!("--seed-ratio: '{s}' must be greater than 0").into());
    }
    Ok(r)
}

/// Parse a duration such as `90s`, `30m`, `2h`, `1d` or `1h30m` into
/// seconds. A bare number is minutes (like Transmission's seeding limits).
pub fn parse_duration_secs(s: &str) -> Result<u64, BoxError> {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return Err("empty duration".into());
    }
    if let Ok(mins) = t.parse::<u64>() {
        return Ok(mins.saturating_mul(60));
    }
    let mut total: u64 = 0;
    let mut num = String::new();
    for c in t.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let unit = match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            _ => return Err(format!("invalid duration '{s}' (use e.g. 90s, 30m, 2h, 1d)").into()),
        };
        let n: u64 = num
            .parse()
            .map_err(|_| format!("invalid duration '{s}' (use e.g. 90s, 30m, 2h, 1d)"))?;
        total = total.saturating_add(n.saturating_mul(unit));
        num.clear();
    }
    if !num.is_empty() {
        return Err(format!("invalid duration '{s}': missing unit after {num}").into());
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_specs_parse() {
        assert_eq!(parse_select("1,3-5").unwrap(), vec![0, 2, 3, 4]);
        assert_eq!(parse_select(" 2 , 2,1 ").unwrap(), vec![0, 1]);
        assert!(parse_select("0").is_err());
        assert!(parse_select("5-3").is_err());
        assert!(parse_select("a").is_err());
        assert!(parse_select(",").is_err());
    }

    #[test]
    fn globs_match_paths_and_base_names() {
        assert!(glob_match("*.mkv", "Show/S01/ep1.MKV"));
        assert!(glob_match("ep?.mkv", "Show/ep1.mkv"));
        assert!(glob_match("show/*", "Show/ep1.mkv"));
        assert!(!glob_match("show/*.srt", "Show/ep1.mkv"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*a*b*", "xxaYYbzz"));
    }

    #[test]
    fn selection_resolves_and_reports_mistakes() {
        let names = ["a.mkv", "b.srt", "c.mkv"];
        assert_eq!(FileSelection::default().resolve(&names).unwrap(), None);
        let sel = FileSelection {
            indices: vec![1],
            globs: vec!["*.mkv".into()],
        };
        assert_eq!(sel.resolve(&names).unwrap(), Some(vec![0, 1, 2]));
        let out_of_range = FileSelection {
            indices: vec![3],
            globs: vec![],
        };
        assert!(out_of_range.resolve(&names).unwrap_err().contains("file 4"));
        let no_match = FileSelection {
            indices: vec![],
            globs: vec!["*.iso".into()],
        };
        assert!(no_match.resolve(&names).is_err());
    }

    #[test]
    fn seed_limits_stop_at_first_limit() {
        let none = SeedLimits::new(None, None);
        assert!(!none.seeds());
        assert_eq!(
            none.should_stop(0, 100, Duration::ZERO),
            Some(SeedStop::Ratio)
        );

        let both = SeedLimits::new(Some(1.5), Some(60));
        assert_eq!(both.should_stop(100, 100, Duration::from_secs(10)), None);
        assert_eq!(
            both.should_stop(150, 100, Duration::from_secs(10)),
            Some(SeedStop::Ratio)
        );
        assert_eq!(
            both.should_stop(0, 100, Duration::from_secs(60)),
            Some(SeedStop::Time)
        );
        // Unknown size never satisfies a ratio, only the time limit.
        let ratio_only = SeedLimits::new(Some(1.0), None);
        assert_eq!(
            ratio_only.should_stop(10, 0, Duration::from_secs(9999)),
            None
        );
        // Invalid values are ignored.
        assert!(!SeedLimits::new(Some(-1.0), Some(0)).seeds());
    }

    #[test]
    fn durations_and_ratios_parse() {
        assert_eq!(parse_duration_secs("30m").unwrap(), 1800);
        assert_eq!(parse_duration_secs("1h30m").unwrap(), 5400);
        assert_eq!(parse_duration_secs("90s").unwrap(), 90);
        assert_eq!(parse_duration_secs("2").unwrap(), 120);
        assert_eq!(parse_duration_secs("1d").unwrap(), 86_400);
        assert!(parse_duration_secs("10x").is_err());
        assert!(parse_duration_secs("5h3").is_err());
        assert_eq!(parse_seed_ratio("1.5").unwrap(), 1.5);
        assert!(parse_seed_ratio("0").is_err());
        assert!(parse_seed_ratio("x").is_err());
        assert_eq!(ratio(150, 100), 1.5);
        assert_eq!(ratio(1, 0), 0.0);
    }
}
