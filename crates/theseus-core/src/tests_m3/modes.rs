//! Modes through whole cores (theseus-lyxy): `edits` lets file edits in the
//! session's directory through and still asks for the rest; `plan` refuses
//! every writer and `proc.run` with words the model can act on, puts a plan
//! to the person, and moves the conversation when it is approved; the plan is
//! the conversation's plan items in the task graph (the owner's one plan) and
//! `plan_step` moves them; `auto` is `notify` and the floor still asks; a shared
//! place has no mode but `ask`; and a mode lives for its conversation alone.

use serde_json::json;
use theseus_protocol::grants::{GrantAnswer, KIND_MODE};
use theseus_protocol::modes::{PlanState, SessionMode};
use theseus_protocol::{error_code, method, PlaceClass};

use super::*;
use crate::approval::Surface::Cli;
use crate::task_graph::plan;

fn new_session(r: &Rig) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

fn put(r: &Rig, sid: &str, mode: SessionMode) {
    r.core.tools.modes.set(sid, mode);
}

/// The order on `tool`'s call, for a session, as the gate runs it: its
/// directory is `dir`, the place `class`.
fn ordered(
    r: &Rig,
    sid: &str,
    tool: &str,
    input: serde_json::Value,
    class: PlaceClass,
    dir: &std::path::Path,
) -> crate::policy::Decision {
    let rt = &r.core.tools;
    let tool = rt.registry.get(tool).unwrap().clone();
    let mut ctx = rt.ctx.clone();
    ctx.cwd = dir.to_path_buf();
    let plan = tool.plan(&input, &ctx).unwrap();
    let held = || Ok(None);
    let mcp = || None;
    let at = crate::toolrun::order::At {
        place: crate::ceiling::PlaceView {
            class,
            ceiling: None,
            voice: false,
            draft: None,
        },
        held: &held,
        labels: &|| None,
        mcp: &mcp,
        glide: None,
        routine: None,
        session: Some(sid),
        ctx: &ctx,
    };
    rt.order(&at, &*tool, &plan, &input, &mut |_, _| {}).0
}

/// The refusal the gate makes before the order, for a session in `class`.
fn refused(
    r: &Rig,
    sid: &str,
    tool: &str,
    input: serde_json::Value,
    dir: &std::path::Path,
) -> Option<String> {
    let rt = &r.core.tools;
    let tool = rt.registry.get(tool).unwrap().clone();
    let mut ctx = rt.ctx.clone();
    ctx.cwd = dir.to_path_buf();
    let plan = tool.plan(&input, &ctx).unwrap();
    let held = || Ok(None);
    let mcp = || None;
    let at = crate::toolrun::order::At {
        place: crate::ceiling::PlaceView {
            class: PlaceClass::Private,
            ceiling: None,
            voice: false,
            draft: None,
        },
        held: &held,
        labels: &|| None,
        mcp: &mcp,
        glide: None,
        routine: None,
        session: Some(sid),
        ctx: &ctx,
    };
    rt.refusal_at(&at, &*tool, &plan)
}

fn write_in(r: &Rig, rel: &str) -> serde_json::Value {
    json!({"path": r.root.join(rel), "content": "x\n"})
}

