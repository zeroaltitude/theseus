//! The China-origin lane's follow-ups (tail2-cn): the `task.create` twins of
//! the `task.start` tests (no inherited open run, a bare hold for an open
//! task; theseus-gmyoy, theseus-5u16e, theseus-qul8y F3), an open session's
//! transcript read (theseus-mhqlm F2), the tools an open request is offered
//! (theseus-qul8y 1), a taint a failed turn loses (theseus-qul8y F2), and a
//! child's private mark taken from its parent after it started
//! (theseus-y8k3u), at every level above it. Every name here is invented.

use serde_json::{json, Value};
use theseus_protocol::task_start::TaskStartParams;

use crate::data::{DataState, OpenRun};
use crate::provider::{ProviderError, Scripted};
use crate::session::SessionRecord;
use crate::test_support::{turn, until};
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, last_user, only_task, parent_session, rig, Rig, ASK,
};

const BRIEF: &str = "Count the boats in the west quay";

/// A rig whose model starts a task with `input` when the parent says `ASK`,
/// and answers the task's own turn.
fn creating(input: Value) -> Rig {
    rig(
        move |req| {
            if first_user(req).contains(BRIEF) {
                return Scripted::text("Counted 14 boats.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            if last_user(req).starts_with("START") {
                return Scripted::tools(
                    "Starting it.",
                    &[("t_task", "task_create", input.clone())],
                );
            }
            Scripted::text("ok")
        },
        |_| {},
    )
}

fn record(core: &crate::Core, sid: &str) -> SessionRecord {
    core.store.get_session(sid).unwrap().unwrap()
}

/// The key test (theseus-gmyoy): a task the model creates starts with no
/// open run whatever its parent's, as `task.start`'s does
/// (`tests_data_state::a_task_never_inherits_its_parents_run`), and the
/// parent's run is as it was.
#[tokio::test]
async fn a_task_the_model_creates_never_inherits_its_parents_run() {
    let r = creating(json!({"brief": BRIEF, "arrangement": arranged()}));
    let parent = parent_session(&r.core);
    let run = OpenRun {
        from: "nd_quay".into(),
        through: "nd_count".into(),
    };
    let state = DataState::new(None, Some(run), None);
    r.core
        .store
        .update_session(&parent, |rec| {
            rec.data = Some(Box::new(state.clone()));
            Ok(vec![])
        })
        .unwrap();
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    let rec = record(&r.core, &task.session_id);
    assert!(rec.data.is_none(), "the task inherited {:?}", rec.data);
    assert_eq!(
        record(&r.core, &parent).data.as_deref(),
        Some(&state),
        "the parent's run is as it was"
    );
}

/// A rig whose parent holds a `web.search` read with a URL and a query.
const URL: &str = "https://example.com/widgetco-ledger";
const QUERY: &str = "widgetco quarterly ledger";

/// theseus-5u16e F4, theseus-qul8y F3: an open task the model creates takes
/// its parent's hold bare, naming its tool but no URL and no query, as
/// `task.start`'s does (`tests_open_request::an_open_tasks_hold_names_no_url_or_query`);
/// a task that is not open keeps both. The parent's hold makes the call
/// wait, and the test answers it.
#[tokio::test]
async fn an_open_task_the_model_creates_holds_no_url_or_query() {
    for open in [true, false] {
        let mut input = json!({"brief": BRIEF});
        match open {
            true => input["open"] = json!(true),
            false => input["arrangement"] = arranged(),
        }
        let r = creating(input);
        let parent = parent_session(&r.core);
        r.core
            .store
            .update_session(&parent, |rec| {
                let mut h = crate::external::read("nod_web1", "web.search", URL, Some(QUERY), 1);
                h.via = None;
                rec.external = Some(h);
                Ok(vec![])
            })
            .unwrap();
        let res = turn(&r.core, &parent, ASK).await;
        let corr = res.awaiting_confirm.clone().expect("task.create waits");
        r.core.confirm_action(&corr, true, None, "test").unwrap();
        let exec_id = record(&r.core, &parent).execution_id.unwrap();
        until("the task", || {
            !r.core.kernel.tasks(Some(&exec_id)).unwrap().is_empty()
        })
        .await;
        let task = only_task(&r.core, &parent);
        let h = record(&r.core, &task.session_id)
            .external
            .expect("the task holds its parent's read");
        assert_eq!(
            (h.url.is_empty(), h.query.is_none()),
            (open, open),
            "open {open}: {h:?}"
        );
        assert_eq!(h.tool, "web.search", "open {open}: the hold names its tool");
        let why = crate::external::why(&h, crate::external::Mode::Ask);
        assert_eq!(why.contains(URL) || why.contains(QUERY), !open, "{why}");
    }
}

/// theseus-qul8y 1: an open request is offered no tool that starts or
/// schedules work outside it (`task.create`, `task.split`, `task.claim`,
/// `wake.at`), and a call of one is refused at the gate; it keeps the
/// public tools that read or act on a task's own record. A shared place's
/// offer is unchanged.
#[tokio::test]
async fn an_open_request_is_not_offered_the_tools_that_start_work() {
    use crate::places::{offered, PlaceClass};
    let open = crate::open_view::Open::of(vec![], std::path::Path::new("/tmp"));
    const DROPPED: [&str; 4] = ["task.create", "task.split", "task.claim", "wake.at"];
    for name in DROPPED {
        assert!(!open.offers(name), "{name} is offered");
        assert!(offered(PlaceClass::Shared, name, &[]), "{name}: shared");
    }
    for name in [
        "web.search",
        "http.fetch",
        "tools.find",
        "task.update",
        "task.close",
    ] {
        assert!(open.offers(name), "{name} is not offered");
    }
    assert!(
        !open.offers("file.read"),
        "a transcript's file is the owner's"
    );
    // An allowlist: a public task tool added later is not offered to an open
    // request until it is judged there, though a shared place offers it.
    assert!(!open.offers("task.schedule"), "an unjudged task tool");
    assert!(offered(PlaceClass::Shared, "task.schedule", &[]));

    // A call of each dropped tool from an open session's turn is refused.
    let calls: Vec<(String, &str, Value)> = [
        ("task_create", json!({"brief": BRIEF, "open": true})),
        ("wake_at", json!({"after": "1h", "note": "check the quay"})),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (n, v))| (format!("t{i}"), n, v))
    .collect();
    let r = rig(
        move |req| match answers_a_call(req) {
            true => Scripted::text("Done."),
            false => {
                let calls: Vec<(&str, &str, Value)> = calls
                    .iter()
                    .map(|(i, n, v)| (i.as_str(), *n, v.clone()))
                    .collect();
                Scripted::tools("", &calls)
            }
        },
        |_| {},
    );
    let sid = parent_session(&r.core);
    crate::tests_open_view::open(&r.core, &sid, None);
    turn(&r.core, &sid, "START the count").await;
    let sent = r.model.requests.lock().unwrap().clone();
    let first = sent.first().expect("the turn's request");
    let names: Vec<&str> = first
        .tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for gone in ["task_create", "task_split", "task_claim", "wake_at"] {
        assert!(!names.contains(&gone), "{gone} offered: {names:?}");
    }
    let results = serde_json::to_string(&sent.last().unwrap().messages).unwrap();
    for name in ["task.create", "wake.at"] {
        assert!(
            results.contains(&format!("{name} is not offered in an open request")),
            "{name}: {results}"
        );
    }
    let rec = record(&r.core, &sid);
    assert!(
        r.core
            .kernel
            .tasks(rec.execution_id.as_deref())
            .unwrap()
            .is_empty(),
        "no task started"
    );
}

