//! `theseus self backlog | drop | move | retry` (theseus-pw1q.7): Theseus's own
//! ranked backlog, and the owner's word on it. `drop`, `move` and `retry` are
//! the owner's: inside a job the CLI refuses them (`OPERATORS`), and the
//! daemon refuses a non-owner and ledgers the refusal.

use anyhow::Result;
use theseus_client::{render, Conn};
use theseus_protocol::method;
use theseus_protocol::rsi::{
    SelfBacklogParams, SelfBacklogResult, SelfDropParams, SelfDropResult, SelfMoveParams,
    SelfMoveResult, SelfRetryParams, SelfRetryResult,
};

use crate::cmd::output;

pub async fn list(conn: &mut Conn, json: bool, all: bool) -> Result<()> {
    let v = conn
        .request(
            method::SELF_BACKLOG,
            serde_json::to_value(SelfBacklogParams { all })?,
        )
        .await?;
    output(json, v, |r: SelfBacklogResult| {
        for l in backlog_lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

pub async fn drop_item(conn: &mut Conn, json: bool, item: String) -> Result<()> {
    let p = SelfDropParams {
        item,
        from_job: theseus_client::client::job_session(),
        ..Default::default()
    };
    let v = conn
        .request(method::SELF_DROP, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: SelfDropResult| {
        println!(
            "{}",
            match r.changed {
                true => format!(
                    "Dropped {} ({}). Its cause, {}, is never filed again.",
                    r.item, r.title, r.cause
                ),
                false => format!("{} ({}) had left the backlog already.", r.item, r.title),
            }
        );
        Ok(())
    })
}

pub async fn move_item(conn: &mut Conn, json: bool, item: String, rank: u32) -> Result<()> {
    let p = SelfMoveParams {
        item,
        rank,
        from_job: theseus_client::client::job_session(),
        ..Default::default()
    };
    let v = conn
        .request(method::SELF_MOVE, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: SelfMoveResult| {
        println!(
            "Moved {} ({}) from rank {} to rank {}; it keeps that rank until moved again.",
            r.item, r.title, r.was, r.rank
        );
        Ok(())
    })
}

pub async fn retry_item(conn: &mut Conn, json: bool, item: String) -> Result<()> {
    let p = SelfRetryParams {
        item,
        from_job: theseus_client::client::job_session(),
        ..Default::default()
    };
    let v = conn
        .request(method::SELF_RETRY, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: SelfRetryResult| {
        println!(
            "Retrying {} ({}): it is ready again at rank {}, and the pipeline starts it on its next free slot.",
            r.item, r.title, r.rank
        );
        Ok(())
    })
}

/// `theseus self backlog`'s lines: the state, then each item with its
/// evidence, best first.
pub fn backlog_lines(r: &SelfBacklogResult) -> Vec<String> {
    let mut out = vec![format!(
        "Self backlog: {}.",
        render::self_state_line(&r.state)
    )];
    if let Some(t) = r.read_ms {
        out.push(format!(
            "Last read {}; at most {} open.",
            render::fmt_date(t),
            r.max_open
        ));
    } else {
        out.push("No night has read yet.".into());
    }
    if r.items.is_empty() {
        out.push("The backlog is empty.".into());
    }
    for i in &r.items {
        let place = match i.rank {
            0 => format!("{:>3}", "·"),
            n => format!("{n:>3}"),
        };
        let keel = i
            .keel
            .as_deref()
            .map_or(String::new(), |k| format!(" [keel: {k}, waits for you]"));
        let pin = if i.pinned { " (pinned)" } else { "" };
        out.push(format!(
            "{place}. {} {} [{}] score {} seen {}x{keel}{pin}",
            i.id, i.title, i.state, i.score, i.count
        ));
        for e in i.evidence.iter().take(5) {
            out.push(format!("       - {e}"));
        }
        if i.evidence.len() > 5 {
            out.push(format!("       - and {} more", i.evidence.len() - 5));
        }
    }
    if !r.dropped.is_empty() {
        out.push(format!(
            "Dropped, never filed again: {}.",
            r.dropped.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::rsi::{SelfBacklogItem, SelfState};

    fn item(rank: u32, keel: Option<&str>) -> SelfBacklogItem {
        SelfBacklogItem {
            id: format!("tsk_{rank}"),
            rank,
            score: 1234,
            title: "Turns fail with timeout".into(),
            objective: String::new(),
            state: "proposed".into(),
            cause: "turn.failed:timeout".into(),
            keel: keel.map(str::to_string),
            count: 3,
            evidence: (0..7).map(|i| format!("ledger:{i}: turn failed")).collect(),
            pinned: rank == 1,
            session: None,
            filed_ms: 0,
        }
    }

    /// The list shows rank, id, state, score, count, the keel mark and the
    /// pin, five evidence lines and the number left out, and the dropped.
    #[test]
    fn the_list_shows_each_item_with_its_evidence() {
        let r = SelfBacklogResult {
            items: vec![item(1, None), item(2, Some("security"))],
            dropped: vec!["health:disk".into()],
            state: SelfState::default(),
            max_open: 20,
            read_ms: None,
        };
        let text = backlog_lines(&r).join("\n");
        assert!(text.contains("No night has read yet."), "{text}");
        assert!(
            text.contains(
                "  1. tsk_1 Turns fail with timeout [proposed] score 1234 seen 3x (pinned)"
            ),
            "{text}"
        );
        assert!(text.contains("[keel: security, waits for you]"), "{text}");
        assert!(
            text.contains("- ledger:4: turn failed") && !text.contains("ledger:5"),
            "{text}"
        );
        assert!(text.contains("- and 2 more"), "{text}");
        assert!(
            text.contains("Dropped, never filed again: health:disk."),
            "{text}"
        );
    }
}
