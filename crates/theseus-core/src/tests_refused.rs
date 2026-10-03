//! One corrupt record no longer stops every continuation (Review 2's R4,
//! theseus-15g). Before, once the history check found a frame corrupt, a
//! read of any of its records was refused, and every list read that reached
//! one failed whole: the driver's `open_executions` among them, which it
//! skipped without a word on every tick, so no continuation and no wake ran
//! for any session. Health's session totals read as nothing, too.
//!
//! The store's list reads now skip a refused record and count it, health
//! shows the count, and the driver logs a failure it cannot get past.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_kernel::{ExecState, Execution};
use theseus_protocol::{SessionKind, TurnSubmitResult};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

fn config(state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    let root = state.join("work");
    std::fs::create_dir_all(&root).unwrap();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg
}

fn core(state: &Path, script: Vec<Scripted>) -> (Arc<Core>, Arc<FakeProvider>) {
    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(
        config(state),
        fake.clone(),
        store,
    ))
    .unwrap();
    (core, fake)
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
            from_discord: false,
        })
        .await
        .unwrap()
}

/// A session that has taken one turn, and its execution's id.
async fn session(core: &Arc<Core>) -> (String, String) {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    turn(core, &rec.session_id, "hello").await;
    let rec: SessionRecord = core.store.get_session(&rec.session_id).unwrap().unwrap();
    (rec.session_id, rec.execution_id.unwrap())
}

/// Flip a bit of the frame that holds `position`, in the store at `dir`: its
/// crc no longer checks, and its record still decodes, its JSON too (the
/// last digit in it moves by one). A record read checks no crc, so until the
/// history check finds the frame, the start reads the record as it is.
fn corrupt_frame_of(dir: &Path, position: u64) {
    use theseus_store::wal::{decode_record, list_segments, segment_path, FRAME_HEADER};
    let wal = dir.join("wal");
    for seg in list_segments(&wal).unwrap() {
        let path = segment_path(&wal, seg);
        let mut b = std::fs::read(&path).unwrap();
        let mut off = 0;
        while off + FRAME_HEADER <= b.len() {
            let len = u32::from_le_bytes(b[off + 4..off + 8].try_into().unwrap()) as usize;
            let body = off + FRAME_HEADER;
            let (first, _) = decode_record(&b[body..body + len], 4).unwrap();
            if first.position == position {
                let digit = (body..body + len)
                    .rev()
                    .find(|&i| b[i].is_ascii_digit())
                    .unwrap();
                b[digit] ^= 0x01;
                std::fs::write(&path, &b).unwrap();
                return;
            }
            off = body + len;
        }
    }
    panic!("no frame holds position {position}");
}

/// Two sessions, each waiting for the driver. The first's execution record,
/// rewritten as it stands in a frame of its own, lands in the history (before
/// the index's checkpoint), and that frame goes bad. After a restart and the
/// check after serving, the driver's list of runnable executions reaches the
/// bad record: it skips it and counts it, and still runs the second session's
/// continuation. Health shows the count, and a read of that execution alone is
/// still refused. (Since lv2, health counts executions from the index's
/// projection, without reading them, and the driver reads only the runnable
/// ones; a corrupt record of a parked execution no longer reaches either.)
#[tokio::test]
async fn a_corrupt_record_skips_its_execution_and_the_driver_continues_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path();
    let (bad_exec, (good_sid, good_exec), position) = {
        let (core, _) = core(state, vec![Scripted::text("Hi."), Scripted::text("Hi.")]);
        let (_, bad_exec) = session(&core).await;
        let good = session(&core).await;
        // Both wait for the driver, which reads the runnable ones.
        core.kernel.wake(&bad_exec, "test").unwrap();
        core.kernel.wake(&good.1, "test").unwrap();
        let e: Execution = core.kernel.execution(&bad_exec).unwrap().unwrap();
        let rec = NewRecord::json(kinds::EXECUTION, Some(&e.id), &e)
            .unwrap()
            .scoped(&e.session_id);
        let position = core.store.inner().append(&[rec]).unwrap()[0];
        core.store.checkpoint().unwrap();
        // One more frame: the open checks from the frame after the
        // checkpoint, so the bad one is history.
        core.store
            .append_ledger(&crate::ledger::LedgerRow::named(
                "test.after",
                None,
                None,
                serde_json::json!({}),
            ))
            .unwrap();
        (bad_exec, good, position)
    };
    corrupt_frame_of(&state.join("store"), position);

    let (core, fake) = core(state, vec![Scripted::text("Still here.")]);
    core.check_store_history()
        .expect("the check runs")
        .join()
        .unwrap();
    let verify = core
        .startup_log
        .snapshot()
        .into_iter()
        .find(|p| p.name == "store.verify")
        .unwrap();
    assert_eq!(verify.detail["outcome"], "corrupt", "{verify:?}");
    let h = core.health();
    assert_eq!(h.sessions, 2, "the sessions are whole");
    assert!(
        core.kernel.execution(&bad_exec).is_err(),
        "a read of it alone is refused"
    );

    // The driver continues the other session: a turn with no input, which
    // has nothing new to read, so it calls no model, and ends.
    tokio::spawn(crate::harness::drive(core.clone()));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let e = core.kernel.execution(&good_exec).unwrap().unwrap();
        let rec: SessionRecord = core.store.get_session(&good_sid).unwrap().unwrap();
        if e.state == ExecState::Waiting && rec.turns == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the driver never ran the continuation: {e:?}, {} turns",
            rec.turns
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let started: Vec<crate::ledger::LedgerRow> = core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(500)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == "turn.started" && r.session_id.as_deref() == Some(good_sid.as_str()))
        .collect();
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(started[1].data["continuation"], true, "the driver's turn");
    assert!(
        fake.requests().is_empty(),
        "no model call after the restart"
    );
    let h = core.health();
    assert_eq!(
        (h.store.refused_records, h.store.refused_positions.clone()),
        (1, vec![position]),
        "the driver's list skipped it, counted once: {:?}",
        h.store
    );
    assert!(h.store.repair.is_some(), "health says what repairs it");
}
