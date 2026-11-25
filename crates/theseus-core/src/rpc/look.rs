//! `work.look` (issue 46): the owner was shown the work board at a position,
//! and the store's look moves there. The look is META `look` in the store it
//! points into, so a restore of an older backup, another store and a copy each
//! keep their own, and `work.digest {since_look}` tells from it.
//!
//! - **Forward only, and never past the head.** A position at or before the
//!   look moves nothing. A position past this store's head is a position from
//!   another history: refused in fixed words, never clamped, since a clamp
//!   would read as "you saw everything".
//! - **A look at its own record writes nothing.** Every look is a frame; one
//!   that has nothing but the last look's own record since that look would
//!   only write itself again.
//! - **The owner's own terminal.** A connection that reads no private text
//!   (Discord, an MCP client) does not move it; Discord keeps its own mark.
//!   `work.look` is on `mcp_server`'s `NOT_FOR_MCP`.

use std::sync::{Arc, Mutex, PoisonError};

use theseus_protocol::{error_code, now_unix_ms, Looked, WorkLookParams, WorkLookResult};
use theseus_store::kinds;

use super::server::RpcFailure;
use super::Core;
use crate::approval::Surface;

/// The META key of the store's look.
pub(crate) const LOOK_KEY: &str = "look";

/// A look past the store's head, in the words the caller is told.
pub(crate) const PAST_HEAD: &str = "that position is past this store's head";

/// One look at a time, so two at once cannot move it back.
static LOOKING: Mutex<()> = Mutex::new(());

impl Core {
    /// `work.look` for a connection from `surface`.
    pub(crate) async fn work_look_for(
        self: &Arc<Self>,
        p: WorkLookParams,
        surface: Surface,
    ) -> Result<WorkLookResult, RpcFailure> {
        if !surface.reads_private() {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                format!(
                    "{} does not move where the owner last looked",
                    surface.name()
                ),
            ));
        }
        let core = Arc::clone(self);
        tokio::task::spawn_blocking(move || core.look_at(p.position, now_unix_ms()))
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
    }

    /// The store's look, none before the first.
    pub(crate) fn store_look(&self) -> Result<Option<Looked>, RpcFailure> {
        Ok(self.store.get_meta::<Looked>(LOOK_KEY)?)
    }

    fn look_at(&self, position: u64, at_ms: u64) -> Result<WorkLookResult, RpcFailure> {
        let _one = LOOKING.lock().unwrap_or_else(PoisonError::into_inner);
        if position > self.store.last_position() {
            return Err(RpcFailure::new(error_code::INVALID_PARAMS, PAST_HEAD));
        }
        let inner = self.store.inner();
        if let Some(rec) = theseus_store::Store::latest_by_key(&**inner, kinds::META, LOOK_KEY)? {
            let old: Looked = rec.decode()?;
            if position <= old.position {
                return Ok(WorkLookResult { look: old });
            }
            // Anything but the look's own record between it and here?
            let since = theseus_store::Store::scan(&**inner, old.position + 1, Some(position), 2)?;
            if since.iter().all(|r| r.position == rec.position) {
                return Ok(WorkLookResult { look: old });
            }
        }
        let look = Looked { position, at_ms };
        self.store.put_meta(LOOK_KEY, &look)?;
        Ok(WorkLookResult { look })
    }
}
