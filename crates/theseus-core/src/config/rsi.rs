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
//! - `[self.budget]`: the self budget's lines, an empty table until the
//!   self-budget row gives it `branch_usd` and `day_usd`.
//! - `sealed_dir` (theseus-pw1q.5): where sealed holdout sets live, out of
//!   Theseus's reach (`crate::rsi::sealed`). Unset (the default) means no
//!   sealed set. It must be an absolute path outside every workspace root,
//!   the state dir, the repository and every `[sandbox] ro_paths` entry
//!   (neither inside one nor holding one), refused at load by the key's name.

use serde::{Deserialize, Deserializer, Serialize};
pub use theseus_protocol::rsi::SelfMode;

/// `[self]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfConfig {
    #[serde(default, deserialize_with = "mode")]
    pub mode: SelfMode,
    #[serde(default, deserialize_with = "digest")]
    pub digest: SelfDigest,
    #[serde(default)]
    pub budget: SelfBudgetConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_dir: Option<String>,
}

/// `[self] digest`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfDigest {
    #[default]
    Weekly,
    Off,
}

/// `[self.budget]`: no key yet (the self-budget row adds its two).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfBudgetConfig {}

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
                && t.contains("# [self.budget]"),
            "the template names [self], its two keys and [self.budget]"
        );
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
}
