//! A repository's instruction trust (theseus-wadu, phase 2): the owner's
//! grant of one repository's `AGENTS.md` and `CLAUDE.md` to the system block,
//! and the revoke of it (`docs/design/project-context.md` §7).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// A grant given or taken back. The row carries the repository's id and
/// root, who acted and by what route; `undo` names the opposite command.
pub struct ProjectTrust<'a> {
    pub id: &'a str,
    pub root: &'a str,
    /// True for a grant, false for a revoke.
    pub granted: bool,
    pub by: &'a str,
    pub via: &'a str,
}

impl Fact for ProjectTrust<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProjectTrust);

    fn row(&self) -> Value {
        let undo = if self.granted { "untrust" } else { "trust" };
        json!({"repo": self.id, "root": self.root, "granted": self.granted,
            "by": self.by, "via": self.via,
            "undo": format!("theseus project {undo} {}", self.root)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = if self.granted {
            "trusted: its AGENTS.md and CLAUDE.md join the system block from the next recompile"
        } else {
            "no longer trusted: its instruction files are named, not read"
        };
        say.line(Approval, format!("Repository {} {what}.", self.id));
    }
}
