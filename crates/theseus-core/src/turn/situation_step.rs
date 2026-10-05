//! The situation's check in the turn (M6 step 35a; `compiler::situation`):
//! after the compile, a request whose situation does not admit a piece, or
//! whose set does not close, fails the turn as `context_unadmitted` before
//! anything is sent, as the core overage does.

use anyhow::Result;

use super::{Failure, Turn, TurnRunner};
use crate::compiler::{compaction, situation, Compiled};
use crate::fact::situation::{words, Unadmitted};
use crate::session::SessionRecord;
use crate::stub::{Kind, Stub};

/// The failure class of a turn whose request its situation does not admit.
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
    /// marks its session compiled in this run. A compaction wrote its
    /// summary, and may hold an assembled section, since `nodes` was read:
    /// the check reads them too.
    pub(super) fn admitted(
        &self,
        t: &mut Turn<'_>,
        compiled: &Compiled,
        nodes: &[(u64, Stub)],
        task_view: bool,
        i: u32,
    ) -> Result<Option<Failure>> {
        let sid = t.tc.session_id;
        let reread = match compiled.compilation.strategy == compaction::STRATEGY {
            true => Some(self.recall_view(t, t.tc.store.transcript(sid)?).0),
            false => None,
        };
        let checked = reread.as_deref().unwrap_or(nodes);
        let last = self.store.last_position();
        if let Some(f) = Self::unadmitted(t, compiled, checked, last, task_view, i) {
            return Ok(Some(f));
        }
        self.run_compiles.mark(sid);
        Ok(None)
    }

    /// The check over `compiled`'s pieces: its row and the turn's failure
    /// when it fails. `task_view` is whether the request carries 39a's view.
    pub(super) fn unadmitted(
        t: &mut Turn<'_>,
        compiled: &Compiled,
        nodes: &[(u64, Stub)],
        last_position: u64,
        task_view: bool,
        i: u32,
    ) -> Option<Failure> {
        let why = situation::check(compiled, nodes, last_position, task_view).err()?;
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
}
