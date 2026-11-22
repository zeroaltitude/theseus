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
const TASK_HEAD: &str = ": a background task";

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

/// The brief of the task whose request `req` is: the last paragraph of the first text block that holds a
/// task's brief (its opening heads its window); none for a conversation's request.
fn own_brief(req: &ProviderRequest) -> Option<String> {
    (req.messages.iter())
        .filter(|m| m["role"] == "user")
        .flat_map(texts)
        .find(|t| t.contains("[Task ") && t.contains(TASK_HEAD))
        .and_then(|t| t.rsplit("\n\n").next().map(str::to_string))
}

/// The task requests of `brief` (a turn's: it offers tools, and a ladder's or a did line's offer none),
/// in the order they were sent.
fn task_requests(r: &Rig, brief: &str) -> Vec<ProviderRequest> {
    (r.model.requests.lock().unwrap().iter())
        .filter(|q| !q.tools.is_empty() && own_brief(q).as_deref() == Some(brief))
        .cloned()
        .collect()
}

/// A rig whose every place, a task's own included, is set to `view`; `task` answers a task's requests, by
/// its brief (none: it is done). The conversation's `CREATE <input>` starts a task with that input.
fn view_rig(
    task: impl Fn(&ProviderRequest, &str) -> Option<Scripted> + Send + Sync + 'static,
) -> Rig {
    rig(
        move |req| {
            if let Some(brief) = own_brief(req) {
                return task(req, &brief).unwrap_or_else(|| Scripted::text("Done."));
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            let last = last_text(req);
            if let Some(input) = last.split("CREATE ").nth(1) {
                // The first JSON value: what follows the message in its user turn is not the input.
                let input: Value = (serde_json::Deserializer::from_str(input).into_iter())
                    .next()
                    .expect("a task.create input")
                    .expect("valid JSON");
                return Scripted::tools("Starting it.", &[("t_task", "task_create", input)]);
            }
            Scripted::text("Noted.")
        },
        |c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)),
    )
}

/// What a test that waited in vain shows: each of `task`'s exchanges, and the newest words of each of
/// its requests.
fn state_of(r: &Rig, task: &Execution, brief: &str) -> String {
    let ex: Vec<String> = (exchanges(&r.core, task).iter())
        .map(|e| format!("{:?} {} end={:?}", e.opened_by, e.place, e.end_ms))
        .collect();
    let asked: Vec<String> = (task_requests(r, brief).iter()).map(last_text).collect();
    format!("exchanges {ex:?}; requests' last words {asked:?}")
}

/// The owner's message `CREATE <input>`, quoting `ASK` so the arrangement holds.
fn create(input: &Value) -> String {
    format!("{ASK} CREATE {input}")
}

/// The owner's `ASK`, said on its own in `parent`, and an arrangement that names it by node (a quote of it
/// would also match the message that starts the task, whose input holds the quote).
async fn asked_apart(core: &Arc<Core>, parent: &str) -> Value {
    crate::test_support::turn(core, parent, ASK).await;
    let node = (core.store.session_nodes(parent).unwrap().into_iter())
        .find(|(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text == ASK))
        .map(|(_, n)| n.id)
        .expect("the owner's message");
    json!({"pieces": [{"node": node, "role": "objective"}]})
}

/// The task the parent's last call opened, by the brief its session holds.
fn task_by_brief(core: &Core, brief: &str) -> Execution {
    (core.kernel.tasks(None).unwrap().into_iter())
        .find(|e| {
            (core.store.session_nodes(&e.session_id).unwrap().iter()).any(
                |(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text.contains(brief)),
            )
        })
        .unwrap_or_else(|| panic!("no task holds {brief:?}: {}", calls_said(core)))
}

/// Every tool result in the store's sessions, for a test that found no task.
fn calls_said(core: &Core) -> String {
    let all: Vec<SessionRecord> = core.store.list_sessions().unwrap_or_default();
    let said: Vec<String> = (all.iter())
        .flat_map(|r| core.store.session_nodes(&r.session_id).unwrap_or_default())
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult { tool, content, .. } => Some(format!("{tool}: {content}")),
            _ => None,
        })
        .collect();
    format!("{said:?}")
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
    let r = view_rig(|req, _| {
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
    // Its opening is laid once in every request: at the head of the window, never again in the tail.
    for q in &reqs {
        assert_eq!(
            user_text(q).matches(TASK_HEAD).count(),
            1,
            "{}",
            user_text(q)
        );
    }
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
    let r = view_rig(|_, brief| match brief {
        CHECKING => Some(Scripted::text("CHECKED: the claim holds.")),
        MAKER => Some(Scripted::text(CLAIM)),
        _ => None,
    });
    let parent = parent_session(&r.core);
    // The message the arrangement quotes, apart from the one that starts the maker: the check's pieces
    // copy the quoted message, and it should not carry the maker's brief.
    let arrangement = asked_apart(&r.core, &parent).await;
    let made = json!({"brief": MAKER, "arrangement": arrangement, "fidelity_ack": true});
    crate::test_support::turn(&r.core, &parent, &format!("CREATE {made}")).await;
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

/// A report to a task opens a turn of that task's, in a view at its own place: the subtask's report is
/// its message, and its first turn is in its window.
#[tokio::test]
async fn a_report_to_a_task_is_a_view_turn_at_the_tasks_place() {
    report_into_a_task(false).await;
}

/// The same in a shared place: the subtask's report into a task whose conversation is a shared channel is
/// shown to the task (`view_gate::report_shown` reads the task's exchange as where it speaks), not held.
#[tokio::test]
async fn a_report_into_a_task_in_a_shared_place_is_shown_there() {
    report_into_a_task(true).await;
}

/// A conversation in a shared channel, as the owner's binding makes one.
fn shared_parent(core: &Arc<Core>) -> String {
    let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox
        .bind_place("channel:77", &rec.session_id)
        .unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: "discord:channel:77".into(),
        name: "the lab".into(),
        private: false,
        ..Default::default()
    });
    rec.session_id
}

