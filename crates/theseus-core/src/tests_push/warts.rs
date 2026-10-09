//! The push's warts (theseus-q5af): what a surface showed stale, each from a
//! live run on a scratch daemon. A frame that changed only `outstanding`
//! sent nothing, so a cancelled call's verified stop left its execution at
//! `outstanding 1` for good, and a job dispatched mid-turn sent no view; a
//! live question's `expires_at_ms` was its ask's time and the TTL, the lists'
//! its plan's; `confirm.list` left a question out until its turn parked,
//! though the board showed it; and `session.open` wrote the session's record
//! a frame after its execution, so a watcher that asked for it at the first
//! view could find none.

use super::*;

use crate::config::WebToolsConfig;
use crate::policy::Posture;
use crate::provider::{DeltaSink, Provider, ProviderFuture, ProviderRequest};
use crate::web::tests::{serve, web};
use theseus_store::Store as _;

/// A stand-in model that fetches the page first and, once a result has come
/// back, answers (as tests_cancel's).
struct Fetcher {
    url: String,
}

impl Provider for Fetcher {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let answered = req.messages.iter().any(|m| {
                m["content"]
                    .as_array()
                    .is_some_and(|b| b.iter().any(|b| b["type"] == "tool_result"))
            });
            let next = if answered {
                Scripted::text("done")
            } else {
                Scripted::tools("", &[("tu_hang", "http_fetch", json!({ "url": self.url }))])
            };
            FakeProvider::scripted(vec![next])
                .stream_message(req, on_delta)
                .await
        })
    }
}

/// A turn of `rec` with `input`, on a task of its own: its error, as a
/// cancelled turn's, is said, not a panic.
fn spawn_turn(
    core: &Arc<Core>,
    rec: SessionRecord,
    input: &str,
) -> tokio::task::JoinHandle<Result<TurnSubmitResult, String>> {
    let core = core.clone();
    let input = input.to_string();
    tokio::spawn(async move {
        let (live, _) = core.live_profile();
        let target = core.runner.resolve_target(&live, None, None, None).unwrap();
        let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
        core.runner
            .run(TurnRequest {
                prompt: None,
                session: rec,
                input: Some(input),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
            })
            .await
            .map_err(|e| format!("{e:?}"))
    })
}

/// A new conversation's record, stored.
fn new_session(core: &Core) -> SessionRecord {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec
}

/// Until the board has applied every kernel record written so far.
async fn settled(core: &Arc<Core>) {
    let last = last_kernel_position(core);
    until("the board applied every frame", || {
        core.push.status(0).position >= last
    })
    .await;
}

/// A cancel's verified stop settles the call it stopped in a frame that
/// changes nothing of its execution but `outstanding`: the board sends that
/// view, so the cancelled execution's last view reads `outstanding` 0, as
/// its record does. Before, `same()` did not compare it and the view stayed
/// at 1 on every surface.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancels_verified_stop_sends_a_view_with_nothing_outstanding() {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Fetcher {
        url: format!("http://site.test:{}/hang", server.port),
    });
    let core = Core::build(crate::rpc::Parts {
        toollets: web(server.port, WebToolsConfig::default(), true).tools(),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    })
    .unwrap();
    let seen = watcher(&core).await;
    until("the board is seeded", || core.push.seeded()).await;
    let turn = spawn_turn(&core, new_session(&core), "fetch the page");
    let mut fetch = None;
    until("a fetch in flight", || {
        fetch =
            core.kernel.actions().unwrap().into_iter().find(|a| {
                a.tool == "http.fetch" && a.state == theseus_kernel::ActionState::Dispatched
            });
        fetch.is_some()
    })
    .await;
    let exec = fetch.unwrap().execution_id;
    let (_, _, verdicts) = core.cancel_execution_judged(&exec, "test").await.unwrap();
    assert_eq!(verdicts[0].state.as_str(), "termination_verified");
    let _ = tokio::time::timeout(Duration::from_secs(10), turn)
        .await
        .expect("the turn ended");
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(e.state.as_str(), "cancelled");
    assert!(e.outstanding.is_empty(), "{:?}", e.outstanding);
    settled(&core).await;
    let board = core.push.view_of_session(&e.session_id).unwrap();
    assert_eq!(
        (board.state.as_str(), board.outstanding),
        ("cancelled", 0),
        "the board's view: {board:?}"
    );
    let last = board.position;
    until("the watcher heard the last view", || {
        applied(&seen.lock().unwrap())
            .get(&exec)
            .is_some_and(|v| v.position >= last)
    })
    .await;
    let v = applied(&seen.lock().unwrap())[&exec].clone();
    assert_eq!((v.state.as_str(), v.outstanding), ("cancelled", 0), "{v:?}");
}

