//! `open` on a task, retired with the open view (theseus-9ymz0): an open task
//! read its brief alone, compiled in a view that carried nothing of the owner's,
//! and its session said so in `SessionRecord.data`. That view is gone, and so is
//! the record's say-so; what keeps a session off a model outside the owner's trust
//! is the labels on it. `task.start { open }` and a model's `task.create { open }`
//! are refused in words, open nothing, and a task without `open` starts as it did.

use serde_json::{json, Value};
use theseus_protocol::task_start::TaskStartParams;

use crate::node::Body;
use crate::provider::Scripted;
use crate::test_support::turn;
use crate::tests_task_brief::{cli, params};
use crate::tests_tasks::{answers_a_call, arranged, last_user, parent_session, rig, Rig, ASK};

/// A rig whose model starts the task its parent's last message asks for.
fn opening(input: Value) -> Rig {
    rig(
        move |req| {
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

/// What the parent's tool results said, joined.
fn results(r: &Rig, parent: &str) -> String {
    r.core
        .store
        .session_nodes(parent)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult { content, .. } => Some(format!("{content:?}")),
            _ => None,
        })
        .collect()
}

/// The key test: `open` is refused by either path, in the words a caller can
/// act on, and nothing opens.
#[tokio::test]
async fn an_open_task_is_refused_by_either_path() {
    const WORDS: &str = "open tasks are retired";
    // `task.start`.
    let r = opening(json!({}));
    let open = TaskStartParams {
        open: true,
        ..params("Count the boats in the north quay", None)
    };
    let err = r.core.task_start(&open, cli()).unwrap_err();
    assert!(err.message.contains(WORDS), "{}", err.message);
    assert!(r.core.kernel.tasks(None).unwrap().is_empty(), "opened one");
    // A model's `task.create`: the refusal is the call's result, whatever
    // else the call said.
    for input in [
        json!({"brief": "Count the boats", "open": true}),
        json!({"brief": "Count the boats", "open": true, "arrangement": arranged()}),
        json!({"title": "Count", "open": true}),
    ] {
        let r = opening(input);
        let parent = parent_session(&r.core);
        turn(&r.core, &parent, ASK).await;
        let said = results(&r, &parent);
        assert!(said.contains(WORDS), "{said}");
        assert!(r.core.kernel.tasks(None).unwrap().is_empty(), "opened one");
    }
}

/// A task without `open` starts as it did, with no content labels.
#[tokio::test]
async fn a_task_without_open_starts_as_it_did() {
    let r = opening(json!({}));
    let plain = r
        .core
        .task_start(&params("Count the boats in the south quay", None), cli())
        .unwrap();
    let rec: crate::session::SessionRecord = r
        .core
        .store
        .get_session(&plain.session_id)
        .unwrap()
        .unwrap();
    assert!(rec.data.is_none(), "{:?}", rec.data);
}

/// Off by default, and the wire says so: a `task.start` without `open` is the
/// bytes it was.
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

/// A task's record starts with no labels, whatever its parent's: its brief is an
/// item like any other, judged on its own; and the task's report leaves the
/// parent's labels as they were (ported from the open view's
/// `a_task_never_inherits_its_parents_run`).
#[tokio::test]
async fn a_task_never_inherits_its_parents_labels() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    let r = crate::tests_tasks::rig(
        |_| crate::provider::Scripted::text("Counted 14 boats."),
        |_| {},
    );
    let home = r.core.home().unwrap();
    let mut parent: crate::session::SessionRecord =
        r.core.store.get_session(&home.session_id).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Confidential,
        node: Some("nod_quay".into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    parent.data = Some(Box::new(flags.clone()));
    r.core
        .store
        .put_session(&parent.session_id, &parent)
        .unwrap();
    let started = r
        .core
        .task_start(&params("Count the boats in the north quay", None), cli())
        .unwrap();
    let task_exec = started.session_id.replacen("ses_", "exe_", 1);
    crate::test_support::until("the task's report", || {
        crate::tests_tasks::exec(&r.core, &task_exec).state == theseus_kernel::ExecState::Complete
    })
    .await;
    let task: crate::session::SessionRecord = r
        .core
        .store
        .get_session(&started.session_id)
        .unwrap()
        .unwrap();
    assert!(task.data.is_none(), "{:?}", task.data);
    let parent: crate::session::SessionRecord =
        r.core.store.get_session(&home.session_id).unwrap().unwrap();
    assert!(crate::tests_tasks::exec(&r.core, &home.execution_id)
        .reports
        .contains(&task_exec));
    assert_eq!(
        parent.data.as_deref(),
        Some(&flags),
        "the report changed nothing of the parent's labels"
    );
}
