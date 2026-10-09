//! People (theseus-wy7y): the `person` kind beyond the transport's DMs.
//!
//! `person` stays **given** for its transport side: a DM's user is read
//! from the session's place at compile and never stored, and a shared
//! place's walk reads only that. Its seed row also names the operator and
//! the import among its assigners, so a person may be declared by either,
//! and a session's **stored** person memberships come from them (never from
//! the transport). A given kind with such a row is a kind with a stored
//! side ([`Kind::stores`]); guild and channel have none.
//!
//! **One person, many handles.** A person carries its handles: `discord:<id>`,
//! `slack:<id>`, `email:<addr>`, and display names as `name:<name>`. A
//! transport's person (`person:<discord user id>`) holds `discord:<id>`
//! without writing it ([`handles_of`]). No two held people share a handle
//! but a name: a write that would is refused, and the core merges them
//! instead (an exact handle is the one automatic merge; a display name alone
//! never merges). A merge ([`Ontology::merge`]) keeps the survivor's id,
//! adds the other's handles to it, moves its memberships and its guidance,
//! and writes the other as `merged_into` the survivor.
//!
//! What a person record may hold is role facts: what they do and own, their
//! projects and channels, how work flows between them and the owner. Never
//! an evaluation of them; the CLI's help and Jev's instructions say so.

use crate::category::{Category, CategoryId, Guidance, MemberList, Membership};
use crate::kind::{Kind, Origin};
use crate::ontology::Ontology;
use crate::refusal::Refusal;
use crate::Record;

/// The kind people are.
pub const KIND: &str = "person";

/// The handle kinds a person may carry, `<kind>:<value>`.
pub const HANDLE_KINDS: [&str; 4] = ["discord", "slack", "email", "name"];

/// The most characters in one handle.
pub const HANDLE_MAX: usize = 200;

/// The most handles one person carries.
pub const HANDLES_MAX: usize = 64;

/// A handle as it is stored: its kind in lowercase, its value trimmed; an
/// email's value in lowercase too. Refused when it is not one of
/// [`HANDLE_KINDS`], or empty, or longer than [`HANDLE_MAX`].
pub fn handle(s: &str) -> Result<String, Refusal> {
    let what = || format!("the handle {s:?}");
    let Some((kind, value)) = s.trim().split_once(':') else {
        return Err(Refusal::invalid(
            what(),
            "a handle is `<kind>:<value>`: discord:<id>, slack:<id>, email:<addr>, or name:<name>",
        ));
    };
    let kind = kind.trim().to_lowercase();
    if !HANDLE_KINDS.contains(&kind.as_str()) {
        return Err(Refusal::invalid(
            what(),
            format!(
                "its kind is {kind:?}, and a handle's is one of {}",
                HANDLE_KINDS.join(", ")
            ),
        ));
    }
    let value = value.trim();
    let value = match kind.as_str() {
        "email" => value.to_lowercase(),
        _ => value.to_string(),
    };
    let out = format!("{kind}:{value}");
    if value.is_empty() || value.chars().any(char::is_control) || out.chars().count() > HANDLE_MAX {
        return Err(Refusal::invalid(
            what(),
            format!("its value is 1 to {HANDLE_MAX} characters on one line"),
        ));
    }
    Ok(out)
}

/// Whether a handle is a display name, which never merges two people.
pub fn is_name(h: &str) -> bool {
    h.starts_with("name:")
}

