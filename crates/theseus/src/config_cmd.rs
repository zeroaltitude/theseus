//! `theseus config history` (issue 22, undo): the last changes the owner
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
use theseus_protocol::{method, ConfigHistoryParams, ConfigHistoryResult};

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
    }
}
