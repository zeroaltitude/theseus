//! Projects through the whole core (theseus-n5ui2): a project and its
//! milestones made in one frame with a weekly review routine, a milestone's
//! date setting a reminder in the owner's Home (moved when the date moves,
//! cancelled when the milestone closes, an overdue notice a grace after),
//! the review's runs landing in its log, layer 1 still guarding the owner's
//! goal, and a conversation attached to a project carrying its slice in every
//! request through three compactions. The pure rules are `project::tests`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::Purpose;
use theseus_protocol::tasks::{TaskKind, TaskState};
use theseus_protocol::{Message, Request, Usage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::tests_task_graph::{results, rig, rows, session, turn, until, Rig};
use crate::{Config, Core};

async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let client = Client::new("sock#1", Surface::Cli);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r.result.ok_or_else(|| r.error.unwrap().message);
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

const MS: [(&str, &str); 3] = [
    ("Survey the moorings", "1h"),
    ("Chart the channel", "2h"),
    ("Publish the chart", "3h"),
];

fn spec(extra: Value) -> Value {
    let mut p = json!({
        "title": "Harbour survey",
        "objective": "Chart every mooring in the outer harbour",
        "acceptance": ["every mooring has a depth"],
        "milestones": MS.iter().map(|(t, d)| json!({"title": t, "due": d})).collect::<Vec<_>>(),
    });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

async fn made(r: &Rig, extra: Value) -> Value {
    rpc(&r.core, "project.create", spec(extra)).await.unwrap()
}

/// The milestone reminders the kernel holds, soonest first: (id, due, note).
fn reminders(core: &Core) -> Vec<(String, u64, String)> {
    let mut v: Vec<(String, u64, String)> = core
        .kernel
        .pending_wakes()
        .unwrap()
        .into_iter()
        .filter(|(_, w)| w.purpose == Purpose::Remind)
        .map(|(_, w)| (w.id, w.due_at_ms, w.note))
        .collect();
    v.sort_by_key(|(_, d, _)| *d);
    v
}

fn milestone(r: &Rig, project: &Value, n: usize) -> String {
    let id = project["records"][n]["id"].as_str().unwrap().to_string();
    assert_eq!(r.task(&id).kind, TaskKind::Milestone);
    id
}

/// The owner makes a project: its record, three milestones, a review routine
/// and six reminders (a due and an overdue each), and the list reads it.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_is_made_with_its_milestones_a_review_and_their_reminders() {
    let r = rig();
    let p = made(&r, json!({})).await;
    let id = p["project"]["id"].as_str().unwrap();
    let rec = r.task(id);
    assert_eq!(rec.kind, TaskKind::Project);
    assert!(!rec.origin.by_model, "the goal is the owner's");
    assert_eq!(rec.objective, "Chart every mooring in the outer harbour");
    assert_eq!(rec.acceptance, ["every mooring has a depth"]);
    let kids: Vec<_> = r
        .tasks()
        .into_iter()
        .filter(|t| t.parent.as_deref() == Some(id))
        .collect();
    assert_eq!(kids.len(), 3);
    assert!(kids
        .iter()
        .all(|k| k.kind == TaskKind::Milestone && k.due_at_ms.is_some() && k.version == 1));
    // The review is a routine, named on the project.
    let review = rec.review.clone().expect("a weekly review");
    let routines = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    let info = &routines["routines"][0];
    assert_eq!(info["id"], review);
    assert_eq!(info["series"], "mon 08:00");
    assert_eq!(info["title"], "Review: Harbour survey");
    // Six reminders in Home: ⚑ due today and ⚑ overdue for each.
    let w = reminders(&r.core);
    assert_eq!(w.len(), 6, "{w:?}");
    let due: Vec<&str> = w
        .iter()
        .filter(|(_, _, n)| n.starts_with("⚑ due today: "))
        .map(|(_, _, n)| n.as_str())
        .collect();
    assert_eq!(
        due,
        [
            "⚑ due today: Harbour survey, milestone 1: Survey the moorings",
            "⚑ due today: Harbour survey, milestone 2: Chart the channel",
            "⚑ due today: Harbour survey, milestone 3: Publish the chart",
        ]
    );
    assert_eq!(
        w.iter()
            .filter(|(_, _, n)| n.starts_with("⚑ overdue: "))
            .count(),
        3
    );
    // The list: nothing done, the next milestone named, a review time.
    let list = rpc(&r.core, "project.list", json!({})).await.unwrap();
    let row = &list["projects"][0];
    assert_eq!(row["progress"]["milestones_total"], 3);
    assert_eq!(row["progress"]["milestones_done"], 0);
    assert_eq!(row["next_title"], "Survey the moorings");
    assert!(row["review_next_ms"].as_u64().is_some(), "{row}");
    let shown = rpc(&r.core, "project.show", json!({"id": id}))
        .await
        .unwrap();
    assert_eq!(shown["records"].as_array().unwrap().len(), 4);
    // `review: none` makes none.
    let bare = rpc(
        &r.core,
        "project.create",
        json!({"title": "Quiet project", "review": "none"}),
    )
    .await
    .unwrap();
    assert!(bare["project"]["review"].is_null(), "{bare}");
}

/// A moved date moves the reminder (it does not make a second), and closing
/// the milestone cancels both of its wakes.
#[tokio::test(flavor = "multi_thread")]
async fn a_moved_date_moves_its_reminder_and_a_close_cancels_it() {
    let r = rig();
    let s = session(&r.core);
    let p = made(&r, json!({"review": "none"})).await;
    let m1 = milestone(&r, &p, 1);
    let before = reminders(&r.core);
    assert_eq!(before.len(), 6);
    let was = r.task(&m1).due_at_ms.unwrap();
    r.calls(&[(
        "task_update",
        json!({"id": m1, "version": 1, "patch": {"due": "5h"}}),
    )]);
    turn(&r.core, &s, "push the survey back").await;
    let now = r.task(&m1);
    let to = now.due_at_ms.unwrap();
    assert!(to > was + 3 * 3_000_000, "{was} -> {to}");
    assert_eq!(now.version, 2);
    let after = reminders(&r.core);
    assert_eq!(after.len(), 6, "moved, not duplicated: {after:?}");
    assert!(
        after.iter().any(|(_, d, n)| *d == to
            && n == "⚑ due today: Harbour survey, milestone 1: Survey the moorings"),
        "{after:?}"
    );
    assert!(
        !after.iter().any(|(_, d, _)| *d == was),
        "the old time is gone: {after:?}"
    );
    // Close it: both its wakes go, the other milestones keep theirs.
    r.calls(&[(
        "task_close",
        json!({"id": m1, "version": 2, "outcome": "done",
               "evidence": [{"identity": "report:survey-1"}]}),
    )]);
    turn(&r.core, &s, "the survey is done").await;
    assert_eq!(r.task(&m1).state, TaskState::Done);
    let left = reminders(&r.core);
    assert_eq!(left.len(), 4, "{left:?}");
    assert!(left.iter().all(|(_, _, n)| !n.contains("milestone 1")));
    let list = rpc(&r.core, "project.list", json!({})).await.unwrap();
    assert_eq!(list["projects"][0]["progress"]["milestones_done"], 1);
    assert_eq!(list["projects"][0]["next_title"], "Chart the channel");
}

/// A closed project takes its milestones' reminders with it, open
/// milestones included (reviewer D's planted revert, a project's state not
/// read, passed every test until this one).
#[tokio::test(flavor = "multi_thread")]
async fn a_closed_project_cancels_every_milestones_reminders() {
    let r = rig();
    let s = session(&r.core);
    let p = made(&r, json!({"review": "none"})).await;
    let id = p["project"]["id"].as_str().unwrap().to_string();
    assert_eq!(reminders(&r.core).len(), 6);
    let version = r.task(&id).version;
    r.calls(&[(
        "task_close",
        json!({"id": id, "version": version, "outcome": "done",
               "evidence": [{"identity": "report:harbour"}]}),
    )]);
    turn(&r.core, &s, "the harbour survey is done").await;
    assert!(r.task(&id).state.is_closed());
    let left = reminders(&r.core);
    assert!(left.is_empty(), "{left:?}");
}

/// A milestone whose date has passed sets no reminder when it is synced
/// again (a re-sync after its wakes fired, or a project-wide one): it would
/// fire at once (reviewer D's planted revert of `at > now` passed every test
/// until this one). The date itself is refused at creation, so the sync is
/// driven directly, as the overdue test does.
#[tokio::test(flavor = "multi_thread")]
async fn a_milestone_dated_in_the_past_sets_no_reminder() {
    use crate::project::remind::{sync_milestone, Place};
    let r = rig();
    let p = rpc(
        &r.core,
        "project.create",
        json!({"title": "Harbour survey", "review": "none",
               "milestones": [{"title": "Survey the moorings", "due": "1h"}]}),
    )
    .await
    .unwrap();
    let mut rec = r.task(&milestone(&r, &p, 1));
    rec.due_at_ms = Some(r.core.kernel.now_ms() - 60_000);
    let tasks = vec![r.task(&rec.parent.clone().unwrap()), rec.clone()];
    let home = r.core.home().unwrap();
    let place = Place {
        execution_id: home.execution_id.clone(),
        target: None,
    };
    for (_, w) in r.core.kernel.pending_wakes().unwrap() {
        r.core
            .kernel
            .cancel_wake(&home.execution_id, &w.id, "test")
            .unwrap();
    }
    let s = sync_milestone(&r.core.kernel, &tasks, &rec, (&place, "test"), 1_000).unwrap();
    assert_eq!(s.set, 0, "{s:?}");
    assert!(reminders(&r.core).is_empty());
}

/// A milestone's date fires its reminder: a notice in the owner's Home, no
/// model call, and `wake.noticed` carrying its words.
#[tokio::test(flavor = "multi_thread")]
async fn a_milestones_date_fires_its_reminder_in_homes_notice_with_no_model_call() {
    let r = rig();
    let p = rpc(
        &r.core,
        "project.create",
        json!({"title": "Harbour survey", "review": "none",
               "milestones": [{"title": "Survey the moorings", "due": "2s"}]}),
    )
    .await
    .unwrap();
    assert_eq!(p["records"].as_array().unwrap().len(), 2);
    until("the due notice", || {
        rows(&r.core, "wake.fired").iter().any(|w| {
            w.data["note"]
                .as_str()
                .is_some_and(|n| n.starts_with("⚑ due today"))
        })
    })
    .await;
    assert!(
        r.model.requests.lock().unwrap().is_empty(),
        "a reminder is no model call"
    );
    // The overdue wake is still pending, a grace after the date.
    let left = reminders(&r.core);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].2.starts_with("⚑ overdue: "), "{left:?}");
}

