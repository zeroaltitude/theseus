//! A turn and the owner's instruction files (his own and the context files
//! his settings name): it waits for the daemon's start step that accepts
//! them, and it is not run while the store cannot say which version of one
//! is accepted (`project_context::held::turn_refusal`, the one place that
//! rule is).

use anyhow::anyhow;

use super::{turn_error, To, TurnRequest, TurnRunner};
use crate::context_files::Adopting;
use crate::fact::own_file::TurnWithoutInstructions;

/// What the start step's end that did not finish says.
const UNFINISHED: &str = "This turn was not run: the daemon's start step that accepts your \
                          instruction files did not finish, and Theseus runs no turn without \
                          them. Restart the daemon.";

impl TurnRunner {
    /// Wait for the start step that accepts the instruction files, then
    /// refuse the turn when they cannot be read as last accepted: one row's
    /// line for the session, and the turn's error, whose words say what the
    /// owner does. Nothing is written for the turn and nothing reaches the
    /// model.
    pub(super) async fn instructions_ready(&self, req: &TurnRequest) -> anyhow::Result<()> {
        let why = match self.context_files.adopted().await {
            Adopting::Unfinished => Some(UNFINISHED.to_string()),
            _ => crate::project_context::held::turn_refusal(
                &self.cfg,
                &self.store,
                &self.context_files,
            ),
        };
        let Some(why) = why else {
            return Ok(());
        };
        let sid = &req.session.session_id;
        if req.input.is_some() {
            self.session_rec(sid, To::Sink(&req.sink))
                .record(&TurnWithoutInstructions {
                    author: &req.author,
                    why: &why,
                });
        }
        tracing::warn!(session_id = %sid, "turn refused: the instruction files cannot be read as last accepted");
        Err(turn_error(
            "instructions_unread",
            sid,
            "",
            0,
            anyhow!("{why}"),
        ))
    }
}
