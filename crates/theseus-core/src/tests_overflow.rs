//! A request past the model's window (theseus-9p88), through the whole core
//! with a scripted stand-in provider:
//! - a 400 "prompt is too long": the turn recompiles with a ring by the
//!   provider's numbers, calls once more, and answers;
//! - an answer cut at the window (`model_context_window_exceeded`): its calls
//!   do not run, the call is made once more on a ring that leaves the cut
//!   answer out, and the reply is the retry's;
//! - a retry that passes the window too, or a ring with nothing to drop: the
//!   turn fails with an error that names the window and the estimate, the
//!   execution waits on input at once, and the next message answers.
//!
//! Before, the 400 failed the turn as an invalid request and every later
//! turn sent the same request, and a cut answer was narrated as an unknown
//! stop.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Wake};
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};

use crate::bus::EventSink;
use crate::node::Body;
use crate::provider::{FakeProvider, ProviderError, Scripted, WINDOW_EXCEEDED};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::{TurnError, TurnRequest, WINDOW_CLASS};
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn config(root: &Path, state: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg
}

fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = config(&root.canonicalize().unwrap(), dir.path());
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

fn session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
            from_discord: false,
        })
        .await
}

/// The ledger rows of `kind`, oldest first.
fn rows(core: &Core, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(800).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

fn thens(core: &Core) -> Vec<String> {
    rows(core, "turn.next")
        .iter()
        .map(|r| r["then"].as_str().unwrap().to_string())
        .collect()
}

/// The provider's refusal of a prompt past its window, as the API words it.
fn too_long(tokens: u64, maximum: u64) -> Scripted {
    Scripted::Fail(ProviderError::InvalidRequest {
        status: 400,
        message: format!(
            "invalid_request_error: prompt is too long: {tokens} tokens > {maximum} maximum"
        ),
    })
}

/// An answer cut at the window: some text and a call whose input was cut,
/// after `input` tokens in and `output` out.
fn cut(text: &str, call: &str, input: u64, output: u64) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: input,
            output_tokens: output,
            ..Default::default()
        },
        then: Box::new(Scripted::Blocks {
            blocks: vec![
                json!({"type": "text", "text": text}),
                json!({"type": "tool_use", "id": call, "name": "fs_read", "input": {}}),
            ],
            stop_reason: WINDOW_EXCEEDED.into(),
        }),
    }
}

/// A request's messages as JSON text.
fn text_of(q: &crate::provider::ProviderRequest) -> String {
    serde_json::to_string(&q.messages).unwrap()
}

/// The text of a request's last message, which must be the operator's.
fn last_user_text(q: &crate::provider::ProviderRequest) -> String {
    let last = q.messages.last().unwrap();
    assert_eq!(last["role"], "user", "{:?}", q.messages);
    let blocks = last["content"].as_array().unwrap();
    blocks
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("")
}

fn parked_on_input(core: &Core, sid: &str) -> bool {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let e = core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    (e.state, e.wake, e.resume_pending) == (ExecState::Waiting, Some(Wake::Input), false)
}

/// theseus-9p88: the provider refuses the second turn's request as past its
/// window (250,000 tokens against 200,000). The turn recompiles with a ring
/// at the provider's window, which drops the first exchange, calls once
/// more, and answers. One `context.overflow` row says what the provider
/// said; the turn did not fail.
#[tokio::test]
async fn a_prompt_the_provider_refuses_as_too_long_rings_and_the_retry_answers() {
    let r = rig(vec![
        Scripted::text("The harbor opens at dawn."),
        too_long(250_000, 200_000),
        Scripted::text("The tide turns at four."),
    ]);
    let sid = session(&r.core);
    let first = turn(&r.core, &sid, "when does the harbor open?")
        .await
        .unwrap();
    assert_eq!(first.output, "The harbor opens at dawn.");
    let second = turn(&r.core, &sid, "and the tide?").await.unwrap();
    assert_eq!(second.output, "The tide turns at four.");

    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3, "the refused call and its one retry");
    assert!(text_of(&reqs[1]).contains("when does the harbor open?"));
    assert!(
        !text_of(&reqs[2]).contains("when does the harbor open?"),
        "the ring dropped the first exchange: {:?}",
        reqs[2].messages
    );
    assert_eq!(last_user_text(&reqs[2]), "and the tide?");

    let over = rows(&r.core, "context.overflow");
    assert_eq!(over.len(), 1, "{over:?}");
    assert_eq!(
        (
            &over[0]["source"],
            &over[0]["provider_tokens"],
            &over[0]["provider_maximum"],
            &over[0]["window"],
            &over[0]["then"],
        ),
        (
            &json!("refused"),
            &json!(250_000),
            &json!(200_000),
            &json!(1_000_000),
            &json!("ring")
        )
    );
    let compiled = rows(&r.core, "context.compiled");
    let last = compiled.last().unwrap();
    assert_eq!(
        (&last["trigger"], &last["strategy"]),
        (&json!("overflow"), &json!("ring")),
        "{last}"
    );
    assert!(rows(&r.core, "turn.failed").is_empty());
    let errors = rows(&r.core, "provider.error");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["class"], "invalid_request");
}

