//! The data floor in the config (theseus-a1wvq.3): a background call whose
//! input is the owner's private material never names a profile of restricted
//! origin (`cn` or `unknown`). The provider chokepoint would refuse each of
//! its requests anyway; the load says so once, in words, instead.

use anyhow::Result;

use super::Config;
use crate::catalog::{origin_of, Catalog, ModelOrigin};

impl Config {
    /// The keys whose profile reads the owner's material, with the profile
    /// each names. `[memory] synth_profile` is one (theseus-z1ros): a
    /// consolidation mixes many sessions, so no open request's pass can ever
    /// clear it; its word `session` is no profile and stays the run time's
    /// (`consolidate/run.rs`, `unrestricted`). `[memory] summary_profile` is
    /// not: a compaction in an open request may use a restricted model with
    /// a pass of its own (`turn/compaction/gate.rs`), and falls back
    /// otherwise.
    ///
    /// An unset key asks Jev (route.v10, theseus-ej6bj), so it names no profile
    /// to check here: a model Jev picks is refused by the job rule at the ask.
    fn owner_material_profiles(&self) -> [(&'static str, Option<&str>); 8] {
        [
            (
                "[people] extract_profile",
                self.people
                    .extract_profile
                    .as_deref(),
            ),
            (
                "[card] profile",
                self.card.profile.as_deref(),
            ),
            (
                "[threads] import_profile",
                self.threads
                    .import_profile
                    .as_deref(),
            ),
            (
                "[judge.learn] writer_profile",
                self.judge
                    .learn
                    .writer_profile
                    .as_deref(),
            ),
            (
                "[self] reviewer_profile",
                self.self_improve
                    .reviewer_profile
                    .as_deref(),
            ),
            ("[initiative] profile", self.initiative.profile.as_deref()),
            (
                "[ontology] name_profile",
                self.ontology
                    .name_profile
                    .as_deref(),
            ),
            (
                "[memory] synth_profile",
                self.memory.synth_profile.as_deref(),
            ),
        ]
    }

    /// Refuse a restricted profile for a call over the owner's material, and
    /// a `[judge] api_base` of China origin: Jev reads every message and calls
    /// outside the core's providers, so the chokepoint never sees its
    /// requests. A profile that is not configured is left to its own check.
    pub(super) fn validate_data_floor(&self) -> Result<()> {
        let catalog = Catalog::with_overrides(&self.catalog);
        for (key, name) in self.owner_material_profiles() {
            let Some(name) = name.filter(|n| self.all_profiles().contains_key(*n)) else {
                continue;
            };
            if self.profile_origin(name, &catalog).restricted() {
                anyhow::bail!(
                    "{key} may not name a profile of restricted origin: its input is the owner's \
                     private material (`{name}`)"
                );
            }
        }
        if origin_of(&self.judge.api_base, None, "", None) == ModelOrigin::Cn {
            anyhow::bail!(
                "[judge] api_base may not name an endpoint of restricted origin: its input is the \
                 owner's private material"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Set = fn(&mut Config, &str);

    /// Each key of a call over the owner's material, and how a test sets it.
    const KEYS: [(&str, Set); 8] = [
        ("[people] extract_profile", |c, p| {
            c.people.extract_profile = Some(p.into())
        }),
        ("[card] profile", |c, p| c.card.profile = Some(p.into())),
        ("[threads] import_profile", |c, p| {
            c.threads.import_profile = Some(p.into())
        }),
        ("[judge.learn] writer_profile", |c, p| {
            c.judge.learn.writer_profile = Some(p.into())
        }),
        ("[self] reviewer_profile", |c, p| {
            c.self_improve.reviewer_profile = Some(p.into())
        }),
        ("[initiative] profile", |c, p| {
            c.initiative.profile = Some(p.into())
        }),
        ("[ontology] name_profile", |c, p| {
            c.ontology.name_profile = Some(p.into())
        }),
        ("[memory] synth_profile", |c, p| {
            c.memory.synth_profile = Some(p.into())
        }),
    ];

    /// A restricted profile for any of them is refused in words; an
    /// unrestricted one loads. So is a `[judge] api_base` of China origin.
    #[test]
    fn background_profiles_refuse_a_restricted_profile() {
        for (key, set) in KEYS {
            let mut c = Config::example();
            set(&mut c, "glm");
            let e = c.validate().unwrap_err().to_string();
            assert!(
                e.contains(key) && e.contains("may not name a profile of restricted origin"),
                "{key}: {e}"
            );
            let mut c = Config::example();
            set(&mut c, "haiku");
            c.validate().unwrap_or_else(|e| panic!("{key}: {e:#}"));
        }
        let mut c = Config::example();
        c.judge.api_base = "https://api.deepseek.com/anthropic".into();
        let e = c.validate().unwrap_err().to_string();
        assert!(e.contains("[judge] api_base"), "{e}");
        let mut c = Config::example();
        c.judge.api_base = "https://jev.example.com/v1".into();
        c.validate().unwrap();
    }

    /// A restricted `[memory] summary_profile` still loads: a compaction in
    /// an open request may run there with a pass of its own, and any other
    /// falls back at run time (theseus-z1ros). The word `session` for
    /// `synth_profile` names no profile, and loads.
    #[test]
    fn a_restricted_summary_profile_still_loads() {
        let mut c = Config::example();
        c.memory.summary_profile = Some("glm".into());
        c.validate().unwrap_or_else(|e| panic!("{e:#}"));
        let mut c = Config::example();
        c.memory.synth_profile = Some(crate::config::memory::SUMMARY_SESSION.into());
        c.validate().unwrap_or_else(|e| panic!("{e:#}"));
    }
}
