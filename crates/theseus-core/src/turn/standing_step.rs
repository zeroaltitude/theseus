//! The owner's standing records in the turn's spec (theseus-a7vi): the block
//! goes first in the system's second block, once a turn with the spec, and
//! the compile's manifest lists what it carried. The situation check
//! refuses a compile that dropped one (`standing::missing`).

use super::{Turn, TurnRunner};
use crate::compiler::{Compiled, RequestSpec};
use crate::standing::{Block, Scope};

impl TurnRunner {
    /// Read the records of the turn's scope from memory and put their block
    /// in `spec`; `t` keeps it for the manifest and the check. A set that
    /// cannot be read is warned of and the turn goes without (the check has
    /// nothing to hold it to).
    pub(super) fn stand(&self, t: &mut Turn<'_>, spec: &mut RequestSpec) {
        t.standing = None;
        let all = match self.standing.all(&self.store) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"),
                    "the standing records cannot be read: this turn goes without them");
                return;
            }
        };
        let place = self.target_of(t.tc.session_id);
        let scope = Scope {
            session: t.tc.session_id,
            place: place.as_deref(),
        };
        let now = theseus_protocol::now_unix_ms();
        if let Some(block) = Block::of(&all, scope, t.tc.class, now) {
            block.apply(spec);
            t.standing = Some(block);
        }
    }

    /// A compilation this compile made lists the records its system carried.
    pub(super) fn standing_listed(t: &Turn<'_>, compiled: &mut Compiled) {
        if compiled.new_compilation {
            if let Some(b) = &t.standing {
                compiled.compilation.manifest.standing = b.used.clone();
            }
        }
    }
}
