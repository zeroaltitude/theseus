//! `work.agenda` (theseus-nuhsz; horizon HZ8): every occurrence in a window,
//! read from the work board's wakes in memory. A series is expanded by the
//! kernel's own arithmetic in the daemon's zone, a paused wake has none, and
//! at most `AGENDA_MAX` (200) lines come back, `more` saying when the window
//! held more. A read: no frame, no lock but the board's, no model.
//!
//! The place rule is `work.watch`'s: a connection that reads no private text
//! must name the conversations and sees those of a shared place alone, and
//! what it reads passes the scrubber.

use std::sync::Arc;

use theseus_protocol::work::horizon::{AgendaParams, AgendaResult};
use theseus_protocol::{error_code, WorkWatchParams};

use super::server::{Conn, RpcFailure};
use super::Core;

/// The longest window: a year, and a day over.
const MAX_WINDOW_MS: u64 = 366 * 86_400_000;

impl Core {
    pub(super) async fn work_agenda_rpc(
        self: &Arc<Self>,
        p: AgendaParams,
        conn: Conn<'_>,
    ) -> Result<AgendaResult, RpcFailure> {
        if p.to_ms <= p.from_ms || p.to_ms - p.from_ms > MAX_WINDOW_MS {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "the window is from_ms up to to_ms, at most a year",
            ));
        }
        let guarded = !conn.surface.reads_private();
        let named = WorkWatchParams {
            roots: p.roots.clone(),
            ..Default::default()
        };
        let scope = self.work_scope(&named, guarded, conn.surface)?;
        self.push.ensure(self).await?;
        let mut out = self
            .push
            .work_agenda(&p, &scope, &self.kernel.config().zone);
        if guarded {
            for l in &mut out.lines {
                l.title = self.tools.scrubber.scrub(&l.title).0;
            }
        }
        Ok(out)
    }
}
