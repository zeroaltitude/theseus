//! A turn that faults after a paid loop still closes its books (Review 2's
//! R1, theseus-xonq). Before, a raw `?` after the first provider call left
//! the turn by a path that skipped them: the session's cost, usage, and turn
//! count missed the loops that had finished, no `turn.failed` row said what
//! they spent, and a call whose answer's frame failed kept its reservation
//! in the budget until the reconcile marked it unknown.
//!
//! Each test plants one fault, a turn frame the store will not write
//! (`Store::fail_turn_frame`), at one exit after a paid loop:
//! - the frame that plans the model's tool call;
//! - the frame that answers calls a response could not run (`not_run`);
//! - the second loop's answer, whose call has ended and been charged;
//! - the budget question a second loop asks when its call does not fit.
//!
//! The turn's other exits after a paid loop (a read of the execution for a
//! `/stop`, the end's park) are reads, which a test cannot fail here; they
//! return through the same one exit, `TurnRunner::fault`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_kernel::{micros_to_usd, ActionState, Execution};
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};
use theseus_store::{kinds, NewRecord};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::{TurnError, TurnRequest, FAULT_CLASS};
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

fn rig_with(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    Rig { core, _dir: dir }
}

fn rig(script: Vec<Scripted>) -> Rig {
    rig_with(script, |_| {})
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
            reply_to: None,
        })
        .await
}

/// A response billed with exactly these tokens in and out.
fn billed(input: u64, output: u64, then: Scripted) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: input,
            output_tokens: output,
            ..Usage::default()
        },
        then: Box::new(then),
    }
}

fn read_harbor() -> Scripted {
    Scripted::tools(
        "Reading it.",
        &[("t1", "fs_read", json!({"path": "harbor.txt"}))],
    )
}

fn payload(r: &NewRecord) -> Value {
    serde_json::from_slice(&r.payload).unwrap_or(Value::Null)
}

/// The ledger rows of `kind` in the session, oldest first.
fn rows(core: &Core, sid: &str, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(1000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(sid))
        .map(|(_, r)| r.data)
        .collect()
}

fn execution(core: &Core, sid: &str) -> Execution {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    core.kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
}

/// What every planted fault must leave: the turn failed as a fault, whose
/// error carries the turn's spend; one `turn.failed` row says it; the
/// session's books hold it; and the budget holds no reservation, its spend
/// what the session's books say. Returns the session's record.
fn books_closed(core: &Core, sid: &str, err: &anyhow::Error, usage: (u64, u64)) -> SessionRecord {
    assert!(format!("{err:#}").contains("an injected fault"), "{err:#}");
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    assert_eq!(rec.turns, 1, "the faulted turn is counted");
    assert_eq!(
        (rec.usage.input_tokens, rec.usage.output_tokens),
        usage,
        "the session's usage is what its calls were billed"
    );
    assert!(rec.cost_usd > 0.0, "the session's cost: {}", rec.cost_usd);
    let te = err
        .downcast_ref::<TurnError>()
        .unwrap_or_else(|| panic!("a fault after a paid loop is a turn error: {err:#}"));
    assert_eq!(te.class, FAULT_CLASS);
    assert_eq!(
        te.cost_usd,
        Some(rec.cost_usd),
        "the error carries the spend"
    );
    assert_eq!(
        (te.usage.input_tokens, te.usage.output_tokens),
        usage,
        "and the usage"
    );
    let failed = rows(core, sid, "turn.failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0]["cost_usd"], json!(rec.cost_usd), "{failed:?}");
    assert!(
        failed[0]["reason"]
            .as_str()
            .is_some_and(|r| r.starts_with(FAULT_CLASS) && r.contains("an injected fault")),
        "{failed:?}"
    );
    let e = execution(core, sid);
    assert_eq!(
        (e.budget.reserved_micros, e.budget.held_unknown_micros),
        (0, 0),
        "nothing held for a call that ended: {:?}",
        e.budget
    );
    assert!(
        (micros_to_usd(e.budget.spent_micros) - rec.cost_usd).abs() < 1e-6,
        "the budget's spend {} is the session's cost {}",
        micros_to_usd(e.budget.spent_micros),
        rec.cost_usd
    );
    rec
}

/// The model asks for a read, and the frame that plans it fails: the loop
/// that asked was paid, and the session's books, the `turn.failed` row, and
/// the error all say what it cost. Before R1 the session read $0 and no
/// turns.
#[tokio::test]
async fn a_fault_on_a_tool_calls_frame_after_a_paid_loop_still_books_the_loop() {
    let r = rig(vec![billed(1_200, 300, read_harbor())]);
    let sid = session(&r.core);
    r.core.store.fail_turn_frame(|records| {
        records.iter().any(|r| {
            r.kind == kinds::ACTION
                && payload(r)["tool"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("fs"))
        })
    });
    let err = turn(&r.core, &sid, "what does harbor.txt say?")
        .await
        .expect_err("the turn faults");
    let rec = books_closed(&r.core, &sid, &err, (1_200, 300));
    assert_eq!(rec.tool_calls, 0, "the call never ran");
    let calls = rows(&r.core, &sid, "provider.call");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0]["cost_usd"], json!(rec.cost_usd));
    assert_eq!(rows(&r.core, &sid, "turn.failed")[0]["loops"], 1);
}

