//! `[self.mint]` (theseus-pw1q.21.3): the pattern miner and the minted tools
//! it feeds. Off by default: `mine = false` runs nothing. The miner reads
//! `mine`, `bench_dirs`, `window_days`, `min_occurrences`, `min_sessions` and
//! `min_days`; the keys marked below are read by the later steps (the mint
//! validation, and the deploy and retire step), declared here so one place
//! holds all of `[self.mint]`.

use serde::{Deserialize, Serialize};

/// The kinds of tool a pattern may become, cheapest first.
pub const KINDS: &[&str] = &["flow", "mcp", "rust"];

/// `[self.mint]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Raw")]
pub struct SelfMintConfig {
    /// Run the nightly miner (with `[self] mode = "act"`). Off: nothing runs.
    pub mine: bool,
    /// Directories of bench trials (Harbor jobs) the miner also reads, absolute
    /// or `~`-relative. None: the ledger alone.
    pub bench_dirs: Vec<String>,
    /// The mining window, in days.
    pub window_days: u32,
    /// A real-ledger motif needs this many occurrences (runs) ...
    pub min_occurrences: u32,
    /// ... in this many distinct sessions ...
    pub min_sessions: u32,
    /// ... on this many distinct days, in one scope.
    pub min_days: u32,
    /// Read by the mint step: motifs started a night (apart from the fixes'
    /// `[self] backlog_starts`).
    pub per_night: u32,
    /// Read by the deploy step: minted tools live at once.
    pub max_live: u32,
    /// Read by the mint step: dollars one motif's minting may spend, on the
    /// self budget.
    pub usd: f64,
    /// Read by the mint step: check-half turns replayed with a model.
    pub replay_turns: u32,
    /// Read by the retire step: a minted tool unused this many days retires.
    pub retire_after_days: u32,
    /// Read by the deploy step: a one-line hint in a result that matches a
    /// minted tool's motif (off until a bench arm shows it earns its place).
    pub hint: bool,
    /// Read by the mint step: the kinds it may mint (`flow`, `mcp`, `rust`).
    pub kinds: Vec<String>,
}

impl Default for SelfMintConfig {
    fn default() -> Self {
        Self {
            mine: false,
            bench_dirs: vec![],
            window_days: 14,
            min_occurrences: 5,
            min_sessions: 3,
            min_days: 2,
            per_night: 1,
            max_live: 10,
            usd: 2.0,
            replay_turns: 5,
            retire_after_days: 30,
            hint: false,
            kinds: vec!["flow".into()],
        }
    }
}

/// The table as written: every key optional, unknown keys refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, default)]
struct Raw {
    mine: bool,
    bench_dirs: Vec<String>,
    window_days: u32,
    min_occurrences: u32,
    min_sessions: u32,
    min_days: u32,
    per_night: u32,
    max_live: u32,
    usd: f64,
    replay_turns: u32,
    retire_after_days: u32,
    hint: bool,
    kinds: Vec<String>,
}

impl Default for Raw {
    fn default() -> Self {
        let d = SelfMintConfig::default();
        Self {
            mine: d.mine,
            bench_dirs: d.bench_dirs,
            window_days: d.window_days,
            min_occurrences: d.min_occurrences,
            min_sessions: d.min_sessions,
            min_days: d.min_days,
            per_night: d.per_night,
            max_live: d.max_live,
            usd: d.usd,
            replay_turns: d.replay_turns,
            retire_after_days: d.retire_after_days,
            hint: d.hint,
            kinds: d.kinds,
        }
    }
}

impl TryFrom<Raw> for SelfMintConfig {
    type Error = String;
    fn try_from(r: Raw) -> Result<Self, String> {
        let c = Self {
            mine: r.mine,
            bench_dirs: r.bench_dirs,
            window_days: r.window_days,
            min_occurrences: r.min_occurrences,
            min_sessions: r.min_sessions,
            min_days: r.min_days,
            per_night: r.per_night,
            max_live: r.max_live,
            usd: r.usd,
            replay_turns: r.replay_turns,
            retire_after_days: r.retire_after_days,
            hint: r.hint,
            kinds: r.kinds,
        };
        c.validate()?;
        Ok(c)
    }
}

impl SelfMintConfig {
    /// The thresholds are at least 1, a bench directory is absolute or
    /// `~`-relative, and the kinds are known.
    pub fn validate(&self) -> Result<(), String> {
        for (k, v) in [
            ("window_days", self.window_days),
            ("min_occurrences", self.min_occurrences),
            ("min_sessions", self.min_sessions),
            ("min_days", self.min_days),
        ] {
            if v < 1 {
                return Err(format!("[self.mint] {k} must be at least 1, not {v}"));
            }
        }
        if !(self.usd.is_finite() && self.usd >= 0.0) {
            return Err("[self.mint] usd must be a number, 0 or more".into());
        }
        for d in &self.bench_dirs {
            if !(d.starts_with('/') || d == "~" || d.starts_with("~/")) {
                return Err(format!(
                    "[self.mint] bench_dirs entry {d:?} must be an absolute path or start with ~/"
                ));
            }
        }
        if let Some(k) = self.kinds.iter().find(|k| !KINDS.contains(&k.as_str())) {
            return Err(format!(
                "[self.mint] kinds: {k:?} is not one of flow, mcp, rust"
            ));
        }
        if self.kinds.is_empty() {
            return Err("[self.mint] kinds must name at least one kind".into());
        }
        Ok(())
    }

    /// The miner's thresholds.
    pub fn params(&self) -> crate::rsi::mine::Params {
        crate::rsi::mine::Params {
            min_occurrences: self.min_occurrences,
            min_sessions: self.min_sessions,
            min_days: self.min_days,
            window_days: self.window_days,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(t: &str) -> Result<SelfMintConfig, String> {
        toml::from_str::<SelfMintConfig>(t).map_err(|e| e.to_string())
    }

    #[test]
    fn nothing_acts_by_default() {
        let c = load("").unwrap();
        assert_eq!(c, SelfMintConfig::default());
        assert!(!c.mine && !c.hint && c.bench_dirs.is_empty());
    }

    #[test]
    fn thresholds_are_at_least_one_and_bench_dirs_are_rooted() {
        for k in ["window_days", "min_occurrences", "min_sessions", "min_days"] {
            let e = load(&format!("{k} = 0")).unwrap_err();
            assert!(e.contains(k) && e.contains("at least 1"), "{e}");
        }
        assert!(load("bench_dirs = [\"relative/jobs\"]").is_err());
        assert!(load("bench_dirs = [\"/srv/jobs\", \"~/jobs\"]").is_ok());
        assert!(load("kinds = [\"wasm\"]").is_err());
        assert!(load("nonsense = 1").is_err());
    }
}
