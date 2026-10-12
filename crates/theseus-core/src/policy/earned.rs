//! Earned autonomy (theseus-uttu; the killer-features plan, §4.8, row R18):
//! the loosening that comes from the record, proposed and never silent.
//!
//! The record already tightens: "should have asked" makes a tool ask first
//! (`tighten.rs`). This is the other direction. `find.rs` reads the ledger
//! for what the owner approved every time, and a weekly card in the owner's
//! DM offers each group back. A yes is a **grant**: the prefix (or the file
//! tool in its root) runs at `notify`, never `open`, with the notice naming
//! the grant and how to end it.
//!
//! What a grant is not:
//! - It never passes the floor, the approve lists, a path outside the
//!   roots, a private address, an AWS guard, a layer-1 task change, the
//!   external-text hold, an MCP client's floor, or a place's floor: the
//!   gate applies it only where the tool's posture alone asked
//!   (`ToolPolicy::posture_only`), and every later layer of the order still
//!   raises the posture it makes.
//! - It never acts in a shared place, and never for a tool a "should have
//!   asked" tightened (the owner's stricter word wins).
//! - It never covers a call with an `env`, or one whose words after the
//!   prefix name a URL, and never a shell or a program
//!   that runs what it is given (`find::prefix_ok`, asked again here of
//!   every grant read, so a hand-edited store gains nothing).
//! - It is the owner's to give, from a private place (`judge_act`), and
//!   anyone's to take away.
//!
//! It persists as tightenings do: one META record, written with its ledger
//! row in one frame, read when the core starts. Later rows, `scoped-grants`
//! (a "don't ask again" from a single answer) and M5's Jev decision on the
//! approval ladder, would hook in beside `gate`: the first adds a grant with
//! a call's own scope, the second a judgment before the card.

pub mod find;
pub mod measure;
#[cfg(test)]
mod tests_find;
#[cfg(test)]
mod tests_measure;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::earned::{EarnedCard, EarnedGrant, EarnedGroup};
use theseus_protocol::PlaceClass;
use theseus_store::{kinds, NewRecord};
use theseus_tools::{paths, Plan, Tool};

use super::{normalized_argv, prefix_match, Decision, Notice, Posture};
use crate::ledger::LedgerRow;
use crate::store::Store;
use crate::toolrun::ToolRuntime;

/// The `meta` key that holds the grants, the groups passed on, and the card.
pub const META_KEY: &str = "policy.earned";

/// A group the owner passed on, or a grant ended, is not proposed again for
/// 90 days.
pub const PASS_MS: u64 = 90 * 86_400_000;

/// A grant revoked by "should have asked" counts as a decline from then on.
pub const FOREVER: u64 = u64::MAX;

/// A group not to propose, and why.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Passed {
    pub until_ms: u64,
    pub why: String,
    pub at_ms: u64,
}

/// The record under `META_KEY`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stored {
    pub schema: u16,
    pub grants: Vec<EarnedGrant>,
    /// Group id → why it is not proposed.
    pub passed: BTreeMap<String, Passed>,
    /// When the last card was posted, unix ms.
    pub last_card_ms: u64,
    /// The card still waiting for answers.
    pub card: Option<EarnedCard>,
}

/// The state in force, as the store holds it.
#[derive(Default)]
pub struct Earned {
    inner: RwLock<Stored>,
}

impl Earned {
    /// Read it from the store: one keyed read.
    pub fn load(&self, store: &Store) -> Result<()> {
        *self.inner.write().unwrap() = store.get_meta::<Stored>(META_KEY)?.unwrap_or_default();
        Ok(())
    }

    pub fn snapshot(&self) -> Stored {
        self.inner.read().unwrap().clone()
    }

    /// Whether there is any grant: the gate's cheap early exit.
    pub fn any(&self) -> bool {
        !self.inner.read().unwrap().grants.is_empty()
    }

    pub fn grants(&self) -> Vec<EarnedGrant> {
        self.inner.read().unwrap().grants.clone()
    }

    /// A group is not to be proposed: a grant, on the open card (one posted
    /// within `fresh_ms`; an older card is stale, and its candidates may ride
    /// the next), or passed on and not yet due.
    pub fn skips(&self, id: &str, now_ms: u64, fresh_ms: u64) -> bool {
        let s = self.inner.read().unwrap();
        s.grants.iter().any(|g| g.group.id == id)
            || s.card.as_ref().is_some_and(|c| {
                c.proposed_ms.saturating_add(fresh_ms) > now_ms
                    && c.candidates.iter().any(|g| g.id == id)
                    && !c.answered.iter().any(|a| a == id)
            })
            || s.passed.get(id).is_some_and(|p| p.until_ms > now_ms)
    }

