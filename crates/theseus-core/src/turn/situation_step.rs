//! The situation's check in the turn (M6 step 35a; `compiler::situation`):
//! after every compile, a request whose set does not close, or that drops a
//! standing record, fails the turn as `context_unadmitted` before anything
//! is sent, as the core overage does.

use anyhow::Result;

use super::{Failure, Turn, TurnRunner};
use crate::compiler::{situation, Compiled};
use crate::fact::situation::{words, Unadmitted};
use crate::session::SessionRecord;
use crate::stub::{Kind, Stub};

/// The failure class of a turn whose request may not be sent (`unadmitted`, `standing_dropped`).
pub const UNADMITTED_CLASS: &str = "context_unadmitted";

impl TurnRunner {
    /// The situation the compile step tells from what it holds: no
    /// compilation yet is a first compile; a session's first compile in this
    /// daemon's run, when the turn brought nothing (no message, wake, report,
    /// or brief of its own), is a resume; else a continuation.
    pub(super) fn situation_of(
        &self,
        t: &Turn<'_>,
        nodes: &[(u64, Stub)],
        first: bool,
        session: &SessionRecord,
    ) -> situation::Situation {
        // Stub fields alone: no node is decoded for it (step 33).
        let brought = nodes.iter().any(|(_, n)| {
            n.turn_id.as_deref() == Some(t.tc.turn_id) && n.kind == Kind::UserMessage
        });
        let resumed = !brought && !self.run_compiles.seen(t.tc.session_id);
        situation::given(first, session.task.is_some(), resumed)
    }

    /// After the compile, the check (`unadmitted`); a request that passes
    /// marks its session compiled in this run.
    pub(super) fn admitted(
        &self,
        t: &mut Turn<'_>,
        compiled: &Compiled,
        i: u32,
    ) -> Result<Option<Failure>> {
        let sid = t.tc.session_id;
        if let Some(f) = Self::unadmitted(t, compiled, i) {
            return Ok(Some(f));
        }
        if let Some(f) = Self::standing_dropped(t, compiled, i) {
            return Ok(Some(f));
        }
        self.run_compiles.mark(sid);
        Ok(None)
    }

    /// The check over `compiled`'s request, on every compile: a set that does
    /// not close (a call without its result, a result without its call) is
    /// not sent. Its row and the turn's failure when it fails.
    pub(super) fn unadmitted(t: &mut Turn<'_>, compiled: &Compiled, i: u32) -> Option<Failure> {
        let why = situation::closed(&compiled.request.messages).err()?;
        let message = words(&why);
        t.record(&Unadmitted {
            situation: &compiled.situation,
            why: &why,
            compilation_id: &compiled.compilation.id,
            loop_index: i,
        });
        tracing::error!(piece = %why.piece, situation = compiled.situation.kind(),
            "a compiled request was not sent: {message}");
        Some(Failure {
            class: UNADMITTED_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{UNADMITTED_CLASS}: {}", why.piece),
            source: anyhow::anyhow!("{message}"),
        })
    }

    /// The standing check (theseus-a7vi): a compile that lacks a record of
    /// the turn's scope is not sent, and its row says which.
    pub(super) fn standing_dropped(
        t: &mut Turn<'_>,
        compiled: &Compiled,
        i: u32,
    ) -> Option<Failure> {
        let why = crate::standing::missing(compiled, t.standing.block.as_ref())
            .and_then(|()| crate::standing::missing_here(compiled, t.standing.here.as_ref()))
            .err()?;
        let message = words(&why);
        t.record(&Unadmitted {
            situation: &compiled.situation,
            why: &why,
            compilation_id: &compiled.compilation.id,
            loop_index: i,
        });
        tracing::error!(piece = %why.piece, "a compiled request was not sent: {message}");
        Some(Failure {
            class: UNADMITTED_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{UNADMITTED_CLASS}: {}", why.piece),
            source: anyhow::anyhow!("{message}"),
        })
    }
}
