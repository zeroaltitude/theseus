//! Content flags' facts (theseus-9ymz0): each item judged, each label raised,
//! each label the operator removed, and each item that could not be judged.
//! The rule is `content.rs`; the ask is `judge/content.rs`; the removal is
//! `rpc/content.rs`.
//!
//! `content.judged` is written for every item judged, whether or not anything
//! was raised, so the bench and `inspect` see every ask. The other three are
//! the things that matter in it.

use serde::Serialize;
use serde_json::{json, Value};
use theseus_judge::band::Band;
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{notify, Event, LedgerKind, UnlabelResult};

use super::{Fact, Say};
use crate::content::{Flag, ItemKind, Label};

/// What the pack answered to one question about one item.
#[derive(Debug, Clone, Serialize)]
pub struct Answered<'a> {
    pub flag: Flag,
    /// The option chosen: `yes` or `no`, or the pack's no-match word.
    pub choice: &'a str,
    pub confidence: f64,
    /// Where the answer fell, when it had a band.
    pub band: Option<Band>,
}

/// `content.judged`: one item was judged, and what each question said. A
/// long item is one row, with `pieces` counting the asks it took.
pub struct ContentJudged<'a> {
    /// The node whose content was judged.
    pub node: &'a str,
    pub kind: ItemKind,
    pub source: &'a str,
    pub answers: &'a [Answered<'a>],
    /// What the asks cost, in millionths of a dollar.
    pub cost_micros: u64,
    pub pieces: u32,
}

impl Fact for ContentJudged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContentJudged);

    fn row(&self) -> Value {
        json!({"node": self.node, "kind": self.kind, "source": self.source,
               "answers": self.answers, "cost_micros": self.cost_micros,
               "pieces": self.pieces})
    }
}

/// `content.labelled`: a label was raised on the session. Written only when
/// the flag was not on it already (`ContentFlags::raise`), so a second raise
/// of the same flag writes nothing.
pub struct ContentLabelled<'a> {
    pub label: &'a Label,
}

impl Fact for ContentLabelled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContentLabelled);

    fn row(&self) -> Value {
        let l = self.label;
        json!({"flag": l.flag, "node": l.node, "kind": l.kind, "source": l.source,
               "confidence": l.confidence, "band": l.band})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let l = self.label;
        say.line(
            Session,
            format!(
                "Content: {} ({}) raised {}: {}.",
                l.kind.as_str(),
                l.source,
                l.flag.as_str(),
                l.flag.means()
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

/// `content.unjudged`: an item could not be judged (Jev failed, the
/// reservation was refused, the state would not build). Nothing is raised and
/// `judged_through` does not advance, so a session that needs the verdict (a
/// request to a model of restricted origin) goes to Claude instead.
pub struct ContentUnjudged<'a> {
    pub node: &'a str,
    pub kind: ItemKind,
    pub source: &'a str,
    /// Why, in the ask's own words.
    pub why: &'a str,
}

impl Fact for ContentUnjudged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContentUnjudged);

    fn row(&self) -> Value {
        json!({"node": self.node, "kind": self.kind, "source": self.source, "why": self.why})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Content: {} ({}) could not be judged ({}); this session stays off models of \
                 restricted origin until it is.",
                self.kind.as_str(),
                self.source,
                self.why
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::LabelView;

    fn label() -> Label {
        Label {
            flag: Flag::Confidential,
            node: Some("n7".into()),
            kind: ItemKind::ToolResult,
            source: "fs.read /srv/notes/plan.md".into(),
            confidence: 0.71,
            band: Band::Confirm,
            at_ms: 5,
        }
    }

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
    /// the cost and the pieces, and is a row whether or not anything was raised.
    #[test]
    fn a_judged_row_names_every_answer() {
        let answers = [
            Answered {
                flag: Flag::Injection,
                choice: "no",
                confidence: 0.97,
                band: None,
            },
            Answered {
                flag: Flag::Secret,
                choice: "yes",
                confidence: 0.64,
                band: Some(Band::Confirm),
            },
        ];
        let f = ContentJudged {
            node: "n7",
            kind: ItemKind::FetchedPage,
            source: "http.fetch example.invalid/x",
            answers: &answers,
            cost_micros: 1200,
            pieces: 3,
        };
        assert_eq!(
            f.row(),
            json!({"node": "n7", "kind": "fetched_page", "source": "http.fetch example.invalid/x",
                   "answers": [
                       {"flag": "injection", "choice": "no", "confidence": 0.97, "band": null},
                       {"flag": "secret", "choice": "yes", "confidence": 0.64, "band": "confirm"},
                   ],
                   "cost_micros": 1200, "pieces": 3})
        );
    }

    /// `content.labelled` is the label as it was raised.
    #[test]
    fn a_labelled_row_is_the_label() {
        let l = label();
        let f = ContentLabelled { label: &l };
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
