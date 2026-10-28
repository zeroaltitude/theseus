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
//! - `[self.sealed]` (theseus-pw1q.5, `config/self_sealed.rs`): the sealed
//!   holdout sets, out of Theseus's reach (`crate::rsi::sealed`): `dir` (unset:
//!   none; absolute and apart from every place a job can reach, refused at load
//!   by the key's name), `every_branch`, `item_usd` and `drift_every_days`.
//! - `backlog_starts` (unset: the pipeline's free build slots) and
//!   `backlog_max_open` (20): the nightly backlog (`crate::rsi::backlog`,
//!   theseus-pw1q.7) starts at most this many top unmarked items a night
//!   (a value set wins), and keeps at most this many open.
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
//! - `reviewer_profile` (theseus-pw1q.9): the profile the self-join's
//!   independent review runs on, `"fable"` by default. Its model must differ
//!   from the branch builder's, or the review is refused with its reason.
//! - `join_lock` (theseus-pw1q.9): the mkdir lock every self-join holds,
//!   `<state>/self/join.lock` when unset. The owner's machine points it at
//!   the outer loop's join lock, so the two never join at once.
//! - `install` (theseus-pw1q.9): `false` by default. `true` (the owner's
//!   standing OK) builds and installs a joined merge on this machine and
//!   restarts the daemon onto it, through a transient `systemd-run --user`
//!   unit.
//! - `narrow_every_days` (7), `narrow_days` (14) and `narrow_read_pages`
//!   (400): the narrowing of the hand roles' policies (theseus-pw1q.15): a
//!   role is looked at at most this often, from this many days of CloudTrail,
//!   reading at most this many pages (AWS reads, never model spend).
//! - `revert_window_hours` (theseus-pw1q.10): `24` by default. For this many
//!   hours after a self-join, a red the join caused reverts it on its own
//!   (`rsi::revert`); `0` watches nothing. The owner's veto works at any time.
//! - `pipeline_builds` (theseus-pw1q.16): how many self builds run at once,
//!   1 by default (the owner raises it); the pipeline starts another only
//!   while fewer build and fewer self trees are open.
//! - `min_free_gb` (theseus-pw1q.16): the free space, in GB, the build trees'
//!   filesystem must keep for the pipeline to start a build (40).

use serde::{Deserialize, Deserializer, Serialize};
pub use theseus_protocol::rsi::SelfMode;

pub use super::self_sealed::SelfSealedConfig;

/// `[self]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfImproveConfig {
    #[serde(default, deserialize_with = "mode")]
    pub mode: SelfMode,
    #[serde(default, deserialize_with = "digest")]
    pub digest: SelfDigest,
    #[serde(default)]
    pub budget: SelfBudgetConfig,
    #[serde(default)]
    pub mint: super::self_mint::SelfMintConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default)]
    pub sealed: SelfSealedConfig,
    /// Items the nightly backlog starts a night (0 files and ranks only).
    /// Unset: the pipeline's free build slots ([`Self::night_starts`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlog_starts: Option<u32>,
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
    /// The self-join's reviewer (theseus-pw1q.9).
    #[serde(default = "default_reviewer_profile")]
    pub reviewer_profile: String,
    /// The self-join's lock directory; none: `<state>/self/join.lock`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_lock: Option<String>,
    /// Build, install and restart onto a joined merge (theseus-pw1q.9).
    #[serde(default)]
    pub install: bool,
    /// A hand role is narrowed at most this often (days).
    #[serde(default = "default_narrow_every_days")]
    pub narrow_every_days: u32,
    /// The window of CloudTrail a narrowing reads (days, 1 to 90).
    #[serde(default = "default_narrow_days")]
    pub narrow_days: u32,
    /// The most CloudTrail pages (50 events each) one narrowing reads.
    #[serde(default = "default_narrow_read_pages")]
    pub narrow_read_pages: u32,
    /// Hours after a self-join in which a red it caused reverts it
    /// (theseus-pw1q.10); 0 watches nothing.
    #[serde(default = "default_revert_window_hours")]
    pub revert_window_hours: u64,
    /// Self builds at once (theseus-pw1q.16).
    #[serde(default = "default_pipeline_builds")]
    pub pipeline_builds: u32,
    /// Free GB the build trees' filesystem keeps before a build starts.
    #[serde(default = "default_min_free_gb")]
    pub min_free_gb: u64,
}

