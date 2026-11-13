//! The OpenClaw sync's state (theseus-pjmj7): what `import.sync.plan` hands
//! the CLI, what `import.sync.record` writes at a run's end, and what health
//! says of the last run. The reading itself is `scripts/openclaw-export.py`
//! (run by `theseus import sync`); the writing is `import.episodes`, and an
//! episode that grew is appended to (`append.rs`).
//!
//! - **One META record**, `import.sync.state`: the exporter's cursor, the last
//!   run's counts and error, and the sessions whose people step is left. It is
//!   written in one frame with the run's `import.synced` row; the cursor moves
//!   only on a run that did not fail, and what a failed run wrote is kept (a
//!   rerun finds it imported and skips it).
//! - **The first run's start**, with no cursor, is the newest `as_of` end of
//!   the imported sessions whose source is `openclaw-store`, so the
//!   2026-10-06 import is not read twice; with no such session the run
//!   refuses and asks for `--since`.
//! - **Health** names the last run, the next one (the next
//!   [`SYNC_MINUTE`] past `[judge] learning_hour`), and a failed or a missed
//!   night as a warning.
//! - **A sync on demand** (theseus-ievo3). A decision recorded in OpenClaw was
//!   readable here only at the next night's run, so for most of a working day
//!   Theseus acted on yesterday's picture. `import.sync.request` records that a
//!   sync is wanted now ([`SyncState::requested_ms`]); the plan carries it, and
//!   `theseus import sync --if-requested` -- which the on-demand timer runs
//!   every few minutes -- is what serves it. **The daemon starts nothing**: the
//!   reading is the CLI's, so the daemon's half of "on demand" is a record.
//!   A run that did not fail clears the request; a run that failed leaves it,
//!   exactly as it leaves the cursor, because nothing was imported.
//!   `--if-requested` passes `no_people`, so an on-demand run can never drag
//!   the people step -- which stays nightly -- along with it.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::import_sync::{
    HealthSync, ImportSyncPlan, ImportSyncRecord, MISSED_AFTER_MS, REQUEST_STALE_AFTER_MS, SAMPLES,
    SYNC_MINUTE,
};

use crate::learning::local_midnight;
use crate::rpc::Core;
use crate::store::Store;

/// The state's META key.
pub const STATE_KEY: &str = "import.sync.state";

/// The source the 2026-10-06 import's sessions have.
pub const OLD_SOURCE: &str = "openclaw-store";

/// The stored state of the sync.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncState {
    /// The exporter's cursor, as it wrote it.
    #[serde(default)]
    pub cursor: Option<Value>,
    pub at_ms: u64,
    pub started_ms: u64,
    pub sessions_new: u64,
    pub sessions_appended: u64,
    pub messages: u64,
    pub rejected: u64,
    #[serde(default)]
    pub people_usd: f64,
    #[serde(default)]
    pub error: Option<String>,
    /// The first rejections of the last run, worded.
    #[serde(default)]
    pub samples: Vec<String>,
    /// Sessions whose people step is left for the next run.
    #[serde(default)]
    pub people_todo: Vec<String>,
    /// When a sync was asked for and no run has served it yet, unix ms; 0 is
    /// none (theseus-ievo3, `Core::sync_request`). A run that did not fail
    /// clears it; a run that failed leaves it, as it leaves the cursor.
    #[serde(default)]
    pub requested_ms: u64,
    /// Who asked for it, as the act's record labels them.
    #[serde(default)]
    pub requested_by: Option<String>,
}

/// The newest `as_of` end, in unix ms, of the imported sessions whose source
/// is `source`, over every tag that holds one.
pub fn newest_end(store: &Store, source: &str) -> Result<Option<u64>> {
    let mut newest: Option<u64> = None;
    for t in super::write::list(store)?.tags {
        if !t.sources.contains_key(source) {
            continue;
        }
        let sessions = super::write::sessions_of_tag(store, &t.tag, &|| false)?.unwrap_or_default();
        for r in sessions.into_values() {
            if let Some(i) = r
                .imported
                .filter(|i| i.source == source && i.erased.is_none())
            {
                newest = Some(newest.map_or(i.as_of.end_ms, |n| n.max(i.as_of.end_ms)));
            }
        }
    }
    Ok(newest)
}

