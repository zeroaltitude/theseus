//! The content flags' facts (theseus-9ymz0): an item judged, a label it raised,
//! and an item that could not be judged. `content.removed` is the removal's own
//! (`rpc/content.rs`).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};
use crate::content::{Flag, ItemKind};

/// One question's answer, as the row keeps it.
#[derive(Clone)]
pub struct AnswerRow {
    pub question: &'static str,
    pub choice: String,
    pub confidence: f64,
    pub band: theseus_judge::band::Band,
}

/// `content.judged`: one item judged, whether or not anything was raised, so the
/// bench and `inspect` see every ask.
pub struct ContentJudged<'a> {
    pub node: &'a str,
    pub kind: ItemKind,
    pub source: &'a str,
    /// How many pieces the item was asked in.
    pub pieces: u32,
    /// Each question's answer, the union of the pieces': for a question asked of
    /// several pieces, the one that raised, else the first.
    pub answers: Vec<AnswerRow>,
    pub cost_micros: Option<u64>,
    /// The item was longer than `[content] max_pieces` pieces, and what is
    /// past them was not read (`content::split`).
    pub truncated: bool,
}

impl Fact for ContentJudged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContentJudged);

    fn row(&self) -> Value {
        let answers: Vec<Value> = self
            .answers
            .iter()
            .map(|a| {
                json!({"question": a.question, "choice": a.choice,
                       "confidence": a.confidence, "band": a.band})
            })
            .collect();
        json!({"node": self.node, "kind": self.kind, "source": self.source,
               "pieces": self.pieces, "truncated": self.truncated, "answers": answers,
               "cost_micros": self.cost_micros})
    }
}

/// `content.labelled`: one label raised on a session.
pub struct ContentLabelled<'a> {
    pub flag: Flag,
    pub node: &'a str,
    pub kind: ItemKind,
    pub source: &'a str,
    pub confidence: f64,
    pub band: theseus_judge::band::Band,
}

impl Fact for ContentLabelled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContentLabelled);

    fn row(&self) -> Value {
        json!({"flag": self.flag, "node": self.node, "kind": self.kind, "source": self.source,
               "confidence": self.confidence, "band": self.band})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let effect = match self.flag {
            Flag::Injection => {
                "writes, programs and sends are refused until the operator clears it"
            }
            _ => "China-origin models are off for this session",
        };
        say.line(
            Session,
            format!(
                "Content: {} ({} from {}), so {effect}.",
                self.flag.means(),
                self.kind.as_str(),
                self.source
            ),
        );
    }
}

/// `content.unjudged`: an item that could not be judged. Nothing is raised, and
/// the session's `judged_through` does not advance past it.
pub struct ContentUnjudged<'a> {
    pub node: &'a str,
    pub kind: ItemKind,
    pub source: &'a str,
    /// `refused` (the day's budget), `failed` (the call), `unbuilt` (the state
    /// or the client would not build).
    pub why: &'a str,
}

impl Fact for ContentUnjudged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContentUnjudged);

    fn row(&self) -> Value {
        json!({"node": self.node, "kind": self.kind, "source": self.source, "why": self.why})
    }
}
