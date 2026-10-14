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