/// The next [`SYNC_MINUTE`] past `hour` o'clock local, after `after_ms`.
pub fn next_run(after_ms: u64, hour: u8) -> u64 {
    const DAY: u64 = 24 * 3_600_000;
    let at =
        |t: u64| local_midnight(t) + u64::from(hour) * 3_600_000 + u64::from(SYNC_MINUTE) * 60_000;
    let today = at(after_ms);
    if today > after_ms {
        today
    } else {
        at(after_ms + DAY)
    }
}

impl SyncState {
    /// How health says it at `now_ms`: the run's counts, the next run, and the
    /// warning a failed or a missed night makes.
    pub fn health(&self, now_ms: u64, hour: u8) -> HealthSync {
        let next_ms = next_run(self.at_ms, hour);
        let warn = match &self.error {
            Some(e) => Some(format!("the last sync failed: {e}")),
            // `at_ms == 0` is a state that holds a request and no run yet
            // (theseus-ievo3): there is no night to have missed.
            None if self.at_ms > 0 && now_ms > next_ms + MISSED_AFTER_MS => Some(format!(
                "a night's sync was missed: it was due {} {hour:02}:{SYNC_MINUTE:02} and has not run",
                crate::judge::spend::local_day(next_ms)
            )),
            // A request nothing served (theseus-ievo3): the on-demand timer's
            // period is minutes and the import-only timer's an hour, so a
            // request older than [`REQUEST_STALE_AFTER_MS`] means neither ran.
            // Said rather than left silent: a request that waits looks exactly
            // like a healthy sync from here otherwise.
            None if self.requested_ms > 0
                && now_ms > self.requested_ms + REQUEST_STALE_AFTER_MS =>
            {
                Some(format!(
                    "a sync was asked for on {} and has not run: check that the sync timers are \
                     installed (`scripts/user-service.sh sync-timer status`), or run `theseus \
                     import sync` yourself",
                    crate::judge::spend::local_day(self.requested_ms)
                ))
            }
            None => None,
        };
        HealthSync {
            at_ms: self.at_ms,
            sessions_new: self.sessions_new,
            sessions_appended: self.sessions_appended,
            messages: self.messages,
            rejected: self.rejected,
            people_usd: self.people_usd,
            next_ms,
            error: self.error.clone(),
            warn,
            requested_ms: self.requested_ms,
        }
    }
}

/// `rec`'s rejections as the stored state keeps them: a count, and the first
/// few worded.
fn samples(rec: &ImportSyncRecord) -> Vec<String> {
    rec.rejected
        .iter()
        .take(SAMPLES)
        .map(|x| match &x.episode_id {
            Some(id) => format!("{id}: {}", x.why),
            None => x.why.clone(),
        })
        .collect()
}

impl Core {
    /// `import.sync.plan`: the config, the cursor, and the first run's start.
    pub fn sync_plan(&self) -> Result<ImportSyncPlan> {
        let cfg = &self.runner.cfg;
        let o = &cfg.import.openclaw;
        let state: Option<SyncState> = self.store.get_meta(STATE_KEY)?;
        let cursor = state.as_ref().and_then(|s| s.cursor.clone());
        let since_ms = match cursor {
            None => newest_end(&self.store, OLD_SOURCE)?,
            Some(_) => None,
        };
        Ok(ImportSyncPlan {
            configured: o.state_dir.is_some(),
            state_dir: o.state_dir.clone(),
            owner_ids: o.owner_ids.clone(),
            agents: o.agents.clone(),
            cursor,
            since_ms,
            learning_hour: cfg.judge.learning_hour,
            requested_ms: state.as_ref().map_or(0, |s| s.requested_ms),
            requested_by: state.as_ref().and_then(|s| s.requested_by.clone()),
            people_todo: state.map_or(0, |s| s.people_todo.len() as u64),
        })
    }