fn default_narrow_every_days() -> u32 {
    7
}

fn default_narrow_days() -> u32 {
    14
}

fn default_narrow_read_pages() -> u32 {
    400
}

/// `[self] revert_window_hours`'s default.
pub const REVERT_WINDOW_HOURS: u64 = 24;

fn default_revert_window_hours() -> u64 {
    REVERT_WINDOW_HOURS
}

fn default_pipeline_builds() -> u32 {
    1
}

fn default_min_free_gb() -> u64 {
    40
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

fn default_reviewer_profile() -> String {
    "fable".into()
}

fn default_backlog_max_open() -> u32 {
    20
}

impl Default for SelfImproveConfig {
    fn default() -> Self {
        Self {
            mode: SelfMode::default(),
            digest: SelfDigest::default(),
            budget: SelfBudgetConfig::default(),
            mint: Default::default(),
            repo: None,
            remote: None,
            sealed: SelfSealedConfig::default(),
            backlog_starts: None,
            backlog_max_open: default_backlog_max_open(),
            exams_day_max: EXAMS_DAY_MAX,
            graduate_min_calls: GRADUATE_MIN_CALLS,
            graduate_window_days: GRADUATE_WINDOW_DAYS,
            reviewer_profile: default_reviewer_profile(),
            join_lock: None,
            install: false,
            narrow_every_days: default_narrow_every_days(),
            narrow_days: default_narrow_days(),
            narrow_read_pages: default_narrow_read_pages(),
            revert_window_hours: REVERT_WINDOW_HOURS,
            pipeline_builds: default_pipeline_builds(),
            min_free_gb: default_min_free_gb(),
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

impl SelfImproveConfig {
    /// What the night may start: `backlog_starts` when set, else the
    /// pipeline's free build slots (`pipeline_builds` less `building`).
    pub fn night_starts(&self, building: usize) -> usize {
        self.backlog_starts.map_or(
            (self.pipeline_builds as usize).saturating_sub(building),
            |n| n as usize,
        )
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether the weekly digest is posted: only while self steps may run.
    pub fn posts_digest(&self) -> bool {
        self.mode == SelfMode::Act && self.digest == SelfDigest::Weekly
    }
}

impl SelfImproveConfig {
    /// `[self.sealed]` against the config it sits in: `dir` absolute, and
    /// apart from every place Theseus or a job may reach (theseus-pw1q.5),
    /// and an item's line above zero.
    pub fn validate(&self, cfg: &crate::Config) -> anyhow::Result<()> {
        if !(self.sealed.item_usd.is_finite() && self.sealed.item_usd > 0.0) {
            anyhow::bail!(
                "[self.sealed] item_usd must be above 0, not {}",
                self.sealed.item_usd
            );
        }
        let Some(raw) = self.sealed.dir.as_deref() else {
            return Ok(());
        };
        if raw.trim().is_empty() {
            anyhow::bail!("[self.sealed] dir is empty: leave it out for no sealed set");
        }
        let expanded = crate::config::expand(raw);
        if !expanded.is_absolute() {
            anyhow::bail!("[self.sealed] dir must be an absolute path, not {raw:?}");
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
                    "[self.sealed] dir is inside {what}: the sealed set must sit outside it"
                );
            }
            if theseus_tools::paths::within(&p, &sealed) {
                anyhow::bail!("[self.sealed] dir holds {what}: the sealed set must sit outside it");
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
        assert_eq!(none.self_improve, SelfImproveConfig::default());
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

    /// `[self.sealed] dir` is unset by default, the template carries the key
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
            cfg.self_improve.sealed.dir = Some(sealed.to_string());
            match cfg.validate() {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("{e:#}"),
            }
        };
        assert!(Config::from_toml("")
            .unwrap()
            .self_improve
            .sealed
            .dir
            .is_none());
        assert!(Config::EXAMPLE_TOML.contains("# dir = \"/srv/theseus-sealed\""));
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
                e.contains("[self.sealed] dir ") && e.contains(what),
                "{path:?}: {e}"
            );
        }
        // Holding a place is as bad as sitting in one.
        let e = load(base.to_str().unwrap(), &[]);
        assert!(e.contains("[self.sealed] dir holds"), "{e}");
        // A link into a root resolves first.
        std::os::unix::fs::symlink(&work, base.join("via")).unwrap();
        let e = load(base.join("via").join("deep").to_str().unwrap(), &[]);
        assert!(e.contains("[self.sealed] dir is inside"), "{e}");
        // A read-only mount of the sealed dir is a bind into it.
        let ro = [outside.join("core").to_string_lossy().into_owned()];
        let e = load(outside.to_str().unwrap(), &ro);
        assert!(
            e.contains("[self.sealed] dir holds a [sandbox] ro_paths entry"),
            "{e}"
        );
        // Relative, empty.
        let e = load("sealed/here", &[]);
        assert!(e.contains("must be an absolute path"), "{e}");
        let e = load(" ", &[]);
        assert!(e.contains("[self.sealed] dir is empty"), "{e}");
    }

    /// `[self.sealed]` (theseus-pw1q.21.5): no set, no scoring of every branch,
    /// a quarter-dollar item and a weekly drift check by default, all in the
    /// template; `[self] sealed_dir` is refused, naming its new place; an
    /// item's line must be above zero.
    #[test]
    fn the_sealed_table_has_its_defaults_and_the_old_key_is_renamed() {
        let none = Config::from_toml("").unwrap().self_improve.sealed;
        assert_eq!(none, SelfSealedConfig::default());
        assert_eq!(
            (
                none.dir,
                none.every_branch,
                none.item_usd,
                none.drift_every_days
            ),
            (None, false, 0.25, 7)
        );
        let t = Config::EXAMPLE_TOML;
        for k in [
            "# [self.sealed]",
            "# every_branch = false",
            "# item_usd = 0.25",
            "# drift_every_days = 7",
        ] {
            assert!(t.contains(k), "the template names {k}");
        }
        let c = Config::from_toml(
            "[self.sealed]\nevery_branch = true\nitem_usd = 0.5\ndrift_every_days = 3\n",
        )
        .unwrap()
        .self_improve
        .sealed;
        assert_eq!(
            (c.every_branch, c.item_usd, c.drift_every_days),
            (true, 0.5, 3)
        );
        assert!(Config::from_toml("[self.sealed]\nweekly = true\n").is_err());
        let e = format!(
            "{:#}",
            Config::parse("[self]\nsealed_dir = \"/srv/sealed\"\n").unwrap_err()
        );
        assert!(
            e.contains("`[self] sealed_dir` was renamed to `[self.sealed] dir`"),
            "{e}"
        );
        for bad in ["0", "-1.0"] {
            let e = format!(
                "{:#}",
                Config::parse(&format!(
                    "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n[self.sealed]\nitem_usd = {bad}\n"
                ))
                .unwrap_err()
            );
            assert!(e.contains("[self.sealed] item_usd must be above 0"), "{e}");
        }
    }

    /// The backlog's two keys default to one start and twenty open items,
    /// the template names them, and a value of the owner's loads.
    #[test]
    fn the_backlog_keys_default_to_one_start_and_twenty_open() {
        let none = Config::from_toml("").unwrap();
        assert_eq!(none.self_improve.backlog_starts, None);
        assert_eq!(none.self_improve.night_starts(0), 1);
        assert_eq!(none.self_improve.backlog_max_open, 20);
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("# backlog_starts = 1") && t.contains("# backlog_max_open = 20"),
            "the template names the backlog's keys with their defaults"
        );
        let cfg = Config::from_toml("[self]\nbacklog_starts = 0\nbacklog_max_open = 5\n").unwrap();
        assert_eq!(cfg.self_improve.backlog_starts, Some(0));
        assert_eq!(cfg.self_improve.backlog_max_open, 5);
        assert!(Config::from_toml("[self]\nbacklog_starts = -1\n").is_err());
        let slots = Config::from_toml("[self]\npipeline_builds = 3\n").unwrap();
        assert_eq!(slots.self_improve.night_starts(1), 2, "free slots, unset");
        assert_eq!(cfg.self_improve.night_starts(1), 0, "a set value wins");
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

    /// The self-join's keys (theseus-pw1q.9): the reviewer is fable, the lock
    /// unset (the state dir's), no install; the template names all three.
    #[test]
    fn the_join_keys_default_to_fable_the_state_dirs_lock_and_no_install() {
        let none = Config::from_toml("").unwrap().self_improve;
        assert_eq!(none.reviewer_profile, "fable");
        assert_eq!(none.join_lock, None);
        assert!(!none.install);
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("\n# reviewer_profile = \"fable\"")
                && t.contains("\n# join_lock = \"")
                && t.contains("\n# install = false"),
            "the template names the join's keys with their defaults"
        );
        let cfg = Config::from_toml(
            "[self]\nreviewer_profile = \"opus\"\njoin_lock = \"/srv/joins.lock\"\ninstall = true\n",
        )
        .unwrap()
        .self_improve;
        assert_eq!(cfg.reviewer_profile, "opus");
        assert_eq!(cfg.join_lock.as_deref(), Some("/srv/joins.lock"));
        assert!(cfg.install);
    }

    /// The narrowing's keys have defaults, are in the template, and take a
    /// value of the owner's.
    #[test]
    fn the_narrowing_keys_have_defaults_and_are_in_the_template() {
        let none = Config::from_toml("").unwrap().self_improve;
        assert_eq!(
            (
                none.narrow_every_days,
                none.narrow_days,
                none.narrow_read_pages
            ),
            (7, 14, 400)
        );
        let t = Config::EXAMPLE_TOML;
        for k in [
            "# narrow_every_days = 7",
            "# narrow_days = 14",
            "# narrow_read_pages = 400",
        ] {
            assert!(t.contains(k), "the template names {k}");
        }
        let c = Config::from_toml("[self]\nnarrow_every_days = 3\nnarrow_days = 30\n").unwrap();
        assert_eq!(
            (c.self_improve.narrow_every_days, c.self_improve.narrow_days),
            (3, 30)
        );
    }

    /// Auto-revert's window (theseus-pw1q.10): a day by default, 0 watches
    /// nothing; the template names it.
    #[test]
    fn the_revert_window_defaults_to_a_day_and_the_template_names_it() {
        let none = Config::from_toml("").unwrap().self_improve;
        assert_eq!(none.revert_window_hours, 24);
        assert!(Config::EXAMPLE_TOML.contains("\n# revert_window_hours = 24"));
        let cfg = Config::from_toml("[self]\nrevert_window_hours = 6\n")
            .unwrap()
            .self_improve;
        assert_eq!(cfg.revert_window_hours, 6);
    }

    /// The pipeline's keys (theseus-pw1q.16): one build at once and 40 GB
    /// kept free by default; the template names both; the owner's load.
    #[test]
    fn the_pipeline_keys_default_to_one_build_and_forty_gb() {
        let none = Config::from_toml("").unwrap().self_improve;
        assert_eq!((none.pipeline_builds, none.min_free_gb), (1, 40));
        let t = Config::EXAMPLE_TOML;
        assert!(
            t.contains("\n# pipeline_builds = 1") && t.contains("\n# min_free_gb = 40"),
            "the template names the pipeline's keys with their defaults"
        );
        let cfg = Config::from_toml("[self]\npipeline_builds = 2\nmin_free_gb = 8\n")
            .unwrap()
            .self_improve;
        assert_eq!((cfg.pipeline_builds, cfg.min_free_gb), (2, 8));
        assert!(Config::from_toml("[self]\npipeline_builds = -1\n").is_err());
    }
}
