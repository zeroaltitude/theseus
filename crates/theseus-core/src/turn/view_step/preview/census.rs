//! `context.census` in a place set to the view (issue 44): a session's next request built by the turn's own
//! assembler (`TurnRunner::view_built`, as `view.preview` builds it: the owner in person, an empty message, no
//! recall run) and compiled as a view turn's first call compiles it (`compiler::view::compile`), never
//! `compiler::compile` over the session's transcript. Nothing is sent, and nothing of the view is kept: no
//! view record, no ledger row.

use anyhow::Result;

use super::super::{Asked, CompileInput, Compiled, TurnRunner};
use crate::config::assembly::Assembly;
use crate::session::SessionRecord;
use crate::view::budget::Overflow;
use crate::view_gate::{place_name, Verified};

/// The turn id a census's view is assembled under: no turn has it.
const CENSUS_TURN: &str = "census";

impl TurnRunner {
    /// The place whose view `rec`'s next request is assembled by, when that place is set to the view: a
    /// conversation's (a task's turns are the transcript's today).
    pub(crate) fn census_view_place(&self, rec: &SessionRecord) -> Option<String> {
        if self.cfg.context.assembly.is_none()
            || rec.task.is_some()
            || rec.kind == theseus_protocol::SessionKind::Task
        {
            return None;
        }
        let place = place_name(self, &rec.session_id)?;
        (self.cfg.context.assembly_for(&place) == Assembly::View).then_some(place)
    }

    /// `rec`'s next request at `place`, as a view turn's first call compiles it, for an empty message from
    /// the owner in person. A floor too large for the model's window fails in words, as a turn would.
    pub(crate) fn census_view_compile(
        &self,
        rec: &SessionRecord,
        place: &str,
        input: CompileInput<'_>,
    ) -> Result<Compiled> {
        let sid = rec.session_id.as_str();
        let built = self.view_built(
            Asked {
                store: &self.store,
                sid,
                turn: CENSUS_TURN,
                person: Some(crate::exchange::OWNER.into()),
                verified: Some(Verified::of_preview(self, sid, place)),
                recalled: Vec::new(),
                message: Some(String::new()),
            },
            input.spec,
            &self.view_budget(input.spec),
        )?;
        if let Err(Overflow::Floor { tokens, room, .. }) = &built.assembled {
            anyhow::bail!(
                "the next request does not fit the model's window: its newest exchange alone is about \
                 {tokens} tokens, and the window has room for {room}"
            );
        }
        Ok(crate::compiler::view::compile(input, &built.frozen))
    }
}