/// Past its grace an open milestone is overdue: one notice, an Interrupt
/// with no sound, and nothing again.
#[tokio::test(flavor = "multi_thread")]
async fn an_overdue_milestone_is_one_notice_without_sound() {
    use crate::project::remind::{sync_milestone, Place};
    use theseus_protocol::notices::{deliver, reminder_notice, Delivery, Urgency, Viewer, Why};
    let r = rig();
    let p = rpc(
        &r.core,
        "project.create",
        json!({"title": "Harbour survey", "review": "none",
               "milestones": [{"title": "Survey the moorings", "due": "1h"}]}),
    )
    .await
    .unwrap();
    let m = milestone(&r, &p, 1);
    // The date is in the future; set the overdue wake a second after a
    // date two seconds out, as a short grace would.
    let mut rec = r.task(&m);
    rec.due_at_ms = Some(r.core.kernel.now_ms() + 1_500);
    let tasks = vec![r.task(&rec.parent.clone().unwrap()), rec.clone()];
    let home = r.core.home().unwrap();
    let place = Place {
        execution_id: home.execution_id.clone(),
        target: None,
    };
    for (_, w) in r.core.kernel.pending_wakes().unwrap() {
        r.core
            .kernel
            .cancel_wake(&home.execution_id, &w.id, "test")
            .unwrap();
    }
    let s = sync_milestone(&r.core.kernel, &tasks, &rec, (&place, "test"), 1_000).unwrap();
    assert_eq!(s.set, 2);
    until("the overdue notice", || {
        rows(&r.core, "wake.fired").iter().any(|w| {
            w.data["note"]
                .as_str()
                .is_some_and(|n| n.starts_with("⚑ overdue"))
        })
    })
    .await;
    let fired = rows(&r.core, "wake.fired");
    assert_eq!(fired.len(), 2, "due, then overdue, once each");
    let note = fired
        .iter()
        .find_map(|w| {
            w.data["note"]
                .as_str()
                .filter(|n| n.starts_with("⚑ overdue"))
        })
        .unwrap()
        .to_string();
    let n = reminder_notice(
        &theseus_protocol::WakeNoticed {
            wake_id: "wak_x".into(),
            short: "x".into(),
            session_id: home.session_id,
            execution_id: home.execution_id,
            purpose: "remind".into(),
            note,
            due_at_ms: 1,
            late_ms: 0,
            while_down: false,
            every: None,
            occurrence: None,
            posted_to: None,
            home: true,
            answers: vec![],
        },
        &|_| "08:00".into(),
    );
    assert_eq!((n.urgency, n.why), (Urgency::Interrupt, Why::Overdue));
    let ping = |sound| Viewer {
        sound,
        ..Default::default()
    };
    assert_eq!(deliver(&n, &ping(true)), Delivery::Ping { sound: false });
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert_eq!(rows(&r.core, "wake.fired").len(), 2, "once");
}

