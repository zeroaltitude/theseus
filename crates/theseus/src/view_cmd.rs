//! `theseus view show TURN [--text]` (issue 44): what a turn was shown and why, from the view record of
//! each view it built: the point of view, each slot with its pieces (ids, kinds, forms, tokens, reasons),
//! what was dropped and why, the totals. `--text` adds each piece's text, at the owner's own terminal only.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::view::{lines, ViewShowParams, ViewShowResult};

#[derive(Subcommand, Debug)]
pub enum ViewCmd {
    /// What TURN was shown, slot by slot, and what was left out and why: ids, kinds, forms, tokens and
    /// reasons. `--text` adds each piece's text (your own terminal only).
    Show {
        turn: String,
        #[arg(long)]
        text: bool,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ViewCmd) -> Result<()> {
    let ViewCmd::Show { turn, text } = cmd;
    let p = ViewShowParams {
        turn_id: turn,
        text,
        from_job: theseus_client::client::job_session(),
    };
    let v = conn
        .request(method::VIEW_SHOW, serde_json::to_value(p)?)
        .await?;
    match json {
        true => println!("{}", serde_json::to_string(&v)?),
        false => print!("{}", said(&serde_json::from_value(v)?)),
    }
    Ok(())
}

/// The result in plain lines.
fn said(r: &ViewShowResult) -> String {
    if r.views.is_empty() {
        return format!(
            "turn {} has no view record: it was compiled as a transcript, or before views were kept\n",
            r.turn_id
        );
    }
    let mut out: String = r.views.iter().map(lines).collect();
    if let Some(w) = &r.withheld {
        out.push_str(&format!("({w})\n"));
    }
    out
}