/// theseus-9p88: the second turn's answer is cut at the window, part text
/// and a call whose input was cut. The call does not run (its result says
/// so), the call is made once more on a ring that leaves the cut answer
/// out, and the reply is the retry's alone. The next turn's request still
/// leaves the cut answer out and begins with the retry's request.
#[tokio::test]
async fn an_answer_cut_at_the_window_is_made_again_on_a_ring_without_it() {
    let r = rig(vec![
        Scripted::text("The harbor opens at dawn."),
        cut("Half of the tide tab", "toolu_cut", 190_000, 10_000),
        Scripted::text("The whole tide table."),
        Scripted::text("You're welcome."),
    ]);
    let sid = session(&r.core);
    turn(&r.core, &sid, "when does the harbor open?")
        .await
        .unwrap();
    let second = turn(&r.core, &sid, "and the tide table?").await.unwrap();
    assert_eq!(
        second.output, "The whole tide table.",
        "the reply leaves the cut answer out"
    );
    assert_eq!(second.loops, 2);

    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3);
    let retry = &reqs[2];
    assert!(!text_of(retry).contains("Half of the tide tab"));
    assert!(!text_of(retry).contains("toolu_cut"));
    assert!(!text_of(retry).contains("when does the harbor open?"));
    assert_eq!(last_user_text(retry), "and the tide table?");

    let over = rows(&r.core, "context.overflow");
    assert_eq!(over.len(), 1, "{over:?}");
    assert_eq!(
        (
            &over[0]["source"],
            &over[0]["provider_tokens"],
            &over[0]["provider_maximum"],
            &over[0]["output_tokens"],
            &over[0]["then"],
        ),
        (
            &json!("cut"),
            &json!(190_000),
            &json!(200_000),
            &json!(10_000),
            &json!("ring")
        )
    );
    // The cut answer is in the record, and its call has a result that says
    // it did not run.
    let nodes = r.core.store.transcript(&sid).unwrap();
    let cut_id = over[0]["node_id"].as_str().unwrap();
    assert!(nodes.iter().any(|(_, n)| n.id == cut_id));
    let not_run = nodes.iter().find_map(|(_, n)| match &n.body {
        Body::ToolResult {
            tool_use_id,
            content,
            ..
        } if tool_use_id == "toolu_cut" => Some(content.clone()),
        _ => None,
    });
    assert!(
        not_run
            .as_deref()
            .is_some_and(|c| c.contains(WINDOW_EXCEEDED)),
        "{not_run:?}"
    );
    let ended = rows(&r.core, "loop.ended");
    assert_eq!(
        ended[ended.len() - 2]["decision"],
        json!({"decision": "continue"})
    );

    let third = turn(&r.core, &sid, "thanks").await.unwrap();
    assert_eq!(third.output, "You're welcome.");
    let reqs = r.fake.requests();
    let next = &reqs[3];
    assert_eq!(
        &next.messages[..retry.messages.len()],
        &retry.messages[..],
        "the next request begins with the retry's"
    );
    assert!(!text_of(next).contains("Half of the tide tab"));
    assert!(text_of(next).contains("The whole tide table."));
}