    /// Change the state and write it with `row` and `more`, one frame. The
    /// closure says whether it changed anything: if not, nothing is written.
    pub fn update(
        &self,
        store: &Store,
        row: &LedgerRow,
        more: Vec<NewRecord>,
        change: impl FnOnce(&mut Stored) -> bool,
    ) -> Result<bool> {
        let mut cur = self.inner.write().unwrap();
        let mut next = cur.clone();
        if !change(&mut next) {
            return Ok(false);
        }
        next.schema = 1;
        let mut records = vec![
            NewRecord::json(kinds::META, Some(META_KEY), &next)?,
            NewRecord::json(kinds::LEDGER, None, row)?,
        ];
        records.extend(more);
        store.append(&records)?;
        *cur = next;
        Ok(true)
    }

    /// The grant that covers a call, if one does.
    pub fn covering(
        &self,
        tool: &str,
        nargv: &[String],
        resources: &[PathBuf],
    ) -> Option<EarnedGrant> {
        self.inner
            .read()
            .unwrap()
            .grants
            .iter()
            .find(|g| covers(&g.group, tool, nargv, resources))
            .cloned()
    }
}

/// Whether a group covers a call: `proc.run`'s argv starts with its prefix
/// (and the prefix is one a grant may be), or a file tool's every resource
/// is under its root.
pub fn covers(g: &EarnedGroup, tool: &str, nargv: &[String], resources: &[PathBuf]) -> bool {
    if g.tool != tool {
        return false;
    }
    match &g.root {
        Some(root) => {
            let root = PathBuf::from(root);
            !resources.is_empty() && resources.iter().all(|r| paths::within(r, &root))
        }
        // What follows the prefix names no URL: a grant on `curl -s` or `git
        // clone` would otherwise be an exit for anything under the roots.
        None => {
            find::prefix_ok(&g.prefix).is_ok()
                && prefix_match(nargv, &g.prefix)
                && !nargv[g.prefix.len()..].iter().any(|w| w.contains("://"))
        }
    }
}

/// A grant's line for `theseus policy`: what it is, and the record that
/// earned it.
pub fn line_of(g: &EarnedGrant) -> String {
    let r = &g.group.record;
    format!(
        "{}: {}{} (earned: {} approvals, {} to {})",
        g.group.id,
        g.group.label,
        if g.group.open_ended && g.group.root.is_none() {
            " …"
        } else {
            ""
        },
        r.approvals,
        day(r.first_ms),
        day(r.last_ms)
    )
}

/// A day, `Oct 1`, on this machine's clock.
pub fn day(ms: u64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let l = crate::wake::local(ms);
    format!("{} {}", MONTHS[(l.month as usize).clamp(1, 12) - 1], l.day)
}

/// The gate's step for an earned grant, after the call's own policy and
/// before the secret broker's: a call the tool's posture alone makes wait,
/// in a private place, that a grant covers, runs at `notify` instead. Any
/// other decision returns as it came. Every later layer still raises it.
pub(crate) fn gate(
    rt: &ToolRuntime,
    class: PlaceClass,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    d: Decision,
) -> Decision {
    if d.posture != Posture::Approve
        || d.floor
        || class != PlaceClass::Private
        || !rt.earned.any()
        || rt.tightened.get(tool.name()).is_some()
        || input
            .get("env")
            .is_some_and(|e| !e.is_null() && !e.as_object().is_some_and(|o| o.is_empty()))
        || !rt.policy.posture_only(tool, plan)
    {
        return d;
    }
    let nargv = normalized_argv(plan.argv.as_deref().unwrap_or(&[]));
    let resources: Vec<PathBuf> = plan
        .resources
        .iter()
        .map(|r| paths::canonical_best_effort(&r.path))
        .collect();
    // A command's path arguments stay inside the roots, as an allow entry's do.
    if !nargv.is_empty() && !rt.policy.args_within_roots(plan) {
        return d;
    }
    // A file tool's resources are all inside the roots a grant names.
    let Some(g) = rt.earned.covering(tool.name(), &nargv, &resources) else {
        return d;
    };
    let r = &g.group.record;
    let rule = format!(
        "{} — notify (earned grant {}: {} approvals, {} to {}; `theseus policy revoke {}` ends it)",
        tool.name(),
        g.group.id,
        r.approvals,
        day(r.first_ms),
        day(r.last_ms),
        g.group.id
    );
    Decision {
        notify: Some(Notice {
            kind: "notify".into(),
            setting: format!("earned grant {}", g.group.id),
            rule: rule.clone(),
        }),
        ..Decision::new(Posture::Notify, rule)
    }
}

/// The setting's grant id: a notice's `setting` of `earned grant ern_…`.
pub fn grant_of_setting(setting: &str) -> Option<&str> {
    setting
        .strip_prefix("earned grant ")
        .filter(|i| i.starts_with("ern_"))
}
