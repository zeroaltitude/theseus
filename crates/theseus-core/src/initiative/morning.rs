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
use super::morning_google::Day;
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
    /// The digest post's (theseus-ss36w).
    pub digest: super::digest::State,
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
            .personas()
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
    /// With `[google]`, the day's meetings and mail join them (`google`), and
    /// what waits on the owner says so. `ontology` is how many writes the
    /// night's sweep and dream made.
    pub(crate) fn morning_facts(
        &self,
        now: u64,
        google: Option<&Day>,
        ontology: u64,
    ) -> Option<(String, u64)> {
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
            (ontology, "ontology changes by the night's sweep and dream"),
        ];
        let total: u64 =
            items.iter().map(|(n, _)| n).sum::<u64>() + google.map_or(0, Day::happened);
        if total == 0 {
            return None;
        }
        let brief = items
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, what)| format!("- {n} {what}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut text = match brief.is_empty() {
            true => String::new(),
            false => format!("In the last day:\n{brief}\n"),
        };
        if let Some(g) = google.filter(|g| !g.lines.is_empty()) {
            text.push_str(&format!("Today and waiting:\n{}\n", g.lines.join("\n")));
        }
        text.push_str(&match google.map_or(0, |g| g.drafts) {
            0 => "Nothing is waiting on the owner.".to_string(),
            1 => "1 draft waits for the owner to send.".to_string(),
            n => format!("{n} drafts wait for the owner to send."),
        });
        Some((text, total))
    }

    async fn morning_note(self: &Arc<Self>, day: &str, now: u64) {
        let google = self.morning_google(now).await;
        // The ontology's writes go in verbatim after the voiced words: the
        // model never rewords an undo id (theseus-0umqz).
        let ontology = self.onto_night_lines(now);
        if let Some((brief, n)) = self.morning_facts(now, google.as_ref(), ontology.len() as u64) {
            let waiting = google.as_ref().is_some_and(|g| g.drafts > 0);
            let mut spoken = self
                .initiative_speak(&Facts {
                    kind: "morning",
                    brief: &brief,
                    line: if waiting {
                        Line::MorningWaiting
                    } else {
                        Line::Morning
                    },
                    n,
                })
                .await;
            if !ontology.is_empty() {
                spoken.text = format!("{}\n\n{}", spoken.text, ontology.join("\n"));
            }
            // A sensitive calendar's events reach the DM and no ledger row.
            let held = google.as_ref().is_some_and(|g| g.sensitive);
            if let Err(e) = self.initiative_post_as("morning", &spoken, held) {
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