/// A job dispatched mid-turn changes only its execution's `outstanding`:
/// a frame that writes its action sends a view that counts it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_dispatched_mid_turn_sends_a_view() {
    let (r, store, _) = rig_jobs(
        vec![
            slow_job("t1", "1.5"),
            Scripted::text("Started; I'll report back."),
        ],
        0,
    );
    let seen = watcher(&r.core).await;
    until("the board is seeded", || r.core.push.seeded()).await;
    let res = turn_result(&r.core, None).await;
    let exec = res.execution_id.clone().unwrap();
    let job = r
        .core
        .kernel
        .actions()
        .unwrap()
        .into_iter()
        .find(|a| a.execution_id == exec && a.tool == "proc.run")
        .expect("the job's action");
    // Each frame with a row that names the job: one of them put it in its
    // execution's `outstanding`.
    let frames = frame_ends(&store);
    let job_frames: Vec<u64> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(1000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.data["correlation_id"] == job.correlation_id.as_str())
        .map(|(p, _)| frames[&p])
        .collect();
    assert!(!job_frames.is_empty(), "no row names the job");
    settled(&r.core).await;
    let last = r
        .core
        .push
        .view_of_session(&res.session_id)
        .unwrap()
        .position;
    until("the watcher heard the turn's end", || {
        applied(&seen.lock().unwrap())
            .get(&exec)
            .is_some_and(|v| v.position >= last)
    })
    .await;
    let views = views_between(&seen.lock().unwrap(), &exec, 0, u64::MAX);
    assert!(
        views.iter().any(|v| job_frames.contains(&v.position)
            && v.state == "running"
            && v.outstanding >= 1),
        "no view at the job's frames {job_frames:?}: {views:?}"
    );
}

/// The `confirm.requested` notifications in `notes`.
fn asked(notes: &[Notification]) -> Vec<theseus_protocol::ConfirmRequest> {
    notes
        .iter()
        .filter(|n| n.method == notify::CONFIRM_REQUESTED)
        .map(|n| serde_json::from_value(n.params.clone()).unwrap())
        .collect()
}

/// A question's expiry is one number everywhere: the live
/// `confirm.requested`, `confirm.list`, the watch's snapshot and the board's
/// view all say its plan's time and the TTL, and the live one's
/// `requested_at_ms` is its plan's time, as the lists say. Before, the live
/// one took the time it was asked, a frame's write later.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_questions_expiry_is_the_same_number_everywhere() {
    let r = rig_full(
        vec![
            Scripted::tools(
                "",
                &[("t1", "fs_write", json!({"path": "a.txt", "content": "a"}))],
            ),
            Scripted::text("Written."),
        ],
        true,
        false,
    );
    let rec = new_session(&r.core);
    let sid = rec.session_id.clone();
    let mut s = Raw::connect(&r.core, "session-watcher", 1 << 20);
    s.call(1, method::SESSION_WATCH, json!({"session_id": sid}))
        .await
        .result
        .unwrap();
    let res = spawn_turn(&r.core, rec, "write it").await.unwrap().unwrap();
    let exec = res.execution_id.clone().unwrap();
    let q = r.core.kernel.pending_confirms().unwrap();
    assert_eq!(q.len(), 1, "{q:?}");
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    let want = q[0].planned_at_ms + ttl;
    // The live one, read on the way to an answer sent after it.
    s.call(2, method::SESSION_WATCH, json!({"session_id": sid}))
        .await;
    let live = asked(&s.notes);
    assert_eq!(live.len(), 1, "{:?}", s.notes);
    assert_eq!(
        (live[0].requested_at_ms, live[0].expires_at_ms),
        (q[0].planned_at_ms, want),
        "confirm.requested"
    );
    let listed = r.core.confirm_list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].requested_at_ms, listed[0].expires_at_ms),
        (q[0].planned_at_ms, want),
        "confirm.list"
    );
    settled(&r.core).await;
    let board = r.core.push.view_of_session(&sid).unwrap();
    assert_eq!(board.pending[0].expires_at_ms, want, "the board: {board:?}");
    let mut w = Raw::connect(&r.core, "watcher", 1 << 20);
    let snap: ExecutionsWatchResult = serde_json::from_value(
        w.call(1, method::EXECUTIONS_WATCH, json!({}))
            .await
            .result
            .unwrap(),
    )
    .unwrap();
    let v = snap
        .executions
        .iter()
        .find(|v| v.execution_id == exec)
        .unwrap();
    assert_eq!(v.pending[0].expires_at_ms, want, "the snapshot: {v:?}");
}