/// theseus-mhqlm F2: a transcript file an open session reads is outside text
/// (§3.9's hold), as a shared place's is. Today the open view does not offer
/// `file.read` at all, so the call is refused and nothing is read; the hold
/// is the second guard if it ever is: whatever the gate says, the file's
/// text never reaches a request unless the session holds it.
#[tokio::test]
async fn an_open_sessions_transcript_read_takes_the_outside_text_hold() {
    const SAID: &str = "The widgetco quay ledger: 41 bollards, 3 cranes.";
    let r = rig(
        |req| match answers_a_call(req) {
            true => Scripted::text("Read it."),
            false => Scripted::tools("", &[("f1", "file_read", json!({"name": "ledger.txt"}))]),
        },
        |_| {},
    );
    let sid = parent_session(&r.core);
    crate::tests_open_view::open(&r.core, &sid, None);
    let attached = theseus_protocol::Attachment {
        name: "ledger.txt".into(),
        media_type: "text/plain".into(),
        size: SAID.len() as u64,
        text: None,
        data: Some(crate::blobs::encode(SAID.as_bytes())),
        not_read: None,
    };
    crate::tests_route::turn_attached(&r.core, Some(&sid), "Read the ledger", None, vec![attached])
        .await;
    let sent = r.model.requests.lock().unwrap().clone();
    assert!(
        sent.len() >= 2,
        "the call's result was sent: {}",
        sent.len()
    );
    let last = sent.last().unwrap().messages.last().cloned().unwrap();
    let result = &last["content"][0];
    assert_eq!(result["type"], "tool_result", "{last}");
    let read = result["is_error"] != json!(true);
    let refused = result
        .to_string()
        .contains("file.read is not offered in an open request");
    assert!(read || refused, "the call failed otherwise: {result}");
    let held = record(&r.core, &sid)
        .external
        .is_some_and(|h| h.url.starts_with("attachment:"));
    assert!(!read || held, "read without the hold: {result}");
}

