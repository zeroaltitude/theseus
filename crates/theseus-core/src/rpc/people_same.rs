//! The automatic merge of people whose names are one key (theseus-jrffh): the
//! second automatic path (run by the people sweep, never by a plain `ontology.category.add`, which makes a person of a display name alone) beside an exact handle's (`people.rs`). Two held
//! people whose `same::name_key`s are equal are one person, unless the owner
//! recorded the pair as not the same (`ontology.person.not_same`). It goes
//! through `person_merge_in`, so each merge has its row, its notice and its
//! undo (`ontology.person.merge --undo`). A bare first name is not a key of a
//! full name, so `bram` never merges into `bram teller` here.

use std::collections::BTreeMap;

use anyhow::Result;
use theseus_ontology::{person, Category, Origin};

use super::Core;
use crate::judge::people::same::{name_key, pair_key};

/// The one to keep of two equal keys: a transport's person (a DM's, which the
/// transport reads by its id), then the one with more handles, then the
/// smaller id.
fn keeps_first(a: &Category, b: &Category) -> bool {
    let rank = |c: &Category| {
        (
            c.added_by != Origin::Transport.name(),
            std::cmp::Reverse(person::handles_of(c).len()),
            c.id.to_string(),
        )
    };
    rank(a) <= rank(b)
}

impl Core {
    /// Merge every group of held people with one name key, each into the
    /// one `keeps_first` picks, leaving a pair the owner said is not the
    /// same, and a pair the ontology refuses (both have guidance). Returns
    /// the ids merged away. One frame a merge.
    pub fn people_same_merge(&self) -> Result<Vec<String>> {
        let mut forbidden = self.not_same_pairs();
        let mut merged = Vec::new();
        loop {
            let o = self.runner.ontology.snapshot(&self.store)?;
            let mut groups: BTreeMap<String, Vec<&Category>> = BTreeMap::new();
            for c in o.categories().filter(|c| {
                c.kind() == person::KIND && c.retired_ms.is_none() && c.merged_into.is_none()
            }) {
                let key = name_key(&c.name);
                if !key.is_empty() {
                    groups.entry(key).or_default().push(c);
                }
            }
            let mut next = None;
            'groups: for g in groups.values().filter(|g| g.len() > 1) {
                let mut g = g.clone();
                g.sort_by(|a, b| match keeps_first(a, b) {
                    true => std::cmp::Ordering::Less,
                    false => std::cmp::Ordering::Greater,
                });
                for (i, keep) in g.iter().enumerate() {
                    for gone in &g[i + 1..] {
                        let pair = pair_key(&keep.id.to_string(), &gone.id.to_string());
                        if !forbidden.contains(&pair) {
                            next = Some((gone.id.clone(), keep.id.clone()));
                            break 'groups;
                        }
                    }
                }
            }
            let Some((gone, keep)) = next else {
                return Ok(merged);
            };
            match self.person_merge_in(&o, &gone, &keep, "automatic", "the name key", "people") {
                Ok(_) => merged.push(gone.to_string()),
                Err(e) => {
                    // A pair the ontology refuses is left, and not tried again.
                    tracing::warn!(absorbed = %gone, survivor = %keep, error = %format!("{e:#}"),
                        "two people with one name key were not merged");
                    forbidden.insert(pair_key(&keep.to_string(), &gone.to_string()));
                }
            }
        }
    }
}
