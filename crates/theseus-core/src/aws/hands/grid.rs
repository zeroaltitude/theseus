//! A group as its surfaces show it (AWS design §3.3, "Watching a hundred
//! hands"; step 40 part 2, theseus-mgw.11): its hands' cells by state, its
//! money against its cap, and its one line.
//!
//! - **`hands.list`** ([`list`]): the newest groups, each a `HandsGroupInfo`;
//!   the cockpit's grid and `theseus hands` read it.
//! - **Discord's one line per group** ([`Lines`]): "🖐️ 37/100 done, 2
//!   failed, $1.84 of $5", posted to the group's session's place as a
//!   `hands` post under the group's key, so each change edits the one
//!   message, never a line per hand. The poller's pass posts a group's line
//!   when it has changed: while the group is open, and once more as it
//!   settles.

use std::collections::BTreeMap;
use std::sync::Mutex;

use anyhow::Result;
use serde_json::json;
use theseus_kernel::{ActionState, CancelState};
use theseus_protocol::{HandsGroupInfo, HandsListParams, HandsListResult};
use theseus_store::{kinds, Store as _};

use super::group::{self, GroupRecord, PREFIX};
use crate::Core;

/// The groups `hands.list` returns unless it is asked for fewer.
const DEFAULT_LIMIT: usize = 20;

/// A group, as its surfaces show it.
pub fn info(core: &Core, rec: &GroupRecord) -> Result<HandsGroupInfo> {
    let actions = group::hands(&core.kernel, rec)?;
    let t = group::tally(&core.store, rec, &actions);
    let cells = actions
        .iter()
        .map(|a| {
            match a.state {
                ActionState::Planned | ActionState::Authorized => "waiting",
                ActionState::Dispatched if a.cancel.is_some() => "stopping",
                ActionState::Dispatched => "running",
                ActionState::Succeeded => "succeeded",
                ActionState::Failed => "failed",
                ActionState::OutcomeUnknown => "unknown",
                ActionState::Cancelled if a.dispatched_at_ms.is_none() => "not_launched",
                ActionState::Cancelled => "cancelled",
            }
            .to_string()
        })
        .collect();
    let reserved: u64 = actions
        .iter()
        .filter(|a| {
            a.state == ActionState::Dispatched
                || (a.state == ActionState::Cancelled
                    && a.completions_seen == 0
                    && matches!(
                        a.cancel,
                        Some(CancelState::Unsupported | CancelState::OutcomeUncertain)
                    ))
        })
        .map(|a| a.reserved_micros)
        .sum();
    let n = rec.hands.len() as u64;
    let cap_micros = rec.request.max_usd.map(|u| (u * 1e6).round() as u64);
    let worst_micros = (rec.hand_max_usd * 1e6).ceil() as u64 * n;
    let spent_micros = (t.spent_usd * 1e6).round() as u64;
    let mut g = HandsGroupInfo {
        group: rec.group.clone(),
        session_id: rec.session_id.clone(),
        execution_id: rec.execution_id.clone(),
        account: rec.account.clone(),
        region: rec.region.clone(),
        backend: rec.backend.as_str().into(),
        until: rec.request.until.words(),
        cells,
        succeeded: t.succeeded,
        failed: t.failed,
        running: t.running,
        cancelled: t.cancelled,
        not_launched: t.not_launched,
        spent_micros,
        reserved_micros: reserved,
        cap_micros,
        worst_micros,
        created_at_unix_ms: rec.created_at_ms,
        settled: rec.settled.clone(),
        line: String::new(),
    };
    g.line = line(&g);
    Ok(g)
}

/// Its one line: "🖐️ 37/100 done, 2 failed, $1.84 of $5".
pub fn line(g: &HandsGroupInfo) -> String {
    let n = g.cells.len();
    let done = g.succeeded
        + g.failed
        + g.cancelled
        + g.not_launched
        + g.cells.iter().filter(|c| *c == "unknown").count() as u32;
    let usd = |m: u64| {
        let d = m as f64 / 1e6;
        if d >= 10.0 || (d * 100.0).fract() == 0.0 && d >= 1.0 {
            format!("${d:.0}")
        } else {
            format!("${d:.2}")
        }
    };
    let mut s = format!("🖐️ {done}/{n} done, {} failed", g.failed);
    if g.running > 0 {
        s.push_str(&format!(", {} running", g.running));
    }
    if g.cancelled > 0 {
        s.push_str(&format!(", {} cancelled", g.cancelled));
    }
    match g.cap_micros {
        Some(cap) => s.push_str(&format!(", {} of {}", usd(g.spent_micros), usd(cap))),
        None => s.push_str(&format!(
            ", {} (at worst {})",
            usd(g.spent_micros),
            usd(g.worst_micros)
        )),
    }
    if let Some(how) = &g.settled {
        s.push_str(match how.as_str() {
            "met" => " · done, until met",
            "cancelled" => " · cancelled",
            _ => " · done, until not met",
        });
    }
    s.push_str(&format!(" ({} on {})", short(&g.group), g.backend));
    s
}

fn short(id: &str) -> &str {
    let s = id.strip_prefix("act_").unwrap_or(id);
    &s[s.len().saturating_sub(6)..]
}

/// Every group, newest first.
fn all(core: &Core) -> Result<Vec<GroupRecord>> {
    let mut v: Vec<GroupRecord> = core
        .store
        .inner()
        .latest_with_prefix(kinds::META, PREFIX)?
        .into_iter()
        .filter_map(|r| r.decode::<GroupRecord>().ok())
        .collect();
    v.sort_by_key(|g| std::cmp::Reverse(g.created_at_ms));
    Ok(v)
}

/// `hands.list`.
pub fn list(core: &Core, p: &HandsListParams) -> Result<HandsListResult> {
    let limit = p.limit.map_or(DEFAULT_LIMIT, |l| l as usize);
    let mut groups = Vec::new();
    for rec in all(core)? {
        if groups.len() >= limit {
            break;
        }
        let g = info(core, &rec)?;
        if p.open == Some(true) && g.settled.is_some() && g.running == 0 {
            continue;
        }
        groups.push(g);
    }
    Ok(HandsListResult { groups })
}

impl Core {
    /// `hands.list` (step 40 part 2).
    pub fn hands_list(&self, p: &HandsListParams) -> Result<HandsListResult> {
        list(self, p)
    }
}

/// The line each group last posted, since the start.
#[derive(Default)]
pub struct Lines {
    posted: Mutex<BTreeMap<String, String>>,
}

impl Lines {
    /// Post each open group's line, and a settled group's last, when it has
    /// changed: a `hands` post to its session's place, under its group's key.
    pub fn post(&self, core: &Core, groups: &[GroupRecord]) -> Result<()> {
        for rec in groups {
            let open = rec.settled.is_none();
            let mut posted = self.posted.lock().unwrap();
            if !open && !posted.contains_key(&rec.group) {
                continue;
            }
            let Some(target) = core.outbox.target(&rec.session_id) else {
                continue;
            };
            let g = info(core, rec)?;
            if posted.get(&rec.group) == Some(&g.line) {
                continue;
            }
            core.outbox.post(
                &rec.session_id,
                &rec.execution_id,
                &target,
                json!({"kind": "hands", "group": rec.group, "text": g.line}),
            )?;
            posted.insert(rec.group.clone(), g.line);
        }
        Ok(())
    }
}
