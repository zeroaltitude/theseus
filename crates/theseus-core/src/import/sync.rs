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
//! - **A sync on demand** (theseus-ievo3, S29-1). A decision recorded in
//!   OpenClaw was readable here only at the next night's run, so for most of a
//!   working day Theseus acted on yesterday's picture. `import.sync.request`
//!   **runs a sync then and there**: the daemon starts its own
//!   `theseus import sync --no-people` as a registered child
//!   ([`crate::import::sync_run`]) and tells the thread it started, then what
//!   it imported. One run at a time; a second request while one goes is
//!   answered "already running". The people step stays nightly -- the argv
//!   carries `--no-people` -- so no number of requests can multiply its cost.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::import_sync::{
    HealthSync, ImportSyncPlan, ImportSyncRecord, ImportSyncStarted, MISSED_AFTER_MS, SAMPLES,
    SYNC_MINUTE,
};

use super::sync_run;

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

    /// `import.sync.request` (theseus-ievo3, S29-1): **run a sync now.**
    ///
    /// The owner's words, 2026-11-13 14:01:14 MST (Discord message
    /// 1570906412190540072): *"How about when the request comes in we just
    /// execute it"*. So this starts the run rather than recording a wish for
    /// a poller, and answers [`ImportSyncStarted`]: that it started, or that
    /// one was already going.
    ///
    /// The whole command is [`crate::import::sync_run`]'s constant and the
    /// child is registered with the daemon's children, so the census holds.
    /// `by` names who asked, for the daemon's log; **it reaches no argument,
    /// no environment variable and no working directory of the child.**
    ///
    /// The second message -- what the run imported -- is posted to the
    /// operator when the child ends (`sync_ran_notice`).
    pub(crate) fn sync_request(self: &Arc<Self>, by: &str) -> ImportSyncStarted {
        self.sync_request_for(None, by)
    }

    /// `sync_request`, saying where the ask came from so the **second** line —
    /// what the run imported — goes back to the conversation that asked
    /// (chat-acts-are-tools, S43-1, `chat.import_sync`). `None` is the RPC's
    /// and the CLI's, where the notice goes where approvals go, as it always
    /// did. The first line is the tool's own result, which the model relays in
    /// the thread it was asked in.
    pub(crate) fn sync_request_for(
        self: &Arc<Self>,
        asked_in: Option<&str>,
        by: &str,
    ) -> ImportSyncStarted {
        let asked_in = asked_in.map(str::to_string);
        let now = theseus_protocol::now_unix_ms();
        match self.on_demand.claim(now) {
            // Already going: nothing is started, nothing is queued, and the
            // answer is a different sentence -- never a quiet success.
            sync_run::Asked::AlreadyRunning(at_ms) => {
                tracing::info!(by, since_ms = at_ms, "a sync was asked for while one runs");
                ImportSyncStarted {
                    started: false,
                    at_ms,
                    say: sync_run::already_running_line(at_ms, now),
                }
            }
            sync_run::Asked::Started(at_ms) => {
                tracing::info!(by, "a sync was asked for: starting one");
                // The state as it is now, so the run's own counts are the
                // difference and not the last run's totals.
                let before: SyncState = self
                    .store
                    .get_meta(STATE_KEY)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let core = self.clone();
                tokio::spawn(async move {
                    let ran = core.on_demand.run_claimed().await;
                    core.sync_ran_notice(asked_in.as_deref(), &ran, &before);
                });
                ImportSyncStarted {
                    started: true,
                    at_ms,
                    say: sync_run::started_line(),
                }
            }
        }
    }

    /// The second message a request gets: what the run imported, read from the
    /// state the run itself recorded. It goes where approvals go, which is the
    /// owner's own DM, so the thread that asked is told without this path
    /// needing a route of its own.
    fn sync_ran_notice(&self, asked_in: Option<&str>, ran: &sync_run::Ran, before: &SyncState) {
        let after: SyncState = self
            .store
            .get_meta(STATE_KEY)
            .ok()
            .flatten()
            .unwrap_or_default();
        let text = sync_run::finished_line(ran, before, &after);
        if !ran.ok() {
            tracing::warn!(text, "the on-demand sync did not work");
        }
        let body = serde_json::json!({"kind": "notice", "text": text});
        // A sync someone asked for IN A CONVERSATION is answered there: the
        // tool's own result said it started, and this says what it imported,
        // in the same place (chat-acts-are-tools, S43-1, `chat.import_sync`).
        // `post_for` gives `None` for a session with no place of its own --
        // the CLI's, the web UI's -- and that, and every request that named no
        // conversation at all, goes where approvals go, exactly as before.
        let posted = match asked_in {
            Some(sid) => self.outbox.post_for(sid, "", body.clone()),
            None => Ok(None),
        };
        match posted {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %format!("{e:#}"),
                "the sync's report could not be posted where it was asked for; saying it where approvals go"),
        }
        if let Err(e) = self.outbox.to_operator(asked_in, body) {
            tracing::warn!(error = %format!("{e:#}"), "the sync's report could not be posted");
        }
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
