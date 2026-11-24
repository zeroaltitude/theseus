//! `context.census` (issue 44): a session's next request built by the turn's own assembler
//! (`TurnRunner::view_built`, as `view.preview` builds it: an empty message, no recall run) and compiled as a
//! view turn's first call compiles it (`compiler::view::compile`). Nothing is sent, and nothing of the view is
//! kept: no view record, no ledger row.
//!
//! Its point of view is the one a next turn would get: a conversation's at its place, for a message from the
//! owner in person (`Verified::of_preview`); a task's at the task's own place, with the presence its turns get
//! from the call that started it (`Verified::of_task`); a fresh session's at an empty place of the kind asked
//! for, which holds nothing to read (`Verified::of_fresh`).
//!
//! A message given (`task.ask`'s question, `task_graph::ask`) is assembled for, and laid as the message of a
//! turn that never runs: a node in memory alone, after the session's own, which no store holds.

use anyhow::{anyhow, Result};
use theseus_memory::recall::Place;

use super::super::{Asked, CompileInput, Compiled, TurnRunner};
use crate::places::PlaceClass;
use crate::session::SessionRecord;
use crate::view::budget::Overflow;
use crate::view_gate::{place_name, Verified};

/// The turn id a census's view is assembled under: no turn has it.
const CENSUS_TURN: &str = "census";
/// Who a given message is from: another conversation, through `task.ask`.
const ASKER: &str = "task.ask";

impl TurnRunner {
    /// The place and the point of view `rec`'s next request is assembled at, or a fresh session's of
    /// `place_kind` (`class` its place's class): a conversation whose place cannot be read has none, as its
    /// turn would fail.
    fn census_pov(
        &self,
        sid: &str,
        rec: Option<&SessionRecord>,
        (place_kind, class): (&str, PlaceClass),
    ) -> Result<(String, Verified)> {
        match rec {
            Some(r) if r.task.is_some() => {
                let place = crate::task_graph::place::of_session(sid);
                let verified = Verified::of_task(self, sid)
                    .ok_or_else(|| anyhow!("the task's session {sid} could not be read"))?;
                Ok((place, verified))
            }
            Some(_) => {
                let place = place_name(self, sid)
                    .ok_or_else(|| anyhow!("this conversation's place could not be read"))?;
                Ok((place.clone(), Verified::of_preview(self, sid, &place)))
            }
            None => {
                let place = self.fresh_place(place_kind);
                let today = match class {
                    PlaceClass::Shared => Place::Shared(place.clone()),
                    _ => Place::Private,
                };
                Ok((place.clone(), Verified::of_fresh(today, &place)))
            }
        }
    }

    /// The place a fresh session of `place_kind` would speak at: the owner's DM where one is bound, an empty
    /// channel, a task's own place, else the terminal's.
    fn fresh_place(&self, place_kind: &str) -> String {
        match place_kind {
            "dm" => match self.place_rule.owner_dm_places().first() {
                Some(p) => format!("discord:{p}"),
                None => crate::exchange::place_of(None, None),
            },
            "channel" => "discord:channel:census".to_string(),
            "task" => format!(
                "{}{}census",
                crate::task_graph::place::PREFIX,
                crate::task_graph::ID_PREFIX
            ),
            _ => crate::exchange::place_of(None, None),
        }
    }

    /// `sid`'s next request (`rec` none: a fresh session's first, of `place_kind`), as a view turn's first
    /// call compiles it, for `message` (empty: none is laid). A floor too large for the model's window fails
    /// in words, as a turn would.
    pub(crate) fn census_view_compile(
        &self,
        (sid, rec): (&str, Option<&SessionRecord>),
        kind: (&str, PlaceClass),
        input: CompileInput<'_>,
        message: &str,
    ) -> Result<Compiled> {
        let (_, verified) = self.census_pov(sid, rec, kind)?;
        let person = rec
            .is_none_or(|r| r.task.is_none())
            .then(|| crate::exchange::OWNER.into());
        let built = self.view_built(
            Asked {
                store: &self.store,
                sid,
                turn: CENSUS_TURN,
                person,
                verified: Some(verified),
                recalled: Vec::new(),
                message: Some(message.to_string()),
                items: Vec::new(),
                // A census measures the view a turn assembles; the harness's part is the turn's own.
                harness: 0,
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
        if message.is_empty() {
            return Ok(crate::compiler::view::compile(input, &built.frozen));
        }
        // The message, as this turn's own: the compile lays it after the block beside it, as a turn's.
        let asked = crate::node::Node::relayed(
            sid,
            Some(CENSUS_TURN),
            crate::node::Origin::Agent,
            ASKER,
            message,
        );
        let nodes: Vec<_> = (input.nodes.iter().cloned())
            .chain([(input.last_position + 1, asked.into())])
            .collect();
        let input = CompileInput {
            nodes: &nodes,
            ..input
        };
        Ok(crate::compiler::view::compile(input, &built.frozen))
    }
}
