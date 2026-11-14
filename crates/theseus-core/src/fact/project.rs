//! A repository's instruction trust (theseus-wadu, phase 2): the owner's
//! grant of one repository's `AGENTS.md` and `CLAUDE.md` to the system block,
//! and the revoke of it (`docs/design/project-context.md` §7).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::{Approval, Context};

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

/// `project.changed` (§8, "After a grant"): one of a granted repository's
/// instruction files is not what the last compile that read it saw. One row
/// a file, riding the turn's next frame with the sighting that now stands.
/// Digests, never text: the file holds the text, and the owner reads the
/// file.
///
/// It is the ledger's half of the notice; the block's half is
/// `project_context::change::Observed::line`, which puts the same fact in
/// the model's own transcript.
pub struct ProjectChanged<'a> {
    /// The repository's id (`repo_id`) and its canonical root.
    pub id: &'a str,
    pub root: &'a str,
    /// The file, as the block's header and the manifest show it.
    pub path: &'a str,
    /// Digests; empty is absent -- a file that has appeared, or gone.
    pub old: &'a str,
    pub new: &'a str,
    /// Whether this compile read the new text: false at `ask` posture, where
    /// the file waits for the owner to grant the repository again.
    pub carried: bool,
    /// The session whose acting call the file moved under, when the writer
    /// key is what holds it (S31-1): that session carried `injection`, so
    /// the new text waits for the owner's yes in **every** session and not
    /// only in the one that is labelled. `None` for every other row, which
    /// is every row that is not about a labelled session's write.
    pub wrote: Option<&'a str>,
}

impl Fact for ProjectChanged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ProjectChanged);

    fn row(&self) -> Value {
        json!({"repo": self.id, "root": self.root, "path": self.path,
            "old": self.old, "new": self.new, "carried": self.carried,
            "wrote": self.wrote})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = match (self.old.is_empty(), self.new.is_empty()) {
            (true, _) => "is new",
            (_, true) => "is gone",
            _ => "changed",
        };
        let did = match (self.carried, self.wrote) {
            (true, _) => "its text is carried".to_string(),
            (false, Some(by)) => format!(
                "it moved during a call of {by}, which carries the `injection` label, \
                 so it is named and not read in any session until the repository is \
                 granted again"
            ),
            (false, None) => {
                "it is named and not read until the repository is granted again".to_string()
            }
        };
        say.line(
            Context,
            format!(
                "Context: {} {what} in the trusted repository {}: {did}.",
                self.path, self.id
            ),
        );
    }
}