/// A question is in `confirm.list` from its plan on, while its turn still
/// runs, as the board shows it: here the turn's end frame is held (the
/// store's fault hook, which writes it after the wait) while the list is
/// read. Before, the list left it out until the turn parked. Then an answer
/// binds and the continuation runs the call. (An answer that lands before
/// the end is the kernel's `a_question_answered_while_its_turn_runs_queues_
/// the_turns_end`: the end queues it, why `answered`.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_question_is_listed_while_its_turn_runs_and_an_answer_binds() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let r = rig_full(
        vec![
            Scripted::tools(
                "",
                &[("t1", "fs_write", json!({"path": "a.txt", "content": "a"}))],
            ),
            Scripted::text("Written."),
        ],
        true,
        false,
    );
    let core = r.core.clone();
    let (held, release) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let (h, go) = (held.clone(), release.clone());
    core.store.fail_turn_frame(move |records| {
        let ends = records.iter().any(|rec| {
            rec.kind == theseus_store::kinds::LEDGER
                && serde_json::from_slice::<crate::ledger::LedgerRow>(&rec.payload)
                    .is_ok_and(|row| row.kind == "execution.waiting")
        });
        if ends {
            h.store(true, Ordering::SeqCst);
            let t0 = std::time::Instant::now();
            while !go.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        false
    });
    let turn = spawn_turn(&core, new_session(&core), "write it");
    until("the turn's end is held", || held.load(Ordering::SeqCst)).await;
    let listed = core.confirm_list().unwrap();
    let running = listed
        .first()
        .and_then(|q| core.kernel.execution(&q.execution_id).ok().flatten())
        .map(|e| e.state.as_str().to_string());
    release.store(true, Ordering::SeqCst);
    assert_eq!(listed.len(), 1, "listed while its turn runs");
    assert_eq!(running.as_deref(), Some("running"));
    let exec = listed[0].execution_id.clone();
    let res = turn.await.unwrap().unwrap();
    assert_eq!(res.execution_id.as_deref(), Some(exec.as_str()));
    let done = core
        .confirm_action(&listed[0].correlation_id, true, None, "test")
        .unwrap();
    assert!(done.approved && done.resumes);
    let e = core.kernel.execution(&exec).unwrap().unwrap();
    assert_eq!(
        (e.state.as_str(), e.resume_pending),
        ("queued", true),
        "{e:?}"
    );
    let next = core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(next.output, "Written.");
    assert!(core.confirm_list().unwrap().is_empty());
}

