//! A task's turns take the view (issue 44; the design's §7, "a task's own view"; `task_graph::place`,
//! `turn::view_step::task`): real turns, every place set to `view`. A task's first turn answers its brief
//! in a view; its later turn (a wake's, a report's) sees the first in its window; a check is a task like
//! any other; every exchange of a task is at the task's own place; and a task's request holds nothing of
//! its parent conversation's window that its presence may not see.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution};

use crate::bus::EventSink;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::exchange::{exchange, Exchange, Opener};
use crate::node::Body;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::test_support::{rows_100k, until_within};
use crate::tests_tasks::{answers_a_call, arranged, exec, parent_session, rig, Rig, ASK};
use crate::turn::TurnRequest;
use crate::Core;

/// What every task's brief opens with (`task::brief_text_in`): a request that holds it in a person's words
/// is a task's.
const TASK_HEAD: &str = "a background task started by session";

/// A message's text blocks (a tool's result is not a text block).
fn texts(m: &Value) -> Vec<String> {
    match &m["content"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(b) => (b.iter())
            .filter_map(|b| b["text"].as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

/// Every text block of the request's user messages, joined.
fn user_text(req: &ProviderRequest) -> String {
    (req.messages.iter())
        .filter(|m| m["role"] == "user")
        .flat_map(texts)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The newest user message's text blocks, joined.
fn last_text(req: &ProviderRequest) -> String {
    let last = req.messages.iter().rev().find(|m| m["role"] == "user");
    last.map(|m| texts(m).join("\n")).unwrap_or_default()
}

/// Whether `req` is a turn's request (it offers tools: a ladder's or a did line's offer none) of the task
/// whose brief says `brief`.
fn of_task(req: &ProviderRequest, brief: &str) -> bool {
    let text = user_text(req);
    !req.tools.is_empty() && text.contains(TASK_HEAD) && text.contains(brief)
}

/// The task requests of `brief`, in the order they were sent.
fn task_requests(r: &Rig, brief: &str) -> Vec<ProviderRequest> {
    (r.model.requests.lock().unwrap().iter())
        .filter(|q| of_task(q, brief))
        .cloned()
        .collect()
}

/// A rig whose every place, a task's own included, is set to `view`; `task` answers a task's requests.
fn view_rig(task: impl Fn(&ProviderRequest) -> Option<Scripted> + Send + Sync + 'static) -> Rig {
    rig(
        move |req| {
            if user_text(req).contains(TASK_HEAD) {
                if let Some(s) = task(req) {
                    return s;
                }
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            let last = last_text(req);
            if let Some(input) = last.split("CREATE ").nth(1) {
                let input: Value = serde_json::from_str(input).expect("a task.create input");
                return Scripted::tools("Starting it.", &[("t_task", "task_create", input)]);
            }
            Scripted::text("Noted.")
        },
        |c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)),
    )
}

/// The owner's message `CREATE <input>`, quoting `ASK` so the arrangement holds.
fn create(input: &Value) -> String {
    format!("{ASK} CREATE {input}")
}

/// The task the parent's last call opened, by the brief its session holds.
fn task_by_brief(core: &Core, brief: &str) -> Execution {
    (core.kernel.tasks(None).unwrap().into_iter())
        .find(|e| {
            (core.store.session_nodes(&e.session_id).unwrap().iter()).any(
                |(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text.contains(brief)),
            )
        })
        .expect("the task")
}

/// The exchanges of a task's turns, oldest first.
fn exchanges(core: &Core, task: &Execution) -> Vec<Exchange> {
    let mut turns: Vec<String> = Vec::new();
    for (_, n) in core.store.session_nodes(&task.session_id).unwrap() {
        if let Some(t) = n.turn_id.filter(|t| !turns.contains(t)) {
            turns.push(t);
        }
    }
    (turns.iter())
        .filter_map(|t| exchange(&core.store, t).unwrap())
        .collect()
}

/// The strategy of a session's last compilation.
fn strategy(core: &Core, session: &str) -> String {
    let rec: SessionRecord = core.store.get_session(session).unwrap().unwrap();
    let id = rec.compilation_id.expect("a compilation");
    core.store.get_compilation(&id).unwrap().unwrap().strategy
}

/// No turn of `task` handed back to the transcript.
fn none_handed_back(core: &Core, task: &Execution) {
    let turns: Vec<String> = exchanges(core, task)
        .into_iter()
        .map(|e| e.turn_id)
        .collect();
    let back: Vec<Value> = (rows_100k(core, "context.view_fallback").into_iter())
        .filter(|r| turns.iter().any(|t| r["turn_id"] == t.as_str()))
        .collect();
    assert!(back.is_empty(), "a task's turn handed back: {back:?}");
}

async fn complete(core: &Core, e: &Execution) {
    until_within("the task complete", 30, || {
        exec(core, &e.id).state == ExecState::Complete
    })
    .await;
}

const COUNT: &str = "COUNT-VIEW: count the boats in the north quay";

/// The key test: a task's first turn answers its brief in a view at its own place, and its wake's turn
/// sees the first turn's answer in its window, with the brief still at its head; each turn's exchange is at
/// the task's place, the first opened by its brief, the second by its wake.
#[tokio::test]
async fn a_tasks_turns_take_the_view_at_its_own_place_and_its_second_sees_its_first() {
    let r = view_rig(|req| {
        let last = last_text(req);
        Some(match () {
            _ if last.contains("⏰ wake") => Scripted::text("SECOND: fifteen boats now."),
            _ if answers_a_call(req) => Scripted::text("FIRST: fourteen boats so far."),
            _ => Scripted::tools(
                "Counting.",
                &[(
                    "t_wake",
                    "wake_at",
                    json!({"after": "1s", "note": "count the boats again"}),
                )],
            ),
        })
    });
    let parent = parent_session(&r.core);
    let input = json!({"brief": COUNT, "arrangement": arranged()});
    crate::test_support::turn(&r.core, &parent, &create(&input)).await;
    let task = task_by_brief(&r.core, COUNT);
    complete(&r.core, &task).await;

    let reqs = task_requests(&r, COUNT);
    assert!(
        reqs.len() >= 3,
        "two calls, then the wake's: {}",
        reqs.len()
    );
    let wake = reqs
        .iter()
        .find(|q| last_text(q).contains("⏰ wake"))
        .expect("the wake's request");
    let seen = user_text(wake) + &serde_json::to_string(&wake.messages).unwrap();
    assert!(seen.contains("FIRST: fourteen boats so far."), "{seen}");
    assert!(
        user_text(wake).contains(COUNT),
        "the brief heads its window"
    );
    assert_eq!(strategy(&r.core, &task.session_id), "view");
    none_handed_back(&r.core, &task);

    let place = crate::task_graph::place::of_session(&task.session_id);
    let ex = exchanges(&r.core, &task);
    assert_eq!(ex.len(), 2, "{ex:?}");
    assert!(ex.iter().all(|e| e.place == place), "{ex:?}");
    assert_eq!(ex[0].opened_by, Opener::Message);
    let brief = (r
        .core
        .store
        .session_nodes(&task.session_id)
        .unwrap()
        .into_iter())
    .find(|(_, n)| n.turn_id.is_none() && matches!(n.body, Body::UserMessage { .. }))
    .map(|(_, n)| n.id)
    .unwrap();
    assert_eq!(ex[0].ask.as_deref(), Some(brief.as_str()));
    assert_eq!(ex[1].opened_by, Opener::Wake);
    assert!(ex.iter().all(|e| e.end_ms.is_some()));
}

const MAKER: &str = "MAKER-VIEW: count the gulls on the pier";
const CLAIM: &str = "REPORT: there are 42 gulls on the pier.";
const CHECKING: &str = "CHECK-VIEW: establish on your own how many gulls the pier holds";

/// A check is a task: its first turn takes the view at its own place and answers, with its brief and the
/// claim it checks, and nothing of the checked task's own turns: not its brief.
#[tokio::test]
async fn a_check_takes_the_view_and_holds_nothing_of_the_checked_tasks_turns() {
    let r = view_rig(|req| {
        let text = user_text(req);
        Some(match () {
            _ if text.contains(CHECKING) => Scripted::text("CHECKED: the claim holds."),
            _ if text.contains(MAKER) => Scripted::text(CLAIM),
            _ => return None,
        })
    });
    let parent = parent_session(&r.core);
    let made = json!({"brief": MAKER, "arrangement": arranged()});
    crate::test_support::turn(&r.core, &parent, &create(&made)).await;
    let maker = task_by_brief(&r.core, MAKER);
    complete(&r.core, &maker).await;
    let short = crate::task::short(&maker.session_id);
    let check = json!({"brief": CHECKING, "check_of": short});
    crate::test_support::turn(&r.core, &parent, &format!("CREATE {check}")).await;
    let c = task_by_brief(&r.core, CHECKING);
    complete(&r.core, &c).await;

    let reqs = task_requests(&r, CHECKING);
    let first = reqs.first().expect("the check's request");
    let text = user_text(first);
    assert!(text.contains(CLAIM), "the claim it checks: {text}");
    assert!(!text.contains(MAKER), "the checked task's brief: {text}");
    assert_eq!(strategy(&r.core, &c.session_id), "view");
    none_handed_back(&r.core, &c);
    let place = crate::task_graph::place::of_session(&c.session_id);
    let ex = exchanges(&r.core, &c);
    assert!(
        !ex.is_empty() && ex.iter().all(|e| e.place == place),
        "{ex:?}"
    );
}

const OUTER: &str = "OUTER-VIEW: have a subtask count the gulls, then say what it found";
const INNER: &str = "INNER-VIEW: count the gulls";
/// What only a subtask's brief says (`task_lineage::Lineage::note`): the root task's brief.
const SUBTASK: &str = "Root goal, the brief of this tree's first task";

/// A report to a task opens a turn of that task's, in a view at its own place: the subtask's report is
/// its message, and its first turn is in its window.
#[tokio::test]
async fn a_report_to_a_task_is_a_view_turn_at_the_tasks_place() {
    let r = view_rig(|req| {
        let (text, last) = (user_text(req), last_text(req));
        Some(match () {
            _ if text.contains(SUBTASK) => Scripted::text("INNER FOUND: 42 gulls."),
            _ if last.contains("[Report from task") => Scripted::text("OUTER: it found 42."),
            _ if text.contains(OUTER) && answers_a_call(req) => {
                Scripted::text("OUTER STARTED: waiting on the count.")
            }
            _ if text.contains(OUTER) => Scripted::tools(
                "Starting the count, and a late look.",
                &[
                    (
                        "t_inner",
                        "task_create",
                        json!({"brief": INNER, "wake_parent": true}),
                    ),
                    (
                        "t_late",
                        "wake_at",
                        json!({"after": "1h", "note": "look again"}),
                    ),
                ],
            ),
            _ => return None,
        })
    });
    let parent = parent_session(&r.core);
    let input = json!({"brief": OUTER, "arrangement": arranged()});
    crate::test_support::turn(&r.core, &parent, &create(&input)).await;
    let outer = task_by_brief(&r.core, OUTER);
    until_within("the report's turn", 30, || {
        exchanges(&r.core, &outer)
            .iter()
            .any(|e| e.opened_by == Opener::Report && e.end_ms.is_some())
    })
    .await;

    let place = crate::task_graph::place::of_session(&outer.session_id);
    let ex = exchanges(&r.core, &outer);
    assert!(ex.iter().all(|e| e.place == place), "{ex:?}");
    let reqs = task_requests(&r, OUTER);
    let report = (reqs.iter())
        .find(|q| last_text(q).contains("[Report from task"))
        .expect("the report's request");
    let seen = serde_json::to_string(&report.messages).unwrap();
    assert!(seen.contains("INNER FOUND: 42 gulls."), "{seen}");
    assert!(
        seen.contains("OUTER STARTED: waiting on the count."),
        "its first turn: {seen}"
    );
    none_handed_back(&r.core, &outer);
}

const SECRET: &str = "SECRET-HARBOUR: the spare key is under the third bollard";
const ERRAND: &str = "ERRAND-VIEW: list the boats in the south quay";

/// A turn of `sid` typed by an agent's process at the terminal: not the owner in person.
async fn agents_turn(core: &Arc<Core>, sid: &str, input: &str) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let mark = crate::view_gate::test_mark(&core.runner, sid, false);
    let sink = EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// A task started by a turn that was not the owner alone holds nothing of its parent conversation's
/// window: the conversation's own next turn lays the earlier words, the task's request does not.
#[tokio::test]
async fn a_tasks_request_holds_nothing_of_the_parents_window_it_may_not_see() {
    let r = view_rig(|req| {
        user_text(req)
            .contains(ERRAND)
            .then(|| Scripted::text("Listed them."))
    });
    let parent = parent_session(&r.core);
    agents_turn(&r.core, &parent, SECRET).await;
    let input = json!({"brief": ERRAND, "arrangement": arranged()});
    agents_turn(&r.core, &parent, &create(&input)).await;
    let task = task_by_brief(&r.core, ERRAND);
    complete(&r.core, &task).await;
    agents_turn(&r.core, &parent, "And now?").await;

    let all = r.model.requests.lock().unwrap().clone();
    let last = (all.iter().rev())
        .find(|q| !q.tools.is_empty() && last_text(q).contains("And now?"))
        .expect("the conversation's next request");
    assert!(
        serde_json::to_string(&last.messages)
            .unwrap()
            .contains(SECRET),
        "the conversation's own window lays it"
    );
    let reqs = task_requests(&r, ERRAND);
    assert!(!reqs.is_empty());
    for q in &reqs {
        let sent = serde_json::to_string(q).unwrap();
        assert!(!sent.contains("third bollard"), "{sent}");
    }
    assert_eq!(strategy(&r.core, &task.session_id), "view");
    none_handed_back(&r.core, &task);
}
