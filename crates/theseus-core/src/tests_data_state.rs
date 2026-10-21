//! A session's data state (theseus-a1wvq.5; `crate::data::DataState`,
//! `SessionRecord.data`): structure only. Nothing opens a session or computes
//! a run yet, so these tests build the state directly: the turn's write-back
//! merges it (a stored taint always wins, a run another writer moved stays,
//! `opened` is never cleared), a task never inherits its parent's run, its
//! ledger rows, and its inspect line. A fork's is `tests_fork`'s.

use theseus_protocol::SessionKind;

use crate::data::{DataState, DataWhy, OpenHow, OpenRun, Opened, Taint};
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;

fn run(from: &str, through: &str) -> OpenRun {
    OpenRun {
        from: from.into(),
        through: through.into(),
    }
}

fn taint() -> Taint {
    Taint {
        why: DataWhy::Attachment,
        node: Some("nd_attached".into()),
        at_ms: 1_790_000_000_005,
    }
}

/// A record as the store gives it back: the turn's copy is read, at its
/// start, from these bytes, and so is the stored record at the write-back.
fn read(rec: &SessionRecord) -> SessionRecord {
    serde_json::from_str(&serde_json::to_string(rec).unwrap()).unwrap()
}

fn with(data: DataState) -> SessionRecord {
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.data = Some(Box::new(data));
    rec
}

fn data(rec: &SessionRecord) -> DataState {
    rec.data.as_deref().cloned().unwrap_or_default()
}

/// The key test: a stored taint survives a turn that read none; a run
/// another writer moved after the turn started is not replaced by the turn's
/// stale one; an unchanged run takes the turn's; `opened` is never cleared.
#[test]
fn turn_write_back_never_erases_a_stored_taint_or_moves_a_changed_run() {
    let opened = Opened {
        how: OpenHow::Draft,
        at_ms: 1_790_000_000_001,
    };
    // A taint and an opening written while the turn ran, which read none.
    let start = SessionRecord::new(SessionKind::Conversation, None);
    let turn = read(&start);
    let mut stored = read(&start);
    stored.data = Some(Box::new(DataState::new(
        Some(opened.clone()),
        None,
        Some(taint()),
    )));
    let mut stored = read(&stored);
    stored.take_turns_fields(&turn);
    assert_eq!(data(&stored).taint, Some(taint()), "the stored taint stays");
    assert_eq!(
        data(&stored).opened,
        Some(opened),
        "opened is never cleared"
    );

    // The turn read run A; another writer moved it to B before the
    // write-back: B stays.
    let start = with(DataState::new(None, Some(run("nd_a", "nd_b")), None));
    let turn = read(&start);
    let mut stored = read(&start);
    stored.data.as_mut().unwrap().run = Some(run("nd_a", "nd_d"));
    let mut stored = read(&stored);
    stored.take_turns_fields(&turn);
    assert_eq!(
        data(&stored).run,
        Some(run("nd_a", "nd_d")),
        "the moved run stays"
    );

    // Unchanged since the turn read it: the turn's run is taken.
    let mut turn = read(&start);
    turn.data.as_mut().unwrap().run = Some(run("nd_a", "nd_e"));
    let mut stored = read(&start);
    stored.take_turns_fields(&turn);
    assert_eq!(data(&stored).run, Some(run("nd_a", "nd_e")));
}

/// Through a whole turn: a session's stored state is the same after a plain
/// turn, which writes nothing new of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plain_turn_keeps_the_sessions_data_state() {
    let r = crate::tests_route::rig(None, 1, |_| {});
    let state = DataState::new(None, Some(run("nd_a", "nd_b")), Some(taint()));
    let rec = with(state.clone());
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let res = crate::tests_route::turn(&r.core, Some(&rec.session_id), "Say done.", None).await;
    let after = crate::tests_route::session(&r.core, &res.session_id);
    assert_eq!(after.turns, 1, "the turn ran");
    assert_eq!(data(&after), state);
}