/// The review is a routine: a run is a fresh task whose brief names the
/// project, and it lands in the routine's run log, which is the project's.
#[tokio::test(flavor = "multi_thread")]
async fn the_review_runs_and_lands_in_the_projects_log() {
    let r = rig();
    let p = made(&r, json!({})).await;
    let review = p["project"]["review"].as_str().unwrap().to_string();
    let id = p["project"]["id"].as_str().unwrap().to_string();
    let run = rpc(&r.core, "routine.run_now", json!({"id": review}))
        .await
        .unwrap();
    assert!(run["task"].is_string(), "{run}");
    let t0 = std::time::Instant::now();
    let runs = loop {
        let runs = rpc(&r.core, "routine.runs", json!({"id": review}))
            .await
            .unwrap();
        if runs["runs"][0]["state"] == "complete" {
            break runs;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "no run in 20 s: {runs}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(runs["runs"].as_array().unwrap().len(), 1);
    let sent = r.model.requests.lock().unwrap().clone();
    let brief = sent
        .iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .find(|t| t.contains("weekly review of the project"))
        .expect("the review's brief reached the model");
    assert!(brief.contains(&id), "it names the project: {brief}");
    assert!(brief.contains("theseus project show"), "{brief}");
    assert!(brief.contains("Decisions for you"), "{brief}");
}

/// Layer 1 still guards the owner's goal: an agent's edit of the acceptance of
/// a project the owner made waits for the owner and changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_edit_of_the_owners_acceptance_is_refused() {
    let r = rig_with_posture();
    let s = session(&r.core);
    let p = made(&r, json!({"review": "none"})).await;
    let id = p["project"]["id"].as_str().unwrap().to_string();
    // The agent's own plan edits apply at once: a milestone's date is its plan.
    let m1 = milestone(&r, &p, 1);
    r.calls(&[(
        "task_update",
        json!({"id": m1, "version": 1, "patch": {"due": "4h"}}),
    )]);
    turn(&r.core, &s, "move the first milestone").await;
    assert_eq!(r.task(&m1).version, 2);
    r.calls(&[(
        "task_update",
        json!({"id": id, "version": 1,
               "patch": {"acceptance": ["any mooring will do"]}}),
    )]);
    turn(&r.core, &s, "loosen the acceptance").await;
    let rec = r.task(&id);
    assert_eq!(
        rec.acceptance,
        ["every mooring has a depth"],
        "unchanged until the owner answers"
    );
    assert!(
        rec.proposal.is_some(),
        "the change waits as a proposal: {rec:?}"
    );
}

fn rig_with_posture() -> Rig {
    crate::tests_task_graph::rig_on(tempfile::tempdir().unwrap(), |c| {
        c.policy.enforcement = crate::policy::Posture::Notify;
    })
}

/// The model plans a milestone under no project: refused with the words. A
/// milestone under a project lands with its date, and the model's attach
/// writes the session's project.
#[tokio::test(flavor = "multi_thread")]
async fn the_model_plans_a_milestone_and_attaches_its_conversation() {
    let r = rig();
    let s = session(&r.core);
    r.calls(&[(
        "task_create",
        json!({"title": "Orphan", "kind": "milestone", "due": "1h"}),
    )]);
    turn(&r.core, &s, "a loose milestone").await;
    let (status, text) = results(&r.core, &s, "task.create").pop().unwrap();
    assert_eq!(status, crate::node::ResultStatus::Error, "{text}");
    assert!(text.contains("goes under a project"), "{text}");
    let p = made(&r, json!({"review": "none"})).await;
    let id = p["project"]["id"].as_str().unwrap().to_string();
    r.calls(&[(
        "task_create",
        json!({"title": "Moor the buoys", "kind": "milestone", "parent": id, "due": "6h"}),
    )]);
    turn(&r.core, &s, "add a milestone").await;
    let added = r.by_title("Moor the buoys");
    assert_eq!(
        (added.kind, added.parent.as_deref()),
        (TaskKind::Milestone, Some(id.as_str()))
    );
    assert!(added.due_at_ms.is_some());
    assert_eq!(reminders(&r.core).len(), 8, "its two wakes joined the six");
    r.calls(&[("project_attach", json!({"project": id}))]);
    turn(&r.core, &s, "attach this conversation").await;
    let rec: crate::session::SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
    assert_eq!(rec.project.as_deref(), Some(id.as_str()));
    // A task is no project.
    r.calls(&[("project_attach", json!({"project": added.id}))]);
    turn(&r.core, &s, "attach to the milestone").await;
    let (status, text) = results(&r.core, &s, "project.attach").pop().unwrap();
    assert_eq!(status, crate::node::ResultStatus::Error, "{text}");
    r.calls(&[("project_attach", json!({"project": "none"}))]);
    turn(&r.core, &s, "detach").await;
    let rec: crate::session::SessionRecord = r.core.store.get_session(&s).unwrap().unwrap();
    assert_eq!(rec.project, None);
}

// ------------------------------------------------------------- the slice

/// And 48,000 until `task_fan_out` joined the catalog (theseus-z9mze.3): its definition grows the tools
/// block by about 500 tokens. 48,250 and up hold (to 52,000, the sweep's end) for this suite, and 48,500 and up for the
/// others in this family of tests; 49,000 is the middle of the near end of that band.
const WINDOW: u64 = 49_000;

fn summary(text: &str) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: 9_000,
            output_tokens: 60,
            ..Default::default()
        },
        then: Box::new(Scripted::text(text)),
    }
}

