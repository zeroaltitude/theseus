//! A proposed extension's facts (M7 43a, design §2.7): proposed (frozen),
//! tested (in L1, through the board), acked, and declined. Each is a ledger
//! row and its narrative lines. The proposal and its test are the proposing
//! turn's, and ride in its next frame; an answer's row rides in the answer's
//! frame.

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

/// The operator acked it: in this step, nothing loads (43b loads it).
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
                "Extension {} {} acked by {}; nothing loads in this build.",
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
