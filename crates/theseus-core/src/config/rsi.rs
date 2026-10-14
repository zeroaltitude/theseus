//! `[self]` (theseus-pw1q.4, theseus-pw1q.2): self-improvement, off by
//! default. Read at the start, like the AWS lines: a change takes a restart.
//!
//! - `mode`: `"off"` (the default: nothing self-directed runs, whatever the
//!   kill switch says) or `"act"` (self steps run while the owner has
//!   released the kill switch; a store that has never seen the owner's
//!   resume is halted, so `"act"` alone starts nothing).
//! - `digest`: `"weekly"` (the default: with `mode = "act"`, the week's
//!   "What Theseus changed about itself" goes to the owner's DM once a week)
//!   or `"off"`.
//! - `[self.budget]` (theseus-pw1q.3): the one spend account for all
//!   self-improvement. `branch_usd` ($10) is a self-branch's line, `day_usd`
//!   ($30) the local day's, across every self loop together. Part of the
//!   hard keel: raising either is the owner's.
//! - `sealed_dir` (theseus-pw1q.5): where sealed holdout sets live, out of
//!   Theseus's reach (`crate::rsi::sealed`). Unset (the default) means no
//!   sealed set. It must be an absolute path outside every workspace root,
//!   the state dir, the repository and every `[sandbox] ro_paths` entry
//!   (neither inside one nor holding one), refused at load by the key's name.
//! - `backlog_starts` (1) and `backlog_max_open` (20): the nightly backlog
//!   (`crate::rsi::backlog`, theseus-pw1q.7) starts at most this many top
//!   unmarked items a night, and keeps at most this many open.
//! - `repo` (theseus-pw1q.8): Theseus's own source checkout, which self
//!   branches are cut from (its `origin/main`). Unset, the default: no self
//!   branches, and `self.propose_change` says so.
//! - `remote` (theseus-pw1q.8): the remote `self/*` branches are pushed to,
//!   with the credential git already has for it (none is read here). Unset,
//!   the default: they stay in the repo. Choosing it is the owner's: a push
//!   needs a credential, and choosing one is a permission change.
//! - `graduate_min_calls` (20) and `graduate_window_days` (14): a loaded
//!   extension called this many times in this many days with no failure
//!   becomes a self item to port it into core (theseus-pw1q.13).
//! - `exams_day_max`: the most exam candidates the nightly exam pass writes
//!   a day (default 10; theseus-pw1q.11).

use serde::{Deserialize, Deserializer, Serialize};
pub use theseus_protocol::rsi::SelfMode;

/// `[self]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfConfig {
    #[serde(default, deserialize_with = "mode")]
    pub mode: SelfMode,
    #[serde(default, deserialize_with = "digest")]
    pub digest: SelfDigest,
    #[serde(default)]
    pub budget: SelfBudgetConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_dir: Option<String>,
    /// Items the nightly backlog starts a night (0 files and ranks only).
    #[serde(default = "default_backlog_starts")]
    pub backlog_starts: u32,
    /// Open items the backlog keeps; the lowest-ranked past it are dropped.
    #[serde(default = "default_backlog_max_open")]
    pub backlog_max_open: u32,
    /// The most exam candidates written a night.
    #[serde(default = "default_exams_day_max")]
    pub exams_day_max: u32,
    /// Calls in the window, with no failure, that graduate an extension.
    #[serde(default = "default_graduate_min_calls")]
    pub graduate_min_calls: u32,
    /// The window of that rule, in days.
    #[serde(default = "default_graduate_window_days")]
    pub graduate_window_days: u32,
}

/// `[self] graduate_min_calls`'s default.
pub const GRADUATE_MIN_CALLS: u32 = 20;
/// `[self] graduate_window_days`'s default.
pub const GRADUATE_WINDOW_DAYS: u32 = 14;

fn default_graduate_min_calls() -> u32 {
    GRADUATE_MIN_CALLS
}

fn default_graduate_window_days() -> u32 {
    GRADUATE_WINDOW_DAYS
}

/// `[self] exams_day_max`'s default.
pub const EXAMS_DAY_MAX: u32 = 10;

fn default_exams_day_max() -> u32 {
    EXAMS_DAY_MAX
}