#[tokio::test]
async fn edits_runs_file_edits_in_the_directory_and_asks_for_everything_else() {
    let r = rig(vec![]);
    let sid = new_session(&r);
    let dir = r.root.clone();
    // `ask`: a write asks, as the config says.
    let d = ordered(
        &r,
        &sid,
        "fs.write",
        write_in(&r, "a.txt"),
        PlaceClass::Private,
        &dir,
    );
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    put(&r, &sid, SessionMode::Edits);
    let d = ordered(
        &r,
        &sid,
        "fs.write",
        write_in(&r, "a.txt"),
        PlaceClass::Private,
        &dir,
    );
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    let setting = &d.notify.as_ref().unwrap().setting;
    assert!(
        setting.starts_with("ran in edits mode: edits under "),
        "{setting}"
    );
    // The other edit tools too.
    let edit = json!({"path": r.root.join("a.txt"), "old_string": "x", "new_string": "y"});
    std::fs::write(r.root.join("a.txt"), "x\n").unwrap();
    let d = ordered(&r, &sid, "fs.edit", edit, PlaceClass::Private, &dir);
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    // A program still asks.
    let run = json!({"argv": ["echo", "hi"], "cwd": r.root});
    let d = ordered(&r, &sid, "proc.run", run, PlaceClass::Private, &dir);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    // A path outside the session's directory asks: the directory is a
    // subdirectory, the write a sibling's.
    std::fs::create_dir_all(r.root.join("sub")).unwrap();
    let sub = r.root.join("sub");
    let d = ordered(
        &r,
        &sid,
        "fs.write",
        write_in(&r, "a.txt"),
        PlaceClass::Private,
        &sub,
    );
    assert_eq!(
        d.posture,
        Posture::Approve,
        "outside the directory: {}",
        d.reason
    );
    let d = ordered(
        &r,
        &sid,
        "fs.write",
        write_in(&r, "sub/b.txt"),
        PlaceClass::Private,
        &sub,
    );
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    // A `..` out of it, and a path outside every root, ask.
    let up = json!({"path": sub.join("../a.txt"), "content": "x\n"});
    let d = ordered(&r, &sid, "fs.write", up, PlaceClass::Private, &sub);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    let away = json!({"path": "/etc/theseus-modes-test", "content": "x\n"});
    let d = ordered(&r, &sid, "fs.write", away, PlaceClass::Private, &dir);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    // The floor asks in every mode.
    let floor = json!({"argv": ["theseusd", "status"], "cwd": r.root});
    let d = ordered(&r, &sid, "proc.run", floor, PlaceClass::Private, &dir);
    assert!(d.floor && d.posture == Posture::Approve, "{}", d.reason);
    // Another conversation is in `ask`, and so is a shared place.
    let other = new_session(&r);
    let d = ordered(
        &r,
        &other,
        "fs.write",
        write_in(&r, "a.txt"),
        PlaceClass::Private,
        &dir,
    );
    assert_eq!(d.posture, Posture::Approve, "a mode is its session's alone");
    let d = ordered(
        &r,
        &sid,
        "fs.write",
        write_in(&r, "a.txt"),
        PlaceClass::Shared,
        &dir,
    );
    assert_eq!(d.posture, Posture::Approve, "a shared place reads no mode");
}

#[tokio::test]
async fn auto_is_notify_for_what_only_the_posture_asked_and_the_floor_still_asks() {
    let r = rig_with(vec![], |cfg| {
        cfg.policy.approve_argv = vec![vec!["echo".into(), "listed".into()]];
    });
    let sid = new_session(&r);
    let dir = r.root.clone();
    put(&r, &sid, SessionMode::Auto);
    let run = |argv: &[&str]| json!({"argv": argv, "cwd": r.root});
    let d = ordered(
        &r,
        &sid,
        "proc.run",
        run(&["echo", "hi"]),
        PlaceClass::Private,
        &dir,
    );
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    assert!(d.notify.unwrap().setting.starts_with("ran in auto mode"));
    let d = ordered(
        &r,
        &sid,
        "fs.write",
        write_in(&r, "a.txt"),
        PlaceClass::Private,
        &dir,
    );
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    for (why, input) in [
        ("the floor", run(&["theseusd", "status"])),
        ("the approve list", run(&["echo", "listed"])),
        ("outside the roots", run(&["echo", "hi", "/etc/hosts"])),
        (
            "an env",
            json!({"argv": ["echo", "hi"], "cwd": r.root, "env": {"A": "b"}}),
        ),
        (
            "an L1 job's egress",
            json!({"argv": ["echo", "hi"], "cwd": r.root,
                   "sandbox": {"egress": ["exfil.example:443"]}}),
        ),
    ] {
        let d = ordered(&r, &sid, "proc.run", input, PlaceClass::Private, &dir);
        assert_eq!(d.posture, Posture::Approve, "{why}: {}", d.reason);
    }
    let d = ordered(
        &r,
        &sid,
        "proc.run",
        run(&["echo", "hi"]),
        PlaceClass::Shared,
        &dir,
    );
    assert_eq!(d.posture, Posture::Approve, "a shared place reads no mode");
    r.core.tighten("proc.run", None, "test").unwrap();
    let d = ordered(
        &r,
        &sid,
        "proc.run",
        run(&["echo", "hi"]),
        PlaceClass::Private,
        &dir,
    );
    assert_eq!(d.posture, Posture::Approve, "a tightened tool asks");
}

