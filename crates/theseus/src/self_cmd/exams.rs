//! `theseus self exams` (theseus-pw1q.11): the self-exams' counts by family
//! and half, today's candidates with their reviews, and what was dropped for
//! want of a sealed store. A sealed item is a number or an id; there is no
//! text of one to print.

use anyhow::Result;
use theseus_client::{render, Conn};
use theseus_protocol::method;
use theseus_protocol::rsi::{SelfExamsParams, SelfExamsResult};

use crate::cmd::output;

pub async fn run(conn: &mut Conn, json: bool) -> Result<()> {
    let v = conn
        .request(
            method::SELF_EXAMS,
            serde_json::to_value(SelfExamsParams::default())?,
        )
        .await?;
    output(json, v, |r: SelfExamsResult| {
        for l in lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

/// The report's lines.
pub fn lines(r: &SelfExamsResult) -> Vec<String> {
    let mut out = vec![format!(
        "Self-improvement: {}.",
        render::self_state_line(&r.state)
    )];
    out.push("Counted exam items, by family and half:".into());
    for f in &r.families {
        out.push(format!(
            "  {:<18} open {:>3}   sealed {:>3}",
            f.family, f.open, f.sealed
        ));
    }
    if r.today.is_empty() {
        out.push("Today: no candidates.".into());
    } else {
        out.push("Today's candidates:".into());
    }
    for t in &r.today {
        let half = if t.sealed { "sealed" } else { "open" };
        let mut l = format!("  {} {:<18} {half:<6} {}", t.item, t.family, t.verdict);
        if let Some(who) = &t.reviewer {
            l.push_str(&format!(" (reviewed by {who})"));
        }
        if let Some(why) = &t.reason {
            l.push_str(&format!(": {why}"));
        }
        out.push(l);
        if let Some(task) = &t.task {
            out.push(format!("      {task}"));
        }
    }
    if r.sealed_dropped > 0 {
        out.push(format!(
            "{} sealed items were held and dropped today: no sealed store is wired yet.",
            r.sealed_dropped
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::rsi::{SelfExamFamily, SelfExamToday, SelfState};

    /// A sealed candidate prints its id and verdict, no text; an open one its task.
    #[test]
    fn a_sealed_item_prints_no_text() {
        let r = SelfExamsResult {
            families: vec![SelfExamFamily {
                family: "self_correction".into(),
                open: 2,
                sealed: 1,
            }],
            today: vec![
                SelfExamToday {
                    item: "aaaa".into(),
                    family: "self_correction".into(),
                    sealed: true,
                    verdict: "counted".into(),
                    reviewer: Some("opus".into()),
                    reason: None,
                    task: None,
                },
                SelfExamToday {
                    item: "bbbb".into(),
                    family: "self_correction".into(),
                    sealed: false,
                    verdict: "counted".into(),
                    reviewer: Some("opus".into()),
                    reason: None,
                    task: Some("Where should it run?".into()),
                },
            ],
            sealed_dropped: 3,
            state: SelfState::default(),
        };
        let text = lines(&r).join("\n");
        assert!(text.contains("open   2   sealed   1"), "{text}");
        assert!(text.contains("aaaa") && text.contains("sealed"), "{text}");
        assert!(text.contains("Where should it run?"), "{text}");
        assert!(
            text.contains("3 sealed items were held and dropped"),
            "{text}"
        );
    }
}
