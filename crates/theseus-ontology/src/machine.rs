//! What the machine origins may write, and how a write of theirs is taken
//! back (the onto-core design): Jev, the sweep and the dream write beside
//! the operator and yield to the operator, so every write of theirs is
//! checked here, and an undo gives the records that restore what one
//! replaced.

use crate::category::{Category, MemberList, Membership};
use crate::kind::{seeds, Basis, Kind, Origin, GIVEN};
use crate::ontology::Ontology;
use crate::refusal::Refusal;
use crate::Record;

/// Whether `added_by` is a machine origin's: `<origin>:<id>` with an id.
fn machine_written(added_by: &str) -> bool {
    [Origin::Jev, Origin::Sweep, Origin::Dream]
        .iter()
        .any(|o| prefixed(added_by, *o))
}

fn prefixed(added_by: &str, by: Origin) -> bool {
    added_by
        .strip_prefix(by.name())
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|id| !id.is_empty())
}

impl Ontology {
    /// Whether `by` may write the kind row `k`. The operator writes any
    /// row (but takes away only a kind that is not given or seeded); the
    /// sweep adds a new interpreted kind, assigned by the sweep and Jev,
    /// and takes away one of its own; no other origin writes a kind.
    pub(crate) fn check_kind_writer(&self, k: &Kind, by: Origin) -> Result<(), Refusal> {
        let old = self.kinds.get(&k.name);
        let sweep_only = |why: &str| Refusal::Writer {
            why: format!(
                "the kinds table is the operator's to write, and the sweep may add a new \
                 interpreted kind: `{by}` may not {why}"
            ),
        };
        match by {
            Origin::Operator => {}
            Origin::Sweep => {
                if !prefixed(&k.added_by, by) {
                    return Err(sweep_only("write a row whose added_by is not `sweep:<id>`"));
                }
                match old {
                    None => {
                        if k.basis != Basis::Interpreted {
                            return Err(sweep_only("add a given kind"));
                        }
                        if ![Origin::Sweep, Origin::Jev]
                            .iter()
                            .all(|o| k.assigned_by.contains(o))
                        {
                            return Err(sweep_only(
                                "add a kind whose assigned_by does not name sweep and jev",
                            ));
                        }
                        if k.retired_ms.is_some() {
                            return Err(sweep_only("add a kind already retired"));
                        }
                    }
                    Some(old) => {
                        let same = Kind {
                            version: k.version,
                            added_by: k.added_by.clone(),
                            retired_ms: k.retired_ms,
                            ..old.clone()
                        };
                        if !(prefixed(&old.added_by, by)
                            && old.retired_ms.is_none()
                            && k.retired_ms.is_some()
                            && same == *k)
                        {
                            return Err(sweep_only(
                                "change a row, but to take away a kind the sweep added",
                            ));
                        }
                    }
                }
            }
            _ => return Err(sweep_only("write a kind")),
        }
        if k.retired_ms.is_some() {
            self.check_kind_retire(k, by)?;
        }
        Ok(())
    }

    /// A kind taken away: held, not given or seeded, and used by no
    /// category and no stored list.
    fn check_kind_retire(&self, k: &Kind, by: Origin) -> Result<(), Refusal> {
        if !self.kinds.contains_key(&k.name) {
            return Err(Refusal::Missing {
                what: "kind",
                id: k.name.clone(),
            });
        }
        if k.is_given() || GIVEN.contains(&k.name.as_str()) {
            return Err(Refusal::Given {
                kind: k.name.clone(),
                why: format!(
                    "`{}` is given by the harness, and a given kind is never taken away",
                    k.name
                ),
            });
        }
        if seeds().iter().any(|s| s.name == k.name) {
            return Err(Refusal::Writer {
                why: format!(
                    "`{}` is a seeded kind, and `{by}` may not take it away: retire its \
                     categories instead",
                    k.name
                ),
            });
        }
        let live = self
            .categories
            .keys()
            .filter(|id| id.kind() == k.name)
            .count();
        if live > 0 {
            return Err(Refusal::InUse {
                id: k.name.clone(),
                by: format!("{live} categories"),
            });
        }
        let lists = self
            .members
            .values()
            .filter(|l| l.kind == k.name && !l.members.is_empty())
            .count();
        if lists > 0 {
            return Err(Refusal::InUse {
                id: k.name.clone(),
                by: format!("{lists} sessions' memberships"),
            });
        }
        Ok(())
    }

    /// A category write against its kind and writer: a retired kind takes
    /// no new one, and a machine origin writes only a live category whose
    /// `added_by` is its own, and never one in place of the operator's.
    pub(crate) fn check_category_writer(
        &self,
        kind: &Kind,
        c: &Category,
        by: Origin,
    ) -> Result<(), Refusal> {
        if kind.retired_ms.is_some() && !c.is_gone() {
            return Err(retired_kind(kind));
        }
        if !by.is_machine() {
            return Ok(());
        }
        if c.is_gone() {
            return Err(Refusal::Writer {
                why: format!(
                    "`{by}` may not take a category away: only the operator retires or merges one"
                ),
            });
        }
        if !prefixed(&c.added_by, by) {
            return Err(Refusal::Writer {
                why: format!(
                    "a category `{by}` writes has an added_by of `{by}:<id>`, and `{}` of `{}` \
                     is not",
                    c.added_by, c.id
                ),
            });
        }
        if let Some(old) = self.categories.get(&c.id) {
            if !machine_written(&old.added_by) {
                return Err(Refusal::Writer {
                    why: format!(
                        "`{}` was written by `{}`: `{by}` does not supersede the operator's",
                        c.id, old.added_by
                    ),
                });
            }
        }
        Ok(())
    }

