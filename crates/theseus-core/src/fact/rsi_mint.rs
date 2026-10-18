//! A minted tool was validated (theseus-pw1q.21.6): `self.mint.validated`, one
//! row for each tool checked, written when its open replays have run. Its
//! fields are `SELF_ROWS`'s (`name`, `kind`, `digest`, `tiers`, `pass`) beside
//! every `self.*` row's `what`, `why`, `numbers` and `undo`. It holds names,
//! digests and counts, never a value; and it loads nothing, so its undo is
//! that there is nothing to undo.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// One tier of the checks: its name, whether it ran and whether it passed.
pub struct TierRow {
    pub name: String,
    pub ran: bool,
    pub pass: bool,
}

/// The tool, its tiers, and the verdict.
pub struct MintValidated {
    pub name: String,
    pub kind: String,
    pub digest: String,
    pub tiers: Vec<TierRow>,
    pub pass: bool,
    /// Why it failed, in a line; empty for a pass.
    pub why: String,
}

impl MintValidated {
    fn passed(&self) -> usize {
        self.tiers.iter().filter(|t| t.pass).count()
    }
}

impl Fact for MintValidated {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfMintValidated);

    fn row(&self) -> Value {
        let tiers: Vec<Value> = self
            .tiers
            .iter()
            .map(|t| json!({"tier": t.name, "ran": t.ran, "pass": t.pass}))
            .collect();
        let what = if self.pass {
            format!(
                "the minted {} {} passed its open replays",
                self.kind, self.name
            )
        } else {
            format!(
                "the minted {} {} did not pass its open replays",
                self.kind, self.name
            )
        };
        json!({"name": self.name, "kind": self.kind, "digest": self.digest,
               "tiers": tiers, "pass": self.pass,
               "what": what,
               "why": if self.why.is_empty() { "every tier ran and passed".to_string() } else { self.why.clone() },
               "numbers": {"tiers": self.tiers.len(), "passed": self.passed()},
               "undo": "nothing was loaded: validating deploys nothing"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "The minted {} {} {}: {} of {} checks passed.",
                self.kind,
                self.name,
                if self.pass {
                    "is validated"
                } else {
                    "is not validated"
                },
                self.passed(),
                self.tiers.len()
            ),
        );
    }
}
