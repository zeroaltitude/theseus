//! The nightly OpenClaw sync's people step (theseus-pjmj7): the people
//! extraction (`run.rs`) over the imported sessions a sync created or
//! appended to, reading only their new messages.
//!
//! - **What it reads.** The nodes of each session written after its mark
//!   (META `judge.people.<session>`, the live point's own key: an imported
//!   session never takes a live turn, so nothing else moves it). The mark is
//!   a WAL position, so an append's nodes are exactly the ones after it. The
//!   pass moves it, in its own frame, to the newest node read; a session with
//!   nothing after its mark costs no read of its text, and one whose new
//!   nodes hold no human-facing line only has its mark moved.
//! - **Under the sweep's cap, on the sweep's day.** `[people]
//!   sweep_usd_per_day` is one budget a local day: the sweep's spend
//!   (`people.sweep.last`) and this step's (`people.sync.day`) are added, and
//!   each leaves the other what is left. A session whose worst case would
//!   pass it is not begun, and stays in the sync's list for the next run.
//! - **Off with the sweep**: a cap of 0, or `[people] live = false`, turns
//!   it off, and the sessions are not kept for later.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_kernel::usd_to_micros;
use theseus_store::kinds;

use super::live::MARK_PREFIX;
use super::run::meta;
use super::sweep::{Due, SweepMark, Swept, LAST_RUN};
use super::{lines, PACK};
use crate::judge::categorize::Mark;
use crate::judge::spend;
use crate::node::Node;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The step's day spend, a META key.
pub const DAY: &str = "people.sync.day";

/// What the step spent on its local day.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncDay {
    pub day: String,
    pub spent_usd: f64,
}

impl Core {
    /// What the sync's people step spent on `day`.
    pub(crate) fn people_sync_spent(&self, day: &str) -> Result<f64> {
        let held: Option<SyncDay> = self.store.get_meta(DAY)?;
        Ok(held.filter(|d| d.day == day).map_or(0.0, |d| d.spent_usd))
    }

    /// The sessions of `todo` with nodes past their marks, each with its new
    /// lines and its mark moved; and the ones with nothing new (done).
    fn people_sync_plan(&self, todo: &[String], now: u64) -> Result<(Vec<Due>, Vec<String>)> {
        let mut known = self.people_known()?;
        let mut plan = Vec::new();
        let mut nothing = Vec::new();
        let mut sessions: Vec<SessionRecord> = Vec::new();
        for sid in todo {
            match self.store.get_session::<SessionRecord>(sid)? {
                Some(r) if r.imported.as_ref().is_some_and(|i| i.erased.is_none()) => {
                    sessions.push(r);
                }
                _ => nothing.push(sid.clone()),
            }
        }
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_active_ms));
        for s in sessions {
            let sid = s.session_id;
            let key = format!("{MARK_PREFIX}{sid}");
            let held: Option<Mark> = self.store.get_meta(&key)?;
            let after = held.as_ref().map_or(0, |m| m.through);
            let fresh: Vec<(u64, Node)> = self
                .store
                .scope_after(&sid, after)?
                .iter()
                .filter(|r| r.kind == kinds::NODE)
                .filter_map(|r| Some((r.position, r.decode::<Node>().ok()?)))
                .collect();
            let Some((through, through_ms)) = fresh.last().map(|(p, n)| (*p, n.created_at_ms))
            else {
                nothing.push(sid);
                continue;
            };
            let mark = meta(
                &key,
                &Mark {
                    judgment: String::new(),
                    through,
                    through_ms,
                    at_ms: now,
                },
            )?;
            plan.push(Due {
                known: known.remove(&sid).unwrap_or_default(),
                title: s.title.unwrap_or_default(),
                sid,
                lines: lines(&fresh),
                mark: Some(mark),
            });
        }
        Ok((plan, nothing))
    }

    /// The people extraction over `todo`'s sessions' new messages, under what
    /// is left of the sweep's day. `done` names the sessions that need no
    /// more; the rest wait for the next run.
    pub async fn people_sync(&self, todo: Vec<String>) -> Result<Swept> {
        let now = theseus_protocol::now_unix_ms();
        let cfg = &self.runner.cfg.people;
        let mut out = Swept::default();
        if cfg.sweep_usd_per_day <= 0.0 || !cfg.live {
            out.stopped = Some(
                "the people sweep is off ([people] sweep_usd_per_day = 0 or live = false)".into(),
            );
            out.done = todo;
            return Ok(out);
        }
        if !self.runner.judge.pack_on(PACK) {
            out.stopped = Some(format!("{PACK} is off"));
            return Ok(out);
        }
        let today = spend::local_day(now);
        let swept: Option<SweepMark> = theseus_store::blocking(|| self.store.get_meta(LAST_RUN))?;
        let swept = swept
            .filter(|m| m.day == today)
            .map_or(0.0, |m| m.spent_usd);
        let mine = self.people_sync_spent(&today)?;
        let cap = usd_to_micros((cfg.sweep_usd_per_day - swept - mine).max(0.0));
        let (plan, nothing) = theseus_store::blocking(|| self.people_sync_plan(&todo, now))?;
        out.done = nothing;
        // A session whose new nodes hold no line only has its mark moved.
        let (read, bare): (Vec<Due>, Vec<Due>) =
            plan.into_iter().partition(|d| !d.lines.is_empty());
        if !bare.is_empty() {
            let marks: Vec<_> = bare.iter().filter_map(|d| d.mark.clone()).collect();
            self.people_write(&marks).await?;
            out.done.extend(bare.into_iter().map(|d| d.sid));
        }
        self.pass_plan(read, "sync", cap, &mut out).await?;
        let spent = theseus_kernel::micros_to_usd(out.spent);
        if out.spent > 0 {
            let day = SyncDay {
                day: today,
                spent_usd: mine + spent,
            };
            self.people_write(&[meta(DAY, &day)?]).await?;
        }
        Ok(out)
    }
}
