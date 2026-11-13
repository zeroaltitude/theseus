//! `[self_issues]`: the repository's own open GitHub issues as a second
//! backlog source (theseus-dl4pu).
//!
//! After 1.0.6 the project's planning moves to GitHub — one issue per bucket
//! of work, each tagged with the Theseus version it is planned for — so the
//! self loop's backlog needs a second source beside the Beads rows and the
//! causes it mines from its own record.
//!
//! **It is off by default**, and it stays off until the operator both turns
//! it on and names the repository. What it reads is text other people write,
//! so it is an injection surface like any fetched page: nothing read here is
//! ever treated as an instruction, and the read is **read-only** — no code in
//! this crate opens, closes, comments on or labels an issue
//! (`crate::rsi::backlog::issues`, whose `Verb` has one variant).
//!
//! **Why this is a section of its own and not `[self]`.** The keys belong with
//! `[self]` by subject, and the card that asked for them said so. `[self]`'s
//! Rust type lives in `config/self_improve.rs`, which `scripts/keel-guard.py`
//! lists in `KEEL_FILES`: an edit to it is a `keel-file` finding that only a
//! commit signed by a key in `scripts/keel-signers` can clear, and a field on
//! `SelfImproveConfig` — or a `[self.issues]` subtable, which needs one — is
//! such an edit. So the section stands beside it, named for what it is. If
//! the owner would rather have `[self] issues_*`, the move is his signature
//! and three lines.

use serde::{Deserialize, Serialize};

/// `[self_issues]`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelfIssuesConfig {
    /// Whether the source is read at all. **Off by default** (the card's rule
    /// for anything that widens what Theseus reaches).
    #[serde(default)]
    pub enabled: bool,
    /// The repository to read, `owner/name`. Empty: the source is off
    /// whatever `enabled` says, and health and the night say so.
    #[serde(default)]
    pub repo: String,
    /// A file holding the issue JSON, as the GitHub REST API returns it for
    /// `GET /repos/{owner}/{repo}/issues?state=open` (a JSON array, or an
    /// object with an `items` array as the search endpoint returns). This is
    /// the only transport this row builds: a live read needs a token on the
    /// owner's own machine and was explicitly outside the card's bar, so the
    /// request is built and validated here
    /// (`crate::rsi::backlog::issues::Request`) and nothing sends it.
    #[serde(default)]
    pub file: String,
    /// Read only the issues labelled with this version (the owner's rule of
    /// 2026-11-12: one issue per bucket, each tagged with the version it is
    /// planned for). Empty: every open issue.
    #[serde(default)]
    pub version_label: String,
    /// The most issues one night takes as candidates, newest first.
    #[serde(default = "default_max")]
    pub max: usize,
}

fn default_max() -> usize {
    20
}

impl Default for SelfIssuesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            repo: String::new(),
            file: String::new(),
            version_label: String::new(),
            max: default_max(),
        }
    }
}

impl SelfIssuesConfig {
    /// A config that says nothing serializes to nothing.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Whether to read the source: switched on, with a repository named.
    pub fn reads(&self) -> bool {
        self.enabled && !self.repo.trim().is_empty()
    }

    /// Why the config refuses to read, in the operator's words, or `None`
    /// when it is readable. Plan-time validation, as the AWS surface does it:
    /// a misshapen repository is said once rather than failing a night.
    pub fn refusal(&self) -> Option<String> {
        if !self.enabled {
            return Some("[self_issues] enabled is false".into());
        }
        let repo = self.repo.trim();
        if repo.is_empty() {
            return Some("[self_issues] repo names no repository".into());
        }
        if crate::rsi::backlog::issues::repo_parts(repo).is_none() {
            return Some(format!(
                "[self_issues] repo must be owner/name, not {repo:?}"
            ));
        }
        if self.file.trim().is_empty() {
            return Some(
                "[self_issues] file names no issue JSON to read (a live API read is not built)"
                    .into(),
            );
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::SelfIssuesConfig;

    #[test]
    fn it_is_off_and_empty_by_default() {
        let c = SelfIssuesConfig::default();
        assert!(!c.enabled, "off by default");
        assert!(!c.reads());
        assert!(c.is_default());
        assert_eq!(
            c.refusal().as_deref(),
            Some("[self_issues] enabled is false")
        );
    }

    #[test]
    fn a_misshapen_repository_is_refused_at_plan_time_and_a_good_one_is_not() {
        let mut c = SelfIssuesConfig {
            enabled: true,
            repo: "not-a-repo".into(),
            file: "/var/example/issues.json".into(),
            ..Default::default()
        };
        assert!(c.refusal().unwrap().contains("owner/name"), "{c:?}");
        assert!(!c.reads() || c.refusal().is_some());
        c.repo = "zeroaltitude/theseus".into();
        assert_eq!(c.refusal(), None);
        assert!(c.reads());
    }

    #[test]
    fn a_named_repository_with_no_file_is_refused_rather_than_read() {
        let c = SelfIssuesConfig {
            enabled: true,
            repo: "zeroaltitude/theseus".into(),
            ..Default::default()
        };
        assert!(c.refusal().unwrap().contains("file"), "{c:?}");
    }
}