/// theseus-qul8y F2: an open session's turn refused at admission for a
/// reason that taints, whose model call then fails, stores no taint (the
/// turn's write-back never ran). The next turn on the same message is
/// refused again, and stores it: the taint is taken late, never lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_taint_lost_to_a_failed_turn_is_taken_again() {
    use crate::tests_open_request::{
        jev, open_session, record, rig_with, tainted, try_say, PRIVATE,
    };
    let jev = jev(&[]);
    crate::tests_route::mode(&jev, "chat", 0.2);
    let (r, zai, _dir) = rig_with(&jev, None, |_| {});
    let sid = open_session(&r).await;
    r.claude
        .script
        .lock()
        .unwrap()
        .push_front(Scripted::Fail(ProviderError::InvalidRequest {
            status: 400,
            message: "invalid_request_error: the harbour feed was down".into(),
        }));
    let failed = try_say(&r.core, &sid, PRIVATE, None).await;
    let taint = record(&r.core, &sid).data.and_then(|d| d.taint);
    assert!(
        taint.is_none(),
        "stored by a failed turn: {taint:?} ({failed:?})"
    );
    assert!(tainted(&r.core, &sid).is_empty());
    let again = try_say(&r.core, &sid, PRIVATE, None).await.unwrap();
    assert_eq!(again.profile, "default", "refused again: {:?}", again.route);
    let rows = tainted(&r.core, &sid);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["why"], "verdict_private");
    assert!(record(&r.core, &sid).data.unwrap().taint.is_some());
    assert!(
        zai.requests().is_empty(),
        "nothing reached the restricted model"
    );
}

