//! The branch builder's fact (theseus-pw1q.8): `self.branch.built`, one for
//! every attempt of `self.propose_change` that opened a tree, red or green,
//! halted or stopped at the keel. Its fields are `SELF_ROWS`'s (`item`,
//! `branch`, `head`, `gate`, `report`, `cost_usd`) beside every `self.*`
//! row's `what`, `why`, `numbers` and `undo`; `stopped_at` names the phase a
//! halt or the keel stopped it before, when one did.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// A self branch built and gated, or stopped on the way.
pub struct SelfBranchBuilt<'a> {
    pub item: &'a str,
    pub issue: &'a str,
    pub branch: &'a str,
    /// Its head commit, once one was made.
    pub head: Option<&'a str>,
    /// `green`, `red`, `keel`, `halted`, `empty` or `failed`.
    pub gate: &'a str,
    /// The frozen report's path, once written.
    pub report: Option<&'a str>,
    pub cost_usd: f64,
    /// The phase it stopped before (a halt, the keel), if it stopped.
    pub stopped_at: Option<&'a str>,
    /// Why the change, in the model's words.
    pub why: &'a str,
    /// The gate's seconds, the diff's size, and the push.
    pub numbers: Value,
}

impl SelfBranchBuilt<'_> {
    fn what(&self) -> String {
        let stop = self
            .stopped_at
            .map(|p| format!(", stopped before the {p}"))
            .unwrap_or_default();
        format!(
            "self branch {} for {} built: gate {}{stop}",
            self.branch, self.item, self.gate
        )
    }
}

impl Fact for SelfBranchBuilt<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBranchBuilt);

    fn row(&self) -> Value {
        json!({"item": self.item, "issue": self.issue, "branch": self.branch,
               "head": self.head, "gate": self.gate, "report": self.report,
               "cost_usd": self.cost_usd, "stopped_at": self.stopped_at,
               "what": self.what(), "why": self.why, "numbers": self.numbers,
               "undo": format!("git branch -D {}", self.branch)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let report = self
            .report
            .map(|r| format!(" Its report: {r}."))
            .unwrap_or_default();
        say.line(Approval, format!("{}.{report}", upper(&self.what())));
    }
}

fn upper(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
