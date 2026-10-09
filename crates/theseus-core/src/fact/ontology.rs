//! The ontology's facts (theseus-8kk.1): a row per change of its records,
//! in the frame that writes the record, and a line of the narrative. Given
//! categories made at a place's first bind are the transport's; the rest
//! are the operator's.

use serde_json::{json, Value};
use theseus_ontology::{Category, Guidance, MemberList};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// A category was made or changed (`ontology.category`): by the operator,
/// or by the transport at a place's first bind.
pub struct CategorySet<'a> {
    pub category: &'a Category,
    /// `operator` or `transport`.
    pub origin: &'a str,
    /// Who, and through what: `the CLI`; the binding for the transport.
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for CategorySet<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OntologyCategory);

    fn row(&self) -> Value {
        let c = self.category;
        json!({"id": c.id, "kind": c.kind(), "name": c.name, "parent": c.parent,
               "origin": self.origin, "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let c = self.category;
        say.line(
            Session,
            format!(
                "Ontology: {} made the {} category {} ({}){}.",
                self.who,
                c.kind(),
                c.name,
                c.id,
                match &c.parent {
                    Some(p) => format!(", under {p}"),
                    None => String::new(),
                }
            ),
        );
    }
}

/// A category's guidance was written (`ontology.guidance`): its version,
/// digest, and size, never its text, which the record holds.
pub struct GuidanceSet<'a> {
    pub guidance: &'a Guidance,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for GuidanceSet<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OntologyGuidance);

    fn row(&self) -> Value {
        let g = self.guidance;
        json!({"category": g.category, "version": g.version, "digest": g.digest,
               "bytes": g.text.len(), "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let g = self.guidance;
        let what = match g.is_empty() {
            true => format!("took away the guidance of {}", g.category),
            false => format!(
                "set the guidance of {} (version {}, {} bytes, {})",
                g.category,
                g.version,
                g.text.len(),
                g.digest
            ),
        };
        say.line(
            Session,
            format!(
                "Ontology: {} {what}; a session that carries it recompiles at its next turn.",
                self.who
            ),
        );
    }
}

/// A session's memberships of one interpreted kind were set
/// (`ontology.membership`): the list as it is now.
pub struct MembershipSet<'a> {
    pub list: &'a MemberList,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for MembershipSet<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OntologyMembership);

    fn row(&self) -> Value {
        let l = self.list;
        let categories: Vec<&str> = l.members.iter().map(|m| m.category.as_str()).collect();
        json!({"kind": l.kind, "categories": categories, "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let l = self.list;
        let now = match l.members.is_empty() {
            true => "none".to_string(),
            false => l
                .members
                .iter()
                .map(|m| m.category.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        };
        say.line(
            Session,
            format!(
                "Ontology: {} set this session's {} memberships to {now}; they apply at its next \
                 recompile.",
                self.who, l.kind
            ),
        );
    }
}

/// Two people were merged into one, or a merge was undone
/// (`ontology.merged`, theseus-wy7y). The row holds what the merge moved:
/// the absorbed person's record as it was, the survivor's handles before,
/// the sessions whose lists moved, and whether guidance moved; an undo
/// reads it back.
pub struct PersonMerged<'a> {
    pub absorbed: &'a Category,
    pub survivor: &'a str,
    pub handles_before: &'a [String],
    pub sessions: &'a [String],
    /// The moved sessions' person lists as they were: what an undo writes.
    pub lists_before: &'a [MemberList],
    pub guidance_moved: bool,
    /// An undo's row.
    pub undone: bool,
    /// `operator`, or `automatic` for an exact handle's merge.
    pub how: &'a str,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for PersonMerged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OntologyMerged);

    fn row(&self) -> Value {
        json!({"absorbed": self.absorbed.id, "survivor": self.survivor,
               "absorbed_record": self.absorbed, "handles_before": self.handles_before,
               "sessions": self.sessions, "lists_before": self.lists_before,
               "guidance_moved": self.guidance_moved,
               "undone": self.undone, "how": self.how, "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.undone {
            true => format!(
                "Ontology: {} undid the merge of {} into {}: {} back.",
                self.who,
                self.absorbed.id,
                self.survivor,
                crate::narrative::count(self.sessions.len() as u64, "session", "sessions"),
            ),
            false => format!(
                "Ontology: {} merged the person {} ({}) into {} ({}): {} moved.",
                self.who,
                self.absorbed.name,
                self.absorbed.id,
                self.survivor,
                self.how,
                crate::narrative::count(self.sessions.len() as u64, "session", "sessions"),
            ),
        };
        say.line(Session, line);
    }
}
