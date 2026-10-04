//! An extension's facts (M7 43a and 43b, design §2.7): proposed (frozen),
//! tested (in L1, through the board), acked, declined, loaded, and revoked.
//! Each is a ledger row and its narrative lines. The proposal and its test
//! are the proposing turn's, and ride in its next frame; an answer's rows,
//! the load's among them, ride in the answer's frame, and a revoke's in its
//! own.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::{Approval, Tool};

use super::{Fact, Say};
use crate::narrative::count;

/// The first characters of a digest, as a card names it.
pub fn short(digest: &str) -> &str {
    &digest[..digest.len().min(6)]
}

/// A tree frozen as a proposal: `<state>/extensions/<name>/<digest>/`.
pub struct ExtendProposed<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    pub source: &'a str,
    pub files: usize,
    pub bytes: u64,
    pub command: &'a [String],
    pub network: &'a [String],
    pub correlation_id: &'a str,
}

impl Fact for ExtendProposed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendProposed);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest, "source": self.source,
            "files": self.files, "bytes": self.bytes, "command": self.command,
            "network": self.network, "correlation_id": self.correlation_id})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "Extension {} {} proposed: {} frozen from {}.",
                self.name,
                short(self.digest),
                count(self.files as u64, "file", "files"),
                self.source
            ),
        );
    }
}

/// Its trial in L1: whether it started, its tools, and its tests' results.
pub struct ExtendTested<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    pub tools: &'a [String],
    pub passed: usize,
    pub tests: usize,
    /// Why it did not start, or did not list its tools.
    pub error: Option<&'a str>,
    pub ms: u64,
}

impl Fact for ExtendTested<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendTested);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest, "tools": self.tools,
            "passed": self.passed, "tests": self.tests, "error": self.error, "ms": self.ms})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let line = match self.error {
            Some(why) => format!(
                "Extension {} {} did not come up in L1: {why}.",
                self.name,
                short(self.digest)
            ),
            None => format!(
                "Extension {} {} tested in L1 in {} ms: {}, {} of {} passed.",
                self.name,
                short(self.digest),
                self.ms,
                count(self.tools.len() as u64, "tool", "tools"),
                self.passed,
                count(self.tests as u64, "test", "tests")
            ),
        };
        say.line(Tool, line);
    }
}

/// The operator acked it: it loads (43b, `ExtendLoaded` in the same frame).
pub struct ExtendAcked<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    pub correlation_id: &'a str,
    pub by: &'a str,
    pub via: &'a str,
}

impl Fact for ExtendAcked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendAcked);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest,
            "correlation_id": self.correlation_id, "by": self.by, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Extension {} {} acked by {}.",
                self.name,
                short(self.digest),
                self.by
            ),
        );
    }
}

/// The operator declined it, or nobody answered in time: nothing loads.
pub struct ExtendDeclined<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    pub correlation_id: &'a str,
    pub by: &'a str,
    pub via: &'a str,
    pub note: Option<&'a str>,
}

impl Fact for ExtendDeclined<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendDeclined);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest,
            "correlation_id": self.correlation_id, "by": self.by, "via": self.via,
            "note": self.note})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Extension {} {} declined by {}; nothing loads.",
                self.name,
                short(self.digest),
                self.by
            ),
        );
    }
}

/// An acked extension loaded (43b): its server `ext-<name>` starts in L1
/// from the frozen copy, and its tools are offered from the next turn.
pub struct ExtendLoaded<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    pub tools: &'a [String],
    pub network: &'a [String],
    /// The digest of the version it replaced.
    pub replaced: Option<&'a str>,
    pub by: &'a str,
    /// The proposing session's place, and its ceiling, at the ack.
    pub place: Option<&'a str>,
    pub ceiling: Option<&'a theseus_protocol::PlaceCeiling>,
}

impl Fact for ExtendLoaded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendLoaded);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest, "server": format!("ext-{}", self.name),
            "tools": self.tools, "network": self.network, "replaced": self.replaced,
            "by": self.by, "place": self.place, "ceiling": self.ceiling})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let replaced = match self.replaced {
            Some(d) => format!(", in place of {}", short(d)),
            None => String::new(),
        };
        say.line(
            Tool,
            format!(
                "Extension {} {} loaded as ext-{}{replaced}: {}, offered from the next turn.",
                self.name,
                short(self.digest),
                self.name,
                count(self.tools.len() as u64, "tool", "tools"),
            ),
        );
    }
}

/// The operator revoked it (43b): its server stopped, its tools dropped from
/// the next turn; the frozen copy stays on disk.
pub struct ExtendRevoked<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    pub tools: &'a [String],
    pub by: &'a str,
    pub via: &'a str,
}

impl Fact for ExtendRevoked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendRevoked);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest, "server": format!("ext-{}", self.name),
            "tools": self.tools, "by": self.by, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Extension {} {} revoked by {}: its server stopped, and its tools are gone \
                 from the next turn.",
                self.name,
                short(self.digest),
                self.by
            ),
        );
    }
}
