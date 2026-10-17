//! The digest post (theseus-ss36w): at each `[discord] digest_at` slot, one
//! message to the owner's DM of what happened since the last digest, only
//! when something did. The driver's tick asks `post_digest_if_due` (one lock
//! and a comparison until a slot's first ask); the read is `work.digest`'s,
//! worded as Discord's `/digest` words it, and runs as a task of the runtime.
//!
//! - **A slot is a local day's `HH:MM`** (`due`, pure): the latest slot today
//!   whose time has passed and is later than the last one claimed. A slot is
//!   claimed in memory when its post is begun and marked in META
//!   (`discord.digest.<date>.<HHMM>`, with `discord.digest.last` for the
//!   next window's start) in the frame that posts, or alone when nothing
//!   happened, so a restart posts none twice and a failed post waits for the
//!   next slot, as the morning note's does.
//! - **One pacing rule** (`pacer.rs`): the digest is an unprompted post, so
//!   while the window is closed the slot waits, unclaimed, and posts when it
//!   opens, within the same local day.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde_json::json;
use theseus_protocol::{digest_text, WorkDigestParams};
use theseus_store::{kinds, NewRecord};

use crate::approval::Surface;
use crate::rpc::Core;
use crate::wake::{local, Local};

/// The META key of the last digest's time, the next window's start.
const LAST: &str = "discord.digest.last";
/// A window with no earlier digest reaches back this far.
const FIRST_MS: u64 = 86_400_000;
/// Discord's limit on a message's text.
const LIMIT: usize = 2000;

/// What the tick remembers.
#[derive(Default)]
pub struct State {
    /// The configured slots, read once (a test sets them first).
    pub(crate) slots: Mutex<Option<Vec<(u32, u32)>>>,
    pub(crate) claimed: Mutex<Option<String>>,
}

/// The slot due at `now`, as `<date>.<HHMM>`: the latest of `slots` today
/// whose time has come and that sorts after `last`, the latest claimed.
pub fn due(now: &Local, slots: &[(u32, u32)], last: Option<&str>) -> Option<String> {
    let at = (now.hour, now.minute);
    let key = |(h, m): &(u32, u32)| {
        format!(
            "{:04}-{:02}-{:02}.{h:02}{m:02}",
            now.year, now.month, now.day
        )
    };
    slots
        .iter()
        .filter(|s| **s <= at)
        .max()
        .map(key)
        .filter(|k| last.is_none_or(|l| k.as_str() > l))
}

impl Core {
    /// The tick: begin the post of a due slot's digest. Whether it began.
    pub fn post_digest_if_due(self: &Arc<Self>) -> bool {
        self.digest_tick(theseus_protocol::now_unix_ms())
    }

    /// `post_digest_if_due` at `now`, which a test chooses.
    pub(crate) fn digest_tick(self: &Arc<Self>, now: u64) -> bool {
        let state = &self.initiative.digest;
        let slots = state
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(|| {
                crate::config::discord::digest_slots(&self.cfg.discord.digest_at)
                    .unwrap_or_default()
            })
            .clone();
        if slots.is_empty() {
            return false;
        }
        let mut claimed = state.claimed.lock().unwrap_or_else(|e| e.into_inner());
        let Some(slot) = due(&local(now), &slots, claimed.as_deref()) else {
            return false;
        };
        // A restart reads the slot's mark once.
        if self
            .store
            .get_meta::<u64>(&format!("discord.digest.{slot}"))
            .ok()
            .flatten()
            .is_some()
        {
            *claimed = Some(slot);
            return false;
        }
        if !self.runner.place_rule.owner_dm_bound() {
            return false;
        }
        // The one pacing rule: the slot waits for the window and is claimed
        // only when it is open.
        if !self
            .outbox
            .pacer
            .open(now, 1000 * self.cfg.notify.unprompted_pace_secs)
        {
            return false;
        }
        *claimed = Some(slot.clone());
        drop(claimed);
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            if let Some(core) = core.upgrade() {
                if let Err(e) = core.digest_post(&slot, now).await {
                    tracing::warn!(error = %format!("{e:#}"), "the digest was not posted");
                }
            }
        });
        true
    }

    /// Read what happened since the last digest and post it, or mark the slot
    /// with nothing posted. The read is the owner's, so every root counts.
    async fn digest_post(self: &Arc<Self>, slot: &str, now: u64) -> Result<()> {
        let since = self
            .store
            .get_meta::<u64>(LAST)?
            .unwrap_or_else(|| now.saturating_sub(FIRST_MS));
        let params = WorkDigestParams {
            since_ms: Some(since),
            ..Default::default()
        };
        let res = self
            .work_digest_for(params, Surface::Cli)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.message))?;
        let mut frame = Vec::new();
        let post = if res.lines.is_empty() {
            None
        } else {
            let text = digest_text(&res, since, &crate::push::hm, LIMIT);
            let body = json!({"kind": "digest", "slot": slot, "text": text});
            let (a, records) = self.outbox.stage_to_operator(None, body)?;
            frame = records;
            Some(a)
        };
        frame.push(NewRecord::json(
            kinds::META,
            Some(&format!("discord.digest.{slot}")),
            &now,
        )?);
        frame.push(NewRecord::json(kinds::META, Some(LAST), &now)?);
        self.store.append(&frame)?;
        if let Some(a) = post {
            self.outbox.posted(&a);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u32, minute: u32) -> Local {
        Local {
            year: 2026,
            month: 10,
            day: 17,
            hour,
            minute,
            second: 0,
            offset_secs: 0,
        }
    }

    const SLOTS: [(u32, u32); 2] = [(8, 0), (18, 0)];

    /// A slot is due from its time to the end of the day, once.
    #[test]
    fn a_slot_is_due_from_its_time_until_it_is_claimed() {
        assert_eq!(due(&at(7, 59), &SLOTS, None), None, "before the first");
        assert_eq!(
            due(&at(8, 0), &SLOTS, None).as_deref(),
            Some("2026-10-17.0800")
        );
        assert_eq!(due(&at(9, 0), &SLOTS, Some("2026-10-17.0800")), None);
        assert_eq!(
            due(&at(18, 30), &SLOTS, Some("2026-10-17.0800")).as_deref(),
            Some("2026-10-17.1800"),
            "the evening's, after the morning's"
        );
        assert_eq!(
            due(&at(23, 59), &SLOTS, None).as_deref(),
            Some("2026-10-17.1800"),
            "a daemon down all day posts the latest slot once, not both"
        );
        assert_eq!(due(&at(12, 0), &[], None), None, "no slots, no digest");
        // Yesterday's mark sorts before today's slot.
        assert_eq!(
            due(&at(8, 5), &SLOTS, Some("2026-10-16.1800")).as_deref(),
            Some("2026-10-17.0800")
        );
    }
}