#[tokio::test]
async fn plan_refuses_each_writer_and_proc_run_in_words_the_model_can_act_on() {
    let r = rig(vec![]);
    let sid = new_session(&r);
    let dir = r.root.clone();
    put(&r, &sid, SessionMode::Plan);
    let edit = json!({"path": r.root.join("a.txt"), "old_string": "x", "new_string": "y"});
    let patch = json!({"patch": "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-x\n+y\n"});
    for (tool, input) in [
        ("fs.write", write_in(&r, "a.txt")),
        ("fs.edit", edit),
        ("fs.patch", patch),
        ("proc.run", json!({"argv": ["echo", "hi"], "cwd": r.root})),
    ] {
        let why = refused(&r, &sid, tool, input, &dir).unwrap_or_else(|| panic!("{tool}"));
        assert!(
            why.starts_with("plan mode: read-only; present the plan with plan_present"),
            "{tool}: {why}"
        );
        assert!(crate::policy::modes::is_refusal(&why));
    }
    // A batch is refused by any step: each is judged as the call alone.
    let batch = json!({"steps": [
        {"argv": ["echo", "one"], "cwd": r.root},
        {"argv": ["echo", "two"], "cwd": r.root},
    ]});
    let tool = r.core.tools.registry.get("proc.run").unwrap().clone();
    let mut ctx = r.core.tools.ctx.clone();
    ctx.cwd = dir.clone();
    let (held, mcp) = (|| Ok(None), || None);
    let at = crate::toolrun::order::At {
        place: crate::ceiling::PlaceView {
            class: PlaceClass::Private,
            ceiling: None,
            voice: false,
            draft: None,
        },
        held: &held,
        labels: &|| None,
        mcp: &mcp,
        glide: None,
        routine: None,
        session: Some(&sid),
        ctx: &ctx,
    };
    let judged = r.core.tools.judge(
        &at,
        &*tool,
        &tool.plan(&batch, &ctx).unwrap(),
        &batch,
        &mut |_, _| {},
    );
    match judged {
        Err(crate::toolrun::Turned::Place(why)) => {
            assert!(why.contains("plan mode: read-only"), "{why}");
        }
        other => panic!(
            "a batch in plan mode runs: {:?}",
            other.map(|(d, _)| d.posture)
        ),
    }
    // A read is not refused, and neither is the plan itself.
    let read = json!({"path": r.root.join("notes.md")});
    std::fs::write(r.root.join("notes.md"), "n\n").unwrap();
    assert_eq!(refused(&r, &sid, "fs.read", read, &dir), None);
    let plan = json!({"steps": ["one", "two"]});
    assert_eq!(refused(&r, &sid, "plan.present", plan, &dir), None);
    // The same call in `ask` or `edits` is not refused by a mode.
    put(&r, &sid, SessionMode::Edits);
    assert_eq!(
        refused(&r, &sid, "fs.write", write_in(&r, "a.txt"), &dir),
        None
    );
}

fn present(id: &str, steps: &[&str]) -> Scripted {
    Scripted::tools("", &[(id, "plan_present", json!({"steps": steps}))])
}

fn step(id: &str, index: u32, state: &str) -> Scripted {
    Scripted::tools(
        "",
        &[(id, "plan_step", json!({"index": index, "state": state}))],
    )
}

fn result_text(r: &Rig, sid: &str, id: &str) -> String {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult {
                tool_use_id,
                content,
                ..
            } if tool_use_id == id => Some(content),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result for {id}"))
}

