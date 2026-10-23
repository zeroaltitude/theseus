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

/// `persona.migrated` (theseus-utfnl): `[personas]` was read once and each
/// persona written as a category, in one frame. Paths and digests, never
/// text.
pub struct PersonaMigrated<'a> {
    /// (key, soul path, digest of its text when it could be read).
    pub personas: &'a [(String, String, Option<String>)],
    /// Files beyond a persona's first, which are not carried.
    pub dropped: &'a [String],
}

impl Fact for PersonaMigrated<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PersonaMigrated);

    fn row(&self) -> Value {
        let ps: Vec<Value> = self
            .personas
            .iter()
            .map(|(k, p, d)| json!({"key": k, "soul": p, "digest": d}))
            .collect();
        json!({"personas": ps, "dropped": self.dropped})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "Context: {} persona(s) moved from the config into the ontology.",
                self.personas.len()
            ),
        );
    }
}

/// `persona.soul` (theseus-utfnl): a persona's soul file was written by the
/// owner. Where it is, and the digests before and after.
pub struct PersonaSoul<'a> {
    pub persona: &'a str,
    pub path: &'a str,
    pub old: Option<&'a str>,
    pub new: &'a str,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for PersonaSoul<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PersonaSoul);

    fn row(&self) -> Value {
        json!({"persona": self.persona, "path": self.path, "old": self.old, "new": self.new,
               "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "Context: {} wrote the soul of the persona {} ({}).",
                self.who, self.persona, self.path
            ),
        );
    }
}
