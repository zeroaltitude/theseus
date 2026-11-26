//! `theseus told undo POST` (issue 57): the Undo of a note that told the owner
//! of an act done unasked, from a terminal. It calls `told.undo`, the same
//! door a Discord press calls, which judges it (the owner, from a private
//! place) and runs the note's undo once; the CLI refuses it inside a job, as
//! every operator verb (`client::OPERATORS`).

use anyhow::Result;
use clap::Subcommand;
use serde::Deserialize;
use serde_json::json;
use theseus_client::Conn;
use theseus_protocol::method;

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum ToldCmd {
    /// Undo the act a note told you of: its post id, or the end of it, as the
    /// note's "Undo:" line names it.
    Undo { post: String },
}

/// `told.undo`'s answer: the note's words now, and whether the undo had
/// already been done (nothing ran this time).
#[derive(Deserialize)]
struct Undone {
    text: String,
    #[serde(default)]
    found: bool,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ToldCmd) -> Result<()> {
    match cmd {
        ToldCmd::Undo { post } => {
            let v = conn
                .request(method::TOLD_UNDO, json!({"post": post}))
                .await?;
            output(json, v, |r: Undone| {
                let head = match r.found {
                    true => "Nothing left to undo: it was already done.",
                    false => "Undone.",
                };
                println!("{head} The note now says:\n{}", r.text);
                Ok(())
            })
        }
    }
}