fn words(tag: &str, tokens: usize) -> String {
    let mut s = format!("{tag} ");
    while s.len() < tokens * 4 {
        s.push_str("the keeper logged the tide and the wind before the lamp was lit. ");
    }
    s
}

/// The text of a request's last message.
fn last_text(q: &ProviderRequest) -> String {
    match &q.messages.last().unwrap()["content"] {
        Value::String(s) => s.clone(),
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn slice_block(q: &ProviderRequest) -> Option<String> {
    let blocks = q.messages.last().unwrap()["content"].as_array()?;
    let last = blocks.last()?["text"].as_str()?;
    last.starts_with(crate::project::slice::HEAD)
        .then(|| last.to_string())
}

/// An attached conversation's request ends with the project's slice, in every
/// request, through three compactions; a conversation with no project carries
/// none.
#[tokio::test(flavor = "multi_thread")]
async fn an_attached_conversation_keeps_the_slice_through_three_compactions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.summary_profile = Some("glm".into());
    // glm only summarizes here: an unrestricted model (theseus-a1wvq.3).
    cfg.serve_glm_unrestricted();
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(WINDOW),
            max_output_tokens: Some(2_000),
            ..Default::default()
        },
    );
    let model = Arc::new(FakeProvider::scripted(vec![]));
    let glm = Arc::new(FakeProvider::scripted(
        (1..=8)
            .map(|n| summary(&format!("SUMMARY {n}: the tides so far.")))
            .collect(),
    ));
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), glm.clone());
    let core = Core::build(parts).unwrap();
    let p = core
        .project_create(
            &serde_json::from_value(spec(json!({"review": "none"}))).unwrap(),
            "the CLI",
        )
        .unwrap();
    let project = p.project.id.clone();
    let attached = session(&core);
    let plain = session(&core);
    core.project_attach(
        &theseus_protocol::projects::ProjectAttachParams {
            session: attached.clone(),
            project: Some(project.clone()),
            ..Default::default()
        },
        "the CLI",
    )
    .unwrap();
    // A plain conversation: one turn, no slice.
    turn(&core, &plain, "hello there").await;
    let q = model.requests().pop().unwrap();
    assert!(slice_block(&q).is_none(), "{}", last_text(&q));
    let mut compactions = 0;
    for k in 0..40 {
        turn(&core, &attached, &words(&format!("turn{k}"), 6_000)).await;
        let q = model.requests().pop().unwrap();
        let slice = slice_block(&q)
            .unwrap_or_else(|| panic!("turn {k}: no slice in the last block: {}", last_text(&q)));
        assert!(slice.contains(&project), "{slice}");
        assert!(slice.contains("Chart every mooring"), "the goal: {slice}");
        assert!(slice.contains("⚑"), "the milestones: {slice}");
        assert!(slice.contains("Survey the moorings"), "{slice}");
        assert!(
            slice.len() as u64 <= crate::project::slice::MAX_TOKENS * 3,
            "{} bytes: {slice}",
            slice.len()
        );
        compactions = glm.requests().len();
        if compactions >= 3 {
            break;
        }
    }
    assert_eq!(compactions, 3, "three compactions happened");
    // And the next request, after the third, still has it.
    turn(&core, &attached, "and after the last compaction?").await;
    let q = model.requests().pop().unwrap();
    assert!(slice_block(&q).is_some(), "{}", last_text(&q));
    // `context.explain` names it, which `theseus why` prints.
    let why = rpc(&core, "context.explain", json!({"session_id": attached}))
        .await
        .unwrap();
    assert_eq!(why["compiled"]["project"]["project"], project, "{why}");
}

