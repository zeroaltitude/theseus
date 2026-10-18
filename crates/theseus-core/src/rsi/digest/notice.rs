//! The morning notice (theseus-pw1q.21.8; RSI loop 1 §6.2): one DM post at the
//! owner's morning, after the night's work has *landed*, in the digest's
//! channel and through its path (the outbox's `self_digest` post: no new
//! Discord code). Sections Fixed, Made, Retired and Waits for you, a header
//! with the night's window, the spend of the day line and the halts, and a
//! footer with what the backlog filed, updated and dropped.
//!
//! **The rules**, each held by a test: every line carries its numbers and its
//! undo; nothing says "in progress" (only what landed is listed: a join, a
//! deploy, a retire); the totals are the ledger's (counted from rows, never
//! kept); no names and no item text from a sealed set (a sealed check
//! contributes only the score line `sealed 38/40 → 39/40`, and the post goes
//! through the scrubber); a bench-born tool is labelled so; with nothing to
//! say there is no post. It is posted only with `[self] mode = "act"`, never
//! while the switch is halted, and once a local day from `[initiative]
//! morning_hour` on.

use std::collections::BTreeSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfBacklogParams, SelfMode};
use theseus_store::{kinds, NewRecord};

use crate::ledger::LedgerRow;
use crate::rpc::Core;
use crate::rsi::mint::rows;
use crate::rsi::{gate, Gate};

/// The META record of the notice's last post.
pub const NOTICE_KEY: &str = "self.notice.posted";
const DAY_MS: u64 = 86_400_000;
/// The kinds the notice reads.
const KINDS: &[&str] = &[
    "self.joined",
    "self.review.verdict",
    "self.sealed.checked",
    "self.installed",
    "self.mint.deployed",
    "self.mint.retired",
    "self.halted",
    "self.backlog.filed",
    "self.backlog.dropped",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Posted {
    day: String,
    until_ms: u64,
}

/// What the notice needs besides the rows.
#[derive(Debug, Clone, Default)]
pub struct Night {
    /// When the window ended: the date of a night that landed nothing to time.
    pub until_ms: u64,
    /// What the day line has spent and its size.
    pub spent_usd: f64,
    pub day_usd: f64,
    /// Lines for the owner, already worded, each with the command to see it.
    pub waits: Vec<String>,
    /// Items of the Rust mints: their joins are told as made, not fixed.
    pub mint_items: BTreeSet<String>,
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_default()
}

fn clock(ms: u64) -> String {
    let l = crate::wake::local(ms);
    format!("{:02}:{:02}", l.hour, l.minute)
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(4)]
}

/// The text of a sentence with its closing full stop, once.
fn sentence(t: &str) -> String {
    let t = t.trim().trim_end_matches('.');
    format!("{t}.")
}

/// One join, told as a fix: what it did, what proved it, how to undo it.
fn fixed(r: &LedgerRow, rows: &[LedgerRow]) -> String {
    let d = &r.data;
    let (branch, head, merge) = (s(d, "branch"), s(d, "head"), s(d, "merge"));
    let of = |kind: &str| {
        rows.iter().rev().find(|x| {
            x.kind == kind && s(&x.data, "branch") == branch && s(&x.data, "head") == head
        })
    };
    let mut proofs: Vec<String> = Vec::new();
    if s(d, "gate") == "green" {
        proofs.push("Gate green in L1".into());
    }
    if let Some(v) = of("self.review.verdict") {
        let who = s(&v.data, "reviewer").split(' ').next().unwrap_or_default();
        let mark = if s(&v.data, "verdict") == "accept" {
            "✔"
        } else {
            "✘"
        };
        let mut line = format!("review ({who}) {mark}");
        match s(&v.data, "planted_revert") {
            "failed" => line.push_str(", planted revert failed the new tests"),
            "" => {}
            other => line.push_str(&format!(", planted revert {other}")),
        }
        proofs.push(line);
    }
    if let Some(c) = of("self.sealed.checked").filter(|c| s(&c.data, "outcome") != "skipped") {
        let n = |k: &str| c.data[k].as_u64().unwrap_or(0);
        proofs.push(crate::rsi::log::sealed_line(
            n("before"),
            n("after"),
            n("total"),
        ));
    }
    let mut landed = format!("joined {}", short(merge));
    if let Some(i) = rows
        .iter()
        .find(|x| x.kind == "self.installed" && s(&x.data, "merge") == merge)
    {
        landed.push_str(&format!(", installed {}", clock(i.at_unix_ms)));
    }
    proofs.push(landed);
    let what = d["why"]
        .as_str()
        .filter(|w| !w.is_empty())
        .unwrap_or(branch);
    format!(
        "• {branch}: {}\n  {}\n  undo: {}",
        sentence(what),
        proofs.join(" · "),
        s(d, "undo")
    )
}

