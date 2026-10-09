//! The weekly digest (theseus-pw1q.4): "What Theseus changed about itself
//! this week", made from `self.log`'s rows of the week: counts by kind,
//! each join with its numbers and its undo, the switch's moves, what it
//! cost, and every change, newest first. `theseus self digest` prints it;
//! the Discord binding posts it to the owner's DM once a week while
//! `[self] mode = "act"` and `digest = "weekly"` (`SelfConfig::posts_digest`),
//! and never while the mode is off.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_protocol::rsi::{SelfDigestParams, SelfDigestResult, SelfLogRow, SelfState};
use theseus_store::{kinds, NewRecord};

use crate::rpc::Core;

/// A week.
pub const WEEK_MS: u64 = 7 * 86_400_000;

/// The most rows a digest reads, and the most changes it lists one by one.
const MAX_ROWS: usize = 2_000;
const LISTED: usize = 40;

/// The META record of the digest's last post to the owner's DM.
pub const DIGEST_KEY: &str = "self.digest.posted";

/// The most of a digest one Discord post carries.
const POST_CHARS: usize = 1_800;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Posted {
    at_ms: u64,
}

/// The week that ends at `until_ms`: `(since, until)`.
pub fn week_of(until_ms: u64) -> (u64, u64) {
    (until_ms.saturating_sub(WEEK_MS), until_ms)
}

impl Core {
    /// The digest of the week that ends at `p.until_ms` (default now).
    pub fn self_digest(&self, p: &SelfDigestParams) -> Result<SelfDigestResult> {
        let (since, until) = week_of(p.until_ms.unwrap_or_else(theseus_protocol::now_unix_ms));
        let (rows, more, building) =
            super::log::read(&self.store, Some(since), Some(until), MAX_ROWS)?;
        let mut text = digest_text(&rows, since, until, &self.self_state());
        if more {
            text.push_str(&format!(
                "\n(Read the first {MAX_ROWS} changes of the week: `theseus self log` pages \
                 the rest.)"
            ));
        }
        if building {
            text.push_str(
                "\n(The ledger's index is still being built after a start: nothing could be \
                 read yet. Ask again in a minute.)",
            );
        }
        Ok(SelfDigestResult {
            since_ms: since,
            until_ms: until,
            text,
            rows: rows.len() as u64,
        })
    }
}

impl Core {
    /// The driver's tick: post the week's digest to the owner's DM once a
    /// week while `[self]` posts it (`mode = "act"`, `digest = "weekly"`),
    /// and a DM with the owner is bound. Nothing at all while the mode is
    /// off: no read, no write. The first week starts when posting is first
    /// on (that tick writes its mark and posts nothing). Whether it posted.
    pub fn post_self_digest_if_due(&self) -> bool {
        if !self.cfg.self_improve.posts_digest() || !self.runner.place_rule.owner_dm_bound() {
            return false;
        }
        let now = theseus_protocol::now_unix_ms();
        let mut at = self
            .rsi
            .digest_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let last = match *at {
            Some(a) => a,
            None => match self.store.get_meta::<Posted>(DIGEST_KEY) {
                Ok(Some(p)) => p.at_ms,
                Ok(None) => {
                    // The first week starts now.
                    let mark = posted(now).and_then(|r| self.store.append(&[r]).map(drop));
                    if let Err(e) = mark {
                        tracing::warn!(error = %format!("{e:#}"), "self digest: its mark was not written");
                        return false;
                    }
                    *at = Some(now);
                    return false;
                }
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "self digest: its mark could not be read");
                    return false;
                }
            },
        };
        *at = Some(last);
        if now < last.saturating_add(WEEK_MS) {
            return false;
        }
        let written = (|| -> Result<()> {
            let d = self.self_digest(&SelfDigestParams {
                until_ms: Some(now),
            })?;
            let body = json!({"kind": "self_digest", "text": capped(&d.text)});
            let (a, mut frame) = self.outbox.stage_to_operator(None, body)?;
            frame.push(posted(now)?);
            self.store.append(&frame)?;
            self.outbox.posted(&a);
            Ok(())
        })();
        // A failed post waits a week too: the log has it all meanwhile.
        *at = Some(now);
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "self digest: the week's post was not written");
            return false;
        }
        true
    }
}