/// A plan in plan mode: the writer is refused with the words, the plan asks
/// as numbered steps, approving it records them and moves to `ask`, and each
/// `plan_step` moves the checklist.
#[tokio::test]
async fn a_plan_is_presented_approved_and_followed_step_by_step() {
    let r = rig(vec![
        Scripted::tools(
            "",
            &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
        ),
        present("p1", &["add the function", "test it", "run all"]),
        Scripted::text("Planned."),
        step("s1", 1, "done"),
        step("s2", 2, "doing"),
        Scripted::text("Going."),
    ]);
    let sid = new_session(&r);
    put(&r, &sid, SessionMode::Plan);
    let res = turn(&r.core, Some(&sid), "plan it").await;
    // The writer was refused, with the words, and nothing was written.
    let text = result_text(&r, &sid, "w1");
    assert!(
        text.starts_with("Not run: plan mode: read-only; present the plan with plan_present"),
        "{text}"
    );
    assert!(!r.root.join("a.txt").exists());
    let tool = ledgered(&r, "tool.invalid_input");
    assert!(tool[0]["reason"]
        .as_str()
        .unwrap()
        .starts_with("mode: plan mode"));
    // The plan is the question: numbered steps, offering edits mode.
    let corr = res.awaiting_confirm.clone().expect("the plan asks");
    let q = r
        .core
        .pending_confirms(&sid)
        .unwrap()
        .into_iter()
        .find(|q| q.correlation_id == corr)
        .unwrap();
    let p = q.preview.clone().expect("the plan is the preview");
    assert_eq!(p.body, "1. add the function\n2. test it\n3. run all");
    let o = q.grant_offer.clone().expect("edits mode is the other yes");
    assert_eq!((o.kind.as_str(), o.key()), (KIND_MODE, 'e'));
    // Still in plan mode, and no plan items, until approved.
    assert_eq!(r.core.tools.modes.mode_of(&sid), SessionMode::Plan);
    assert!(plan::view(&r.core.store, &sid).is_empty());
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    assert_eq!(
        r.core.tools.modes.mode_of(&sid),
        SessionMode::Ask,
        "a plain yes moves to ask"
    );
    // The steps are plan items of the task graph: records with no session and
    // no parent, by the model, the ones the work board counts.
    let items = plan::items_of(&r.core.store, &sid).unwrap();
    assert_eq!(items.len(), 3);
    assert!(items
        .iter()
        .all(|t| t.session.is_none() && t.parent.is_none() && t.origin.by_model));
    assert_eq!(items[0].title, "add the function");
    assert_eq!(crate::task_graph::all(&r.core.store).unwrap().len(), 3);
    assert!(plan::view(&r.core.store, &sid)
        .iter()
        .all(|s| s.state == PlanState::Todo));
    assert_eq!(ledgered(&r, "task.created").len(), 3);
    // `plan_step` moves the checklist, and says so.
    let res = turn(&r.core, Some(&sid), "go").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    let states: Vec<_> = plan::view(&r.core.store, &sid)
        .iter()
        .map(|s| s.state)
        .collect();
    assert_eq!(states, [PlanState::Done, PlanState::Doing, PlanState::Todo]);
    // `doing` is the record's `in_progress`, and `done` closes it with its mark.
    let items = plan::items_of(&r.core.store, &sid).unwrap();
    assert_eq!(items[1].state, crate::task_graph::TaskState::InProgress);
    assert_eq!(items[0].state, crate::task_graph::TaskState::Done);
    assert_eq!(items[0].evidence[0].identity, "plan_step:1");
    let said = result_text(&r, &sid, "s2");
    assert!(said.contains("plan 1/3"), "{said}");
    assert!(
        said.contains("◼ 2. test it") && said.contains("✔ 1. add the function"),
        "{said}"
    );
    // Only the move to a mode is a `session.mode` row, and it holds no plan:
    // the plan is the task graph's, a `task.*` row for each of its changes.
    let rows = ledgered(&r, "session.mode");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["mode"], "ask");
    assert!(rows[0].get("steps").is_none(), "{rows:?}");
    assert_eq!(ledgered(&r, "task.closed").len(), 1);
    assert_eq!(ledgered(&r, "task.updated").len(), 1);
}