/// A person's handles: its stored ones, and for the transport's person of a
/// Discord DM (`person:<digits>`), its `discord:<id>`.
pub fn handles_of(c: &Category) -> Vec<String> {
    let mut out = c.handles.clone();
    if c.kind() == KIND {
        let local = c.id.local();
        if !local.is_empty() && local.bytes().all(|b| b.is_ascii_digit()) {
            let h = format!("discord:{local}");
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out
}

/// A category's handles against its own rules: only a person carries any,
/// each is stored as [`handle`] stores it, none twice, at most
/// [`HANDLES_MAX`].
pub(crate) fn check_handles(c: &Category) -> Result<(), Refusal> {
    if c.handles.is_empty() {
        return Ok(());
    }
    let what = || format!("category `{}`'s handles", c.id);
    if c.kind() != KIND {
        return Err(Refusal::invalid(
            what(),
            format!("only a {KIND} carries handles, and this is a {}", c.kind()),
        ));
    }
    if c.handles.len() > HANDLES_MAX {
        return Err(Refusal::invalid(
            what(),
            format!(
                "it has {}, and a person has at most {HANDLES_MAX}",
                c.handles.len()
            ),
        ));
    }
    for (i, h) in c.handles.iter().enumerate() {
        if handle(h)? != *h {
            return Err(Refusal::invalid(
                what(),
                format!("{h:?} is not stored as a handle is (its kind in lowercase, trimmed)"),
            ));
        }
        if c.handles[..i].contains(h) {
            return Err(Refusal::Duplicate {
                why: format!("`{}` lists the handle {h:?} twice", c.id),
            });
        }
    }
    Ok(())
}

impl Kind {
    /// Whether the kind keeps stored memberships: an interpreted kind
    /// always; a given one when its row names an assigner beside the
    /// transport (`person`).
    pub fn stores(&self) -> bool {
        !self.is_given() || self.assigned_by.iter().any(|o| *o != Origin::Transport)
    }
}

impl Ontology {
    /// The held person another holds `c`'s handle as: the first of `c`'s
    /// handles but a name that another held person carries, with that
    /// person's id.
    pub fn handle_twin(&self, c: &Category) -> Option<(String, CategoryId)> {
        if c.kind() != KIND {
            return None;
        }
        let mine: Vec<String> = handles_of(c).into_iter().filter(|h| !is_name(h)).collect();
        if mine.is_empty() {
            return None;
        }
        self.categories()
            .filter(|o| o.kind() == KIND && o.id != c.id)
            .find_map(|o| {
                let theirs = handles_of(o);
                mine.iter()
                    .find(|h| theirs.contains(h))
                    .map(|h| (h.clone(), o.id.clone()))
            })
    }

    /// The person holding `handle` exactly (not a name), if one does.
    pub fn person_by_handle(&self, handle: &str) -> Option<&Category> {
        if is_name(handle) {
            return None;
        }
        self.categories()
            .filter(|c| c.kind() == KIND)
            .find(|c| handles_of(c).iter().any(|h| h == handle))
    }

    /// The records that merge `absorbed` into `survivor`, in the
    /// order they apply: every session's person list that holds `absorbed`
    /// with it replaced by `survivor` (once), the survivor with the other's
    /// handles and its guidance when it has none, and `absorbed` written as
    /// merged. Refused when either is not a held person, when they are one,
    /// when the transport made `absorbed` (its DM reads its id: keep it, and
    /// merge the other into it), or when both have guidance.
    pub fn merge(&self, absorbed: &CategoryId, survivor: &CategoryId) -> Result<Merge, Refusal> {
        let held = |id: &CategoryId| {
            self.category(id)
                .filter(|c| c.kind() == KIND)
                .ok_or_else(|| Refusal::Missing {
                    what: "person",
                    id: id.to_string(),
                })
        };
        let (gone, kept) = (held(absorbed)?, held(survivor)?);
        if gone.id == kept.id {
            return Err(Refusal::Duplicate {
                why: format!("`{}` is one person: a merge takes two", gone.id),
            });
        }
        if gone.added_by == Origin::Transport.name() {
            return Err(Refusal::Given {
                kind: KIND.into(),
                why: format!(
                    "`{}` is a DM's person, from the transport, which reads it by its id: keep it, \
                     and merge `{}` into it instead",
                    gone.id, kept.id
                ),
            });
        }
        let guide = |id: &CategoryId| self.guidance(id).filter(|g| !g.is_empty());
        let moved_guidance = match (guide(&gone.id), guide(&kept.id)) {
            (Some(_), Some(_)) => {
                return Err(Refusal::InUse {
                    id: gone.id.to_string(),
                    by: format!(
                        "its guidance, and `{}` has its own: clear one, then merge",
                        kept.id
                    ),
                })
            }
            (Some(g), None) => Some(g.text.clone()),
            _ => None,
        };
        let mut records = Vec::new();
        let mut sessions = Vec::new();
        let mut lists_before = Vec::new();
        for l in self.members.values().filter(|l| l.kind == KIND) {
            if !l.members.iter().any(|m| m.category == gone.id) {
                continue;
            }
            let mut members: Vec<Membership> = Vec::with_capacity(l.members.len());
            for m in &l.members {
                let mut m = m.clone();
                if m.category == gone.id {
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
                kind: KIND.into(),
                members,
            }));
        }
        let before = kept.handles.clone();
        let mut next = kept.clone();
        for h in handles_of(gone) {
            if !handles_of(&next).contains(&h) {
                next.handles.push(h);
            }
        }
        if !next.handles.contains(&format!("name:{}", gone.name)) && gone.name != kept.name {
            if let Ok(h) = handle(&format!("name:{}", gone.name)) {
                next.handles.push(h);
            }
        }
        next.handles.truncate(HANDLES_MAX);
        // Its own handles leave it before the survivor takes them, so the
        // two never hold one at once.
        let mut taken = gone.clone();
        taken.handles.clear();
        taken.merged_into = Some(kept.id.clone());
        if let Some(text) = &moved_guidance {
            let g = Guidance::new(
                kept.id.clone(),
                text,
                self.guidance(&kept.id).map_or(1, |g| g.version + 1),
                "merge",
            );
            records.push(Record::Guidance(g));
        }
        records.push(Record::Category(taken));
        records.push(Record::Category(next));
        Ok(Merge {
            absorbed: gone.clone(),
            survivor_handles_before: before,
            sessions,
            lists_before,
            guidance: moved_guidance,
            records,
        })
    }

    /// The records that undo a merge, from what it moved: `absorbed` held
    /// again as it was, the survivor's handles as they were, and each moved
    /// session's list as it was before the merge. The survivor's guidance
    /// stays (an edit, not a move, after the merge).
    pub fn unmerge(
        &self,
        absorbed: &Category,
        survivor: &CategoryId,
        handles_before: &[String],
        lists_before: &[MemberList],
    ) -> Result<Vec<Record>, Refusal> {
        let kept = self.category(survivor).ok_or_else(|| Refusal::Missing {
            what: "person",
            id: survivor.to_string(),
        })?;
        if self.category(&absorbed.id).is_some() {
            return Err(Refusal::Duplicate {
                why: format!("`{}` is held already: nothing to undo", absorbed.id),
            });
        }
        let mut back = kept.clone();
        back.handles = handles_before.to_vec();
        let mut again = absorbed.clone();
        again.merged_into = None;
        again.retired_ms = None;
        let mut records = vec![Record::Category(back), Record::Category(again)];
        records.extend(
            lists_before
                .iter()
                .filter(|l| l.kind == KIND)
                .cloned()
                .map(Record::Members),
        );
        Ok(records)
    }
}

/// What a merge writes, and what it moved: the ledger row keeps the moved
/// part, which [`Ontology::unmerge`] reads to undo it.
#[derive(Debug, Clone, PartialEq)]
pub struct Merge {
    /// The person merged away, as it was.
    pub absorbed: Category,
    pub survivor_handles_before: Vec<String>,
    /// The sessions whose lists it rewrote, and those lists as they were.
    pub sessions: Vec<String>,
    pub lists_before: Vec<MemberList>,
    /// The guidance it moved, if any.
    pub guidance: Option<String>,
    pub records: Vec<Record>,
}