async fn report_into_a_task(shared: bool) {
    let r = view_rig(|req, brief| {
        let last = last_text(req);
        Some(match brief {
            INNER => Scripted::text("INNER FOUND: 42 gulls."),
            OUTER if last.contains("[Report from task") => Scripted::text("OUTER: it found 42."),
            OUTER if answers_a_call(req) => Scripted::text("OUTER STARTED: waiting on the count."),
            OUTER => Scripted::tools(
                "Starting the count, and a late look.",
                &[
                    (
                        "t_inner",
                        "task_create",
                        json!({"brief": INNER, "wake_parent": true, "arrangement": {"pieces": [
                            {"quote": "have a subtask count the gulls", "role": "objective"}]}}),
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
    let parent = match shared {
        true => shared_parent(&r.core),
        false => parent_session(&r.core),
    };
    // The quoted message apart: the outer task's arrangement copies it, and the subtask's quote must be
    // found in the outer task's brief alone.
    let arrangement = asked_apart(&r.core, &parent).await;
    let input = json!({"brief": OUTER, "arrangement": arrangement, "fidelity_ack": true});
    crate::test_support::turn(&r.core, &parent, &format!("CREATE {input}")).await;
    let outer = task_by_brief(&r.core, OUTER);
    let t0 = std::time::Instant::now();
    while !(exchanges(&r.core, &outer).iter())
        .any(|e| e.opened_by == Opener::Report && e.end_ms.is_some())
    {
        let waited = t0.elapsed() > std::time::Duration::from_secs(30);
        assert!(!waited, "no report's turn: {}", state_of(&r, &outer, OUTER));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

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
    let r = view_rig(|_, brief| (brief == ERRAND).then(|| Scripted::text("Listed them.")));
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

const NOPROOF: &str = "NOPROOF-VIEW: count the herons on the east marsh";

/// A task started by `task.start` (no set-up proof) reads its own history in a later turn: its history
/// tool's point of view is the one its view is assembled under (its own place), so it pages and opens its
/// first turn's exchange, and reads no other place.
#[tokio::test]
async fn a_task_with_no_proof_pages_and_opens_its_own_history_in_a_later_turn() {
    use crate::history_tool::{answer, pov_of, Board};
    use crate::view::history::HistoryArgs;
    let r = view_rig(|req, brief| {
        (brief == NOPROOF).then(|| match () {
            _ if last_text(req).contains("⏰ wake") => Scripted::text("SECOND: herons again."),
            _ if answers_a_call(req) => Scripted::text("FIRST-HERONS: nine herons so far."),
            _ => Scripted::tools(
                "Counting.",
                &[(
                    "t_wake",
                    "wake_at",
                    json!({"after": "1s", "note": "count again"}),
                )],
            ),
        })
    });
    let started = r
        .core
        .task_start(
            &crate::tests_task_brief::params(NOPROOF, None),
            crate::tests_task_brief::cli(),
        )
        .unwrap();
    let task = task_by_brief(&r.core, NOPROOF);
    assert_eq!(task.session_id, started.session_id);
    complete(&r.core, &task).await;
    let ex = exchanges(&r.core, &task);
    assert_eq!(ex.len(), 2, "{}", state_of(&r, &task, NOPROOF));
    let (first, last) = (&ex[0].turn_id, &ex[1].turn_id);
    let sid = task.session_id.as_str();
    let pov = pov_of(&r.core, sid, last, None, crate::places::PlaceClass::Private);
    let place = crate::task_graph::place::of_session(sid);
    assert_eq!(pov.anchor, crate::view::Anchor::Place(place.clone()));
    let args = |v: Value| serde_json::from_value::<HistoryArgs>(v).unwrap();
    let ask = |a: HistoryArgs| {
        let (core, pov) = (r.core.clone(), pov.clone());
        let last = last.clone();
        async move { answer(&core, &Board::default(), (sid, &last), &pov, &a).await }
    };
    let opened = ask(args(json!({"open": first}))).await.unwrap();
    assert!(opened.text.contains("FIRST-HERONS"), "{}", opened.text);
    let page = ask(args(json!({}))).await.unwrap();
    assert!(
        !page.text.contains("cannot be established"),
        "{}",
        page.text
    );
    let elsewhere = ask(args(json!({"place": "local"}))).await;
    assert!(elsewhere.is_err(), "another place: {elsewhere:?}");
}
