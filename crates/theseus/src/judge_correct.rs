//! `theseus judge correct` and `theseus judge corrections` (theseus-q31l):
//! the owner's correction of a turn's routing, judged as a label is (the
//! owner, from a private place; refused inside a job,
//! `client::refuse_in_a_job`), and the live correction layer, a read.

use anyhow::Result;
use clap::Args;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::route::{RouteCorrectParams, RouteCorrectResult, RouteCorrectionsResult};

use crate::cmd::output;

#[derive(Args, Debug)]
pub struct CorrectArgs {
    /// Where it should have run: a profile (`fable`), a mode of the route pack (`deep_coding`),
    /// `stronger`, or `cheaper`.
    to: String,
    /// The session whose turn it was.
    #[arg(long, short, value_name = "SESSION")]
    session: String,
    /// The turn (`turn_…`); none: the session's last turn routing decided.
    #[arg(long, value_name = "TURN")]
    turn: Option<String>,
}

/// `theseus judge correct`: the owner's label on the turn's route judgment,
/// and the session's next turn where the owner says.
pub async fn correct(conn: &mut Conn, json: bool, a: CorrectArgs) -> Result<()> {
    let p = RouteCorrectParams {
        session_id: a.session,
        turn_id: a.turn,
        to: a.to,
        via: Some("cli".into()),
        provenance: None,
        discord: None,
    };
    let v = conn.request(method::ROUTE_CORRECT, p).await?;
    output(json, v, |r: RouteCorrectResult| {
        println!("{}", r.line);
        Ok(())
    })
}

/// `theseus judge corrections`: the live layer, newest first.
pub async fn corrections(conn: &mut Conn, json: bool) -> Result<()> {
    let v = conn
        .request(method::ROUTE_CORRECTIONS, serde_json::Value::Null)
        .await?;
    output(json, v, |r: RouteCorrectionsResult| {
        for line in lines(&r) {
            println!("{line}");
        }
        Ok(())
    })
}

/// The layer's lines: its head, then one per entry.
pub fn lines(r: &RouteCorrectionsResult) -> Vec<String> {
    let state = if r.enabled { "on" } else { "off" };
    let mut out = vec![format!(
        "corrections ({state}): {} of {} under {}, at {:.2} of words in common; {} retired",
        r.entries.len(),
        r.max_entries,
        r.pack,
        r.similarity,
        r.retired
    )];
    for e in &r.entries {
        let words: Vec<&str> = e.words.iter().take(8).map(String::as_str).collect();
        let more = e.words.len().saturating_sub(words.len());
        let more = if more > 0 {
            format!(" +{more}")
        } else {
            String::new()
        };
        out.push(format!(
            "  {} → {}  {} of {}  [{}{more}]{}",
            e.id,
            e.to,
            e.turn_id,
            e.session_id,
            words.join(" "),
            e.label
                .as_deref()
                .map(|l| format!("  {l}"))
                .unwrap_or_default()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::route::RouteCorrectionInfo;

    #[test]
    fn the_layer_reads_a_line_an_entry() {
        let r = RouteCorrectionsResult {
            pack: "route.v2".into(),
            entries: vec![RouteCorrectionInfo {
                id: "rcx_1".into(),
                label: Some("lbl_1".into()),
                pack: "route.v2".into(),
                session_id: "ses_a".into(),
                turn_id: "turn_1".into(),
                to: "fable".into(),
                words: [
                    "format",
                    "lighthouse",
                    "log",
                    "parser",
                    "rust",
                    "write",
                    "x",
                    "y",
                    "z",
                ]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
                at_ms: 1,
            }],
            max_entries: 64,
            similarity: 0.5,
            retired: 2,
            enabled: true,
        };
        assert_eq!(
            lines(&r),
            [
                "corrections (on): 1 of 64 under route.v2, at 0.50 of words in common; 2 retired",
                "  rcx_1 → fable  turn_1 of ses_a  [format lighthouse log parser rust write x y +1]  lbl_1",
            ]
        );
    }
}