fn default_backlog_starts() -> u32 {
    1
}

fn default_backlog_max_open() -> u32 {
    20
}

impl Default for SelfConfig {
    fn default() -> Self {
        Self {
            mode: SelfMode::default(),
            digest: SelfDigest::default(),
            budget: SelfBudgetConfig::default(),
            repo: None,
            remote: None,
            sealed_dir: None,
            backlog_starts: default_backlog_starts(),
            backlog_max_open: default_backlog_max_open(),
            exams_day_max: EXAMS_DAY_MAX,
            graduate_min_calls: GRADUATE_MIN_CALLS,
            graduate_window_days: GRADUATE_WINDOW_DAYS,
        }
    }
}

/// `[self] digest`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfDigest {
    #[default]
    Weekly,
    Off,
}

/// `[self.budget]`: the two spend lines (theseus-pw1q.3), read at the start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfBudgetConfig {
    /// What one self branch may spend, all its loops together.
    #[serde(default = "default_branch_usd")]
    pub branch_usd: f64,
    /// What all self-improvement may spend in one local day.
    #[serde(default = "default_day_usd")]
    pub day_usd: f64,
}

fn default_branch_usd() -> f64 {
    10.0
}

fn default_day_usd() -> f64 {
    30.0
}

impl Default for SelfBudgetConfig {
    fn default() -> Self {
        Self {
            branch_usd: default_branch_usd(),
            day_usd: default_day_usd(),
        }
    }
}

impl SelfConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether the weekly digest is posted: only while self steps may run.
    pub fn posts_digest(&self) -> bool {
        self.mode == SelfMode::Act && self.digest == SelfDigest::Weekly
    }
}

impl SelfConfig {
    /// `[self] sealed_dir` against the config it sits in: absolute, and
    /// apart from every place Theseus or a job may reach (theseus-pw1q.5).
    pub fn validate(&self, cfg: &crate::Config) -> anyhow::Result<()> {
        let Some(raw) = self.sealed_dir.as_deref() else {
            return Ok(());
        };
        if raw.trim().is_empty() {
            anyhow::bail!("[self] sealed_dir is empty: leave it out for no sealed set");
        }
        let expanded = crate::config::expand(raw);
        if !expanded.is_absolute() {
            anyhow::bail!("[self] sealed_dir must be an absolute path, not {raw:?}");
        }
        let canon = |p: &std::path::Path| theseus_tools::paths::canonical_best_effort(p);
        let sealed = canon(&expanded);
        let mut near: Vec<(String, std::path::PathBuf)> = Vec::new();
        let t = &cfg.tools;
        for (i, r) in t.projects_dir.iter().chain(t.roots.iter()).enumerate() {
            let what = if i == 0 && t.projects_dir.is_some() {
                "[tools] projects_dir"
            } else {
                "a [tools] root"
            };
            near.push((what.into(), canon(&crate::config::expand(r))));
        }
        if let Some(c) = &t.cwd {
            near.push(("[tools] cwd".into(), canon(&crate::config::expand(c))));
        }
        near.push(("the state dir".into(), canon(&cfg.state_dir())));
        for r in &cfg.sandbox.ro_paths {
            near.push((
                "a [sandbox] ro_paths entry".into(),
                canon(&crate::config::expand(r)),
            ));
        }
        let start = t
            .cwd
            .as_deref()
            .map(crate::config::expand)
            .or_else(|| std::env::current_dir().ok());
        if let Some(repo) = start.as_deref().and_then(repo_of) {
            near.push(("the repository".into(), canon(&repo)));
        }
        for (what, p) in near {
            if theseus_tools::paths::within(&sealed, &p) {
                anyhow::bail!(
                    "[self] sealed_dir is inside {what}: the sealed set must sit outside it"
                );
            }
            if theseus_tools::paths::within(&p, &sealed) {
                anyhow::bail!("[self] sealed_dir holds {what}: the sealed set must sit outside it");
            }
        }
        Ok(())
    }
}

/// The git work tree that holds `start`, if any: the nearest ancestor with a
/// `.git`.
fn repo_of(start: &std::path::Path) -> Option<std::path::PathBuf> {
    start
        .ancestors()
        .find(|a| a.join(".git").exists())
        .map(std::path::Path::to_path_buf)
}

