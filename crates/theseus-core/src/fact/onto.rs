//! Jev's filing into the ontology (theseus-xzxqv): what one `onto.v1`
//! judgment came to where it acted, asked or was refused, and the naming
//! call's cost. A judgment whose answers were all `none` or below the
//! confirm band writes neither: its own `judge.call` row is the record, so a
//! plain turn adds nothing.

use serde_json::{json, Value};
use theseus_protocol::{LedgerKind, NarrativePart::Session};

use super::{Fact, Say};
use crate::judge::onto::decide::{Plan, Step};

/// One judgment's plan, in the frame that wrote its records (`ontology.write:<judgment>`
/// scope beside the records' own rows): per kind, how the answer landed.
pub struct OntoFiled<'a> {
    pub plan: &'a Plan,
}

fn step_row(s: &Step) -> Value {
    match s {
        Step::Filed {
            kind,
            id,
            minted,
            confidence,
            ..
        } => json!({"kind": kind, "outcome": "filed", "category": id, "minted": minted,
                    "confidence": confidence}),
        Step::Ask {
            kind, confidence, ..
        } => {
            json!({"kind": kind, "outcome": "asked", "confidence": confidence})
        }
        Step::Unnamed { kind, confidence } => {
            json!({"kind": kind, "outcome": "unnamed", "confidence": confidence})
        }
        Step::Refused {
            kind,
            why,
            disagrees,
        } => json!({"kind": kind, "outcome": "refused", "why": why, "disagrees": disagrees}),
    }
}

impl Fact for OntoFiled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OntoFiled);

    fn row(&self) -> Value {
        json!({"judgment": self.plan.judgment, "steps": self.plan.steps.iter().map(step_row).collect::<Vec<_>>()})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let p = self.plan;
        let line = match p.footer() {
            Some(f) => format!("Ontology: Jev {f}."),
            None => format!(
                "Ontology: Jev's answer for this session was {}.",
                p.steps
                    .iter()
                    .map(|s| match s {
                        Step::Ask { kind, .. } => format!("a question about the {kind}"),
                        Step::Unnamed { kind, .. } => format!("a new {kind} with no name yet"),
                        Step::Refused { kind, why, .. } => format!("refused for the {kind}: {why}"),
                        Step::Filed { .. } => String::new(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        say.line(Session, line);
    }
}

/// The naming call (`[ontology] name_profile`): three names and a line for a
/// new category Jev called for and no mined name fit. A day-ceiling spend
/// (`day_ceiling::SPEND_KINDS`).
pub struct OntoNamed<'a> {
    pub session: &'a str,
    pub kind: &'a str,
    pub profile: &'a str,
    pub model: &'a str,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub names: &'a [String],
    pub description: &'a str,
    pub failed: Option<&'a str>,
}

impl Fact for OntoNamed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OntoNamed);

    fn row(&self) -> Value {
        json!({"session": self.session, "kind": self.kind, "profile": self.profile,
               "model": self.model, "cost_usd": self.cost_usd,
               "input_tokens": self.input_tokens, "output_tokens": self.output_tokens,
               "names": self.names, "description": self.description, "failed": self.failed})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let cost = crate::narrative::dollars(theseus_judge::price::usd_to_micros(self.cost_usd));
        let line = match self.failed {
            Some(why) => format!(
                "Ontology: {}'s naming of a new {} failed ({why}), for {cost}.",
                self.profile, self.kind
            ),
            None => format!(
                "Ontology: {} suggested {} for a new {}, for {cost}.",
                self.profile,
                crate::narrative::count(self.names.len() as u64, "name", "names"),
                self.kind
            ),
        };
        say.line(Session, line);
    }
}
