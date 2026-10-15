//! A persona changed between compiles (theseus-slvl.2): one row for each
//! difference, riding the turn's next frame with the state that now stands.
//! Digests, never text: the file holds the text.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Context;

use super::{Fact, Say};

/// `persona.changed`: a persona's file or one of its fields differs from the
/// last compile that saw it.
pub struct PersonaChanged<'a> {
    pub persona: &'a str,
    /// `file`, or a field's name.
    pub field: &'a str,
    pub path: Option<&'a str>,
    /// Digests; empty is absent (a file not readable, a field not set).
    pub old: &'a str,
    pub new: &'a str,
}

impl Fact for PersonaChanged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PersonaChanged);

    fn row(&self) -> Value {
        json!({"persona": self.persona, "field": self.field, "path": self.path,
               "old": self.old, "new": self.new})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = match self.path {
            Some(p) => format!("its file {p}"),
            None => format!("its {}", self.field),
        };
        say.line(
            Context,
            format!(
                "Context: the persona {} changed: {what} is not what the last compile saw.",
                self.persona
            ),
        );
    }
}

/// `persona.engaged` (theseus-c1uz): which persona answered a turn, how it
/// came to (an address, Jev's pick, the prior), and what Jev picked when
/// that differs. Rides the turn's next frame. It is the record the route
/// line, a correction's label and the learning report read.
pub struct PersonaEngaged<'a> {
    pub turn: &'a str,
    pub persona: Option<&'a str>,
    pub name: &'a str,
    /// `explicit`, `jev`, `jev_shown` or `prior`.
    pub how: &'a str,
    pub p: Option<f64>,
    pub runner_up: Option<(&'a str, f64)>,
    pub terms: &'a [String],
    /// The route judgment that carried the persona question.
    pub judgment: Option<&'a str>,
    /// Jev's pick when it was not what engaged.
    pub jev: Option<&'a str>,
}

impl Fact for PersonaEngaged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PersonaEngaged);

    fn row(&self) -> Value {
        json!({"turn": self.turn, "persona": self.persona, "name": self.name, "how": self.how,
               "p": self.p, "runner_up": self.runner_up, "terms": self.terms,
               "judgment": self.judgment, "jev": self.jev})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let by = match self.how {
            "explicit" => "the message addressed it",
            "prior" => "it answered last",
            _ => "Jev picked it",
        };
        say.line(
            Context,
            format!("Persona: {} answers this turn ({by}).", self.name),
        );
    }
}