/// One of `allowed`, or an error naming the key and its values.
fn one_of<'de, D: Deserializer<'de>, T: Copy>(
    d: D,
    key: &str,
    allowed: &[(&str, T)],
) -> Result<T, D::Error> {
    let s = String::deserialize(d)?;
    allowed
        .iter()
        .find(|(name, _)| *name == s)
        .map(|(_, v)| *v)
        .ok_or_else(|| {
            let names: Vec<String> = allowed.iter().map(|(n, _)| format!("\"{n}\"")).collect();
            serde::de::Error::custom(format!(
                "[self] {key} must be {}, not \"{s}\"",
                names.join(" or ")
            ))
        })
}

fn mode<'de, D: Deserializer<'de>>(d: D) -> Result<SelfMode, D::Error> {
    one_of(d, "mode", &[("off", SelfMode::Off), ("act", SelfMode::Act)])
}

fn digest<'de, D: Deserializer<'de>>(d: D) -> Result<SelfDigest, D::Error> {
    one_of(
        d,
        "digest",
        &[("weekly", SelfDigest::Weekly), ("off", SelfDigest::Off)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    /// `[self]` is off by default, the template carries it as the default,
    /// and an unknown mode is refused at load with the key's name.
    #[test]
    fn the_self_section_is_off_by_default_and_refuses_an_unknown_mode() {
        let none = Config::from_toml("").unwrap();
        assert_eq!(none.self_improve, SelfConfig::default());
        assert_eq!(none.self_improve.mode, SelfMode::Off);
        assert_eq!(none.self_improve.digest, SelfDigest::Weekly);
        assert!(!none.self_improve.posts_digest());
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("# [self]\n# mode = \"off\"")
                && t.contains("# digest = \"weekly\"")
                && t.contains("# exams_day_max = 10")
                && t.contains("# [self.budget]")
                && t.contains("# branch_usd = 10.0")
                && t.contains("# day_usd = 30.0"),
            "the template names [self], its two keys and [self.budget]"
        );
        assert_eq!(none.self_improve.budget.branch_usd, 10.0);
        assert_eq!(none.self_improve.budget.day_usd, 30.0);
        let cfg =
            Config::from_toml("[self]\nmode = \"act\"\ndigest = \"off\"\n[self.budget]\n").unwrap();
        assert_eq!(cfg.self_improve.mode, SelfMode::Act);
        assert_eq!(cfg.self_improve.digest, SelfDigest::Off);
        assert!(!cfg.self_improve.posts_digest());
        let act = Config::from_toml("[self]\nmode = \"act\"\n").unwrap();
        assert!(act.self_improve.posts_digest());
        let e = format!(
            "{:#}",
            Config::from_toml("[self]\nmode = \"on\"\n").unwrap_err()
        );
        assert!(
            e.contains("[self] mode must be \"off\" or \"act\", not \"on\""),
            "{e}"
        );
        let e = format!(
            "{:#}",
            Config::from_toml("[self]\ndigest = \"daily\"\n").unwrap_err()
        );
        assert!(e.contains("[self] digest must be"), "{e}");
        assert!(Config::from_toml("[self]\nbudget_usd = 1.0\n").is_err());
        assert_eq!(none.self_improve.exams_day_max, 10);
        let capped = Config::from_toml("[self]\nexams_day_max = 3\n").unwrap();
        assert_eq!(capped.self_improve.exams_day_max, 3);
        let b = Config::from_toml("[self.budget]\nbranch_usd = 4.5\nday_usd = 12.0\n").unwrap();
        assert_eq!(b.self_improve.budget.branch_usd, 4.5);
        assert_eq!(b.self_improve.budget.day_usd, 12.0);
        assert!(Config::from_toml("[self.budget]\nweek_usd = 1.0\n").is_err());
    }

    /// `[self] sealed_dir` is unset by default, the template carries the key
    /// commented, and a path inside a workspace root, the state dir or the
    /// repository (or holding one) is refused at load, by the key's name.
    #[test]
    fn a_sealed_dir_must_sit_outside_every_place_a_job_can_reach() {
        let d = tempfile::tempdir().unwrap();
        let base = d.path().canonicalize().unwrap();
        let (work, state, repo) = (base.join("work"), base.join("state"), base.join("repo"));
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        let load = |sealed: &str, ro: &[String]| {
            let mut cfg = Config::example();
            cfg.server.state_dir = state.to_string_lossy().into_owned();
            cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
            cfg.tools.roots = vec![];
            cfg.tools.cwd = Some(repo.join("src").to_string_lossy().into_owned());
            cfg.sandbox.ro_paths = ro.to_vec();
            cfg.self_improve.sealed_dir = Some(sealed.to_string());
            match cfg.validate() {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("{e:#}"),
            }
        };
        assert!(Config::from_toml("")
            .unwrap()
            .self_improve
            .sealed_dir
            .is_none());
        assert!(Config::EXAMPLE_TOML.contains("# sealed_dir = \"/srv/theseus-sealed\""));
        let outside = base.join("sealed");
        assert_eq!(load(outside.to_str().unwrap(), &[]), "ok");
        for (path, what) in [
            (work.join("exams"), "inside [tools] projects_dir"),
            (work.clone(), "inside [tools] projects_dir"),
            (state.join("sealed"), "inside the state dir"),
            (repo.join("tests").join("sealed"), "inside the repository"),
            (repo.clone(), "holds [tools] cwd"),
        ] {
            let e = load(path.to_str().unwrap(), &[]);
            assert!(
                e.contains("[self] sealed_dir ") && e.contains(what),
                "{path:?}: {e}"
            );
        }
        // Holding a place is as bad as sitting in one.
        let e = load(base.to_str().unwrap(), &[]);
        assert!(e.contains("[self] sealed_dir holds"), "{e}");
        // A link into a root resolves first.
        std::os::unix::fs::symlink(&work, base.join("via")).unwrap();
        let e = load(base.join("via").join("deep").to_str().unwrap(), &[]);
        assert!(e.contains("[self] sealed_dir is inside"), "{e}");
        // A read-only mount of the sealed dir is a bind into it.
        let ro = [outside.join("core").to_string_lossy().into_owned()];
        let e = load(outside.to_str().unwrap(), &ro);
        assert!(
            e.contains("[self] sealed_dir holds a [sandbox] ro_paths entry"),
            "{e}"
        );
        // Relative, empty.
        let e = load("sealed/here", &[]);
        assert!(e.contains("must be an absolute path"), "{e}");
        let e = load(" ", &[]);
        assert!(e.contains("[self] sealed_dir is empty"), "{e}");
    }

    /// The backlog's two keys default to one start and twenty open items,
    /// the template names them, and a value of the owner's loads.
    #[test]
    fn the_backlog_keys_default_to_one_start_and_twenty_open() {
        let none = Config::from_toml("").unwrap();
        assert_eq!(none.self_improve.backlog_starts, 1);
        assert_eq!(none.self_improve.backlog_max_open, 20);
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("# backlog_starts = 1") && t.contains("# backlog_max_open = 20"),
            "the template names the backlog's keys with their defaults"
        );
        let cfg = Config::from_toml("[self]\nbacklog_starts = 0\nbacklog_max_open = 5\n").unwrap();
        assert_eq!(cfg.self_improve.backlog_starts, 0);
        assert_eq!(cfg.self_improve.backlog_max_open, 5);
        assert!(Config::from_toml("[self]\nbacklog_starts = -1\n").is_err());
    }

    /// `[self] repo` and `remote` are unset by default (no self branches,
    /// none pushed), the template names both, and each reads as written.
    #[test]
    fn the_self_repo_and_remote_are_unset_by_default() {
        let none = Config::from_toml("").unwrap().self_improve;
        assert_eq!((none.repo, none.remote), (None, None));
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("\n# repo = \"") && t.contains("\n# remote = \"origin\""),
            "the template names [self] repo and remote"
        );
        let cfg =
            Config::from_toml("[self]\nmode = \"act\"\nrepo = \"/srv/ship\"\nremote = \"fleet\"\n")
                .unwrap()
                .self_improve;
        assert_eq!(cfg.repo.as_deref(), Some("/srv/ship"));
        assert_eq!(cfg.remote.as_deref(), Some("fleet"));
    }
}