/// theseus-y8k3u 1: a task started from a plain parent is private once its
/// parent is, however late the parent took the mark: the child's next call
/// is held by the floor (the default provider here is of EU origin, so only
/// the mark can refuse it). A child whose parent stays plain makes it.
#[tokio::test]
async fn a_child_started_plain_is_private_once_its_parent_is() {
    use crate::tests_private_background::{mark, private_on};
    const CHILD: &str = "Count the quays of the harbour";
    for marked in [false, true] {
        let mut r = rig(
            |req| {
                if !first_user(req).contains(CHILD) {
                    return Scripted::text("ok");
                }
                match answers_a_call(req) {
                    true => Scripted::text("Counted 3 quays."),
                    false => Scripted::tools("", &[("t1", "tools_find", json!({"query": "quay"}))]),
                }
            },
            private_on,
        );
        let home = r.core.home().unwrap().session_id;
        *r.model.hold.lock().unwrap() = Some(CHILD.into());
        let p = TaskStartParams {
            brief: CHILD.into(),
            ..Default::default()
        };
        let child = r
            .core
            .task_start(&p, crate::tests_task_brief::cli())
            .unwrap();
        r.entered.recv().await.expect("the child's first call");
        *r.model.hold.lock().unwrap() = None;
        if marked {
            mark(&r.core, &home);
        }
        r.model.gate.add_permits(1);
        let id = child.session_id.replacen("ses_", "exe_", 1);
        until("the child's turn to end", || {
            !matches!(
                exec(&r.core, &id).state,
                theseus_kernel::ExecState::Queued | theseus_kernel::ExecState::Running
            )
        })
        .await;
        let calls = r
            .model
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|q| first_user(q).contains(CHILD))
            .count();
        assert_eq!(
            calls,
            if marked { 1 } else { 2 },
            "marked {marked}: the child's calls that reached the model"
        );
    }
}

/// theseus-y8k3u, every level: a task whose grandparent takes the mark after
/// it started is private too, though its parent never read a Google result.
/// Here the task's parent (the owner's Home) is itself a task of a third
/// session, and that session is marked while the task waits on its first
/// call: the task's next call is held by the floor. Unmarked, it makes it.
#[tokio::test]
async fn a_grandchild_started_plain_is_private_once_its_grandparent_is() {
    use crate::tests_private_background::{mark, private_on};
    const CHILD: &str = "Count the quays of the harbour";
    const TOP: &str = "ses_the_grandparent";
    for marked in [false, true] {
        let mut r = rig(
            |req| {
                if !first_user(req).contains(CHILD) {
                    return Scripted::text("ok");
                }
                match answers_a_call(req) {
                    true => Scripted::text("Counted 3 quays."),
                    false => Scripted::tools("", &[("t1", "tools_find", json!({"query": "quay"}))]),
                }
            },
            private_on,
        );
        let home = r.core.home().unwrap();
        *r.model.hold.lock().unwrap() = Some(CHILD.into());
        let p = TaskStartParams {
            brief: CHILD.into(),
            ..Default::default()
        };
        let child = r
            .core
            .task_start(&p, crate::tests_task_brief::cli())
            .unwrap();
        r.entered.recv().await.expect("the child's first call");
        *r.model.hold.lock().unwrap() = None;
        // The child's parent is itself a task of TOP.
        let mut middle = record(&r.core, &home.session_id);
        middle.task = Some(crate::session::TaskOf {
            parent_session: TOP.into(),
            parent_execution: "exe_the_grandparent".into(),
            by: "t0".into(),
            target: None,
            arrangement: None,
            check: None,
            notify_done: false,
        });
        r.core.store.put_session(&home.session_id, &middle).unwrap();
        if marked {
            mark(&r.core, TOP);
        }
        r.model.gate.add_permits(1);
        let id = child.session_id.replacen("ses_", "exe_", 1);
        until("the child's turn to end", || {
            !matches!(
                exec(&r.core, &id).state,
                theseus_kernel::ExecState::Queued | theseus_kernel::ExecState::Running
            )
        })
        .await;
        let calls = r
            .model
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|q| first_user(q).contains(CHILD))
            .count();
        assert_eq!(
            calls,
            if marked { 1 } else { 2 },
            "marked {marked}: the grandchild's calls that reached the model"
        );
    }
}