/// One deploy, told as made: what it is, where it came from, what checked it.
fn made(r: &LedgerRow) -> String {
    let d = &r.data;
    let n = &d["numbers"];
    let label = if d["bench_born"] == true {
        "bench-born "
    } else {
        ""
    };
    let mut from = format!(
        "From {} by-hand runs ({} sessions, {} days).",
        n["runs"], n["sessions"], n["days"]
    );
    let mut checks: Vec<String> = Vec::new();
    if n["check_total"].is_u64() {
        checks.push(format!(
            "{}/{} same result",
            n["check_same"], n["check_total"]
        ));
    }
    if let Some(t) = n["tokens_pct"].as_f64() {
        checks.push(format!("{t:+}% tokens"));
    }
    if let Some(t) = n["time_pct"].as_f64() {
        checks.push(format!("{t:+}% time"));
    }
    if let Some(x) = n["decisions_removed"].as_f64() {
        checks.push(format!("{x} decisions removed"));
    }
    if n["tests"].is_u64() {
        checks.push(format!(
            "{} of {} declared tests passed",
            n["tests_passed"], n["tests"]
        ));
    }
    if !checks.is_empty() {
        from.push_str(&format!(" Check half: {}", checks.join(" · ")));
    }
    format!(
        "• {label}{}\n  {from}\n  undo: {}",
        s(d, "what"),
        s(d, "undo")
    )
}

fn retired(r: &LedgerRow) -> String {
    let d = &r.data;
    let label = if d["bench_born"] == true {
        "bench-born "
    } else {
        ""
    };
    format!(
        "• {label}{}: {}\n  undo: {}",
        s(d, "tool"),
        sentence(s(d, "why")),
        s(d, "undo")
    )
}

/// The notice, from the night's rows (oldest first) and what is waiting; none
/// when there is nothing to say.
pub fn notice_text(rows: &[LedgerRow], n: &Night) -> Option<String> {
    let of = |kind: &'static str| rows.iter().filter(move |r| r.kind == kind);
    let joins: Vec<&LedgerRow> = of("self.joined").collect();
    let (rust, fixes): (Vec<&LedgerRow>, Vec<&LedgerRow>) = joins
        .into_iter()
        .partition(|r| n.mint_items.contains(s(&r.data, "item")));
    let fixed_lines: Vec<String> = fixes.iter().map(|r| fixed(r, rows)).collect();
    let mut made_lines: Vec<String> = of("self.mint.deployed").map(made).collect();
    made_lines.extend(rust.iter().map(|r| {
        let d = &r.data;
        format!(
            "• rust {}: {}\n  joined {}\n  undo: {}",
            s(d, "branch"),
            sentence(
                d["why"]
                    .as_str()
                    .filter(|w| !w.is_empty())
                    .unwrap_or(s(d, "branch"))
            ),
            short(s(d, "merge")),
            s(d, "undo")
        )
    }));
    let retired_lines: Vec<String> = of("self.mint.retired").map(retired).collect();
    if fixed_lines.is_empty()
        && made_lines.is_empty()
        && retired_lines.is_empty()
        && n.waits.is_empty()
    {
        return None;
    }
    let landed = fixes
        .iter()
        .chain(&rust)
        .copied()
        .chain(of("self.mint.deployed"))
        .chain(of("self.mint.retired"));
    let (first, last) = landed.fold((u64::MAX, 0), |(a, b), r| {
        (a.min(r.at_unix_ms), b.max(r.at_unix_ms))
    });
    let (first, last) = if last == 0 { (0, 0) } else { (first, last) };
    let l = crate::wake::local(if last == 0 { n.until_ms } else { last });
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let halts = of("self.halted").count();
    let window = match last {
        0 => String::new(),
        _ => format!("{}–{} · ", clock(first), clock(last)),
    };
    let mut out = format!(
        "Self, night of {} {} ({window}${:.2} of ${:.0} · {halts} halts)\n",
        months[(l.month.clamp(1, 12) - 1) as usize],
        l.day,
        n.spent_usd,
        n.day_usd
    );
    for (title, lines) in [
        ("Fixed", fixed_lines),
        ("Made", made_lines),
        ("Retired", retired_lines),
        ("Waits for you", n.waits.clone()),
    ] {
        if !lines.is_empty() {
            out.push_str(&format!("\n{title}\n{}\n", lines.join("\n")));
        }
    }
    let filed = of("self.backlog.filed");
    let (new, updated) = filed.fold((0, 0), |(a, b), r| {
        match r.data["numbers"]["count"].as_u64() {
            Some(c) if c > 1 => (a, b + 1),
            _ => (a + 1, b),
        }
    });
    out.push_str(&format!(
        "\nFiled {new} · updated {updated} · dropped {} — \"halt self\" stops everything",
        of("self.backlog.dropped").count()
    ));
    Some(out)
}

