//! `session.mode` (theseus-lyxy): a conversation's mode, read or set, and the
//! read of the modes back from the ledger after serving. The gate's step, the
//! board and the plan's tools are `policy::modes`.
//!
//! Setting `ask` or `plan` only tightens, so it is anyone's; `edits` and
//! `auto` loosen, so they are the owner's, from a private place
//! (`judge_act(Act::Session)`, and the CLI refuses the method inside a job).
//! A shared place has no mode but `ask`. A change is one `session.mode` row
//! with the mode alone, written as the board changes, and the same fact is
//! the session's notification. The result and the notification carry the
//! conversation's plan items, read from the task graph (`task_graph::plan`):
//! the plan is not a field of the mode. Entering `plan` asks for a recompile
//! on the session's next turn, so the request that follows loads the `plan`
//! tool family (`tools_defer`) whatever the conversation's last compilation
//! loaded.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use theseus_protocol::modes::{PlanItem, SessionMode, SessionModeParams, SessionModeResult};
use theseus_protocol::{method, PlaceClass};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use super::server::{Conn, RpcFailure};
use super::sessions::refused;
use super::{Act, Core};
use crate::compiler::Recompile;
use crate::fact::modes::ModeChanged;
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;

/// How long the read after serving waits for the ledger's index, at most.
const LOAD_TRIES: u32 = 300;

/// The most rows of one kind a read takes (a bound, said when it cuts).
const ROWS_MAX: usize = 400_000;

/// The rows of `kind` from `since_ms` on, oldest first, and whether the cap
/// cut them. `None` while the ledger's index is being built.
fn rows_of(core: &Core, kind: &str, since_ms: u64) -> Result<Option<(Vec<LedgerRow>, bool)>> {
    let mut out: Vec<LedgerRow> = Vec::new();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind(kind)],
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: None,
            limit: 512,
        };
        let Some(got) = core.store.ledger_page(&page)? else {
            return Ok(None);
        };
        out.extend(
            got.records
                .iter()
                .filter_map(|r| r.decode::<LedgerRow>().ok())
                .filter(|r| r.at_unix_ms >= since_ms),
        );
        if out.len() >= ROWS_MAX {
            out.sort_by_key(|r| r.at_unix_ms);
            return Ok(Some((out, true)));
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => break,
        }
    }
    out.sort_by_key(|r| r.at_unix_ms);
    Ok(Some((out, false)))
}

impl Core {
    /// `session.mode`: with a mode, set it; without, read it.
    pub(super) fn session_mode(
        &self,
        p: SessionModeParams,
        conn: Conn<'_>,
    ) -> Result<SessionModeResult, RpcFailure> {
        if self
            .store
            .get_session::<SessionRecord>(&p.session_id)?
            .is_none()
        {
            return Err(RpcFailure::invalid(anyhow!("no session {}", p.session_id)));
        }
        let modes = &self.tools.modes;
        let was = modes.mode_of(&p.session_id);
        let plan = crate::task_graph::plan::view(&self.store, &p.session_id);
        let Some(mode) = p.mode.filter(|m| *m != was) else {
            return Ok(result(&p.session_id, was, &plan, None));
        };
        if mode != SessionMode::Ask && self.runner.class_of(&p.session_id) != PlaceClass::Private {
            return Err(RpcFailure::invalid(anyhow!(
                "a shared place has no mode but ask. The conversation is in {}.",
                was.name()
            )));
        }
        let who = conn.answerer(None, None);
        if matches!(mode, SessionMode::Edits | SessionMode::Auto) {
            let what = format!("Moving session {} to {} mode", p.session_id, mode.name());
            self.judge_act(
                &who,
                Act::Session {
                    method: method::SESSION_MODE,
                    what: &what,
                },
            )
            .map_err(refused)?;
        }
        modes.set(&p.session_id, mode);
        self.session_rec(&p.session_id).record(&ModeChanged {
            session_id: &p.session_id,
            mode,
            previous: Some(was),
            plan: &plan,
            by: &who.label,
        });
        if mode == SessionMode::Plan {
            // The mode is set whatever this says: a failure only costs the next
            // turn a search for the plan tools.
            if let Err(e) =
                self.request_recompile(&p.session_id, Recompile::Transcript, "plan mode")
            {
                tracing::warn!(error = %format!("{e:#}"), "a recompile for plan mode was not requested");
            }
        }
        Ok(result(&p.session_id, mode, &plan, Some(was)))
    }

    /// Read the modes back from the ledger: each session's newest
    /// `session.mode` row, for a session that is there and not retired.
    /// False while the ledger's index is being built.
    pub fn load_modes(&self) -> Result<bool> {
        let Some((rows, _)) = rows_of(self, "session.mode", 0)? else {
            return Ok(false);
        };
        let mut newest = std::collections::BTreeMap::new();
        for r in rows {
            if let Some(s) = r.session_id.clone() {
                newest.insert(s, r.data);
            }
        }
        for (session, data) in newest {
            let live = self
                .store
                .get_session::<SessionRecord>(&session)?
                .is_some_and(|s| s.retired.is_none());
            let mode = data["mode"]
                .as_str()
                .and_then(SessionMode::parse)
                .unwrap_or_default();
            if live {
                self.tools.modes.set(&session, mode);
            }
        }
        Ok(true)
    }

    /// `load_modes` after serving, off the start path, waiting on tokio's
    /// timer while the ledger's index is built.
    pub fn warm_modes(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            for _ in 0..LOAD_TRIES {
                let Some(c) = core.upgrade() else { return };
                match tokio::task::spawn_blocking(move || c.load_modes()).await {
                    Ok(Ok(true)) | Err(_) => return,
                    Ok(Ok(false)) => {}
                    Ok(Err(e)) => {
                        tracing::warn!(error = %format!("{e:#}"), "the conversations' modes cannot be read");
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}

fn result(
    session: &str,
    mode: SessionMode,
    plan: &[PlanItem],
    previous: Option<SessionMode>,
) -> SessionModeResult {
    SessionModeResult {
        session_id: session.into(),
        mode,
        previous,
        plan: plan.to_vec(),
    }
}
