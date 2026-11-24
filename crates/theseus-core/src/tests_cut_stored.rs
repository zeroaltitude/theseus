//! What the core keeps stops at a known value's edge, through the code that
//! writes it: a plan's summary in its call's gate record, a failed task's
//! reason, a fault's reason, a prompt's title, a started task's title,
//! wording and quote, a wake's words and its done row, a history line, and
//! an arrangement's pieces. "Whole or absent" is `tests_cut_sites`'s. All
//! values invented.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::task_start::TaskStartParams;
use theseus_store::kinds;

use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::node::Body;
use crate::provider::{ProviderError, Scripted};
use crate::session::SessionRecord;
use crate::tests_cut_sites::{knowing_core, tool_results, whole_or_absent, V};
use crate::tests_places::{answers_a_call, result_of, session, turn, OWNER};
use crate::turn::TurnRequest;
use crate::Core;

/// 50 characters, then V across a title's cut at 60.
fn across() -> String {
    format!("{}{V} and after", "x".repeat(50))
}

fn titled() -> String {
    format!("{}…", "x".repeat(50))
}

fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(4000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

/// The plan summary the gate record of the call `id` keeps.
fn summary_of(core: &Core, sid: &str, id: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolCall {
                tool_use_id,
                gate: Some(g),
                ..
            } if tool_use_id == id => g.plan.map(|p| p.summary),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no planned call {id}"))
}

/// A model that calls `tool` with `input` as the call `id` once, then says
/// it is done.
fn calling(
    id: &'static str,
    tool: &'static str,
    input: Value,
) -> impl Fn(&crate::provider::ProviderRequest) -> Scripted + Send + Sync + 'static {
    move |req| {
        if answers_a_call(req) {
            return Scripted::text("Done.");
        }
        Scripted::tools("Doing it.", &[(id, tool, input.clone())])
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_creates_plan_summary_stops_before_a_known_value() {
    let input = json!({"brief": across(), "arrangement": crate::tests_tasks::arranged()});
    let r = knowing_core(V, |_| {}, calling("t_task", "task_create", input));
    r.core.push.ensure(&r.core).await.unwrap();
    let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &sid, crate::tests_tasks::ASK).await;
    let summary = summary_of(&r.core, &sid, "t_task");
    assert_eq!(summary, format!("start a task: {}", titled()));
    whole_or_absent("the plan's summary", &summary, V);
}

#[tokio::test]
async fn a_wakes_plan_summary_stops_before_a_known_value() {
    let input = json!({"after": "1h", "note": across()});
    let r = knowing_core(V, |_| {}, calling("w1", "wake_at", input));
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "remind me").await;
    let summary = summary_of(&r.core, &sid, "w1");
    assert_eq!(summary, format!("wake in 1h: {}", titled()));
    whole_or_absent("the plan's summary", &summary, V);
}