    /// `import.sync.request` (theseus-ievo3): record that a sync is wanted now
    /// and answer with the effective request's time. **It starts no run** --
    /// the reading is the CLI's (`scripts/openclaw-export.py`), so what the
    /// daemon can do on demand is say that a run is wanted; the next
    /// `theseus import sync --if-requested` serves it.
    ///
    /// A second request while one waits does not move the first: the wait that
    /// matters is the oldest one, and moving it would hide a request nothing
    /// served from [`SyncState::health`]'s warning.
    pub(crate) async fn sync_request(&self, by: &str) -> Result<u64> {
        let prev: SyncState =
            theseus_store::blocking(|| self.store.get_meta(STATE_KEY))?.unwrap_or_default();
        if prev.requested_ms > 0 {
            return Ok(prev.requested_ms);
        }
        let at = theseus_protocol::now_unix_ms();
        let state = SyncState {
            requested_ms: at,
            requested_by: Some(by.to_string()),
            ..prev
        };
        self.people_write(&[crate::judge::people::run::meta(STATE_KEY, &state)?])
            .await?;
        Ok(at)
    }

    /// Health's `sync`: the last run, once one has run.
    pub fn sync_health(&self, now_ms: u64) -> Option<HealthSync> {
        let state: SyncState = self.store.get_meta(STATE_KEY).ok().flatten()?;
        Some(state.health(now_ms, self.runner.cfg.judge.learning_hour))
    }

    /// The first frame of `import.sync.record`: the new state and the run's
    /// row, in one frame. The state before it is returned with the new one.
    pub(crate) async fn sync_write(
        &self,
        rec: &ImportSyncRecord,
        by: &str,
    ) -> Result<(SyncState, Vec<String>)> {
        let prev: SyncState =
            theseus_store::blocking(|| self.store.get_meta(STATE_KEY))?.unwrap_or_default();
        let mut todo = prev.people_todo.clone();
        for t in &rec.touched {
            if !todo.contains(&t.session_id) {
                todo.push(t.session_id.clone());
            }
        }
        let now = theseus_protocol::now_unix_ms();
        let state = SyncState {
            // A failed run leaves the cursor where it was.
            cursor: match rec.error {
                Some(_) => prev.cursor.clone(),
                None => rec.cursor.clone().or(prev.cursor.clone()),
            },
            at_ms: now,
            started_ms: rec.started_ms,
            sessions_new: rec.imported,
            sessions_appended: rec.appended,
            messages: rec.messages,
            rejected: rec.rejected.len() as u64,
            people_usd: 0.0,
            error: rec.error.clone(),
            samples: samples(rec),
            people_todo: todo.clone(),
            // A run that did not fail has read everything up to its cursor, so
            // it served whatever was asked for; one that failed imported
            // nothing, so the request stands, as the cursor does.
            requested_ms: match rec.error {
                Some(_) => prev.requested_ms,
                None => 0,
            },
            requested_by: match rec.error {
                Some(_) => prev.requested_by.clone(),
                None => None,
            },
        };
        let row = crate::fact::import::ImportSynced {
            by,
            started_ms: rec.started_ms,
            episodes: rec.episodes,
            messages: rec.messages,
            imported: rec.imported,
            appended: rec.appended,
            skipped: rec.skipped,
            rejected: state.rejected,
            error: rec.error.as_deref(),
        };
        self.people_write(&[
            crate::judge::people::run::meta(STATE_KEY, &state)?,
            crate::fact::row(&row, None, None)?.scoped(&crate::fact::import::sync_scope()),
        ])
        .await?;
        self.rec(None).announce(&row);
        Ok((state, todo))
    }
}
