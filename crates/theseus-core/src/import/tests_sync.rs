//! The OpenClaw sync's daemon half (theseus-pjmj7): the plan (the config, the
//! cursor, the first run's start: the newest end of the old import's
//! `openclaw-store` sessions), the record (the cursor, the counts and the
//! row in one frame; a failed run's cursor stays; the people step's list and
//! its handles), health's `sync` and its warnings (goldens), and the owner's
//! act from a private place alone.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::import_sync::{
    ImportSyncPlan, ImportSyncRecord, ImportSyncResult, MISSED_AFTER_MS,
};
use theseus_protocol::method;

use super::sync::{next_run, SyncState, STATE_KEY};
use super::tests::{call, call_as, fixture, import, lines, live};
use super::tests_append::{sync, TAG};
use super::{episode, session_id_of};
use crate::approval::{Client, Surface};
use crate::config::Config;
use crate::Core;

async fn plan(c: &Arc<Core>) -> ImportSyncPlan {
    serde_json::from_value(
        call(c, method::IMPORT_SYNC_PLAN, Value::Null)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn record(c: &Arc<Core>, rec: &ImportSyncRecord) -> ImportSyncResult {
    serde_json::from_value(
        call(
            c,
            method::IMPORT_SYNC_RECORD,
            serde_json::to_value(rec).unwrap(),
        )
        .await
        .unwrap(),
    )
    .unwrap()
}

/// The first run's start is the newest end of the old import's
/// `openclaw-store` sessions alone; with none there is none, and a cursor
/// replaces it.
#[tokio::test]
async fn the_first_runs_start_is_the_old_imports_newest_store_session() {
    let r = live();
    let c = &r.core;
    let p = plan(c).await;
    assert_eq!(
        (p.configured, p.since_ms, p.cursor.is_none()),
        (false, None, true)
    );
    // The fixture holds one session of each source: its openclaw-store one
    // (the first) ends 2026-05-02T15:40:02Z; the others end later.
    let eps = fixture();
    import(c, lines(&eps)).await;
    let want = episode::parse(&serde_json::to_string(&eps[0]).unwrap())
        .unwrap()
        .as_of
        .end_ms;
    let latest_of_any = eps
        .iter()
        .map(|e| {
            episode::parse(&serde_json::to_string(e).unwrap())
                .unwrap()
                .as_of
                .end_ms
        })
        .max()
        .unwrap();
    assert!(latest_of_any > want, "the other sources end later");
    assert_eq!(plan(c).await.since_ms, Some(want));
    // A sync session of a later end does not move it: only openclaw-store counts.
    import(c, lines(&[sync(1, TAG, &[0, 1, 2])])).await;
    assert_eq!(plan(c).await.since_ms, Some(want));
    // With a cursor, the start is the cursor's.
    record(
        c,
        &ImportSyncRecord {
            cursor: Some(json!({"version": 1, "floor_ms": want, "agents": {}})),
            ..ImportSyncRecord::default()
        },
    )
    .await;
    let p = plan(c).await;
    assert_eq!((p.since_ms, p.cursor.is_some()), (None, true));
}

#[tokio::test]
async fn the_plan_carries_the_owners_config_and_the_cursor() {
    let r = crate::tests_recall::rig_with(crate::config::MemoryMode::Off, |c: &mut Config| {
        c.import.openclaw.state_dir = Some("/srv/reef-claw".into());
        c.import.openclaw.owner_ids = vec!["U-WREN".into()];
        c.import.openclaw.agents = vec!["main".into()];
    });
    let p = plan(&r.core).await;
    assert_eq!(p.state_dir.as_deref(), Some("/srv/reef-claw"));
    assert_eq!(
        (p.owner_ids, p.agents, p.learning_hour),
        (vec!["U-WREN".to_string()], vec!["main".to_string()], 3)
    );
    assert!(p.configured);
}

/// A run's end: the cursor, the counts and the row in one frame; health
/// names the run and the next one; a failed run leaves the cursor and
/// warns; the sessions it touched wait for the people step.
#[tokio::test]
async fn a_record_writes_the_cursor_the_counts_and_the_row_and_health_names_them() {
    let r = live();
    let c = &r.core;
    assert!(c.health().imported.sync.is_none(), "no run, no line");
    let e = sync(2, TAG, &[0, 1, 2, 3]);
    let got = import(c, lines(std::slice::from_ref(&e))).await;
    assert_eq!(got.touched.len(), 1);
    assert_eq!(got.touched[0].tag, TAG);
    let sid = session_id_of(e["episode_id"].as_str().unwrap());
    assert_eq!(got.touched[0].session_id, sid);
    let frames = c.store.stats().unwrap().frames_appended;
    let cursor = json!({"version": 1, "floor_ms": 5, "agents": {"main": {"events_rowid": 9}}});
    let res = record(
        c,
        &ImportSyncRecord {
            started_ms: 7,
            cursor: Some(cursor.clone()),
            episodes: 1,
            messages: 4,
            imported: 1,
            skipped: 0,
            touched: got.touched.clone(),
            no_people: true,
            ..ImportSyncRecord::default()
        },
    )
    .await;
    assert_eq!(
        c.store.stats().unwrap().frames_appended,
        frames + 1,
        "the cursor, the counts and the row are one frame"
    );
    assert_eq!(
        (
            res.sync.sessions_new,
            res.sync.sessions_appended,
            res.sync.messages
        ),
        (1, 0, 4)
    );
    assert!(res.sync.warn.is_none() && res.sync.error.is_none());
    assert!(res.sync.next_ms > res.sync.at_ms);
    assert!(res.people.is_none(), "no_people: no step");
    let p = plan(c).await;
    assert_eq!(p.cursor, Some(cursor.clone()));
    assert_eq!(
        p.people_todo, 1,
        "the touched session waits for its people step"
    );
    let h = c.health().imported.sync.expect("health names the run");
    assert_eq!((h.sessions_new, h.messages, h.rejected), (1, 4, 0));
    let row = c
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(50)
        .unwrap()
        .into_iter()
        .map(|(_, x)| x)
        .find(|x| x.kind == "import.synced")
        .expect("the run's row");
    assert_eq!(row.data["episodes"], 1);
    assert_eq!(row.data["imported"], 1);

    // A failed run: the cursor stays, and health says why.
    let res = record(
        c,
        &ImportSyncRecord {
            cursor: Some(
                json!({"version": 1, "floor_ms": 5, "agents": {"main": {"events_rowid": 99}}}),
            ),
            error: Some("python3 did not start".into()),
            no_people: true,
            ..ImportSyncRecord::default()
        },
    )
    .await;
    assert_eq!(plan(c).await.cursor, Some(cursor));
    assert_eq!(
        res.sync.warn.as_deref(),
        Some("the last sync failed: python3 did not start")
    );
    assert_eq!(
        c.health().imported.sync.unwrap().error.as_deref(),
        Some("python3 did not start")
    );
    assert_eq!(plan(c).await.people_todo, 1, "still waiting");
}

/// A run with the people step on, with the people extractor off in this core
/// (no Jev): the touched tags' handles are made from the stored records,
/// and the sessions stay in the list for the next run.
#[tokio::test]
async fn the_people_step_makes_the_touched_tags_handles_and_keeps_what_it_could_not_read() {
    let r = live();
    let c = &r.core;
    let e = sync(3, TAG, &[0, 1, 2, 3]);
    let got = import(c, lines(std::slice::from_ref(&e))).await;
    let res = record(
        c,
        &ImportSyncRecord {
            episodes: 1,
            messages: 4,
            imported: 1,
            touched: got.touched,
            ..ImportSyncRecord::default()
        },
    )
    .await;
    let p = res.people.expect("the step ran");
    assert_eq!(p.tags, vec![TAG.to_string()]);
    assert!(p.declared >= 1, "Marlo Quill spoke in a session: {p:?}");
    assert!(p.stopped.is_some(), "no Jev here: {p:?}");
    assert_eq!(p.left, 1);
    assert_eq!(plan(c).await.people_todo, 1);
}

/// What health shows of a run, a failed night and a missed one: goldens.
#[test]
fn health_words_the_run_and_a_failed_or_missed_night() {
    let st = SyncState {
        at_ms: 1_790_000_000_000,
        sessions_new: 3,
        sessions_appended: 12,
        messages: 148,
        rejected: 1,
        people_usd: 0.31,
        ..SyncState::default()
    };
    let next = next_run(st.at_ms, 3);
    assert!(next > st.at_ms && next - st.at_ms <= 24 * 3_600_000 + 3_600_000);
    let h = st.health(st.at_ms + 3_600_000, 3);
    assert_eq!(
        (h.next_ms, h.warn.clone(), h.people_usd),
        (next, None, 0.31)
    );
    // Missed: the next run's time and the grace have passed.
    let late = st.health(next + MISSED_AFTER_MS + 1, 3);
    let w = late.warn.expect("a missed night warns");
    assert!(
        w.starts_with("a night's sync was missed: it was due 20"),
        "{w}"
    );
    assert!(w.ends_with("03:40 and has not run"), "{w}");
    // Within the grace: not yet.
    assert!(st.health(next + MISSED_AFTER_MS - 1, 3).warn.is_none());
    // Failed: the reason, and it outranks a miss.
    let failed = SyncState {
        error: Some("an agent's store would not open".into()),
        ..st
    };
    assert_eq!(
        failed
            .health(next + 10 * MISSED_AFTER_MS, 3)
            .warn
            .as_deref(),
        Some("the last sync failed: an agent's store would not open")
    );
}

/// The plan and the record are the owner's acts: a place that is not private
/// is refused, and nothing is written.
#[tokio::test]
async fn the_sync_is_the_owners_act_from_a_private_place() {
    let r = live();
    let c = &r.core;
    for (m, params) in [
        (method::IMPORT_SYNC_PLAN, Value::Null),
        (
            method::IMPORT_SYNC_RECORD,
            serde_json::to_value(ImportSyncRecord::default()).unwrap(),
        ),
    ] {
        let shared = Client::new("test", Surface::Unnamed);
        let e = call_as(c, shared, m, params).await.unwrap_err();
        assert_eq!(e.0, theseus_protocol::error_code::REFUSED, "{m}: {e:?}");
    }
    assert!(c.store.get_meta::<SyncState>(STATE_KEY).unwrap().is_none());
}
