//! `session.mode` (theseus-lyxy): a conversation's mode, read or set, and the
//! read of the modes back from the ledger after serving. The gate's step, the
//! board and the plan's tools are `policy::modes`.
//!
//! Setting `ask` or `plan` only tightens, so it is anyone's; `edits` and
//! `auto` loosen, so they are the owner's, from a private place
//! (`judge_act(Act::Session)`, and the CLI refuses the method inside a job).
//! A shared place has no mode but `ask`. A change is one `session.mode` row
//! with the steps as they stand, written as the board changes, and the same
//! fact is the session's notification.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use theseus_protocol::modes::{SessionMode, SessionModeParams, SessionModeResult};
use theseus_protocol::{method, PlaceClass};

use super::server::{Conn, RpcFailure};
use super::sessions::refused;
use super::{Act, Core};
use crate::fact::modes::ModeChanged;
use crate::policy::modes::ModeState;
use crate::session::SessionRecord;

/// How long the read after serving waits for the ledger's index, at most.
const LOAD_TRIES: u32 = 300;

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
        let modes = &self.tools.earned.modes;
        let state = modes.get(&p.session_id);
        let Some(mode) = p.mode.filter(|m| *m != state.mode) else {
            return Ok(result(&p.session_id, &state, None));
        };
        if mode != SessionMode::Ask && self.runner.class_of(&p.session_id) != PlaceClass::Private {
            return Err(RpcFailure::invalid(anyhow!(
                "a shared place has no mode but ask. The conversation is in {}.",
                state.mode.name()
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
        let next = ModeState {
            mode,
            steps: state.steps.clone(),
        };
        modes.set(&p.session_id, next.clone());
        self.session_rec(&p.session_id).record(&ModeChanged {
            session_id: &p.session_id,
            mode,
            previous: Some(state.mode),
            steps: &next.steps,
            by: &who.label,
        });
        Ok(result(&p.session_id, &next, Some(state.mode)))
    }

    /// Read the modes back from the ledger: each session's newest
    /// `session.mode` row, for a session that is there and not retired.
    /// False while the ledger's index is being built.
    pub fn load_modes(&self) -> Result<bool> {
        let Some((rows, _)) = super::earned::rows_of(self, "session.mode", 0)? else {
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
            let steps = serde_json::from_value(data["steps"].clone()).unwrap_or_default();
            if live {
                self.tools
                    .earned
                    .modes
                    .set(&session, ModeState { mode, steps });
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

fn result(session: &str, state: &ModeState, previous: Option<SessionMode>) -> SessionModeResult {
    SessionModeResult {
        session_id: session.into(),
        mode: state.mode,
        previous,
        steps: state.steps.clone(),
    }
}
