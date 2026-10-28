//! The content flags' facts (theseus-9ymz0): an item judged, a label it raised,
//! a label the operator removed, and an item that could not be judged. The rule
//! is `content.rs`; the ask is `judge/content.rs`; the removal is
//! `rpc/content.rs`.
//!
//! `content.judged` is written for every item judged, whether or not anything
//! was raised, so the bench and `inspect` see every ask. The other three are
//! the things that matter in it.

use serde_json::{json, Value};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, LedgerKind, UnlabelResult};

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
    /// The item was longer than `[content_based_security] max_pieces` pieces, and what is
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

/// `content.removed`: the operator removed a label. Its row rides in the same
/// frame as the record, and its notification, `session.unlabelled`, goes to
/// the session's watchers once that frame is written.
pub struct ContentRemoved<'a> {
    pub result: &'a UnlabelResult,
}

impl Fact for ContentRemoved<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContentRemoved);
    const METHOD: Option<&'static str> = Some(notify::SESSION_UNLABELLED);

    fn row(&self) -> Value {
        let r = self.result;
        json!({"flag": r.flag, "by": r.by, "who": r.who, "via": r.via, "how": r.how,
               "correlation_id": r.correlation_id, "node": r.label.node,
               "kind": r.label.kind, "source": r.label.source})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::SessionUnlabelled(self.result.clone()))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.result;
        say.line(
            Session,
            format!(
                "Content: {} removed {} from this session ({}).",
                r.by, r.flag, r.via
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

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_judge::band::Band;
    use theseus_protocol::LabelView;

    fn result() -> UnlabelResult {
        UnlabelResult {
            session_id: "ses_1".into(),
            flag: "confidential".into(),
            by: "the CLI".into(),
            who: "cli".into(),
            via: "cli".into(),
            how: "content.unlabel".into(),
            correlation_id: None,
            at_ms: 9,
            label: LabelView {
                flag: "confidential".into(),
                node: Some("n7".into()),
                kind: "tool_result".into(),
                source: "fs.read /srv/notes/plan.md".into(),
                confidence: 0.71,
                band: "confirm".into(),
                at_ms: 5,
                judgment: None,
            },
        }
    }

    /// Each row is listed under its own kind, and the kind is the one the
    /// registry knows by its word, so a query by the word finds it.
    #[test]
    fn the_four_rows_are_registered_under_their_words() {
        for (word, kind) in [
            ("content.judged", LedgerKind::ContentJudged),
            ("content.labelled", LedgerKind::ContentLabelled),
            ("content.removed", LedgerKind::ContentRemoved),
            ("content.unjudged", LedgerKind::ContentUnjudged),
        ] {
            assert_eq!(kind.as_str(), word);
            assert_eq!(LedgerKind::parse(word), Some(kind));
            assert!(
                super::super::FACTS.iter().any(|f| f.kind == Some(kind)),
                "{word} is listed in FACTS"
            );
        }
        let removed = super::super::FACTS
            .iter()
            .find(|f| f.kind == Some(LedgerKind::ContentRemoved))
            .unwrap();
        assert_eq!(removed.method, Some("session.unlabelled"));
    }

    /// `content.judged` carries each question's choice, confidence and band,
    /// the cost, the pieces and whether the bound cut the item short, and is a
    /// row whether or not anything was raised.
    #[test]
    fn a_judged_row_names_every_answer() {
        let f = ContentJudged {
            node: "n7",
            kind: ItemKind::FetchedPage,
            source: "http.fetch example.invalid/x",
            pieces: 3,
            answers: vec![
                AnswerRow {
                    question: "injection",
                    choice: "no".into(),
                    confidence: 0.97,
                    band: Band::Act,
                },
                AnswerRow {
                    question: "secret",
                    choice: "yes".into(),
                    confidence: 0.64,
                    band: Band::Confirm,
                },
            ],
            cost_micros: Some(1200),
            truncated: true,
        };
        assert_eq!(
            f.row(),
            json!({"node": "n7", "kind": "fetched_page", "source": "http.fetch example.invalid/x",
                   "pieces": 3, "truncated": true,
                   "answers": [
                       {"question": "injection", "choice": "no", "confidence": 0.97, "band": "act"},
                       {"question": "secret", "choice": "yes", "confidence": 0.64, "band": "confirm"},
                   ],
                   "cost_micros": 1200})
        );
    }

    /// `content.labelled` is the label as it was raised.
    #[test]
    fn a_labelled_row_is_the_label() {
        let f = ContentLabelled {
            flag: Flag::Confidential,
            node: "n7",
            kind: ItemKind::ToolResult,
            source: "fs.read /srv/notes/plan.md",
            confidence: 0.71,
            band: Band::Confirm,
        };
        assert_eq!(
            f.row(),
            json!({"flag": "confidential", "node": "n7", "kind": "tool_result",
                   "source": "fs.read /srv/notes/plan.md", "confidence": 0.71,
                   "band": "confirm"})
        );
    }

    /// `content.removed` names the flag, who, how and through what, the
    /// approval when one pressed it, and what the label was; its notification is
    /// the result itself.
    #[test]
    fn a_removed_row_names_who_and_what() {
        let r = result();
        let f = ContentRemoved { result: &r };
        let row = f.row();
        assert_eq!(row["flag"], "confidential");
        assert_eq!(row["by"], "the CLI");
        assert_eq!(row["via"], "cli");
        assert_eq!(row["how"], "content.unlabel");
        assert_eq!(row["correlation_id"], Value::Null);
        assert_eq!(row["node"], "n7");
        match f.event() {
            Some(Event::SessionUnlabelled(e)) => assert_eq!(e, r),
            other => panic!("the notification is session.unlabelled: {other:?}"),
        }
        assert_eq!(f.event().unwrap().method(), "session.unlabelled");
    }

    /// `content.unjudged` says why, and nothing else.
    #[test]
    fn an_unjudged_row_says_why() {
        let f = ContentUnjudged {
            node: "n8",
            kind: ItemKind::OperatorMessage,
            source: "discord:dm",
            why: "the reservation was refused",
        };
        assert_eq!(
            f.row(),
            json!({"node": "n8", "kind": "operator_message", "source": "discord:dm",
                   "why": "the reservation was refused"})
        );
    }
}
