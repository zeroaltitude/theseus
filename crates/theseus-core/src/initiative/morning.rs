//! The morning note (theseus-slvl.9): at most one a local day, from
//! `[initiative] morning_hour` on, only on a day something happened, and
//! only when the persona's initiative lets it (`normal` and `chatty`). The
//! driver's tick asks `post_morning_if_due` every half second, and the answer
//! is one lock and a comparison until the day's first ask after the hour; the
//! work (four ledger pages and a short model call) is a task of the runtime,
//! never the tick's. The day is claimed before the work starts and marked in
//! META after it, so a restart does not write a second note and a failed one
//! waits for tomorrow, as the self digest's does.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_protocol::{LedgerKind, Proactive};
use theseus_store::{kinds, NewRecord};

use super::lines::Line;
use super::speak::Facts;
use crate::rpc::Core;
use crate::wake::local;

/// The META key of the last day a note was considered.
pub const MARK: &str = "initiative.morning";
const DAY_MS: u64 = 86_400_000;

/// What the tick remembers.
#[derive(Default)]
pub struct State {
    day: Mutex<Option<String>>,
}

impl Core {
    /// The tick: claim today's note when it is due. Whether the work began.
    pub fn post_morning_if_due(self: &Arc<Self>) -> bool {
        let now = theseus_protocol::now_unix_ms();
        let here = local(now);
        let day = format!("{:04}-{:02}-{:02}", here.year, here.month, here.day);
        // The tick's answer on all but a day's first ask is this comparison.
        if self
            .initiative
            .day
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            == Some(day.as_str())
            || here.hour < self.cfg.initiative.morning_hour
        {
            return false;
        }
        // The config is fixed for the daemon's life: a level that says no says
        // it for the day, and the tick asks nothing more.
        if !self
            .cfg
            .persona_for(None, None)
            .initiative
            .lets(Proactive::Morning)
        {
            *self
                .initiative
                .day
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(day);
            return false;
        }
        {
            let mut last = self
                .initiative
                .day
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // A restart reads its mark once.
            if last.is_none()
                && self
                    .store
                    .get_meta::<String>(MARK)
                    .ok()
                    .flatten()
                    .as_deref()
                    == Some(day.as_str())
            {
                *last = Some(day);
                return false;
            }
            if !self.runner.place_rule.owner_dm_bound() {
                return false;
            }
            // The one pacing rule (`pacer.rs`): an unprompted post waits for
            // the window, and the day is claimed only when it is open.
            let pace = 1000 * self.cfg.notify.unprompted_pace_secs;
            if !self.outbox.pacer.open(now, pace) {
                return false;
            }
            *last = Some(day.clone());
        }
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            if let Some(core) = core.upgrade() {
                core.morning_note(&day, now).await;
            }
        });
        true
    }

    /// What happened in the last day, as the note's facts: none, nothing to say.
    pub(crate) fn morning_facts(&self, now: u64) -> Option<(String, u64)> {
        let since = now.saturating_sub(DAY_MS);
        let count = |k| {
            self.synthesis_rows(k, Some(since))
                .map(|r| r.len() as u64)
                .unwrap_or(0)
        };
        let items = [
            (count(LedgerKind::TaskEnded), "tasks ended"),
            (count(LedgerKind::WakeFired), "wakes fired"),
            (
                count(LedgerKind::PromiseUnkept),
                "follow-ups promised with nothing scheduled",
            ),
            (
                count(LedgerKind::SynthesisProposed),
                "memory notes proposed",
            ),
        ];
        let total: u64 = items.iter().map(|(n, _)| n).sum();
        if total == 0 {
            return None;
        }
        let brief = items
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| format!("- {n} {what}"))
            .collect::<Vec<_>>()
            .join("\n");
        Some((
            format!("In the last day:\n{brief}\nNothing is waiting on the owner."),
            total,
        ))
    }

    async fn morning_note(self: &Arc<Self>, day: &str, now: u64) {
        if let Some((brief, n)) = self.morning_facts(now) {
            let spoken = self
                .initiative_speak(&Facts {
                    kind: "morning",
                    brief: &brief,
                    line: Line::Morning,
                    n,
                })
                .await;
            if let Err(e) = self.initiative_post("morning", &spoken) {
                tracing::warn!(error = %format!("{e:#}"), "the morning note was not written");
            }
        }
        let mark = NewRecord::json(kinds::META, Some(MARK), &json!(day))
            .and_then(|r| self.store.append(&[r]).map(drop));
        if let Err(e) = mark {
            tracing::warn!(error = %format!("{e:#}"), "the morning note's mark was not written");
        }
    }
}