/// theseus-9p88: the retry is refused too. The turn fails once, with an
/// error that names the window and the estimate, and makes no third call;
/// the execution waits on input at once (no driver retry, which would send
/// the same request). The next message answers.
#[tokio::test]
async fn a_retry_past_the_window_too_fails_the_turn_without_a_loop() {
    let r = rig(vec![
        Scripted::text("The harbor opens at dawn."),
        too_long(250_000, 200_000),
        too_long(230_000, 200_000),
        Scripted::text("Back on course."),
    ]);
    let sid = session(&r.core);
    turn(&r.core, &sid, "when does the harbor open?")
        .await
        .unwrap();
    let err = turn(&r.core, &sid, "and the tide?")
        .await
        .expect_err("past the window twice");
    let te = err.downcast_ref::<TurnError>().unwrap();
    assert_eq!(te.class, WINDOW_CLASS);
    let message = format!("{:#}", te.source);
    for part in [
        "past its window again",
        "counting 230,000 tokens against a maximum of 200,000",
        "Theseus estimated",
        "a window of 200,000 (the provider's; the catalog says 1,000,000)",
    ] {
        assert!(message.contains(part), "{part:?} in {message}");
    }
    assert_eq!(r.fake.requests().len(), 3, "no third call in the turn");
    let over = rows(&r.core, "context.overflow");
    assert_eq!(
        over.iter().map(|o| o["then"].clone()).collect::<Vec<_>>(),
        [json!("ring"), json!("fail")]
    );
    assert_eq!(rows(&r.core, "turn.failed").len(), 1);
    assert_eq!(thens(&r.core), ["park"], "waits on input at once");
    assert!(parked_on_input(&r.core, &sid));
    let not_ready = r
        .core
        .continue_execution(
            &r.core
                .store
                .get_session::<SessionRecord>(&sid)
                .unwrap()
                .unwrap()
                .execution_id
                .unwrap(),
        )
        .await
        .unwrap_err();
    assert!(format!("{not_ready:#}").contains("not ready for a continuation turn"));
    assert_eq!(r.fake.requests().len(), 3);

    let res = turn(&r.core, &sid, "try again, shorter").await.unwrap();
    assert_eq!(res.output, "Back on course.");
    assert_eq!(r.fake.requests().len(), 4);
}

/// theseus-9p88: the session's first message alone passes the window, so a
/// ring has nothing earlier to drop: the turn fails at once, without a
/// second call, and says so.
#[tokio::test]
async fn a_first_message_past_the_window_fails_at_once_without_a_retry() {
    let r = rig(vec![too_long(250_000, 200_000)]);
    let sid = session(&r.core);
    let err = turn(&r.core, &sid, "a very long paste")
        .await
        .expect_err("nothing to drop");
    let te = err.downcast_ref::<TurnError>().unwrap();
    assert_eq!(te.class, WINDOW_CLASS);
    let message = format!("{:#}", te.source);
    assert!(
        message.contains("nothing earlier in the session can be dropped"),
        "{message}"
    );
    assert_eq!(r.fake.requests().len(), 1);
    let over = rows(&r.core, "context.overflow");
    assert_eq!(over.len(), 1);
    assert_eq!(over[0]["then"], "ring");
    assert_eq!(thens(&r.core), ["park"]);
    assert!(parked_on_input(&r.core, &sid));
}

/// theseus-9p88: the retry of a cut answer is cut too. The turn fails with
/// the window's error after two calls, and none of either answer's calls
/// ran.
#[tokio::test]
async fn an_answer_cut_twice_fails_the_turn() {
    let r = rig(vec![
        Scripted::text("The harbor opens at dawn."),
        cut("Half", "toolu_a", 190_000, 10_000),
        cut("Half again", "toolu_b", 120_000, 80_000),
    ]);
    let sid = session(&r.core);
    turn(&r.core, &sid, "when does the harbor open?")
        .await
        .unwrap();
    let err = turn(&r.core, &sid, "and the tide table?")
        .await
        .expect_err("cut twice");
    let te = err.downcast_ref::<TurnError>().unwrap();
    assert_eq!(te.class, WINDOW_CLASS);
    let message = format!("{:#}", te.source);
    assert!(
        message.contains("cut there after 120,000 tokens in and 80,000 out"),
        "{message}"
    );
    assert_eq!(r.fake.requests().len(), 3);
    let results: Vec<String> = r
        .core
        .store
        .transcript(&sid)
        .unwrap()
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results.iter().all(|c| c.starts_with("Not run:")),
        "no call ran: {results:?}"
    );
    assert_eq!(thens(&r.core), ["park"]);
}
