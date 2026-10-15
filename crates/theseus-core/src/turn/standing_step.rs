//! The owner's standing records in the turn's spec (theseus-a7vi): the block
//! goes first in the system's second block, once a turn with the spec, and
//! the compile's manifest lists what it carried. The situation check
//! refuses a compile that dropped one (`standing::missing`).
//!
//! The records scoped to the turn's surface (theseus-slvl.4) are a second
//! block, "Standing here:", in the request's tail (`standing::Block::here`),
//! so the system is byte-identical across surfaces; the check holds it too
//! (`standing::missing_here`).

use super::{Turn, TurnRunner};
use crate::compiler::{Compiled, RequestSpec};
use crate::standing::{Block, Scope};

/// What a turn holds of the records: the system block and the tail block it
/// carries (for the manifest and the checks), and the surface its input came
/// through.
#[derive(Default)]
pub(super) struct Held {
    pub(super) block: Option<Block>,
    pub(super) here: Option<Block>,
    pub(super) surface: Option<&'static str>,
}

impl TurnRunner {
    /// Read the records of the turn's scope from memory and put their block
    /// in `spec`; `t` keeps it for the manifest and the check. A set that
    /// cannot be read is warned of and the turn goes without (the check has
    /// nothing to hold it to).
    pub(super) fn stand(&self, t: &mut Turn<'_>, spec: &mut RequestSpec) {
        (t.standing.block, t.standing.here) =
            self.standing_blocks(t.tc.session_id, t.tc.class, t.standing.surface);
        if let Some(block) = &t.standing.block {
            block.apply(spec);
        }
    }

    /// The block of `session_id`'s scope in a place of `class`, from memory:
    /// what its next turn's system carries (the keep-warm read builds that
    /// spec too, theseus-rr15). A surface's records are never in it.
    pub(crate) fn standing_block(
        &self,
        session_id: &str,
        class: crate::places::PlaceClass,
    ) -> Option<Block> {
        self.standing_blocks(session_id, class, None).0
    }

    /// The system's block and the tail's (the records of `surface`) of
    /// `session_id`'s scope in a place of `class`, from memory.
    fn standing_blocks(
        &self,
        session_id: &str,
        class: crate::places::PlaceClass,
        surface: Option<&'static str>,
    ) -> (Option<Block>, Option<Block>) {
        let all = match self.standing.all(&self.store) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"),
                    "the standing records cannot be read: this turn goes without them");
                return (None, None);
            }
        };
        let place = self.target_of(session_id);
        let scope = Scope {
            session: session_id,
            place: place.as_deref(),
            surface,
        };
        let now = theseus_protocol::now_unix_ms();
        (
            Block::of(&all, scope, class, now),
            Block::here(&all, scope, class, now),
        )
    }

    /// A compilation this compile made lists the records its system carried.
    pub(super) fn standing_listed(t: &Turn<'_>, compiled: &mut Compiled) {
        if compiled.new_compilation {
            if let Some(b) = &t.standing.block {
                compiled.compilation.manifest.standing = b.used.clone();
            }
        }
    }

    /// The surface's records go in the request's tail, after the cached
    /// prefix: a plain turn on a surface with none adds nothing.
    pub(super) fn standing_here(t: &Turn<'_>, compiled: &mut Compiled) {
        if let Some(b) = &t.standing.here {
            b.attach_here(compiled);
        }
    }
}
