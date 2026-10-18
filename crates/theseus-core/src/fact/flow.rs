//! A flow's facts (theseus-pw1q.21.4): loaded, retired, and run. A step of a
//! run is not a fact here: it is an ordinary tool call and writes its own
//! rows through `ToolRuntime`, so one run is its steps' rows and one
//! `flow.ran`. A load and a retire ride their own frame; a run's row rides
//! the turn's next frame.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::{Approval, Tool};

use super::{Fact, Say};
use crate::narrative::count;

/// The first characters of a digest, as a card names it.
fn short(digest: &str) -> &str {
    &digest[..digest.len().min(6)]
}

/// A flow frozen and loaded: offered from the next turn, in its scope.
pub struct FlowLoaded<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    pub steps: usize,
    pub repos: &'a [String],
    pub places: &'a [String],
    pub by: &'a str,
    pub via: &'a str,
}

impl Fact for FlowLoaded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FlowLoaded);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest, "steps": self.steps,
            "repos": self.repos, "places": self.places, "by": self.by, "via": self.via,
            "undo": format!("theseus self revert flow:{}", self.name)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Flow {} {} loaded by {}: {}, offered from the next turn.",
                self.name,
                short(self.digest),
                self.by,
                count(self.steps as u64, "step", "steps")
            ),
        );
    }
}

/// A flow left the loaded set; its frozen copy stays on disk.
pub struct FlowRetired<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    /// `owner`, or `integrity` when its frozen copy no longer read.
    pub by: &'a str,
    pub via: &'a str,
}

impl Fact for FlowRetired<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FlowRetired);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest, "by": self.by, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Flow {} {} retired by {}: gone from the next turn.",
                self.name,
                short(self.digest),
                self.by
            ),
        );
    }
}

/// One run of a flow: how it ended, and what it cost in steps and seconds.
pub struct FlowRan<'a> {
    pub name: &'a str,
    pub digest: &'a str,
    /// `ok`, an exit's name, or `handback:<name>`.
    pub exit: &'a str,
    pub steps: usize,
    pub secs: f64,
}

impl Fact for FlowRan<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FlowRan);

    fn row(&self) -> Value {
        json!({"name": self.name, "digest": self.digest, "exit": self.exit,
            "steps": self.steps, "secs": (self.secs * 10.0).round() / 10.0})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "Flow {} ended {} after {} in {:.1} s.",
                self.name,
                self.exit,
                count(self.steps as u64, "step", "steps"),
                self.secs
            ),
        );
    }
}
