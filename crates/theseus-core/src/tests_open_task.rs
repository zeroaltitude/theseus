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

/// A task's record starts with its parent's labels (theseus-gyjyo.5, as a fork's
/// does; this said the opposite until then), its brief an item like any other,
/// judged on its own; and the task's report leaves the parent's labels as they
/// were.
#[tokio::test]
async fn a_task_inherits_its_parents_labels() {
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
    // It carries the parent's labels from its first call (theseus-gyjyo.5; a
    // task of a labelled session is no way around the label), and the report
    // that came back changed nothing of the parent's.
    assert_eq!(
        task.data.as_deref().map(|d| d.labels.clone()),
        Some(flags.labels.clone()),
        "{:?}",
        task.data
    );
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

/// [`opening`], with `[content_based_security] injection` written out. Every
/// test of S31-1 says which mode it is testing rather than leaning on the
/// configured one: the whole point of the row is that the hole is closed
/// either way, and a test that read the mode from the config could not show
/// that.
fn opening_at(mode: crate::content::InjectionMode, input: Value) -> Rig {
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
        move |c| c.content_based_security.injection = mode,
    )
}

/// The `task.create` input a scripted model makes: a brief, and the
/// arrangement that quotes what the operator asked for (M5 27). Without the
/// arrangement the call is refused for a reason that has nothing to do with
/// this row, and the task list is then empty for the wrong reason.
fn creating() -> Value {
    json!({"brief": "Count the boats in the north quay", "arrangement": arranged()})
}

/// Put `injection` on `session`, as a judged item that tried to steer it
/// would. The label returned, for an assertion to compare against.
fn label_injection(core: &std::sync::Arc<crate::Core>, session: &str) -> crate::content::Label {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    let l = Label {
        flag: Flag::Injection,
        node: Some("nod_fetched".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch https://elsewhere.example/page".into(),
        confidence: 0.95,
        band: theseus_judge::band::Band::Act,
        at_ms: 1,
        judgment: String::new(),
    };
    let mut rec: crate::session::SessionRecord = core.store.get_session(session).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    flags.raise(l.clone());
    rec.data = Some(Box::new(flags));
    core.store.put_session(session, &rec).unwrap();
    l
}

/// **S31-1 (i), at `notify`: a task created by a session carrying
/// `injection` starts carrying it**, so the warning the operator was shown
/// on the parent is shown on the child too (the owner, 2026-11-13: "B is
/// fine" -- the warning follows the content, and nothing is frozen).
///
/// This is the laundering path the retired freeze used to close, and at
/// `notify` the freeze does not close it: `policy/modes.rs`'s
/// `content_refusal` returns before any label is read, so the parent **can**
/// call `task.create`. If the label did not travel, the text that steered
/// the parent would arrive in a child session with no warning on it at all.
#[tokio::test]
async fn at_notify_a_task_of_a_labelled_session_carries_the_label() {
    use crate::content::{Flag, InjectionMode};
    let r = opening_at(InjectionMode::Notify, creating());
    let parent = parent_session(&r.core);
    let raised = label_injection(&r.core, &parent);
    turn(&r.core, &parent, ASK).await;
    crate::test_support::until("the task to open", || {
        !r.core.kernel.tasks(None).unwrap().is_empty()
    })
    .await;
    let tasks = r.core.kernel.tasks(None).unwrap();
    assert_eq!(
        tasks.len(),
        1,
        "at notify the call runs: {}",
        results(&r, &parent)
    );
    let child: crate::session::SessionRecord = r
        .core
        .store
        .get_session(&tasks[0].session_id)
        .unwrap()
        .unwrap();
    let data = child.data.as_deref().expect("the child carries labels");
    assert!(
        data.has(Flag::Injection),
        "the child starts carrying the parent's label: {data:?}"
    );
    // The label is the parent's own, source and all: the child's warning
    // names the item that steered its parent, not a new finding of its own.
    assert_eq!(
        data.label(Flag::Injection).map(|l| l.source.clone()),
        Some(raised.source),
        "{data:?}"
    );
}

/// **S31-1 (i), at `refuse`: behaviour as before** -- the same hole stays
/// closed, by the other mechanism. A parent carrying `injection` is frozen,
/// `task.create` is not a local read, so no child is created at all and
/// there is nothing for an injection to launder itself into.
///
/// The pair is the claim: whichever mode is configured, an injection cannot
/// reach an unlabelled child. At `notify` it reaches a labelled one; at
/// `refuse` it reaches none.
#[tokio::test]
async fn at_refuse_a_labelled_session_creates_no_task_at_all() {
    use crate::content::InjectionMode;
    // The control first, and it is not optional: an empty task list is met
    // by a `task.create` that was never going to work. The same rig, the
    // same call, no label -- a task opens.
    let c = opening_at(InjectionMode::Refuse, creating());
    let clean = parent_session(&c.core);
    turn(&c.core, &clean, ASK).await;
    crate::test_support::until("the control's task to open", || {
        !c.core.kernel.tasks(None).unwrap().is_empty()
    })
    .await;
    assert_eq!(
        c.core.kernel.tasks(None).unwrap().len(),
        1,
        "the control opens one: {}",
        results(&c, &clean)
    );

    let r = opening_at(InjectionMode::Refuse, creating());
    let parent = parent_session(&r.core);
    label_injection(&r.core, &parent);
    turn(&r.core, &parent, ASK).await;
    let said = results(&r, &parent);
    assert!(
        r.core.kernel.tasks(None).unwrap().is_empty(),
        "a frozen session opened a task: {said}"
    );
    assert!(
        said.contains("theseus content remove"),
        "and the model is told why, in words it can act on: {said}"
    );
}