/// The model plans a project with a weekly review: the review is a routine,
/// which spends money on a schedule, so the call waits on one card. A no
/// makes nothing; a yes makes the project, its milestones (one frame, as the
/// model's own) and the routine, and the reminders go to its conversation.
#[tokio::test(flavor = "multi_thread")]
async fn the_models_project_waits_on_one_card_and_a_yes_makes_it_with_its_review() {
    let r = rig_with_posture();
    let s = session(&r.core);
    let input = json!({
        "title": "Harbour survey", "kind": "project",
        "objective": "Chart every mooring in the outer harbour",
        "milestones": [{"title": "Survey the moorings", "due": "1h"}, {"title": "Chart the channel", "due": "2h"}],
    });
    r.calls(&[("task_create", input.clone())]);
    turn(&r.core, &s, "plan a harbour survey project").await;
    let q = r.core.confirm_list().unwrap().pop().expect("a card");
    assert_eq!(q.tool, "task.create");
    assert!(
        q.reason
            .contains("Create project \"Harbour survey\" with 2 milestones and a weekly review"),
        "{}",
        q.reason
    );
    assert!(r.tasks().is_empty(), "nothing is made before the yes");
    // A no makes nothing.
    r.core
        .confirm_action(&q.correlation_id, false, Some("not yet"), "cli")
        .unwrap();
    until("the declined call's answer", || {
        results(&r.core, &s, "task.create")
            .last()
            .is_some_and(|(st, _)| *st == crate::node::ResultStatus::Declined)
    })
    .await;
    assert!(r.tasks().is_empty());
    // A yes makes it all.
    tokio::time::sleep(Duration::from_millis(100)).await;
    r.calls(&[("task_create", input)]);
    turn(&r.core, &s, "plan it again").await;
    let q = r.core.confirm_list().unwrap().pop().expect("a card");
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    until("the project", || {
        r.tasks().iter().any(|t| t.kind == TaskKind::Project)
    })
    .await;
    let all = r.tasks();
    let project = all.iter().find(|t| t.kind == TaskKind::Project).unwrap();
    assert!(project.origin.by_model, "the model wrote this goal");
    let review = project.review.clone().expect("the review is named");
    assert_eq!(
        all.iter().filter(|t| t.kind == TaskKind::Milestone).count(),
        2
    );
    let routines = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    assert_eq!(routines["routines"][0]["id"], review);
    until("the reminders", || reminders(&r.core).len() == 4).await;
    let (status, text) = results(&r.core, &s, "task.create").pop().unwrap();
    assert_eq!(status, crate::node::ResultStatus::Ok, "{text}");
    assert!(text.contains("weekly review is routine"), "{text}");
}

/// A project with `review: none` is plain bookkeeping: no card, no routine.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_with_no_review_needs_no_card_and_makes_no_routine() {
    let r = rig_with_posture();
    let s = session(&r.core);
    r.calls(&[(
        "task_create",
        json!({"title": "Paint the lighthouse", "kind": "project", "review": "none",
               "milestones": [{"title": "Scrape it"}]}),
    )]);
    turn(&r.core, &s, "plan a painting project").await;
    assert!(r.core.confirm_list().unwrap().is_empty());
    let all = r.tasks();
    let project = all.iter().find(|t| t.kind == TaskKind::Project).unwrap();
    assert_eq!(project.review, None);
    assert_eq!(all.len(), 2, "the project and its milestone");
    assert_eq!(
        all[1].due_at_ms, None,
        "an undated milestone reminds of nothing"
    );
    assert!(reminders(&r.core).is_empty());
    let routines = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    assert!(routines["routines"].as_array().unwrap().is_empty());
}
