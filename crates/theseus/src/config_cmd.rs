//! `theseus config history` (issue 22, undo) and `theseus config show` (issue 23): the last changes the owner
//! approved to Theseus's own settings, with who asked and who approved each,
//! as the daemon words them (`config.history`, a read).
//!
//! To change a setting, or to put one change back, the owner asks Theseus in a
//! conversation: it proposes the change, the owner answers its card, and the
//! daemon writes the file itself. Nothing here writes. It is the owner's alone
//! and is refused inside a job, as the daemon's own place rule refuses it.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::{
    method, ConfigHistoryParams, ConfigHistoryResult, ConfigWholeParams, ConfigWholeResult,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// The last changes to Theseus's settings, newest first: when, each setting
    /// before and after, who asked, who approved, and whether it was put back.
    /// Yours alone (refused inside a job).
    History {
        /// How many changes, at most ten.
        #[arg(long)]
        limit: Option<u32>,
    },
    /// The whole text, before and after, of a change waiting for your yes: a prompt file's, or a setting
    /// too long for its card. Every line, with any character you cannot see written out. Yours alone
    /// (refused inside a job).
    Show {
        /// The waiting question's id; left out when only one such change is waiting.
        question: Option<String>,
    },
}

impl Default for ConfigCmd {
    fn default() -> Self {
        ConfigCmd::History { limit: None }
    }
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::History { limit } => {
            let v = conn
                .request(
                    method::CONFIG_HISTORY,
                    ConfigHistoryParams {
                        limit,
                        ..Default::default()
                    },
                )
                .await?;
            output(json, v, |r: ConfigHistoryResult| {
                println!("{}", r.words.trim_end());
                Ok(())
            })
        }
        ConfigCmd::Show { question } => {
            let v = conn
                .request(
                    method::CONFIG_WHOLE,
                    ConfigWholeParams {
                        question,
                        ..Default::default()
                    },
                )
                .await?;
            output(json, v, |r: ConfigWholeResult| {
                print!("{}", r.words);
                Ok(())
            })
        }
    }
}
