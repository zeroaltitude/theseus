//! The nightly sweep's plan (theseus-63tfk, R5 slice one): what the sweep
//! writes over the ontology as it stands, with no model and no clock but the
//! `now_ms` it is given, so it is the same on any host. Two things, both
//! reversible by `Ontology::undo`:
//!
//! - **Decay.** A membership a machine origin wrote (Jev, the sweep, the
//!   dream) that is older than [`DECAY_MS`] (30 days since its `as_of_ms`;
//!   Jev's next filing of the same category renews it) is dropped from its
//!   list. The operator's and the import's memberships are never touched.
//! - **Merge.** Two categories of one kind under one parent whose names fold
//!   to the same text (lowercase letters and digits only: `Rust Harness`,
//!   `rust-harness`) become one: the survivor is the one the operator wrote,
//!   else the one more sessions hold, else the smaller id; the others merge
//!   into it when a machine wrote them, they have no guidance and no
//!   children, and no session's operator or import membership names them.
//!
//! Every record of the plan is checked as the sweep's write against the
//! ontology with the steps before it applied, and a step that does not check
//! (a kind the sweep may not assign, an operator's membership in the way) is
//! left out: the plan is only what the sweep may write.

use std::collections::BTreeMap;

use crate::category::{CategoryId, MemberList};
use crate::kind::Origin;
use crate::machine::machine_written;
use crate::ontology::Ontology;
use crate::Record;

/// How long a machine's membership stands unconfirmed: 30 days.
pub const DECAY_MS: u64 = 30 * 24 * 3_600_000;

/// One write of the sweep: its records go in one frame, so each is one
/// undo.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Memberships dropped: each session's and the category dropped from it.
    Decay {
        dropped: Vec<(String, CategoryId)>,
        records: Vec<Record>,
    },
    /// `absorbed` written into `survivor`, with the sessions it moved.
    Merge {
        absorbed: CategoryId,
        survivor: CategoryId,
        sessions: Vec<String>,
        records: Vec<Record>,
    },
}

impl Step {
    pub fn records(&self) -> &[Record] {
        match self {
            Step::Decay { records, .. } | Step::Merge { records, .. } => records,
        }
    }

    /// A short word for the write's id: `decay`, or `merge`.
    pub fn word(&self) -> &'static str {
        match self {
            Step::Decay { .. } => "decay",
            Step::Merge { .. } => "merge",
        }
    }
}

/// A name as the merge compares it: its letters and digits, lowercase.
pub fn fold(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl Ontology {
    /// The sweep's writes at `now_ms`, decay first, then merges, in a fixed
    /// order. Empty when there is nothing to do, which is most nights.
    pub fn sweep_plan(&self, now_ms: u64) -> Vec<Step> {
        let mut working = self.clone();
        let mut plan = Vec::new();
        if let Some(step) = working.plan_decay(now_ms) {
            plan.push(step);
        }
        plan.extend(working.plan_merges());
        plan
    }

    /// Decay as one step, applied to `self`; the lists the sweep may not
    /// write are left as they are.
    fn plan_decay(&mut self, now_ms: u64) -> Option<Step> {
        let lists: Vec<MemberList> = self.lists().cloned().collect();
        let (mut dropped, mut records) = (Vec::new(), Vec::new());
        for l in lists {
            let stale = |m: &crate::category::Membership| {
                m.origin.is_machine() && now_ms.saturating_sub(m.as_of_ms) > DECAY_MS
            };
            if !l.members.iter().any(stale) {
                continue;
            }
            let next = MemberList {
                members: l.members.iter().filter(|m| !stale(m)).cloned().collect(),
                ..l.clone()
            };
            let record = Record::Members(next);
            if self.put(record.clone(), Origin::Sweep).is_err() {
                continue;
            }
            dropped.extend(
                l.members
                    .iter()
                    .filter(|m| stale(m))
                    .map(|m| (l.session.clone(), m.category.clone())),
            );
            records.push(record);
        }
        (!records.is_empty()).then_some(Step::Decay { dropped, records })
    }

    /// The merges of duplicate categories, each applied to `self` as it is
    /// planned.
    fn plan_merges(&mut self) -> Vec<Step> {
        type Group = (String, Option<CategoryId>, String);
        let mut groups: BTreeMap<Group, Vec<CategoryId>> = BTreeMap::new();
        for c in self.categories() {
            let folded = fold(&c.name);
            if folded.is_empty() {
                continue;
            }
            let key = (c.kind().to_string(), c.parent.clone(), folded);
            groups.entry(key).or_default().push(c.id.clone());
        }
        let counts = self.member_counts();
        let mut steps = Vec::new();
        for (_, mut ids) in groups.into_iter().filter(|(_, ids)| ids.len() > 1) {
            let rank = |id: &CategoryId| {
                let machine = self
                    .category(id)
                    .is_some_and(|c| machine_written(&c.added_by));
                (
                    machine,
                    std::cmp::Reverse(counts.get(id).copied().unwrap_or(0)),
                    id.clone(),
                )
            };
            ids.sort_by_key(rank);
            let survivor = ids.remove(0);
            for absorbed in ids {
                let Ok(merge) = self.merge_category(&absorbed, &survivor, Origin::Sweep) else {
                    continue;
                };
                let mut trial = self.clone();
                if merge
                    .records
                    .iter()
                    .any(|r| trial.put(r.clone(), Origin::Sweep).is_err())
                {
                    continue;
                }
                *self = trial;
                steps.push(Step::Merge {
                    absorbed,
                    survivor: survivor.clone(),
                    sessions: merge.sessions,
                    records: merge.records,
                });
            }
        }
        steps
    }
}