/// A task's record starts with no run, whatever its parent's, and the task's
/// report leaves the parent's run as it was.
#[tokio::test]
async fn a_task_never_inherits_its_parents_run() {
    use theseus_protocol::task_start::TaskStartParams;
    let r = crate::tests_tasks::rig(
        |_| crate::provider::Scripted::text("Counted 14 boats."),
        |_| {},
    );
    let home = r.core.home().unwrap();
    let mut parent: SessionRecord = r.core.store.get_session(&home.session_id).unwrap().unwrap();
    let state = DataState::new(None, Some(run("nd_quay", "nd_count")), None);
    parent.data = Some(Box::new(state.clone()));
    r.core
        .store
        .put_session(&parent.session_id, &parent)
        .unwrap();
    let answerer = crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    };
    let params = TaskStartParams {
        brief: "Count the boats in the north quay".into(),
        ..Default::default()
    };
    let started = r.core.task_start(&params, answerer).unwrap();
    let task_exec = started.session_id.replacen("ses_", "exe_", 1);
    crate::test_support::until("the task's report", || {
        crate::tests_tasks::exec(&r.core, &task_exec).state == theseus_kernel::ExecState::Complete
    })
    .await;
    let task: SessionRecord = r
        .core
        .store
        .get_session(&started.session_id)
        .unwrap()
        .unwrap();
    assert!(task.data.is_none(), "{:?}", task.data);
    let parent: SessionRecord = r.core.store.get_session(&home.session_id).unwrap().unwrap();
    assert!(crate::tests_tasks::exec(&r.core, &home.execution_id)
        .reports
        .contains(&task_exec));
    assert_eq!(
        data(&parent),
        state,
        "the report changed nothing of the parent's run"
    );
}

/// `session.data`'s two rows, as records and back.
#[test]
fn session_data_ledger_rows_round_trip() {
    let sid = "ses_00000000000000000000000000000501";
    let opened = crate::data::opened_row(sid, OpenHow::Task).unwrap();
    let tainted = crate::data::tainted_row(sid, &taint()).unwrap();
    let decode = |r: &theseus_store::NewRecord| -> LedgerRow {
        assert_eq!(r.kind, theseus_store::kinds::LEDGER);
        serde_json::from_slice(&r.payload).unwrap()
    };
    let (o, t) = (decode(&opened), decode(&tainted));
    assert_eq!(
        (o.kind.as_str(), o.session_id.as_deref()),
        ("session.data", Some(sid))
    );
    assert_eq!(o.data["row"], "opened");
    let how: OpenHow = serde_json::from_value(o.data["how"].clone()).unwrap();
    assert_eq!(how, OpenHow::Task);
    assert_eq!(
        (t.kind.as_str(), t.data["row"].as_str()),
        ("session.data", Some("tainted"))
    );
    let why: DataWhy = serde_json::from_value(t.data["why"].clone()).unwrap();
    assert_eq!(
        (why, t.data["node"].as_str()),
        (DataWhy::Attachment, Some("nd_attached"))
    );
}

/// The state's line, which `turn.inspect` shows a private place.
#[test]
fn the_inspect_line_names_the_opening_the_runs_ends_and_the_taint() {
    let state = DataState::new(
        Some(Opened {
            how: OpenHow::Task,
            at_ms: 1_790_000_000_001,
        }),
        Some(run("nd_a", "nd_b")),
        Some(taint()),
    );
    assert_eq!(
        state.line(),
        "opened (task) at 1790000000001; open run nd_a to nd_b; tainted (attachment) at nd_attached"
    );
    assert_eq!(DataState::default().line(), "not opened; no open run");
}

/// A fork's cut, by positions: `from` before the point gives `from..=` the
/// point (or its own `through`, when earlier); `from` after it gives none; a
/// node with no position cuts the run.
#[test]
fn a_forks_run_is_cut_at_the_fork_point() {
    let at = |id: &str| match id {
        "nd_a" => Some(10),
        "nd_b" => Some(20),
        "nd_c" => Some(30),
        _ => None,
    };
    let source = |r: OpenRun| DataState::new(None, Some(r), Some(taint()));
    let cut = |s: &DataState, p: (u64, &str)| crate::fork::data_through(Some(s), p, at).map(|d| *d);
    let s = source(run("nd_a", "nd_c"));
    assert_eq!(
        cut(&s, (20, "nd_b")).unwrap().run,
        Some(run("nd_a", "nd_b"))
    );
    assert_eq!(
        cut(&s, (10, "nd_a")).unwrap().run,
        Some(run("nd_a", "nd_a"))
    );
    assert_eq!(
        cut(&s, (40, "nd_d")).unwrap().run,
        Some(run("nd_a", "nd_c"))
    );
    let late = cut(&s, (5, "nd_z")).unwrap();
    assert_eq!((late.run, late.taint), (None, Some(taint())));
    let lost = source(run("nd_gone", "nd_c"));
    assert_eq!(cut(&lost, (40, "nd_d")).unwrap().run, None, "fail closed");
    assert!(crate::fork::data_through(None, (40, "nd_d"), at).is_none());
}
