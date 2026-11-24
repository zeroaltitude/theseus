//! The sealed check of one try's merge (theseus-pw1q.21.7), between main's
//! gate and the signature: the module doc of `rsi::sealed::hold` says what
//! it holds on and why. It runs when the branch's diff against the try's
//! main touches a behaviour path, or `[self.sealed] every_branch` is set.
//!
//! The candidate is the merge's own build, which main's gate has just made
//! in the merge's tree (a branch's own build tree is gone once self-pr has
//! closed it). Main is the installed build, and only while that build is the
//! try's main commit: when it is another (main moved past the install, or the
//! install has not finished), nothing is scored and the join stops before
//! main's gate, to be tried again (`main_build_clear`). Main's score is cached
//! for the try's main commit and the sets' digest. The row names main's commit
//! and the commit of the build scored as main's. The row (`self.sealed.checked`) is
//! checked against the sets' own strings before it is written, as the
//! scorer's output is.
//!
//! A hold is a measurement, not a keel card: `self.join.stopped` with phase
//! `sealed` and the numbers (the head is not tried again, a new head is) and
//! the item back on the backlog with the same evidence. A check that could
//! not run is a stop that the next pass tries again.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::joining::Merged;
use super::{blocking, git, Run};
use crate::fact::rsi_sealed::SelfSealedChecked;
use crate::fact::Fact;
use crate::rsi::sealed::hold::{self, Verdict, Whose};

impl Run<'_> {
    /// Before main's gate: true unless the sealed check will score `m` and the installed build, the
    /// only build of main it has, is not the try's main commit. Then the join stops, to be tried again.
    pub(super) async fn main_build_clear(&mut self, m: &Merged) -> bool {
        match self.waits_for_main(m).await {
            Ok(None) => true,
            Ok(Some(installed)) => {
                let said = installed.as_deref().map_or_else(
                    || "a build that names no commit".to_string(),
                    |c| short(c).to_string(),
                );
                let why = format!(
                    "the sealed check scores main at {} on that commit's own build, and the installed \
                     build is {said}: it waits for main's install",
                    short(&m.base)
                );
                self.stop(
                    "sealed",
                    &why,
                    json!({"main": m.base, "installed": installed}),
                );
                false
            }
            Err(e) => {
                let why = format!("the sealed check could not run: {e:#}");
                self.stop("sealed", &why, Value::Null);
                false
            }
        }
    }

    /// The installed build's commit when the check will score `m` and that build is not main's; none
    /// when the check may go on.
    async fn waits_for_main(&self, m: &Merged) -> Result<Option<Option<String>>> {
        let installed = (self.seams.installed)();
        if installed.as_deref() == Some(m.base.as_str()) || !self.sealed_due(m).await? {
            return Ok(None);
        }
        match self.core.sealed() {
            Some(s) if s.counts().values().any(|n| *n > 0) => Ok(Some(installed)),
            _ => Ok(None),
        }
    }

    /// Whether the branch's diff against the try's main touches a behaviour path, or
    /// `[self.sealed] every_branch` is set.
    async fn sealed_due(&self, m: &Merged) -> Result<bool> {
        let (p, base, head) = (self.place.clone(), m.base.clone(), self.b.head.clone());
        let paths: Vec<String> = blocking(move || git::paths(&p, &base, &head))
            .await?
            .into_iter()
            .map(|(_, path)| path)
            .collect();
        Ok(hold::touches(&paths).is_some() || self.core.cfg.self_improve.sealed.every_branch)
    }

    /// The sealed check of `m`: true when the join may go on.
    pub(super) async fn sealed_clear(&mut self, m: &Merged) -> bool {
        if !self.phase("sealed") {
            return false;
        }
        match self.sealed_check(m).await {
            Ok(go) => go,
            Err(e) => {
                let why = format!("the sealed check could not run: {e:#}");
                self.stop("sealed", &why, Value::Null);
                false
            }
        }
    }

    async fn sealed_check(&mut self, m: &Merged) -> Result<bool> {
        let core = self.core.clone();
        if !self.sealed_due(m).await? {
            return Ok(true);
        }
        let Some(sealed) = core.sealed().cloned() else {
            return self.sealed_skipped("[self.sealed] dir is not set");
        };
        if sealed.counts().values().all(|n| *n == 0) {
            return self.sealed_skipped("the sealed sets hold no item");
        }
        let scorer = self.seams.sealed.clone();
        let branch = self.b.branch.clone();
        let main = Whose {
            branch: branch.clone(),
            head: m.base.clone(),
            bins: installed_bins()?,
        };
        let (before, cached) = core.sealed_main(&*scorer, &main).await?;
        let candidate = Whose {
            branch,
            head: self.b.head.clone(),
            bins: m.t.target.join("debug"),
        };
        let after = scorer.score(&core, &candidate).await?;
        let verdict = hold::judge(&before, &after);
        let outcome = if verdict.holds() { "held" } else { "passed" };
        let build = (self.seams.installed)();
        let fact = SelfSealedChecked {
            branch: &self.b.branch,
            head: &self.b.head,
            outcome,
            reason: None,
            verdict: &verdict,
            cached,
            main: Some((&m.base, build.as_deref())),
        };
        if !sealed.allows(&fact.row().to_string())? {
            bail!("its numbers held a string of a sealed set, so the filter withheld them");
        }
        core.rec(None).record(&fact);
        if verdict.holds() {
            self.sealed_held(&verdict)?;
            return Ok(false);
        }
        Ok(true)
    }

    /// The phase did not run, and its row says why.
    fn sealed_skipped(&self, reason: &str) -> Result<bool> {
        let verdict = Verdict::default();
        self.core.rec(None).record(&SelfSealedChecked {
            branch: &self.b.branch,
            head: &self.b.head,
            outcome: "skipped",
            reason: Some(reason),
            verdict: &verdict,
            cached: false,
            main: None,
        });
        Ok(true)
    }

    /// The hold: the stop, with the numbers, and the item back on the backlog.
    fn sealed_held(&mut self, v: &Verdict) -> Result<()> {
        let numbers = json!({"before": v.before, "after": v.after, "total": v.total,
                             "holds": v.holds, "reported": v.reported, "broken": v.broken});
        let identity = format!(
            "sealed:{}@{}",
            self.b.branch,
            &self.b.head[..self.b.head.len().min(12)]
        );
        let note = format!("held by the sealed sets: {} ({})", v.line(), v.why());
        let _one = self
            .core
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(task) = self.reopened(identity, note)? {
            self.core.store.append(&[task])?;
        }
        drop(_one);
        self.hold("sealed", &v.why(), numbers);
        Ok(())
    }
}

/// A commit's first 12 characters, as a reason names it.
fn short(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

/// The directory of the running daemon's own binaries: the installed main,
/// which holds the `theseusd` and `theseus` the runner copies.
pub(super) fn installed_bins() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    match exe.parent() {
        Some(dir) => Ok(dir.to_path_buf()),
        None => bail!("the daemon's binary has no directory"),
    }
}
