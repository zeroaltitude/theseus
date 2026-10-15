//! `theseus mode [MODE] [SESSION]` (theseus-lyxy): the conversation's mode, as
//! Shift+Tab sets it in the chat and the terminal UI: `ask` (the config's
//! postures), `edits` (file edits in this project run with a notice), `plan`
//! (read-only until you approve a plan) or `auto` (what only its posture made
//! wait runs with a notice; the floor still asks). With no mode it says the
//! one it is in, and the plan's checklist. `session.mode` is in
//! `client::OPERATORS`: a job's shell is refused before anything is sent.

use anyhow::{anyhow, Result};
use clap::Args;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::modes::{checklist, SessionMode, SessionModeParams, SessionModeResult};

use crate::cmd::output;

#[derive(Args, Debug)]
pub struct ModeArgs {
    /// `ask`, `edits`, `plan` or `auto`; none says the current one.
    #[arg(value_name = "MODE", value_parser = ["ask", "edits", "plan", "auto"])]
    pub mode: Option<String>,
    /// The conversation: its id, or at least its last four characters (default: the newest).
    #[arg(value_name = "SESSION")]
    pub session: Option<String>,
}

pub async fn run(conn: &mut Conn, json: bool, a: ModeArgs) -> Result<()> {
    let session = match a.session {
        Some(s) => theseus_client::resolve::session(conn, &s).await?,
        None => crate::cmd::latest_session(conn).await?,
    };
    let mode = a
        .mode
        .as_deref()
        .map(|m| SessionMode::parse(m).ok_or_else(|| anyhow!("{m} is not a mode")))
        .transpose()?;
    let v = conn
        .request(
            method::SESSION_MODE,
            SessionModeParams {
                session_id: session,
                mode,
            },
        )
        .await?;
    output(json, v, |r: SessionModeResult| {
        println!("{}", r.line());
        for l in checklist(&r.steps) {
            println!("  {l}");
        }
        Ok(())
    })
}
