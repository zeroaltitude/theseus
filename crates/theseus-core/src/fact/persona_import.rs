//! The persona import's facts (theseus-a8n7): the import itself, and the
//! owner's answer to a proposed standing record. Rows hold ids, counts and
//! digests, never the agent's words.

use serde_json::{json, Value};
use theseus_protocol::NarrativePart::Session;
use theseus_protocol::{LedgerKind, StandingProposal};

use super::{Fact, Say};

/// A persona's files were carried over (`persona.imported`), and these
/// standing records were proposed from its feedback. Written only when the
/// import wrote something: a dry run writes nothing.
pub struct PersonaImported<'a> {
    pub name: &'a str,
    pub agent: &'a str,
    pub version: u32,
    pub files: usize,
    pub unchanged: bool,
    pub proposed: &'a [StandingProposal],
    pub known: u32,
}

impl Fact for PersonaImported<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PersonaImported);

    fn row(&self) -> Value {
        json!({"persona": self.name, "agent": self.agent, "version": self.version,
               "files": self.files, "unchanged": self.unchanged,
               "proposed": self.proposed.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
               "known": self.known})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Persona {} imported from agent {} as version {} ({} files, {} standing records proposed for you to keep).",
                self.name,
                self.agent,
                self.version,
                self.files,
                self.proposed.len()
            ),
        );
    }
}

/// The owner kept or dismissed a proposal (`standing.proposal_answered`).
pub struct StandingProposalAnswered<'a> {
    pub proposal: &'a StandingProposal,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for StandingProposalAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::StandingProposalAnswered);

    fn row(&self) -> Value {
        let p = self.proposal;
        json!({"id": p.id, "persona": p.persona, "kind": p.kind.name(), "answer": p.state,
               "record": p.record, "who": self.who, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let p = self.proposal;
        say.line(
            Session,
            format!(
                "Standing: {} {} the proposal {} for persona {}.",
                self.who, p.state, p.id, p.persona
            ),
        );
    }
}
