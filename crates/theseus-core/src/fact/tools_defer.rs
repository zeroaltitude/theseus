//! The deferred tools' facts (theseus-7m7w.2): a turn's use of its tools
//! (`tools.used`, which the selector learns from), a search (`tools.searched`)
//! and a call to a tool the request did not load (`tools.missed`). The rows
//! hold names and counts, never a query's text beyond its first 200 characters.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Turn;

use super::{Fact, Say};
use crate::tools_defer::board::{Miss, Search, TurnUse};
use crate::tools_defer::PlaceKind;

/// A turn's use of its tools, once at its end.
pub struct ToolsUsed<'a> {
    pub turn: &'a TurnUse,
}

impl Fact for ToolsUsed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolsUsed);

    fn row(&self) -> Value {
        let t = self.turn;
        let mut row = json!({
            "place": t.kind.as_str(),
            "mechanism": t.mechanism.map(|m| m.as_str()),
            "loaded": t.loaded,
            "deferred": t.deferred,
            "used": t.used,
            "searched": t.searched,
            "unused": t.unused,
        });
        // Only when a family took the request past the place's budget (theseus-xlri5).
        if let Some(r) = &t.raised {
            row["raised"] = json!(r);
        }
        row
    }
}

/// A search the model made for a tool that was not loaded.
pub struct ToolsSearched<'a> {
    pub place: PlaceKind,
    pub search: &'a Search,
}

impl Fact for ToolsSearched<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolsSearched);

    fn row(&self) -> Value {
        let s = self.search;
        json!({
            "place": self.place.as_str(),
            "query": s.query,
            "surfaced": s.surfaced,
            "families": s.families,
            "called": s.called,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let s = self.search;
        say.line(
            Turn,
            format!(
                "Tool search for {:?} surfaced {}{}.",
                s.query,
                if s.surfaced.is_empty() {
                    "nothing".to_string()
                } else {
                    s.surfaced.join(", ")
                },
                if s.called { ", and it was called" } else { "" }
            ),
        );
    }
}

/// A call to a tool the request did not load, with no search before it. It
/// was not run; the answer named the search.
pub struct ToolsMissed<'a> {
    pub place: PlaceKind,
    pub miss: &'a Miss,
}

impl Fact for ToolsMissed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolsMissed);

    fn row(&self) -> Value {
        json!({
            "place": self.place.as_str(),
            "kind": "unsearched_call",
            "tool": self.miss.tool,
            "family": self.miss.family,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Turn,
            format!(
                "A call to {} was not run: its {} family is not loaded here, and no search found it first.",
                self.miss.tool, self.miss.family
            ),
        );
    }
}
