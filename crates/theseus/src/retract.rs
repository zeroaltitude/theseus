//! `theseus retract <node-or-handle> "<why>"` (theseus-2ced): mark one node
//! wrong, and print where it went as a plain table. The table is
//! [`table_lines`], a function of the answer alone, so the full exposure view
//! (the next row's) replaces it without touching the command.

use anyhow::Result;
use theseus_client::render::{fmt_date, Line, Tag};
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::retract::{FactRetractParams, FactRetractResult};

use crate::cmd::output;

/// `theseus retract`: the owner's act, refused inside a job (`OPERATORS`).
pub async fn run(conn: &mut Conn, json: bool, node: String, why: Vec<String>) -> Result<()> {
    let p = FactRetractParams {
        node,
        why: why.join(" "),
        session_id: None,
    };
    let v = conn
        .request(method::FACT_RETRACT, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: FactRetractResult| {
        crate::print::lines(&mut std::io::stdout().lock(), &table_lines(&r))?;
        Ok(())
    })
}

/// The answer as lines: what was retracted, and a row for each exposure:
/// kind, where, when, used or seen, and what happens.
pub fn table_lines(r: &FactRetractResult) -> Vec<Line> {
    let mut out = Vec::new();
    let what = match &r.gist {
        Some(g) => format!("\u{201c}{g}\u{201d} ({})", r.node_id),
        None => r.node_id.clone(),
    };
    let head = if r.already {
        format!(
            "{what} was already retracted by {} on {}: {}. Nothing was written now.",
            r.by,
            fmt_date(r.at_ms),
            r.why
        )
    } else {
        format!("retracted {what}: {}", r.why)
    };
    out.push(Line::new(Tag::Plain, head));
    if r.exposures.is_empty() {
        out.push(Line::new(Tag::Dim, "no session, task or note used it"));
    } else {
        let w = r
            .exposures
            .iter()
            .map(|e| e.place.chars().count())
            .max()
            .unwrap_or(0)
            .max("where".len());
        out.push(Line::new(
            Tag::Dim,
            format!(
                "{:<9}  {:<w$}  {:<16}  {:<6}  what happens",
                "kind", "where", "when", "used"
            ),
        ));
        for e in &r.exposures {
            let used = match e.used {
                Some(true) => "used",
                Some(false) => "seen",
                None => "-",
            };
            let when = e.at_ms.map_or_else(|| "-".to_string(), fmt_date);
            out.push(Line::new(
                Tag::Plain,
                format!(
                    "{:<9}  {:<w$}  {:<16}  {:<6}  {}",
                    e.kind, e.place, when, used, e.what
                ),
            ));
        }
    }
    if r.partial {
        out.push(Line::new(
            Tag::Dim,
            "the walk stopped at its caps (16 generations, 256 copies): there are more",
        ));
    }
    if r.card {
        out.push(Line::new(
            Tag::Dim,
            "one card with the tasks to re-run went to your DM",
        ));
    }
    out
}