/// One plan, one view (the owner's decision 2): the steps of an approved plan
/// are the plan items the work board counts, so the board's `progress` for the
/// conversation, the checklist's count and `plan_step`'s own words agree as
/// the steps are marked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_approved_plans_steps_are_what_the_work_board_counts() {
    use theseus_protocol::modes::plan_line;
    use theseus_protocol::work::WorkKind;
    let r = rig(vec![
        present("p1", &["one", "two", "three"]),
        Scripted::text("Planned."),
        step("s1", 1, "done"),
        step("s2", 3, "skipped"),
        Scripted::text("Going."),
    ]);
    let sid = new_session(&r);
    put(&r, &sid, SessionMode::Plan);
    // The board is built when the first watcher looks.
    let (_watcher, _) = crate::tests_work::Watcher::start(&r.core, Default::default()).await;
    let res = turn(&r.core, Some(&sid), "plan it").await;
    let corr = res.awaiting_confirm.clone().expect("the plan asks");
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    let board = |want: (u32, u32)| {
        let r = &r;
        let sid = sid.clone();
        async move {
            crate::tests_task_graph::until("the board counts the plan", || {
                let (_, views, _) = r.core.push.work_snapshot(200, &Default::default());
                views
                    .iter()
                    .find(|v| {
                        v.kind == WorkKind::Conversation && v.session_id.as_deref() == Some(&sid)
                    })
                    .and_then(|v| v.progress.as_ref())
                    .is_some_and(|p| (p.steps_done, p.steps_total) == want)
            })
            .await;
        }
    };
    board((0, 3)).await;
    assert_eq!(
        plan_line(&plan::view(&r.core.store, &sid)).as_deref(),
        Some("plan 0/3")
    );
    turn(&r.core, Some(&sid), "go").await;
    // One done, one skipped (the board leaves a cancelled step out): 1 of 2.
    board((1, 2)).await;
    assert_eq!(
        plan_line(&plan::view(&r.core.store, &sid)).as_deref(),
        Some("plan 1/2")
    );
    let said = result_text(&r, &sid, "s2");
    assert!(
        said.contains("plan 1/2") && said.contains("⊘ 3. three"),
        "{said}"
    );
}

/// Every change of the plan is told to the conversation's watchers with the
/// items as they stand (`session.mode`'s `plan`), and two marks in one
/// response keep their order (a write with no path runs in a group of its own).
#[tokio::test]
async fn a_plans_changes_are_told_with_its_items_and_marks_keep_their_order() {
    let calls = |a: &[(&str, &str, u32)]| -> Scripted {
        let v: Vec<(&str, &str, serde_json::Value)> = a
            .iter()
            .map(|(id, state, index)| (*id, "plan_step", json!({"index": index, "state": state})))
            .collect();
        Scripted::tools("", &v)
    };
    let r = rig(vec![
        present("p1", &["one", "two", "three"]),
        Scripted::text("Planned."),
        // doing then done on one step, in one response: only the order makes it done.
        calls(&[("a", "doing", 1), ("b", "done", 1), ("c", "doing", 2)]),
        Scripted::text("Going."),
    ]);
    let sid = new_session(&r);
    put(&r, &sid, SessionMode::Plan);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    r.core.bus.watch(&sid, "w", tx);
    let res = turn(&r.core, Some(&sid), "plan it").await;
    let corr = res.awaiting_confirm.clone().expect("the plan asks");
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    turn(&r.core, Some(&sid), "go").await;
    let mut told: Vec<(String, Vec<String>)> = Vec::new();
    while let Ok(m) = rx.try_recv() {
        if let theseus_protocol::Message::Notification(n) = m {
            if n.method == theseus_protocol::notify::SESSION_MODE {
                let states = n.params["plan"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| i["state"].as_str().unwrap().to_string())
                    .collect();
                told.push((n.params["mode"].as_str().unwrap().into(), states));
            }
        }
    }
    let last = told.last().expect("the plan's changes were told");
    assert_eq!(last.1, ["done", "doing", "todo"], "{told:?}");
    assert!(
        told.iter().any(|(_, s)| s == &["todo", "todo", "todo"]),
        "the approval carries the three items: {told:?}"
    );
    let items = plan::items_of(&r.core.store, &sid).unwrap();
    assert_eq!(
        (items[0].state, items[1].state, items[2].state),
        (
            crate::task_graph::TaskState::Done,
            crate::task_graph::TaskState::InProgress,
            crate::task_graph::TaskState::Accepted
        ),
        "doing then done in one response ends done"
    );
}

