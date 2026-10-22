//! What the sweep may ask the model about, and what it writes from the
//! answers (theseus-nzzz4). The deterministic plan (`sweep.rs`) merges two
//! categories whose names fold to one text; here a pair that only looks alike
//! is a question (`same_thing.v1`), and a category too large to say
//! anything is another (`two_things.v1`). Pure, like the plan: the model's
//! answers are arguments, never read here.
//!
//! - **Pairs** ([`Ontology::merge_pairs`]): two categories of one kind under
//!   one parent, both written by a machine, with no guidance and nothing
//!   below them, whose names are not one name once folded (the plan merges
//!   those) and share a word of [`WORD`] letters or more, or one holds the
//!   other's letters (the shorter at least [`WORD`] of them). Oldest first:
//!   by the newer of the two's earliest membership.
//! - **Splits** ([`Ontology::split_candidates`]): a category a machine wrote,
//!   of a kind whose categories nest (its parent kind is its own name),
//!   that [`SPLIT_AT`] sessions or more hold, largest first.
//! - [`Ontology::plan_split`] writes the two children and moves the sessions
//!   the model put in them, as the sweep's.

use std::collections::{BTreeMap, BTreeSet};

use crate::category::{Category, CategoryId, MemberList};
use crate::kind::Origin;
use crate::machine::machine_written;
use crate::ontology::Ontology;
use crate::sweep::{fold, Step};
use crate::Record;

/// The letters a shared word, or a name inside another, needs to make a pair.
pub const WORD: usize = 4;
/// The sessions that make a category large enough to ask about.
pub const SPLIT_AT: u64 = 12;

fn words(name: &str) -> BTreeSet<String> {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= WORD)
        .map(|w| w.chars().flat_map(char::to_lowercase).collect())
        .collect()
}

/// Whether two names look alike enough to ask: a shared word, or one name's
/// letters inside the other's.
fn alike(a: &str, b: &str) -> bool {
    let (fa, fb) = (fold(a), fold(b));
    let inside = |short: &str, long: &str| short.chars().count() >= WORD && long.contains(short);
    !words(a).is_disjoint(&words(b)) || inside(&fa, &fb) || inside(&fb, &fa)
}

impl Category {
    /// Written by a machine origin (`jev:`, `sweep:` or `dream:`), not by the
    /// operator, the import or the transport.
    pub fn by_machine(&self) -> bool {
        machine_written(&self.added_by)
    }
}

impl Ontology {
    /// A machine's category nothing else holds on to: no guidance, no child.
    pub(crate) fn loose(&self, c: &Category) -> bool {
        c.by_machine()
            && !self.guidance(&c.id).is_some_and(|g| !g.is_empty())
            && self.children(Some(&c.id)).is_empty()
    }

    /// The pairs of categories the sweep may ask `same_thing.v1` about,
    /// oldest first, in a fixed order.
    pub fn merge_pairs(&self) -> Vec<(CategoryId, CategoryId)> {
        let mut earliest: BTreeMap<&CategoryId, u64> = BTreeMap::new();
        for l in self.lists() {
            for m in &l.members {
                let at = earliest.entry(&m.category).or_insert(m.as_of_ms);
                *at = (*at).min(m.as_of_ms);
            }
        }
        let age = |id: &CategoryId| earliest.get(id).copied().unwrap_or(u64::MAX);
        let mut groups: BTreeMap<(&str, Option<&CategoryId>), Vec<&Category>> = BTreeMap::new();
        for c in self.categories().filter(|c| self.loose(c)) {
            if c.kind() != crate::person::KIND {
                groups
                    .entry((c.kind(), c.parent.as_ref()))
                    .or_default()
                    .push(c);
            }
        }
        let mut pairs: Vec<(u64, CategoryId, CategoryId)> = Vec::new();
        for group in groups.values() {
            for (i, a) in group.iter().enumerate() {
                for b in &group[i + 1..] {
                    if fold(&a.name) != fold(&b.name) && alike(&a.name, &b.name) {
                        let (x, y) = match a.id <= b.id {
                            true => (a.id.clone(), b.id.clone()),
                            false => (b.id.clone(), a.id.clone()),
                        };
                        pairs.push((age(&x).max(age(&y)), x, y));
                    }
                }
            }
        }
        pairs.sort();
        pairs.into_iter().map(|(_, a, b)| (a, b)).collect()
    }

    /// The categories the sweep may ask `two_things.v1` about, largest first.
    pub fn split_candidates(&self) -> Vec<CategoryId> {
        let counts = self.member_counts();
        let mut large: Vec<(u64, CategoryId)> = self
            .categories()
            .filter(|c| c.by_machine())
            .filter(|c| {
                self.kind(c.kind())
                    .is_some_and(|k| k.parent.as_deref() == Some(k.name.as_str()))
            })
            .filter_map(|c| {
                let n = counts.get(&c.id).copied().unwrap_or(0);
                (n >= SPLIT_AT).then(|| (n, c.id.clone()))
            })
            .collect();
        large.sort_by(|(na, a), (nb, b)| nb.cmp(na).then_with(|| a.cmp(b)));
        large.into_iter().map(|(_, id)| id).collect()
    }

    /// `parent` split in two, as the sweep's write, applied to `self`: the
    /// children are `names`, and each session of `moved[i]` holds child `i`
    /// in place of `parent`, where its membership of `parent` is a
    /// machine's (an operator's or the import's stays). Every other session
    /// stays in `parent`. None when a child has no session to move into it,
    /// or a record does not check.
    pub fn plan_split(
        &mut self,
        parent: &CategoryId,
        names: [&str; 2],
        moved: [&[String]; 2],
        write: &str,
    ) -> Option<Step> {
        let kind = parent.kind().to_string();
        let by = format!("sweep:{write}");
        let mut trial = self.clone();
        let mut children: Vec<CategoryId> = Vec::new();
        let mut records: Vec<Record> = Vec::new();
        for name in names {
            let id = trial.mint_id(&kind, name).ok()?;
            let child = Category {
                parent: Some(parent.clone()),
                ..Category::new(id.clone(), name, by.clone())
            };
            trial
                .put(Record::Category(child.clone()), Origin::Sweep)
                .ok()?;
            records.push(Record::Category(child));
            children.push(id);
        }
        let mut sessions: Vec<String> = Vec::new();
        let mut per_child = [0usize; 2];
        for (n, group) in moved.iter().enumerate() {
            for session in group.iter() {
                let Some(list) = trial.member_list(session, &kind) else {
                    continue;
                };
                let Some(held) = list.members.iter().find(|m| &m.category == parent) else {
                    continue;
                };
                if !held.origin.is_machine() || sessions.contains(session) {
                    continue;
                }
                let mut members = list.members.clone();
                members.retain(|m| &m.category != parent);
                let mut entry = held.clone();
                entry.category = children[n].clone();
                entry.origin = Origin::Sweep;
                members.push(entry);
                let record = Record::Members(MemberList {
                    session: session.clone(),
                    kind: kind.clone(),
                    members,
                });
                if trial.put(record.clone(), Origin::Sweep).is_ok() {
                    records.push(record);
                    sessions.push(session.clone());
                    per_child[n] += 1;
                }
            }
        }
        if per_child.contains(&0) {
            return None;
        }
        *self = trial;
        Some(Step::Split {
            parent: parent.clone(),
            children: [children[0].clone(), children[1].clone()],
            sessions,
            records,
        })
    }
}
