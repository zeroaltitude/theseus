//! `theseus budgets` (step 42a, theseus-ext.7): where the money is, from
//! `budget.list`. A table of the open executions, each task under its
//! parent with its carve, then the questions waiting, the totals, and the
//! judge's shadow day budget, and the daemon's day ceiling (theseus-kp20).

use anyhow::Result;
use serde_json::Value;
use theseus_client::{render, Conn};
use theseus_protocol::{method, BudgetListResult, BudgetRow};

use crate::cmd::output;

pub async fn budgets(conn: &mut Conn, json: bool) -> Result<()> {
    let v = conn.request(method::BUDGET_LIST, Value::Null).await?;
    output(json, v, |r: BudgetListResult| {
        print!("{}", table(&r));
        Ok(())
    })
}

/// The table, and the lines after it.
pub fn table(r: &BudgetListResult) -> String {
    let mut out = String::new();
    if r.executions.is_empty() {
        out.push_str("no open executions\n");
    } else {
        out.push_str(&format!(
            "{:<14} {:<9} {:<22} {:>9} {:>9} {:>9} {:>9} {:>9}  resets\n",
            "session", "state", "limit", "spent", "reserved", "held", "available", "lifetime"
        ));
    }
    let mut after = Vec::new();
    for e in &r.executions {
        row(&mut out, &mut after, e, "");
        for t in &e.tasks {
            row(&mut out, &mut after, t, "└ ");
        }
    }
    for line in after {
        out.push_str(&line);
        out.push('\n');
    }
    let t = &r.totals;
    out.push_str(&format!(
        "total: {} execution{} and {} task{} · spent ${:.4} of ${:.2} · reserved ${:.4} · held ${:.4} · available ${:.4} · lifetime ${:.4}{}\n",
        t.executions,
        plural(t.executions),
        t.tasks,
        plural(t.tasks),
        t.spent_usd,
        t.limit_usd,
        t.reserved_usd,
        t.held_unknown_usd,
        t.available_usd,
        t.lifetime_usd,
        match t.questions {
            0 => String::new(),
            n => format!(" · {n} question{} waiting", plural(n)),
        }
    ));
    out.push_str(&format!(
        "a task's spend is its parent's too, and its carve a reservation of its parent's: the totals add the top rows · the config's limit is ${:.2}\n",
        r.config_limit_usd
    ));
    if let Some(j) = &r.judge {
        out.push_str(&match j.enabled {
            true => format!(
                "judge: shadow budget ${:.4} of ${:.2} on {}{}\n",
                j.spent_usd,
                j.limit_usd,
                j.day,
                if j.paused {
                    " · paused at its limit until tomorrow"
                } else {
                    ""
                }
            ),
            false => "judge: off ([judge] enabled = false), so no shadow budget is spent\n".into(),
        });
    }
    if let Some(d) = &r.day_ceiling {
        out.push_str(&day_ceiling_line(d));
    }
    out
}

/// The daemon's day ceiling (theseus-kp20): today's model spend against it,
/// and, once reached, that no model call is made until the day turns.
pub fn day_ceiling_line(d: &theseus_protocol::DayCeilingBudget) -> String {
    let held = match d.held_usd > 0.0 {
        true => format!(" (${:.4} held by calls in flight)", d.held_usd),
        false => String::new(),
    };
    match d.reached {
        true => format!(
            "day ceiling: REACHED: ${:.4} of ${:.2} on {}{held} · no model call until {} \
             local time · raise [kernel] daily_spend_ceiling_usd to go on sooner\n",
            d.spent_usd, d.ceiling_usd, d.day, d.turns_at
        ),
        false => format!(
            "day ceiling: ${:.4} of ${:.2} on {}{held} · the day turns at {} local time\n",
            d.spent_usd, d.ceiling_usd, d.day, d.turns_at
        ),
    }
}

/// One row, and what it adds after the table: its question, its last reset.
fn row(out: &mut String, after: &mut Vec<String>, e: &BudgetRow, lead: &str) {
    let from = match (&e.limit_from[..], &e.limit_by) {
        ("carve", Some(p)) => format!("carve of {}", render::short_id(p)),
        ("place", Some(p)) => p.clone(),
        (from, _) => from.to_string(),
    };
    let limit = format!("${:.2} {from}", e.limit_usd);
    let resets = match (&e.last_reset, &e.last_reset_unread) {
        (_, _) if e.resets == 0 => "0".to_string(),
        (Some(r), _) => format!(
            "{} (last at {} by {})",
            e.resets,
            render::fmt_time(r.at_ms),
            r.by
        ),
        (None, Some(why)) => format!("{} (the last not read: {why})", e.resets),
        (None, None) => e.resets.to_string(),
    };
    out.push_str(&format!(
        "{:<14} {:<9} {:<22} {:>9} {:>9} {:>9} {:>9} {:>9}  {resets}\n",
        format!("{lead}{}", render::short_id(&e.session_id)),
        e.state,
        limit,
        format!("${:.4}", e.spent_usd),
        format!("${:.4}", e.reserved_usd),
        format!("${:.4}", e.held_unknown_usd),
        format!("${:.4}", e.available_usd),
        format!("${:.4}", e.lifetime_usd),
    ));
    if let Some(c) = e.carve_held_usd {
        after.push(format!(
            "{} is a task of {}: its parent holds ${c:.4} of its ${:.2} carve",
            render::short_id(&e.session_id),
            e.limit_by.as_deref().map_or("?".into(), render::short_id),
            e.limit_usd
        ));
    }
    if e.mode.as_deref() == Some("notify") && e.spent_usd >= e.limit_usd {
        after.push(format!(
            "{} is past its limit and goes on: the limit notifies · theseus stop {} ends the work",
            render::short_id(&e.session_id),
            e.session_id
        ));
    }
    if let Some(q) = &e.question {
        after.push(format!(
            "{} waits at its limit: its next call needs ${:.4} · theseus confirm {}",
            render::short_id(&e.session_id),
            q.needs_usd,
            q.correlation_id
        ));
    }
}

fn plural(n: u32) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use theseus_protocol::DayCeilingBudget;

    use super::day_ceiling_line;

    /// The day ceiling's line (theseus-kp20): today against the ceiling, and
    /// once reached, that no model call is made until the day turns.
    #[test]
    fn the_day_ceiling_line_says_today_and_the_stop() {
        let d = DayCeilingBudget {
            day: "2026-10-08".into(),
            ceiling_usd: 200.0,
            spent_usd: 12.5,
            held_usd: 0.0,
            reached: false,
            reached_at_ms: None,
            turns_at_ms: 1,
            turns_at: "2026-10-09 00:00".into(),
        };
        assert_eq!(
            day_ceiling_line(&d),
            "day ceiling: $12.5000 of $200.00 on 2026-10-08 · the day turns at 2026-10-09 00:00 local time\n"
        );
        let stopped = DayCeilingBudget {
            spent_usd: 199.9,
            held_usd: 0.25,
            reached: true,
            reached_at_ms: Some(5),
            ..d
        };
        assert_eq!(
            day_ceiling_line(&stopped),
            "day ceiling: REACHED: $199.9000 of $200.00 on 2026-10-08 ($0.2500 held by calls in flight) · no \
             model call until 2026-10-09 00:00 local time · raise [kernel] daily_spend_ceiling_usd to go on \
             sooner\n"
        );
    }
}
