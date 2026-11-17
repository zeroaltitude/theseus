//! The owner's second proof (issue 22): each check of a code, and each change
//! of what is enrolled. Neither row ever carries a code or the secret: a row
//! says what came of the check and where it was made, and nothing a model
//! reading the ledger could use.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

/// A code was checked (`owner.proof.checked`): good or not, every time. The
/// outcome is one of `ok`, `wrong`, `used`, `locked`, `not_enrolled`,
/// `unreadable`, `unsaved`, and `agent` (an answer from an agent's process, refused
/// before any code was read, and no try); the place is `cli`, `web`, `discord:dm` or the
/// like.
pub struct ProofChecked<'a> {
    pub outcome: &'a str,
    pub place: &'a str,
    /// When a lock ends, for a check that found one or caused one.
    pub locked_until_ms: Option<u64>,
}

impl Fact for ProofChecked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnerProofChecked);

    fn row(&self) -> Value {
        let mut row = json!({"outcome": self.outcome, "place": self.place});
        if let Some(until) = self.locked_until_ms {
            row["locked_until_ms"] = until.into();
        }
        row
    }
}

/// What is enrolled changed, or was asked to (`owner.proof.changed`): an
/// enrolment kept, a reset, or one refused where it was asked from.
pub struct ProofChanged<'a> {
    /// `enrolled`, `reset`, or `refused`.
    pub what: &'a str,
    pub place: &'a str,
    /// Why, for a refusal: the words the person was told.
    pub why: Option<&'a str>,
}

impl Fact for ProofChanged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnerProofChanged);

    fn row(&self) -> Value {
        let mut row = json!({"what": self.what, "place": self.place});
        if let Some(why) = self.why {
            row["why"] = why.into();
        }
        row
    }
}

/// A good code opened a window (`owner.proof.window_opened`): for a while an
/// answer from this place needs no code. The place and when it ends.
pub struct WindowOpened<'a> {
    pub place: &'a str,
    pub until_ms: u64,
}

impl Fact for WindowOpened<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnerProofWindowOpened);

    fn row(&self) -> Value {
        json!({"place": self.place, "until_ms": self.until_ms})
    }
}

/// A question went through on a window's earlier proof
/// (`owner.proof.window_used`): which question, from where, and when the
/// window ends. The window's end does not move.
pub struct WindowUsed<'a> {
    pub question: &'a str,
    pub place: &'a str,
    pub until_ms: u64,
}

impl Fact for WindowUsed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnerProofWindowUsed);

    fn row(&self) -> Value {
        json!({"question": self.question, "place": self.place, "until_ms": self.until_ms})
    }
}

/// Every open window was ended (`owner.proof.locked`): by the owner (`lock`,
/// from where) or by the daemon (a config rollback), and how many closed.
pub struct WindowsEnded<'a> {
    pub by: &'a str,
    pub place: &'a str,
    pub closed: usize,
}

impl Fact for WindowsEnded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnerProofLocked);

    fn row(&self) -> Value {
        json!({"by": self.by, "place": self.place, "closed": self.closed})
    }
}
