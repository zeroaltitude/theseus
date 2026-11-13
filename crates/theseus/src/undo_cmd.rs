//! `theseus undo list` and `theseus undo allow ID` (theseus-p5kln): the calls
//! `undo.v1` held for losing something nothing holds a copy of, and the
//! owner's word that the original was right.
//!
//! A hold makes no card on purpose — that is what keeps a reshapeable hold
//! from costing attention — so these are the two verbs that reach one. The
//! allowance lets the ORIGINAL call run, as it was first proposed, and
//! nothing else: a re-planned call is still held. `allow` is the owner's own
//! act and is refused inside a job (`client::OPERATORS`), as a repository's
//! trust is: the daemon judges it with `places::owner_in_person`, and the CLI
//! says so first rather than letting the socket refuse it.
//!
//! The chat route is the other half, and the one the owner asked for: saying
//! "run the original" in the conversation does the same thing
//! (`rpc/allow_in_words.rs`).

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::work::undo::{
    HeldCall, UndoAllowParams, UndoAllowResult, UndoListParams, UndoListResult,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum UndoCmd {
    /// The calls held for losing something that nothing holds a copy of;
    /// `--all` adds the ones an override already let run.
    List {
        #[arg(long)]
        all: bool,
        /// One conversation's holds.
        #[arg(long = "session")]
        session: Option<String>,
    },
    /// Let a held call run as it was first proposed: yours alone, from your
    /// own shell, the web UI, or a DM with you. The id is whole or its last
    /// four characters or more.
    Allow { id: String },
}

impl Default for UndoCmd {
    fn default() -> Self {
        UndoCmd::List {
            all: false,
            session: None,
        }
    }
}

pub async fn run(conn: &mut Conn, json: bool, cmd: UndoCmd) -> Result<()> {
    match cmd {
        UndoCmd::List { all, session } => {
            let v = conn
                .request(
                    method::UNDO_LIST,
                    UndoListParams {
                        session_id: session,
                        all,
                    },
                )
                .await?;
            output(json, v, |r: UndoListResult| {
                println!("{}", lines(&r.held));
                Ok(())
            })
        }
        UndoCmd::Allow { id } => {
            let v = conn
                .request(
                    method::UNDO_ALLOW,
                    UndoAllowParams {
                        hold: id,
                        ..Default::default()
                    },
                )
                .await?;
            output(json, v, |r: UndoAllowResult| {
                println!("{}", r.words);
                Ok(())
            })
        }
    }
}

/// The holds as the owner reads them: one line each, and the verb under them.
fn lines(held: &[HeldCall]) -> String {
    if held.is_empty() {
        return "No call is held.".to_string();
    }
    let mut out: Vec<String> = held.iter().map(HeldCall::line).collect();
    out.push(String::new());
    out.push("theseus undo allow <id> lets one run as it was first proposed.".to_string());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn held(id: &str) -> HeldCall {
        HeldCall {
            id: id.into(),
            session_id: "ses_1".into(),
            turn_id: None,
            tool: "fs.write".into(),
            input: json!({"path": "/w/notes.txt"}),
            summary: "write notes.txt".into(),
            judgment: "jdg_1".into(),
            shape: Some("move_aside".into()),
            held_ms: 1,
            allowed: None,
            ran_ms: None,
        }
    }

    #[test]
    fn an_empty_list_says_so_and_a_full_one_names_the_verb() {
        assert_eq!(lines(&[]), "No call is held.");
        let out = lines(&[held("hld_1a2b3c4d")]);
        assert!(out.contains("hld_1a2b3c4d"), "{out}");
        assert!(out.contains("write notes.txt"), "{out}");
        assert!(out.contains("theseus undo allow <id>"), "{out}");
    }
}
