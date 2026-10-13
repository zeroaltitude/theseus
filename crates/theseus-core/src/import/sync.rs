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

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::import_sync::{
    HealthSync, ImportSyncPlan, ImportSyncRecord, MISSED_AFTER_MS, SAMPLES, SYNC_MINUTE,
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
            None if now_ms > next_ms + MISSED_AFTER_MS => Some(format!(
                "a night's sync was missed: it was due {} {hour:02}:{SYNC_MINUTE:02} and has not run",
                crate::judge::spend::local_day(next_ms)
            )),
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
            people_todo: state.map_or(0, |s| s.people_todo.len() as u64),
        })
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