/// A response that stopped at its output cap with a call in it: the call is
/// answered "not run", and that frame fails.
#[tokio::test]
async fn a_fault_answering_calls_a_response_could_not_run_still_books_the_loop() {
    let cut = Scripted::Blocks {
        blocks: vec![
            json!({"type": "text", "text": "Reading it"}),
            json!({"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "harbor.txt"}}),
        ],
        stop_reason: "max_tokens".into(),
    };
    let r = rig(vec![billed(900, 4_000, cut)]);
    let sid = session(&r.core);
    r.core.store.fail_turn_frame(|records| {
        records.iter().any(|r| {
            let p = payload(r);
            r.kind == kinds::NODE
                && p["body"]["kind"] == "tool_result"
                && p["body"]["meta"]["not_run"].is_string()
        })
    });
    let err = turn(&r.core, &sid, "what does harbor.txt say?")
        .await
        .expect_err("the turn faults");
    books_closed(&r.core, &sid, &err, (900, 4_000));
}

/// The second loop's call answers and is charged, and its answer's frame
/// fails. Both calls are booked; the second settles alone at its price, so
/// the budget holds no reservation for it, and its action says its answer
/// was not written.
#[tokio::test]
async fn a_fault_on_the_second_answers_frame_books_both_calls_and_holds_nothing() {
    let r = rig(vec![
        billed(1_000, 200, read_harbor()),
        billed(1_500, 60, Scripted::text("It says the tide turns at four.")),
    ]);
    let sid = session(&r.core);
    let answers = Arc::new(AtomicU32::new(0));
    let seen = answers.clone();
    r.core.store.fail_turn_frame(move |records| {
        records
            .iter()
            .any(|r| r.kind == kinds::NODE && payload(r)["body"]["kind"] == "assistant_message")
            && seen.fetch_add(1, Ordering::SeqCst) == 1
    });
    let err = turn(&r.core, &sid, "what does harbor.txt say?")
        .await
        .expect_err("the turn faults");
    assert_eq!(
        answers.load(Ordering::SeqCst),
        2,
        "the second answer's frame failed"
    );
    let rec = books_closed(&r.core, &sid, &err, (2_500, 260));
    assert_eq!(rec.tool_calls, 1, "the first loop's read ran");
    let calls = rows(&r.core, &sid, "provider.call");
    assert_eq!(calls.len(), 1, "the second's row rode in the failed frame");
    let first = calls[0]["cost_usd"].as_f64().unwrap();
    assert!(
        rec.cost_usd > first,
        "{} books the second call too",
        rec.cost_usd
    );
    let provider_calls: Vec<_> = r
        .core
        .kernel
        .actions()
        .unwrap()
        .into_iter()
        .filter(|a| a.tool == theseus_protocol::PROVIDER_TOOL)
        .collect();
    assert_eq!(provider_calls.len(), 2);
    assert!(
        provider_calls
            .iter()
            .all(|a| a.state == ActionState::Succeeded),
        "{provider_calls:?}"
    );
    assert_eq!(
        provider_calls
            .iter()
            .filter(|a| a.result_ref.is_none())
            .count(),
        1,
        "the second settled alone, with no answer node"
    );
}

/// The template's live profile reserves its 128,000-token output cap at $10
/// per million ($1.28) and its input estimate at $2: a $1.40 limit fits the
/// first call, not the second once the first's 30,000 tokens out ($0.30) are
/// spent. The second loop asks the budget question, and its frame fails.
#[tokio::test]
async fn a_fault_on_the_budget_question_books_the_loop_that_fit() {
    let first = billed(
        2_000,
        30_000,
        Scripted::tools(
            "Diffing.",
            &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        ),
    );
    let r = rig_with(vec![first], |c| c.kernel.spend_limit_usd = 1.40);
    let sid = session(&r.core);
    r.core.store.fail_turn_frame(|records| {
        records
            .iter()
            .any(|r| r.kind == kinds::ACTION && payload(r)["tool"] == theseus_protocol::BUDGET_TOOL)
    });
    let err = turn(&r.core, &sid, "diff these")
        .await
        .expect_err("the turn faults");
    let rec = books_closed(&r.core, &sid, &err, (2_000, 30_000));
    assert_eq!(rec.tool_calls, 1, "the diff ran");
    assert_eq!(
        rows(&r.core, &sid, "turn.failed")[0]["loops"],
        2,
        "it faulted in its second loop"
    );
}
