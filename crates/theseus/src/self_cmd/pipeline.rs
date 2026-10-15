//! `theseus self pipeline [--week]` (theseus-pw1q.16): the self pipeline's
//! queues (ready, building, reviewing, joining, and the owner's), what holds
//! its next start and why, today's spend and rows, and with `--week` the
//! week's measures. The owner's words on it are `theseus self drop`, `self
//! move` and `self halt`, which act at once.

use anyhow::Result;
use theseus_client::{render, Conn};
use theseus_protocol::method;
use theseus_protocol::rsi::{SelfPipelineItem, SelfPipelineParams, SelfPipelineResult};

use crate::cmd::output;

pub async fn run(conn: &mut Conn, json: bool, week: bool) -> Result<()> {
    let v = conn
        .request(
            method::SELF_PIPELINE,
            serde_json::to_value(SelfPipelineParams { week })?,
        )
        .await?;
    output(json, v, |r: SelfPipelineResult| {
        for l in lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

fn item_line(i: &SelfPipelineItem) -> String {
    let mut l = format!("  {}. {}  {}", i.place, i.item, i.title);
    if let Some(b) = &i.branch {
        l.push_str(&format!("  [{b}]"));
    }
    if i.rejections > 0 {
        l.push_str(&format!("  rejected {}x", i.rejections));
    }
    l
}

fn usd(v: f64) -> String {
    format!("${v:.2}")
}

/// Its lines: the state, the room, each queue, today, and the week.
pub fn lines(r: &SelfPipelineResult) -> Vec<String> {
    let mut out = vec![format!(
        "Self-improvement: {}.",
        render::self_state_line(&r.state)
    )];
    if r.building_index {
        out.push(
            "The ledger's index is still being built after a start: ask again in a minute.".into(),
        );
        return out;
    }
    let room = &r.room;
    let mut host = format!(
        "Builder: {}. Slots: {} building, {} trees open, of {}.",
        r.builder, room.building, room.trees, room.slots
    );
    if let Some(gb) = room.free_gb {
        host.push_str(&format!(
            " Disk: {gb:.1} GB free (keeps {}).",
            room.min_free_gb
        ));
    }
    let psi = |v: Option<f64>| v.map_or_else(|| "-".to_string(), |v| format!("{v:.1}%"));
    host.push_str(&format!(
        " Pressure: memory {}, CPU {}.",
        psi(room.memory_psi),
        psi(room.cpu_psi)
    ));
    out.push(host);
    match (&room.hold, &room.why) {
        (Some(h), Some(w)) => out.push(format!("Next start waits ({h}): {w}.")),
        (None, Some(w)) => out.push(format!("{w}.")),
        _ => out.push("Next start: free to go.".into()),
    }
    for (name, q) in [
        ("Ready", &r.ready),
        ("Building", &r.building),
        ("Reviewing", &r.reviewing),
        ("Joining (one at a time, under the join lock)", &r.joining),
        ("Held for the owner", &r.held),
    ] {
        if q.is_empty() {
            out.push(format!("{name}: none."));
            continue;
        }
        out.push(format!("{name}:"));
        for i in q {
            out.push(item_line(i));
            if let Some(w) = &i.why {
                out.push(format!("     {}", w.chars().take(200).collect::<String>()));
            }
        }
    }
    let spend = r.budget.as_ref().map_or_else(
        || "no self budget (mode off)".to_string(),
        |b| {
            format!(
                "{} of {} spent today",
                usd(b.day_spent_usd),
                usd(b.day_line_usd)
            )
        },
    );
    out.push(format!("Today: {spend}; {} pipeline rows.", r.rows_today));
    if let Some(w) = &r.week {
        out.push(
            "The week (day: started, built, accepted/rejected, joined, reverted, cost):".into(),
        );
        for d in &w.days {
            out.push(format!(
                "  {}: {} started, {} built, {}/{} reviewed, {} joined, {} reverted, {}",
                d.day,
                d.started,
                d.built,
                d.accepted,
                d.rejected,
                d.joined,
                d.reverted,
                usd(d.cost_usd)
            ));
        }
        let rate = w
            .accept_rate
            .map_or_else(|| "none yet".to_string(), |a| format!("{:.0}%", a * 100.0));
        let row = |c: Option<f64>| c.map_or_else(|| "no row yet".to_string(), usd);
        out.push(format!(
            "Rows a day: {:.2}. Accept rate: {rate}. Regressions: {}. Cost a row: {} this week, {} today.",
            w.rows_a_day,
            w.regressions,
            row(w.cost_a_row_usd),
            row(w.today_cost_a_row_usd)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::rsi::{SelfPipelineDay, SelfPipelineRoom, SelfPipelineWeek, SelfState};

    #[test]
    fn the_pipeline_says_its_queues_its_hold_and_its_week() {
        let mut r = SelfPipelineResult {
            state: SelfState {
                gate: "allowed".into(),
                ..Default::default()
            },
            builder: "local".into(),
            room: SelfPipelineRoom {
                slots: 2,
                building: 2,
                trees: 2,
                free_gb: Some(120.0),
                min_free_gb: 40,
                hold: Some("slots".into()),
                why: Some("every slot is taken".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        r.ready.push(SelfPipelineItem {
            item: "tsk_mast".into(),
            title: "Mend the mast".into(),
            queue: "ready".into(),
            place: 1,
            rejections: 1,
            branch: Some("self/20261014-tsk-mast".into()),
            why: Some("rejected by fable: unproved".into()),
            ..Default::default()
        });
        let l = lines(&r);
        assert_eq!(
            l[1],
            "Builder: local. Slots: 2 building, 2 trees open, of 2. Disk: 120.0 GB free (keeps 40). \
             Pressure: memory -, CPU -."
        );
        assert_eq!(l[2], "Next start waits (slots): every slot is taken.");
        assert_eq!(l[3], "Ready:");
        assert_eq!(
            l[4],
            "  1. tsk_mast  Mend the mast  [self/20261014-tsk-mast]  rejected 1x"
        );
        assert_eq!(l[5], "     rejected by fable: unproved");
        assert_eq!(l[6], "Building: none.");
        assert_eq!(l[10], "Today: no self budget (mode off); 0 pipeline rows.");
        r.week = Some(SelfPipelineWeek {
            days: vec![SelfPipelineDay {
                day: "2026-10-14".into(),
                started: 3,
                built: 2,
                accepted: 1,
                rejected: 1,
                joined: 1,
                reverted: 0,
                cost_usd: 4.5,
            }],
            rows_a_day: 0.14,
            accept_rate: Some(0.5),
            regressions: 0,
            cost_a_row_usd: Some(4.5),
            today_cost_a_row_usd: None,
        });
        let l = lines(&r);
        assert_eq!(
            l[12],
            "  2026-10-14: 3 started, 2 built, 1/1 reviewed, 1 joined, 0 reverted, $4.50"
        );
        assert_eq!(
            l[13],
            "Rows a day: 0.14. Accept rate: 50%. Regressions: 0. Cost a row: $4.50 this week, no row yet today."
        );
    }
}