#[tokio::test]
async fn the_other_yes_on_a_plan_moves_to_edits_and_a_decline_stays_in_plan() {
    let r = rig(vec![
        present("p1", &["one", "two"]),
        Scripted::text("Planned."),
        present("p2", &["one", "two"]),
        Scripted::text("Planned."),
    ]);
    let sid = new_session(&r);
    put(&r, &sid, SessionMode::Plan);
    let res = turn(&r.core, Some(&sid), "plan it").await;
    let corr = res.awaiting_confirm.clone().unwrap();
    // A pattern that is not the offer is refused, and nothing changes.
    let e = r
        .core
        .confirm_granting(
            &corr,
            None,
            "test".into(),
            false,
            &GrantAnswer {
                pattern: "auto".into(),
            },
        )
        .unwrap_err();
    assert!(format!("{e:#}").contains("not the one offered"), "{e:#}");
    r.core
        .confirm_granting(
            &corr,
            None,
            "test".into(),
            false,
            &GrantAnswer {
                pattern: "edits".into(),
            },
        )
        .unwrap();
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    assert_eq!(r.core.tools.modes.mode_of(&sid), SessionMode::Edits);
    assert!(
        ledgered(&r, "policy.granted").is_empty(),
        "a mode is no grant"
    );
    // A declined plan stays a plan, in plan mode.
    put(&r, &sid, SessionMode::Plan);
    let res = turn(&r.core, Some(&sid), "again").await;
    let corr = res.awaiting_confirm.clone().unwrap();
    r.core.confirm_action(&corr, false, None, "test").unwrap();
    r.core
        .continue_execution(&res.execution_id.unwrap())
        .await
        .unwrap();
    assert_eq!(r.core.tools.modes.mode_of(&sid), SessionMode::Plan);
}

#[tokio::test]
async fn plan_step_without_a_plan_or_past_its_steps_says_what_is_wrong() {
    // The two items are the model's own `task_create`s: the same plan items a
    // plan's approval makes, so the steps are one list however they were made.
    let r = rig_with(
        vec![
            step("s1", 1, "done"),
            Scripted::text("x"),
            Scripted::tools(
                "",
                &[
                    ("c1", "task_create", json!({"title": "first thing"})),
                    ("c2", "task_create", json!({"title": "second thing"})),
                ],
            ),
            step("s3", 3, "done"),
            step("s4", 0, "done"),
            step("s5", 1, "done"),
            step("s6", 1, "skipped"),
            Scripted::text("y"),
        ],
        |cfg| {
            cfg.policy
                .tools
                .insert("task.create".into(), crate::policy::Posture::Open);
        },
    );
    let sid = new_session(&r);
    turn(&r.core, Some(&sid), "go").await;
    assert!(result_text(&r, &sid, "s1").contains("No plan is recorded"));
    turn(&r.core, Some(&sid), "list").await;
    let view = plan::view(&r.core.store, &sid);
    assert_eq!(view.len(), 2, "{view:?}");
    let e = result_text(&r, &sid, "s3");
    assert!(e.contains("The plan has 2 steps"), "{e}");
    let e = result_text(&r, &sid, "s4");
    assert!(e.contains("is not one"), "{e}");
    // The model's `task_create` items are the plan: `plan_step` moves the first,
    // and a closed step stays as it is.
    let ok = result_text(&r, &sid, "s5");
    assert!(
        ok.contains("plan 1/2") && ok.contains("✔ 1. first thing"),
        "{ok}"
    );
    let again = result_text(&r, &sid, "s6");
    assert!(again.contains("is done already"), "{again}");
}