    /// A member list against its kind and writer: a retired kind takes no
    /// list but an empty one, and a machine origin's list keeps every entry
    /// of the current one that the operator or the import made, unchanged,
    /// and adds entries of its own origin only.
    pub(crate) fn check_list_writer(
        &self,
        kind: &Kind,
        l: &MemberList,
        by: Origin,
    ) -> Result<(), Refusal> {
        if kind.retired_ms.is_some() && !l.members.is_empty() {
            return Err(retired_kind(kind));
        }
        if !by.is_machine() {
            return Ok(());
        }
        let current: &[Membership] = self
            .member_list(&l.session, &l.kind)
            .map_or(&[], |c| &c.members);
        for old in current
            .iter()
            .filter(|m| matches!(m.origin, Origin::Operator | Origin::Import))
        {
            if !l.members.contains(old) {
                return Err(Refusal::Writer {
                    why: format!(
                        "the {} membership of `{}` in `{}` is the {}'s: `{by}` may not drop or \
                         change it",
                        l.kind, l.session, old.category, old.origin
                    ),
                });
            }
        }
        if let Some(m) = l
            .members
            .iter()
            .find(|m| m.origin != by && !current.contains(m))
        {
            return Err(Refusal::Writer {
                why: format!(
                    "`{by}` may add its own memberships only, and `{}` is from `{}`",
                    m.category, m.origin
                ),
            });
        }
        Ok(())
    }

    /// The records that undo a write: for each record of `wrote`, with what
    /// it replaced in `before` (the same order; `None` for a record that was
    /// new), the record that puts its key back: the replaced one as it was,
    /// byte for byte. What was new is taken away: a list becomes empty, a
    /// category or a kind (a sweep's) is retired at `at_ms`, and is refused
    /// with `InUse` if something else uses it now. Lists come first, so a
    /// category the write's own lists named is free to go. Each record
    /// checks as the operator's write; the ontology is not changed. A record
    /// changed since the write (the operator's later edit, or a later write
    /// of the same key) is refused: an undo never puts a key back over a
    /// write it did not make.
    pub fn undo(
        &self,
        wrote: &[Record],
        before: &[Option<Record>],
        at_ms: u64,
    ) -> Result<Vec<Record>, Refusal> {
        if wrote.len() != before.len() {
            return Err(Refusal::invalid(
                "an undo",
                "it needs what each written record replaced, one for one",
            ));
        }
        for (w, now) in wrote.iter().zip(self.replaced(wrote)) {
            if now.as_ref() != Some(w) {
                return Err(Refusal::invalid(
                    "an undo",
                    format!(
                        "`{}` changed since the write, so it is not taken back",
                        w.key()
                    ),
                ));
            }
        }
        let mut inverse: Vec<Record> = Vec::new();
        for (w, b) in wrote.iter().zip(before) {
            inverse.push(match (w, b) {
                (_, Some(old)) if old.key() == w.key() => old.clone(),
                (Record::Members(l), None) => Record::Members(MemberList {
                    members: Vec::new(),
                    ..l.clone()
                }),
                (Record::Category(c), None) => Record::Category(c.retired(at_ms)),
                (Record::Kind(k), None) => Record::Kind(Kind {
                    version: k.version + 1,
                    added_by: "operator:undo".into(),
                    retired_ms: Some(at_ms),
                    ..k.clone()
                }),
                (Record::Guidance(_), _) | (_, Some(_)) => {
                    return Err(Refusal::invalid(
                        "an undo",
                        format!("`{}` is not a record a machine write replaces", w.key()),
                    ))
                }
            });
        }
        let rank = |r: &Record| match r {
            Record::Members(_) => 0,
            Record::Guidance(_) => 1,
            Record::Category(_) => 2,
            Record::Kind(_) => 3,
        };
        inverse.sort_by_key(rank);
        let mut trial = self.clone();
        for r in &inverse {
            trial.check(r, Origin::Operator)?;
            trial.apply(r.clone());
        }
        Ok(inverse)
    }
}

impl Ontology {
    /// The record each of `records` would replace, as the snapshot holds it
    /// now (`None` for a new one): what an undo needs, taken before a write.
    pub fn replaced(&self, records: &[Record]) -> Vec<Option<Record>> {
        records
            .iter()
            .map(|r| match r {
                Record::Kind(k) => self.kinds.get(&k.name).cloned().map(Record::Kind),
                Record::Category(c) => self.categories.get(&c.id).cloned().map(Record::Category),
                Record::Guidance(g) => self
                    .guidance
                    .get(&g.category)
                    .cloned()
                    .map(Record::Guidance),
                Record::Members(l) => self
                    .member_list(&l.session, &l.kind)
                    .cloned()
                    .map(Record::Members),
            })
            .collect()
    }
}

fn retired_kind(kind: &Kind) -> Refusal {
    Refusal::Writer {
        why: format!(
            "`{}` is retired: it takes no new categories or lists",
            kind.name
        ),
    }
}
