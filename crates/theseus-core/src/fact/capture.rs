//! The facts of capture (theseus-slvl.5): what `directive.v1` heard in the
//! owner's words, and what the core did about it. A kept record is also a
//! `standing.added` row (`fact::standing`), in the same frame; these rows say
//! why it was kept and which judgment said so. Rows hold ids, kinds and sizes,
//! never the owner's words, which the record (or the open ask) holds; the
//! sentences, which only the owner's own session hears, do say them.

use serde_json::{json, Value};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{Event, LedgerKind, StandingKind, StandingNoticed, StandingScope};

use super::{Fact, Say};

/// A record was kept from the owner's words (`standing.captured`): the
/// notice, with its undo.
pub struct StandingCaptured<'a> {
    pub session: &'a str,
    pub record: &'a str,
    pub kind: StandingKind,
    pub scope: &'a StandingScope,
    pub node: &'a str,
    pub judgment: &'a str,
    /// The pack that judged: a label on the judgment finds it here.
    pub pack: &'a str,
    pub confidence: f64,
    /// The notice's words, as the owner reads them.
    pub line: &'a str,
}

impl Fact for StandingCaptured<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingCaptured);
    const METHOD: Option<&'static str> = Some(theseus_protocol::notify::STANDING_NOTICED);

    fn event(&self) -> Option<Event> {
        Some(Event::StandingNoticed(StandingNoticed {
            session_id: self.session.into(),
            what: "kept".into(),
            id: self.record.into(),
            line: self.line.into(),
        }))
    }

    fn row(&self) -> Value {
        json!({"record": self.record, "kind": self.kind.name(), "scope": self.scope.words(),
               "node": self.node, "judgment": self.judgment, "pack": self.pack,
               "confidence": self.confidence})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, self.line.to_string());
    }
}

/// A directive waits on the owner (`standing.asked`): unsure, an order, or a
/// correction made twice.
pub struct StandingAsked<'a> {
    pub session: &'a str,
    pub ask: &'a str,
    pub kind: StandingKind,
    pub scope: &'a StandingScope,
    pub why: &'a str,
    pub node: &'a str,
    pub judgment: &'a str,
    pub confidence: f64,
    pub line: &'a str,
}

impl Fact for StandingAsked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingAsked);
    const METHOD: Option<&'static str> = Some(theseus_protocol::notify::STANDING_NOTICED);

    fn event(&self) -> Option<Event> {
        Some(Event::StandingNoticed(StandingNoticed {
            session_id: self.session.into(),
            what: "asked".into(),
            id: self.ask.into(),
            line: self.line.into(),
        }))
    }

    fn row(&self) -> Value {
        json!({"ask": self.ask, "kind": self.kind.name(), "scope": self.scope.words(),
               "why": self.why, "node": self.node, "judgment": self.judgment,
               "confidence": self.confidence})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, self.line.to_string());
    }
}

/// The owner answered an ask, or it lapsed (`standing.answered`): `keep`,
/// `everywhere`, `not`, or `lapsed`.
pub struct StandingAnswered<'a> {
    pub ask: &'a str,
    pub answer: &'a str,
    pub record: Option<&'a str>,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for StandingAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingAnswered);

    fn row(&self) -> Value {
        json!({"ask": self.ask, "answer": self.answer, "record": self.record,
               "who": self.who, "via": self.via})
    }
}

/// A correction below the asking band was noted (`standing.candidate`), to
/// be proposed if the owner makes it twice in a week.
pub struct StandingCandidate<'a> {
    pub node: &'a str,
    pub judgment: &'a str,
    /// The probability `directive.v1` gave `correction`.
    pub strength: f64,
}

impl Fact for StandingCandidate<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingCandidate);

    fn row(&self) -> Value {
        json!({"node": self.node, "judgment": self.judgment, "strength": self.strength})
    }
}

/// The owner said a live record again (`standing.repeated`): the record's
/// wording or its delivery failed.
pub struct StandingRepeated<'a> {
    pub record: &'a str,
    pub node: &'a str,
    pub judgment: &'a str,
    /// Repeats of this record in the last seven days, this one included.
    pub week: usize,
}

impl Fact for StandingRepeated<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingRepeated);

    fn row(&self) -> Value {
        json!({"record": self.record, "node": self.node, "judgment": self.judgment,
               "week": self.week})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Standing: that was already {} (said {} time(s) this week): its wording or its delivery failed.",
                self.record, self.week
            ),
        );
    }
}