/// `session.open` writes its execution, the execution's row, the session's
/// record and `session.opened` in one frame, so a watcher that asks
/// `session.list {ids}` at a new session's first `execution.changed` always
/// finds its record, label included. Before, the record was a frame after
/// the execution's, and the TUI, which asks once per session at its first
/// view, kept `ses ab12cd` for a labelled session.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_watcher_finds_a_new_sessions_record_at_its_first_view() {
    let r = rig();
    let mut w = Raw::connect(&r.core, "watcher", 1 << 20);
    w.call(1, method::EXECUTIONS_WATCH, json!({}))
        .await
        .result
        .unwrap();
    let mut o = Raw::connect(&r.core, "opener", 1 << 20);
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..20u64 {
        let label = format!("errand {i}");
        o.send(100 + i, method::SESSION_OPEN, json!({ "label": label }))
            .await;
        let first = w
            .until_view(|v| !seen.contains(&v.session_id))
            .await
            .expect("the new session's first view");
        seen.insert(first.session_id.clone());
        let got = w
            .call(
                1000 + i,
                method::SESSION_LIST,
                json!({"ids": [first.session_id]}),
            )
            .await;
        let list: theseus_protocol::SessionListResult =
            serde_json::from_value(got.result.unwrap()).unwrap();
        assert_eq!(list.sessions.len(), 1, "no record at its first view: {i}");
        assert_eq!(list.sessions[0].label.as_deref(), Some(label.as_str()));
        o.answer(100 + i).await.result.unwrap();
    }
}

/// The records of the WAL's last frame, as (kind, key or row kind).
fn last_frame(r: &Rig) -> Vec<(u16, String)> {
    let ends = frame_ends(&r._dir.path().join("store"));
    let last = *ends.values().max().unwrap();
    ends.iter()
        .filter(|(_, e)| **e == last)
        .filter_map(|(p, _)| r.core.store.inner().get(*p).ok().flatten())
        .map(|rec| {
            let what = match rec.kind {
                theseus_store::kinds::LEDGER => {
                    rec.decode::<crate::ledger::LedgerRow>().unwrap().kind
                }
                _ => rec.key.clone().unwrap_or_default(),
            };
            (rec.kind, what)
        })
        .collect()
}

/// The open is one frame, counted from the WAL: on the plain path, and on
/// the hold's path (a session a holding session's job opens).
#[tokio::test]
async fn a_sessions_open_is_one_frame_on_either_path() {
    use theseus_store::kinds::{EXECUTION, LEDGER, SESSION};
    let r = rig();
    let wal = r._dir.path().join("store");
    let count = || {
        frame_ends(&wal)
            .values()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    };
    let before = count();
    let plain = r
        .core
        .open_session(SessionOpenParams {
            label: Some("plain".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(count() - before, 1, "the plain open's frames");
    assert_eq!(
        last_frame(&r),
        vec![
            (EXECUTION, plain.execution_id.clone().unwrap()),
            (LEDGER, "execution.opened".into()),
            (SESSION, plain.session_id),
            (LEDGER, "session.opened".into()),
        ]
    );
    // A holding session, and one its job opens: it takes the hold in the
    // open's one frame.
    let mut holder = SessionRecord::new(SessionKind::Conversation, None);
    holder.external = Some(
        serde_json::from_value(json!({
            "since_ms": 1, "tool": "http.fetch", "url": "https://pages.example/", "node_id": "nod_1"
        }))
        .unwrap(),
    );
    r.core
        .store
        .put_session(&holder.session_id, &holder)
        .unwrap();
    let before = count();
    let held = r
        .core
        .open_session(SessionOpenParams {
            label: Some("held".into()),
            opened_from: Some(holder.session_id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(count() - before, 1, "the hold's open's frames");
    assert_eq!(
        last_frame(&r),
        vec![
            (EXECUTION, held.execution_id.clone().unwrap()),
            (LEDGER, "execution.opened".into()),
            (LEDGER, "session.opened".into()),
            (LEDGER, "session.external_read".into()),
            (SESSION, held.session_id.clone()),
        ]
    );
    let stored = r
        .core
        .store
        .get_session::<SessionRecord>(&held.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.label.as_deref(), Some("held"));
    assert_eq!(stored.execution_id, held.execution_id);
    assert_eq!(
        stored.external.and_then(|h| h.from_session),
        Some(holder.session_id)
    );
}