impl Core {
    /// What waits for the owner, worded: each backlog item held for the owner
    /// (a keel class) that was filed in the window, with the command that shows
    /// it (an older one was told when it was filed: the notice is quiet).
    fn notice_waits(&self, since_ms: u64) -> Vec<String> {
        let Ok(b) = self.self_backlog(&SelfBacklogParams::default()) else {
            return vec![];
        };
        b.items
            .iter()
            .filter(|i| i.state == "waiting_human" && i.filed_ms >= since_ms)
            .map(|i| {
                let why = i
                    .keel
                    .as_deref()
                    .map_or(String::new(), |k| format!(" ({k})"));
                format!("• {}{why}: theseus self backlog show {}", i.title, i.rank)
            })
            .collect()
    }

    /// The items of the Rust mints: the backlog's tasks that carry a `mint:`
    /// tag in their evidence.
    fn notice_mint_items(&self) -> BTreeSet<String> {
        self.self_backlog(&SelfBacklogParams::default())
            .map(|b| {
                b.items
                    .iter()
                    .filter(|i| i.evidence.iter().any(|e| e.starts_with("mint:")))
                    .map(|i| i.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The notice for `(since_ms, until_ms]`, from the ledger's rows. None when
    /// there is nothing to say.
    pub fn self_notice(&self, since_ms: u64, until_ms: u64) -> Result<Option<String>> {
        let rows = rows::between(&self.store, KINDS, since_ms, until_ms)?;
        let spent = self.runner.self_budget.view();
        let night = Night {
            until_ms,
            spent_usd: spent.as_ref().map_or(0.0, |b| b.day_spent_usd),
            day_usd: spent.as_ref().map_or(0.0, |b| b.day_line_usd),
            waits: self.notice_waits(since_ms),
            mint_items: self.notice_mint_items(),
        };
        Ok(notice_text(&rows, &night).map(|t| self.tools.scrubber.scrub(&t).0))
    }

    /// The driver's tick: post the night's notice to the owner's DM once a
    /// local day, from `[initiative] morning_hour`, while `[self] mode = "act"`
    /// and the switch is not halted, and a DM with the owner is bound. A day
    /// with nothing to say writes only its mark. Nothing at all with the mode
    /// off or halted: no read, no write. Whether it posted.
    pub fn post_self_notice_if_due(&self) -> bool {
        if self.cfg.self_improve.mode != SelfMode::Act {
            return false;
        }
        let now = theseus_protocol::now_unix_ms();
        let l = crate::wake::local(now);
        if l.hour < self.cfg.initiative.morning_hour {
            return false;
        }
        let day = format!("{:04}-{:02}-{:02}", l.year, l.month, l.day);
        let mut seen = self
            .rsi
            .notice_day
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.as_deref() == Some(day.as_str()) {
            return false;
        }
        if gate(&self.self_ctx()) != Gate::Allowed || !self.runner.place_rule.owner_dm_bound() {
            return false;
        }
        let last = match self.store.get_meta::<Posted>(NOTICE_KEY) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self notice: its mark could not be read");
                return false;
            }
        };
        if last.as_ref().is_some_and(|p| p.day == day) {
            *seen = Some(day);
            return false;
        }
        // The window runs from the last notice, else a day back.
        let since = last.map_or(now.saturating_sub(DAY_MS), |p| p.until_ms);
        let written = (|| -> Result<bool> {
            let text = self.self_notice(since, now)?;
            let mark = NewRecord::json(
                kinds::META,
                Some(NOTICE_KEY),
                &Posted {
                    day: day.clone(),
                    until_ms: now,
                },
            )?;
            let Some(text) = text else {
                self.store.append(&[mark])?;
                return Ok(false);
            };
            let body = json!({"kind": "self_digest", "text": super::capped(&text)});
            let (a, mut frame) = self.outbox.stage_to_operator(None, body)?;
            frame.push(mark);
            self.store.append(&frame)?;
            self.outbox.posted(&a);
            Ok(true)
        })();
        // A day is settled once, posted or failed: the log has it all meanwhile.
        *seen = Some(day);
        match written {
            Ok(posted) => posted,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self notice: the post was not written");
                false
            }
        }
    }
}