/// The digest's mark at `at_ms`.
fn posted(at_ms: u64) -> Result<NewRecord> {
    NewRecord::json(kinds::META, Some(DIGEST_KEY), &Posted { at_ms })
}

/// A digest cut to one post, saying where the rest is.
fn capped(text: &str) -> String {
    if text.chars().count() <= POST_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(POST_CHARS).collect();
    let cut = cut.rsplit_once('\n').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}\n… the rest: `theseus self digest`.")
}

/// A day's date, on this machine's clock: `2026-10-09`.
fn date(ms: u64) -> String {
    let l = crate::wake::local(ms);
    format!("{:04}-{:02}-{:02}", l.year, l.month, l.day)
}

/// What a row cost, from its numbers: a cost, a writer's cost, or a spend.
fn cost_of(r: &SelfLogRow) -> f64 {
    ["cost_usd", "writer_usd", "spent_usd"]
        .iter()
        .filter_map(|k| r.numbers[*k].as_f64())
        .fold(0.0, |a, c| a + c)
}

/// A row in a line: its time, what, its numbers and its undo.
fn line(r: &SelfLogRow) -> String {
    let mut l = format!("- {} {}: {}", date(r.at_unix_ms), r.kind, r.what);
    if let Some(w) = &r.why {
        l.push_str(&format!(" ({w})"));
    }
    if !r.numbers.is_null() {
        l.push_str(&format!(" [{}]", r.numbers));
    }
    if let Some(u) = &r.undo {
        l.push_str(&format!(" undo: `{u}`"));
    }
    l
}

/// The week's text from its rows, newest first.
pub fn digest_text(rows: &[SelfLogRow], since: u64, until: u64, state: &SelfState) -> String {
    let mut out = format!(
        "What Theseus changed about itself this week ({} to {})\n",
        date(since),
        date(until)
    );
    out.push_str(&format!(
        "Self-improvement: mode {}, {}.\n",
        match state.mode {
            theseus_protocol::rsi::SelfMode::Off => "off",
            theseus_protocol::rsi::SelfMode::Act => "act",
        },
        if state.halted { "halted" } else { "running" }
    ));
    if rows.is_empty() {
        out.push_str("Nothing changed.\n");
        return out;
    }
    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for r in rows {
        *counts.entry(r.kind.as_str()).or_default() += 1;
    }
    let counts: Vec<String> = counts.iter().map(|(k, n)| format!("{n} {k}")).collect();
    out.push_str(&format!("{} changes: {}.\n", rows.len(), counts.join(", ")));
    // From 0.0: an empty f64 sum is -0.0, which prints "$-0.00".
    let cost: f64 = rows.iter().map(cost_of).fold(0.0, |a, c| a + c);
    out.push_str(&format!("Cost: ${cost:.2}.\n"));
    for (title, kinds) in [
        (
            "Joins",
            &["self.joined", "self.reverted", "self.vetoed"][..],
        ),
        ("The kill switch", &["self.halted", "self.resumed"][..]),
    ] {
        let of: Vec<&SelfLogRow> = rows
            .iter()
            .filter(|r| kinds.contains(&r.kind.as_str()))
            .collect();
        if !of.is_empty() {
            out.push_str(&format!("{title}:\n"));
            for r in of {
                out.push_str(&line(r));
                out.push('\n');
            }
        }
    }
    out.push_str("Every change, newest first:\n");
    for r in rows.iter().take(LISTED) {
        out.push_str(&line(r));
        out.push('\n');
    }
    if rows.len() > LISTED {
        out.push_str(&format!(
            "and {} more: `theseus self log --since 7d`.\n",
            rows.len() - LISTED
        ));
    }
    out
}
