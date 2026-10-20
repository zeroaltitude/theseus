//! A category merge beyond people (theseus-63tfk): a topic, resource,
//! event, capability, or expertise category written into another of its
//! kind. [`Ontology::merge_category`] is `person.rs`'s `merge` without the
//! handles: every session's list that holds the absorbed category holds the
//! survivor instead, and the absorbed one is written as `merged_into` the
//! survivor. The operator may merge any two categories of an interpreted
//! kind; the sweep, only one a machine wrote (`machine.rs` checks it), and
//! it moves a machine's memberships only, as its own.
//!
//! A merge never moves guidance: a category with guidance is not merged
//! (move or clear the guidance first), and nothing may hang below it. The
//! undo is `Ontology::undo`'s: the absorbed record comes back as it was and
//! each moved list as it was.

use crate::category::{Category, CategoryId, MemberList, Membership};
use crate::kind::Origin;
use crate::ontology::Ontology;
use crate::refusal::Refusal;
use crate::Record;

/// What a category merge writes, and what it moved.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryMerge {
    /// The category merged away, as it was.
    pub absorbed: Category,
    pub survivor: CategoryId,
    /// The sessions whose lists it rewrote, and those lists as they were.
    pub sessions: Vec<String>,
    pub lists_before: Vec<MemberList>,
    /// The records, in the order they apply: the lists, then the absorbed
    /// category, written as merged.
    pub records: Vec<Record>,
}

impl Ontology {
    /// The records that merge `absorbed` into `survivor` as `by`: refused
    /// when either is not held, when they are one, when they are of two
    /// kinds, when the kind is `person` (a person merges with its handles:
    /// [`Ontology::merge`]), when `absorbed` has guidance or children, and,
    /// for a machine origin, when a session's operator or import
    /// membership names `absorbed` (it is theirs, and stays). A moved
    /// membership of a machine origin becomes `by`'s, as a machine's list
    /// may add its own entries only; its confidence and time stay.
    pub fn merge_category(
        &self,
        absorbed: &CategoryId,
        survivor: &CategoryId,
        by: Origin,
    ) -> Result<CategoryMerge, Refusal> {
        let held = |id: &CategoryId| {
            self.category(id).ok_or_else(|| Refusal::Missing {
                what: "category",
                id: id.to_string(),
            })
        };
        let (gone, kept) = (held(absorbed)?, held(survivor)?);
        if gone.id == kept.id {
            return Err(Refusal::Duplicate {
                why: format!("`{}` is one category: a merge takes two", gone.id),
            });
        }
        if gone.kind() != kept.kind() {
            return Err(Refusal::WrongKind {
                why: format!(
                    "a category merges only into one of its own kind: `{}` into `{}`",
                    gone.id, kept.id
                ),
            });
        }
        if gone.kind() == crate::person::KIND {
            return Err(Refusal::WrongKind {
                why: format!(
                    "`{}` is a person, which merges with its handles: `theseus ontology person \
                     merge`",
                    gone.id
                ),
            });
        }
        if self.guidance(&gone.id).is_some_and(|g| !g.is_empty()) {
            return Err(Refusal::InUse {
                id: gone.id.to_string(),
                by: "its guidance: a merge does not move it, so clear or move it first".into(),
            });
        }
        let kids = self.children(Some(&gone.id)).len();
        if kids > 0 {
            return Err(Refusal::InUse {
                id: gone.id.to_string(),
                by: format!("{kids} below it"),
            });
        }
        let mut records = Vec::new();
        let mut sessions = Vec::new();
        let mut lists_before = Vec::new();
        for l in self.lists().filter(|l| l.kind == gone.kind()) {
            if !l.members.iter().any(|m| m.category == gone.id) {
                continue;
            }
            let mut members: Vec<Membership> = Vec::with_capacity(l.members.len());
            for m in &l.members {
                let mut m = m.clone();
                if m.category == gone.id {
                    if by.is_machine() {
                        if !m.origin.is_machine() {
                            return Err(Refusal::Writer {
                                why: format!(
                                    "the {} membership of `{}` in `{}` is the {}'s: `{by}` does \
                                     not move it",
                                    l.kind, l.session, gone.id, m.origin
                                ),
                            });
                        }
                        m.origin = by;
                    }
                    m.category = kept.id.clone();
                }
                if !members.iter().any(|o| o.category == m.category) {
                    members.push(m);
                }
            }
            sessions.push(l.session.clone());
            lists_before.push(l.clone());
            records.push(Record::Members(MemberList {
                session: l.session.clone(),
                kind: l.kind.clone(),
                members,
            }));
        }
        records.push(Record::Category(Category {
            merged_into: Some(kept.id.clone()),
            ..gone.clone()
        }));
        Ok(CategoryMerge {
            absorbed: gone.clone(),
            survivor: kept.id.clone(),
            sessions,
            lists_before,
            records,
        })
    }
}
