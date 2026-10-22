//! A category's rename as the sweep's write (theseus-63tfk.1; the design's
//! section 3.1: "a rename is a Choice among mined names with the old name kept
//! in the description"). Pure, like the plan: the model's pick is an argument.
//!
//! - **Candidates** ([`Ontology::rename_candidates`]): a category a machine
//!   wrote with no guidance and nothing below it. Whether its name has gone
//!   stale is the sessions' evidence, which the caller reads.
//! - **The write** ([`Ontology::plan_rename`]): the category again with the new
//!   name, `was: <old name>` ahead of its description, and the sweep as its
//!   writer. The id stays, so every membership holds; the undo is the record
//!   as it was.

use crate::category::{Category, CategoryId};
use crate::kind::Origin;
use crate::ontology::Ontology;
use crate::sweep::{fold, Step};
use crate::Record;

impl Ontology {
    /// The categories the sweep may rename, in id order: a machine's, loose,
    /// and not a person.
    pub fn rename_candidates(&self) -> Vec<CategoryId> {
        let mut ids: Vec<CategoryId> = self
            .categories()
            .filter(|c| self.loose(c) && c.kind() != crate::person::KIND)
            .map(|c| c.id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// `id` renamed to `name` as the sweep's write `write`, applied to `self`.
    /// None when the name is the old one once folded, a sibling's, or does
    /// not check.
    pub fn plan_rename(&mut self, id: &CategoryId, name: &str, write: &str) -> Option<Step> {
        let old = self.category(id).filter(|c| self.loose(c))?.clone();
        if fold(name).is_empty() || fold(name) == fold(&old.name) {
            return None;
        }
        let was = format!("was: {}", old.name);
        let renamed = Category {
            name: name.trim().to_string(),
            description: match old.description.is_empty() {
                true => was,
                false => format!("{was}. {}", old.description),
            },
            added_by: format!("sweep:{write}"),
            ..old.clone()
        };
        let mut trial = self.clone();
        trial
            .put(Record::Category(renamed.clone()), Origin::Sweep)
            .ok()?;
        *self = trial;
        Some(Step::Rename {
            category: id.clone(),
            from: old.name,
            records: vec![Record::Category(renamed)],
        })
    }
}
