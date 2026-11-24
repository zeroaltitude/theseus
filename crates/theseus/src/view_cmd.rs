//! `theseus view show TURN [--text]` (issue 44): what a turn was shown and why, from the view record of
//! each view it built: the point of view, each slot with its pieces (ids, kinds, forms, tokens, reasons),
//! what was dropped and why, the totals. `--text` adds each piece's text, `--tally` each view's counts. The
//! owner's own inspection: the daemon answers him in person alone.
//!
//! `theseus view preview PLACE [--message TEXT] [--text]`: the view a hypothetical next turn of that place
//! would get, opened by the owner's message, assembled now by the turn's own assembler, sent nowhere and
//! kept nowhere, with its counts, whether the turn would fall back to the transcript and why, and the
//! window and folds it lays. The owner's in person alone, as `view show` is.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::view::{
    lines, preview_lines, tally_lines, ViewPreviewParams, ViewShowParams, ViewShowResult,
};

#[derive(Subcommand, Debug)]
pub enum ViewCmd {
    /// What TURN was shown, slot by slot, and what was left out and why: ids, kinds, forms, tokens and
    /// reasons. `--text` adds each piece's text, `--tally` the counts by slot and reason.
    Show {
        turn: String,
        #[arg(long)]
        text: bool,
        #[arg(long)]
        tally: bool,
    },
    /// The view PLACE's next turn would get, assembled now and neither sent nor kept: each slot's pieces,
    /// what was left out and why, the totals against the budget, and whether the turn would fall back to
    /// the transcript. PLACE as the ledger names it (`theseus ledger exchanges` lists them).
    Preview {
        place: String,
        /// The next turn's message (else an empty one).
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        text: bool,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ViewCmd) -> Result<()> {
    let (turn, text, tally) = match cmd {
        ViewCmd::Show { turn, text, tally } => (turn, text, tally),
        ViewCmd::Preview {
            place,
            message,
            text,
        } => return preview(conn, json, place, message, text).await,
    };
    let p = ViewShowParams {
        turn_id: turn,
        text,
        tally,
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

/// `view preview`.
async fn preview(
    conn: &mut Conn,
    json: bool,
    place: String,
    message: Option<String>,
    text: bool,
) -> Result<()> {
    let p = ViewPreviewParams {
        place,
        message,
        text,
        from_job: theseus_client::client::job_session(),
    };
    let v = conn
        .request(method::VIEW_PREVIEW, serde_json::to_value(p)?)
        .await?;
    match json {
        true => println!("{}", serde_json::to_string(&v)?),
        false => print!("{}", preview_lines(&serde_json::from_value(v)?)),
    }
    Ok(())
}

/// The result in plain lines.
fn said(r: &ViewShowResult) -> String {
    if r.views.is_empty() && r.unread.is_empty() {
        return format!(
            "turn {} has no view record: it was compiled as a transcript, or before views were kept\n",
            r.turn_id
        );
    }
    let mut out: String = r.views.iter().map(lines).collect();
    out.extend((r.unread.iter()).map(|c| {
        format!("view {c}: this build cannot read its record (a later build may have written it)\n")
    }));
    out.extend(r.tallies.iter().map(tally_lines));
    if let Some(w) = &r.withheld {
        out.push_str(&format!("({w})\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view this build cannot read is said in a plain line, never as a turn with no view record.
    #[test]
    fn a_view_this_build_cannot_read_is_said_plainly() {
        let r = ViewShowResult {
            turn_id: "turn_1".into(),
            unread: vec!["cmp_1".into()],
            ..ViewShowResult::default()
        };
        let out = said(&r);
        assert_eq!(
            out,
            "view cmp_1: this build cannot read its record (a later build may have written it)\n"
        );
    }
}