/// A task whose model call fails with V again and again: the reason its
/// execution ended, in its row, holds each copy whole or not at all.
#[tokio::test]
async fn a_failed_tasks_reason_holds_a_known_value_whole_or_not_at_all() {
    use crate::tests_tasks::{arranged, first_user, parent_session, ASK};
    let r = crate::tests_tasks::rig_knowing(
        |req| {
            if first_user(req).contains("CHILD: fails") {
                return Scripted::Fail(ProviderError::InvalidRequest {
                    status: 400,
                    message: V.repeat(30),
                });
            }
            if crate::tests_tasks::answers_a_call(req) {
                return Scripted::text("Started.");
            }
            Scripted::tools(
                "Starting.",
                &[(
                    "t_1",
                    "task_create",
                    json!({"brief": "CHILD: fails", "arrangement": arranged()}),
                )],
            )
        },
        |_| {},
        vec![(V.into(), "cut_test".into())],
    );
    let parent = parent_session(&r.core);
    crate::tests_tasks::turn(&r.core, &parent, ASK).await;
    crate::test_support::until("the task's failure", || {
        !rows(&r.core, "execution.failed").is_empty()
    })
    .await;
    let reason = rows(&r.core, "execution.failed")[0]["reason"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(reason.contains("not shown"), "the reason was cut: {reason}");
    whole_or_absent("the failed task's reason", &reason, V);
}

/// A turn on the owner's input, its result as it came.
async fn turn_result(
    core: &Arc<Core>,
    sid: &str,
    input: Option<&str>,
    prompt: Option<crate::mcp::prompts::PromptInput>,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let mark = crate::view_gate::test_mark(&core.runner, sid, true);
    let sink = EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(TurnRequest {
            prompt,
            surface: None,
            session: rec,
            input: input.map(str::to_string),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// A frame fails with V again and again in its error: the reason in the
/// turn's `turn.failed` row holds each copy whole or not at all.
#[tokio::test]
async fn a_faults_reason_holds_a_known_value_whole_or_not_at_all() {
    let r = knowing_core(
        V,
        |_| {},
        calling("p1", "fs_read", json!({"path": "open/README-copy.md"})),
    );
    let sid = session(&r.core, None);
    r.core.store.fail_turn_frame_saying(
        format!("an injected fault: {}", V.repeat(30)),
        |records| {
            records.iter().any(|r| {
                let p: Value = serde_json::from_slice(&r.payload).unwrap_or(Value::Null);
                r.kind == kinds::NODE && p["body"]["kind"] == "tool_result"
            })
        },
    );
    let err = turn_result(&r.core, &sid, Some("run it"), None).await;
    assert!(err.is_err(), "the turn faults");
    let failed = rows(&r.core, "turn.failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    let reason = failed[0]["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("not shown"), "the reason was cut: {reason}");
    whole_or_absent("the fault's reason", reason, V);
}

#[tokio::test]
async fn a_prompts_title_stops_before_a_known_value() {
    let r = knowing_core(V, |_| {}, |_| Scripted::text("Noted."));
    let sid = session(&r.core, None);
    let prompt = crate::mcp::prompts::PromptInput {
        author: "prompt:srv/p".into(),
        messages: vec![crate::mcp::prompts::PromptMessage {
            text: across(),
            files: vec![],
        }],
    };
    // As `turn.submit` sends a prompt's turn: its text is the input too.
    turn_result(&r.core, &sid, Some(&across()), Some(prompt))
        .await
        .unwrap();
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let title = rec.title.unwrap();
    assert_eq!(title, titled());
    whole_or_absent("the title", &title, V);
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// `task.start` with no title: the task's title, and its quote's first line
/// in its arrangement, stop before V.
#[tokio::test]
async fn a_started_tasks_title_and_quote_stop_before_a_known_value() {
    let r = crate::tests_tasks::rig_knowing(
        |_| Scripted::text("ok"),
        |_| {},
        vec![(V.into(), "cut_test".into())],
    );
    let started = r
        .core
        .task_start(
            &TaskStartParams {
                brief: across(),
                quotes: vec![across()],
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    let task = crate::task_graph::all(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|t| t.session.as_deref() == Some(started.session_id.as_str()))
        .expect("the task's record");
    assert_eq!(task.title, titled());
    whole_or_absent("the task's title", &task.title, V);
    let pieces = r
        .core
        .store
        .session_nodes(&started.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::Arrangement { pieces, .. } => Some(pieces),
            _ => None,
        })
        .expect("the quotes' arrangement");
    assert_eq!(pieces[0].first_line, titled());
    whole_or_absent("the quote's first line", &pieces[0].first_line, V);
}

/// A `task.start` refused from a shared place: its row says what was asked,
/// and that wording stops before V.
#[tokio::test]
async fn a_refused_task_starts_wording_stops_before_a_known_value() {
    let r = crate::tests_tasks::rig_knowing(
        |_| Scripted::text("ok"),
        |_| {},
        vec![(V.into(), "cut_test".into())],
    );
    const LAB: u64 = 314_159_265_358_979_323;
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    let stranger = Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: LAB.to_string(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    let start = TaskStartParams {
        brief: across(),
        ..Default::default()
    };
    r.core.task_start(&start, stranger).unwrap_err();
    let refused = rows(&r.core, "approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    let what = refused[0]["what"].as_str().unwrap_or_default();
    assert_eq!(what, format!("starting a task ({})", titled()));
    whole_or_absent("the refusal's wording", what, V);
}

/// The id of the one pending wake.
fn the_wake(core: &Core) -> String {
    let pending = core.kernel.pending_wakes().unwrap();
    assert_eq!(pending.len(), 1, "one wake pending");
    pending[0].1.id.clone()
}

#[tokio::test]
async fn a_done_wakes_row_stops_before_a_known_value() {
    let note = format!("{}{V} after", "x".repeat(190));
    let input = json!({"after": "1h", "note": note});
    let r = knowing_core(V, |_| {}, calling("w1", "wake_at", input));
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "remind me").await;
    let id = the_wake(&r.core);
    crate::tests_mcp_memory::rpc(&r.core, Surface::Cli, "wake.done", json!({"wake": id}))
        .await
        .unwrap();
    let done = rows(&r.core, "wake.done");
    assert_eq!(done.len(), 1, "{done:?}");
    let kept = done[0]["note"].as_str().unwrap_or_default();
    assert_eq!(kept, "x".repeat(190));
    whole_or_absent("the done row's note", kept, V);
}

/// At the cap, a wake is refused with the pending ones listed by title.
#[tokio::test]
async fn a_refused_wakes_list_stops_before_a_known_value() {
    let r = knowing_core(
        V,
        |c| c.kernel.max_wakes = 1,
        |req| {
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            match crate::tests_places::last_user(req).as_str() {
                l if l.contains("first") => Scripted::tools(
                    "Setting it.",
                    &[("w1", "wake_at", json!({"after": "1h", "note": across()}))],
                ),
                _ => Scripted::tools(
                    "Setting another.",
                    &[("w2", "wake_at", json!({"after": "2h", "note": "another"}))],
                ),
            }
        },
    );
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "the first").await;
    turn(&r.core, &sid, "the second").await;
    let refused = result_of(&r.core, &sid, "w2");
    assert!(refused.starts_with("Refused:"), "{refused}");
    assert!(refused.contains(&format!(": {}", titled())), "{refused}");
    whole_or_absent("the refusal's list", &refused, V);
}

#[tokio::test]
async fn a_cancelled_wakes_words_stop_before_a_known_value() {
    let id: Arc<Mutex<String>> = Arc::default();
    let shared = id.clone();
    let r = knowing_core(
        V,
        |_| {},
        move |req| {
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            match crate::tests_places::last_user(req).as_str() {
                l if l.contains("cancel") => Scripted::tools(
                    "Cancelling.",
                    &[(
                        "c1",
                        "wake_cancel",
                        json!({"id": shared.lock().unwrap().clone()}),
                    )],
                ),
                _ => Scripted::tools(
                    "Setting it.",
                    &[("w1", "wake_at", json!({"after": "1h", "note": across()}))],
                ),
            }
        },
    );
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "remind me").await;
    *id.lock().unwrap() = the_wake(&r.core);
    turn(&r.core, &sid, "cancel it").await;
    let said = result_of(&r.core, &sid, "c1");
    assert!(said.starts_with("Cancelled wake"), "{said}");
    assert!(said.contains(&format!("\"{}\"", titled())), "{said}");
    whole_or_absent("the cancel's words", &said, V);
}

/// The owner's words in an earlier exchange run past a history line's cut:
/// the line the history tool returns, as kept and as sent, holds V whole or
/// not at all.
#[tokio::test]
async fn a_history_line_through_the_tool_stops_at_a_known_value() {
    let r = knowing_core(
        V,
        |_| {},
        |req| {
            if answers_a_call(req) {
                return Scripted::text("Found it.");
            }
            if crate::tests_places::last_user(req).contains("look back") {
                return Scripted::tools("Looking.", &[("h1", "context_history", json!({}))]);
            }
            Scripted::text("Noted.")
        },
    );
    let sid = session(&r.core, None);
    turn(
        &r.core,
        &sid,
        &format!("{}{V} and the rest", "x".repeat(290)),
    )
    .await;
    turn(&r.core, &sid, "look back").await;
    let kept = result_of(&r.core, &sid, "h1");
    assert!(
        kept.contains(&format!("{}\u{2026}", "x".repeat(290))),
        "{kept}"
    );
    whole_or_absent("the kept history page", &kept, V);
    let sent: Vec<String> = r.requests().iter().flat_map(tool_results).collect();
    assert!(!sent.is_empty());
    for s in &sent {
        whole_or_absent("the sent history page", s, V);
    }
}

/// A task arranged from the owner's long message: the piece's text and its
/// first line, as the task's arrangement keeps them, hold V whole or not at
/// all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_arrangements_piece_holds_a_known_value_whole_or_not_at_all() {
    const ASKED: &str = "Survey the harbour moorings for the board";
    let message = format!("{ASKED} {}", V.repeat(800));
    let arrangement = json!({"pieces": [{"quote": ASKED, "role": "objective"}]});
    let r = knowing_core(
        V,
        |_| {},
        calling(
            "t_task",
            "task_create",
            json!({"brief": "Survey the moorings.", "arrangement": arrangement}),
        ),
    );
    r.core.push.ensure(&r.core).await.unwrap();
    let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &sid, &message).await;
    let task = crate::task_graph::all(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|t| t.title.starts_with("Survey the moorings"))
        .expect("the task's record");
    let task_session = task.session.expect("its session");
    let pieces = r
        .core
        .store
        .session_nodes(&task_session)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::Arrangement { pieces, .. } => Some(pieces),
            _ => None,
        })
        .expect("the task's arrangement");
    let text = pieces[0].text.clone().expect("the piece's text");
    assert!(text.contains("not shown"), "the piece was cut");
    whole_or_absent("the piece's text", &text, V);
    assert!(
        pieces[0].first_line.starts_with(ASKED),
        "{}",
        pieces[0].first_line
    );
    whole_or_absent("the piece's first line", &pieces[0].first_line, V);
}

/// A page asked for a place whose name holds a known value on two lines:
/// the page has no exchange, and its heading, as kept, holds neither line.
#[tokio::test]
async fn a_history_pages_asked_place_stops_before_a_known_value_on_two_lines() {
    const TWO: &str = "kelp-one-0417\nferry-two-0417";
    let r = knowing_core(
        TWO,
        |_| {},
        |req| {
            if answers_a_call(req) {
                return Scripted::text("Found it.");
            }
            let place = json!({"place": format!("place:{TWO}")});
            Scripted::tools("Looking.", &[("h1", "context_history", place)])
        },
    );
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "look back").await;
    let kept = result_of(&r.core, &sid, "h1");
    assert!(kept.contains("of place:\u{2026}'s history"), "{kept}");
    for half in TWO.lines() {
        assert!(!kept.contains(half), "the kept page holds {half:?}: {kept}");
    }
}
