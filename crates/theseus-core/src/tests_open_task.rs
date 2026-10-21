//! An open task (theseus-a1wvq.6): `task.start { open }` and a model's
//! `task.create { open }` open a task session that starts from its brief
//! alone, and whose record says in the opening frame that it was opened open
//! (`SessionRecord.data.opened`, one `session.data` row). Neither path reads
//! the parent's conversation, and the words that refuse a mix are the
//! caller's to read. The open view itself is theseus-a1wvq.7's.

use serde_json::{json, Value};
use theseus_protocol::task_start::TaskStartParams;

use crate::data::{OpenHow, Opened};
use crate::node::Body;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::test_support::{ledger_rows_100k as rows, turn};
use crate::tests_task_brief::{cli, params};
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, last_user, only_task, parent_session, rig, until,
    Rig, ASK,
};

const MARKER: &str = "quay-marker-7731";

/// A rig whose model starts the task its parent's last message asks for, and
/// answers a task's turn.
fn opening(input: Value) -> Rig {
    rig(
        move |req| {
            if first_user(req).contains("Count the boats") {
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

/// The `session.data` rows that name `sid`.
fn data_rows(core: &crate::Core, sid: &str) -> Vec<Value> {
    rows(core, "session.data")
        .into_iter()
        .filter(|r| r.session_id.as_deref() == Some(sid))
        .map(|r| r.data)
        .collect()
}

/// A task that ran to its report: its session record.
async fn finished(r: &Rig, sid: &str) -> SessionRecord {
    let id = sid.replacen("ses_", "exe_", 1);
    until("the task's report", || {
        exec(&r.core, &id).state == theseus_kernel::ExecState::Complete
    })
    .await;
    record(&r.core, sid)
}

/// The key test: an open task's session says it was opened open, in its
/// opening frame, whichever way it started, and a task without `open` says
/// nothing and writes no row.
#[tokio::test]
async fn each_open_task_writes_opened() {
    // `task.start`.
    let r = opening(json!({}));
    let at = r.core.kernel.now_ms();
    let open = r
        .core
        .task_start(
            &TaskStartParams {
                open: true,
                ..params("Count the boats in the north quay", None)
            },
            cli(),
        )
        .unwrap();
    let rec = record(&r.core, &open.session_id);
    let opened = rec.data.as_deref().and_then(|d| d.opened.clone()).unwrap();
    assert_eq!(opened.how, OpenHow::Task);
    assert!(
        (at..=r.core.kernel.now_ms()).contains(&opened.at_ms),
        "{opened:?}"
    );
    let d = rec.data.as_deref().unwrap();
    assert!(d.run.is_none() && d.taint.is_none(), "{d:?}");
    let written = data_rows(&r.core, &open.session_id);
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(written[0]["row"], "opened");
    assert_eq!(written[0]["how"], "task");
    // The turn's own write-back keeps it.
    let rec = finished(&r, &open.session_id).await;
    assert!(matches!(
        rec.data.as_deref().and_then(|d| d.opened.as_ref()),
        Some(Opened {
            how: OpenHow::Task,
            ..
        })
    ));
    // The same call without `open`.
    let plain = r
        .core
        .task_start(&params("Count the boats in the south quay", None), cli())
        .unwrap();
    assert!(record(&r.core, &plain.session_id).data.is_none());
    assert!(data_rows(&r.core, &plain.session_id).is_empty());

    // A model's `task.create`.
    for open in [true, false] {
        let mut input = json!({"brief": "Count the boats in the west quay"});
        if open {
            input["open"] = json!(true);
        } else {
            input["arrangement"] = arranged();
        }
        let r = opening(input);
        let parent = parent_session(&r.core);
        let at = r.core.kernel.now_ms();
        turn(&r.core, &parent, ASK).await;
        let task = only_task(&r.core, &parent);
        let rec = finished(&r, &task.session_id).await;
        let rows = data_rows(&r.core, &task.session_id);
        if open {
            let opened = rec.data.as_deref().and_then(|d| d.opened.clone()).unwrap();
            assert_eq!(opened.how, OpenHow::Task);
            assert!(opened.at_ms >= at, "{opened:?}");
            let d = rec.data.as_deref().unwrap();
            assert!(d.run.is_none() && d.taint.is_none(), "{d:?}");
            assert_eq!(rows.len(), 1, "{rows:?}");
            assert_eq!(
                (rows[0]["row"].clone(), rows[0]["how"].clone()),
                ("opened".into(), "task".into())
            );
        } else {
            assert!(rec.data.is_none(), "{:?}", rec.data);
            assert!(rows.is_empty(), "{rows:?}");
        }
    }
}

/// An open task reads its brief and nothing else of its parent: from either
/// path, its transcript is the brief, it has no arrangement node and no
/// arrangement edge, and its first request holds none of the parent's words.
#[tokio::test]
async fn an_open_task_carries_only_its_brief() {
    let check = |r: &Rig, task: &str, parent: &str| {
        let nodes: Vec<_> = r.core.store.session_nodes(task).unwrap();
        let before: Vec<_> = nodes
            .iter()
            .take_while(|(_, n)| !matches!(n.body, Body::AssistantMessage { .. }))
            .collect();
        assert_eq!(
            before.len(),
            1,
            "the brief alone before its first answer: {nodes:?}"
        );
        assert!(nodes
            .iter()
            .all(|(_, n)| !matches!(n.body, Body::Arrangement { .. })));
        assert!(crate::task::is_brief(&nodes[0].1));
        assert!(!format!("{:?}", nodes[0].1.body).contains(MARKER));
        assert!(record(&r.core, task).task.unwrap().arrangement.is_none());
        for (_, n) in r.core.store.session_nodes(parent).unwrap() {
            let edges = r
                .core
                .store
                .scope_after(&format!("in:{}", n.id), 0)
                .unwrap();
            for e in edges {
                let e: crate::graph::Edge = e.decode().unwrap();
                assert_ne!(e.via, crate::graph::VIA_ARRANGEMENT, "{e:?}");
            }
        }
        let sent = r.model.requests.lock().unwrap().clone();
        let tasks: Vec<_> = sent
            .iter()
            .filter(|q| first_user(q).contains("Count the boats"))
            .collect();
        assert!(!tasks.is_empty(), "the task's first request");
        for q in tasks {
            let body = serde_json::to_string(&json!({"m": q.messages, "s": q.system})).unwrap();
            assert!(
                !body.contains(MARKER),
                "the parent's words reached the task"
            );
        }
    };

    // `task.create`, from a parent that said three things.
    let r = opening(json!({"brief": "Count the boats in the east quay", "open": true}));
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, "Good morning.").await;
    turn(
        &r.core,
        &parent,
        &format!("The notice board says {MARKER} for the harbour."),
    )
    .await;
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    finished(&r, &task.session_id).await;
    check(&r, &task.session_id, &parent);

    // `task.start`, under a parent that said the same.
    let r = opening(json!({}));
    let home = r.core.home().unwrap();
    turn(&r.core, &home.session_id, "Good morning.").await;
    turn(
        &r.core,
        &home.session_id,
        &format!("The notice board says {MARKER} for the harbour."),
    )
    .await;
    turn(&r.core, &home.session_id, "Anything else?").await;
    let started = r
        .core
        .task_start(
            &TaskStartParams {
                open: true,
                ..params("Count the boats in the north quay", None)
            },
            cli(),
        )
        .unwrap();
    finished(&r, &started.session_id).await;
    check(&r, &started.session_id, &home.session_id);
    // Its record in the task graph takes its objective from the brief.
    let graph = crate::task_graph::all(&r.core.store).unwrap();
    let g = graph
        .iter()
        .find(|t| t.session.as_deref() == Some(&started.session_id))
        .unwrap();
    assert_eq!(g.objective, "Count the boats in the north quay");
}

/// `open` refuses what it cannot carry, in words, and opens nothing.
#[tokio::test]
async fn open_and_check_of_refuse_each_other() {
    // A model's `task.create`: the refusal is the call's result.
    let refused = |input: Value| async move {
        let r = opening(input);
        let parent = parent_session(&r.core);
        turn(&r.core, &parent, ASK).await;
        let said: String = r
            .core
            .store
            .session_nodes(&parent)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::ToolResult { content, .. } => Some(format!("{content:?}")),
                _ => None,
            })
            .collect();
        assert!(
            r.core.kernel.tasks(None).unwrap().is_empty(),
            "opened a task"
        );
        assert!(rows(&r.core, "session.data").is_empty(), "wrote a row");
        said
    };
    let said =
        refused(json!({"brief": "Count the boats", "open": true, "check_of": "abc123"})).await;
    assert!(said.contains("an open task cannot be a check: a check reads its task's claim, which is not judged open"), "{said}");
    let said =
        refused(json!({"brief": "Count the boats", "open": true, "arrangement": arranged()})).await;
    assert!(said.contains("an open task carries only its brief: leave out `arrangement` and write what it needs into `brief`"), "{said}");
    let said = refused(json!({"title": "Count", "open": true})).await;
    assert!(
        said.contains("`open` is for a task session: give it a `brief`"),
        "{said}"
    );

    // `task.start`.
    let r = opening(json!({}));
    let err = r
        .core
        .task_start(
            &TaskStartParams {
                open: true,
                quotes: vec!["The notice board says something about the quay".into()],
                ..params("Count the boats in the north quay", None)
            },
            cli(),
        )
        .unwrap_err();
    assert!(
        err.message
            .contains("an open task carries only its brief: leave out `quotes`"),
        "{}",
        err.message
    );
    assert!(rows(&r.core, "session.data").is_empty());
    assert!(r.core.kernel.tasks(None).unwrap().is_empty());
}

/// Off by default, and the wire says so: a `task.start` without `open` is
/// the bytes it was.
#[test]
fn the_wire_leaves_open_out_until_it_is_set() {
    let p = params("Count the boats", None);
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(json, r#"{"brief":"Count the boats"}"#);
    let back: TaskStartParams = serde_json::from_str(&json).unwrap();
    assert!(!back.open);
    let open = serde_json::to_string(&TaskStartParams { open: true, ..p }).unwrap();
    assert_eq!(open, r#"{"brief":"Count the boats","open":true}"#);
}