/// `session.mode` over the protocol: it reads, sets, tightens for anyone and
/// loosens for the owner alone, and the change is one row and one
/// notification.
#[tokio::test]
async fn session_mode_reads_and_sets_and_the_row_and_the_notice_follow() {
    let r = rig(vec![]);
    let sid = new_session(&r);
    let cli = || surface("sock#1", Cli);
    let read = rpc_as(
        &r.core,
        cli(),
        method::SESSION_MODE,
        json!({"session_id": sid}),
    )
    .await
    .unwrap();
    assert_eq!(read["mode"], "ask");
    let set = rpc_as(
        &r.core,
        cli(),
        method::SESSION_MODE,
        json!({"session_id": sid, "mode": "plan"}),
    )
    .await
    .unwrap();
    assert_eq!(
        (set["mode"].as_str(), set["previous"].as_str()),
        (Some("plan"), Some("ask"))
    );
    assert_eq!(r.core.tools.modes.mode_of(&sid), SessionMode::Plan);
    let rows = ledgered(&r, "session.mode");
    assert_eq!((rows.len(), rows[0]["mode"].as_str()), (1, Some("plan")));
    // The same mode again writes nothing.
    rpc_as(
        &r.core,
        cli(),
        method::SESSION_MODE,
        json!({"session_id": sid, "mode": "plan"}),
    )
    .await
    .unwrap();
    assert_eq!(ledgered(&r, "session.mode").len(), 1);
    // An unknown session, and an unknown mode, are refused.
    let e = rpc_as(
        &r.core,
        cli(),
        method::SESSION_MODE,
        json!({"session_id": "ses_nope", "mode": "ask"}),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("no session"), "{e:?}");
    let e = rpc_as(
        &r.core,
        cli(),
        method::SESSION_MODE,
        json!({"session_id": sid, "mode": "yolo"}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, error_code::INVALID_PARAMS, "{e:?}");
}

/// The loosening modes are the owner's from a private place: an MCP client
/// is no approval surface.
#[tokio::test]
async fn edits_and_auto_are_the_owners_and_an_mcp_client_cannot_set_them() {
    let r = rig(vec![]);
    let sid = new_session(&r);
    for mode in ["edits", "auto"] {
        let e = rpc_as(
            &r.core,
            surface("mcp", crate::approval::Surface::Mcp),
            method::SESSION_MODE,
            json!({"session_id": sid, "mode": mode}),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, error_code::REFUSED, "{mode}: {e:?}");
    }
    assert_eq!(r.core.tools.modes.mode_of(&sid), SessionMode::Ask);
    assert!(ledgered(&r, "session.mode").is_empty());
}

#[tokio::test]
async fn a_shared_place_has_no_mode_but_ask() {
    let r = crate::tests_places::rig();
    let shared = crate::tests_places::session(
        &r.core,
        Some(&format!("channel:{}", crate::tests_places::LAB)),
    );
    let cli = || surface("sock#1", Cli);
    for mode in ["edits", "plan", "auto"] {
        let e = rpc_as(
            &r.core,
            cli(),
            method::SESSION_MODE,
            json!({"session_id": shared, "mode": mode}),
        )
        .await
        .unwrap_err();
        assert!(e.message.contains("no mode but ask"), "{mode}: {e:?}");
    }
    let ask = rpc_as(
        &r.core,
        cli(),
        method::SESSION_MODE,
        json!({"session_id": shared, "mode": "ask"}),
    )
    .await
    .unwrap();
    assert_eq!(ask["mode"], "ask");
    assert!(!r.core.tools.modes.any());
    // Nor is a shared place offered the plan's tools: it has no mode to plan in.
    for tool in ["plan.present", "plan.step"] {
        assert!(
            !crate::places::offered(PlaceClass::Shared, tool, &[]),
            "{tool}"
        );
        assert!(
            crate::places::offered(PlaceClass::Private, tool, &[]),
            "{tool}"
        );
    }
}

/// A mode lasts for its conversation: a restart reads it back from the
/// ledger, a new conversation starts in `ask`, and a retired one comes back
/// as nothing.
#[tokio::test]
async fn a_mode_survives_a_restart_and_never_a_new_conversation() {
    let r = rig(vec![]);
    let (a, b) = (new_session(&r), new_session(&r));
    let cli = || surface("sock#1", Cli);
    for (sid, mode) in [(&a, "plan"), (&b, "edits")] {
        rpc_as(
            &r.core,
            cli(),
            method::SESSION_MODE,
            json!({"session_id": sid, "mode": mode}),
        )
        .await
        .unwrap();
    }
    // A restart's read, into a board of its own.
    assert!(r.core.load_modes().unwrap());
    let fresh = new_session(&r);
    assert_eq!(r.core.tools.modes.mode_of(&fresh), SessionMode::Ask);
    r.core.tools.modes.set(&a, SessionMode::Ask);
    r.core.tools.modes.set(&b, SessionMode::Ask);
    assert!(!r.core.tools.modes.any());
    r.core.retire_session(&b, &"test".into()).unwrap();
    assert!(r.core.load_modes().unwrap());
    assert_eq!(r.core.tools.modes.mode_of(&a), SessionMode::Plan);
    assert_eq!(
        r.core.tools.modes.mode_of(&b),
        SessionMode::Ask,
        "a retired conversation's mode is not read back"
    );
    assert_eq!(r.core.tools.modes.mode_of(&fresh), SessionMode::Ask);
}
