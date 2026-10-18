//! Arm B of the A/B replay (RSI loop 1, theseus-pw1q.21.6): a minted flow
//! loaded beside the recorded tool set in a whole-turn replay. The model is
//! not told to use it; it is listed, and when it calls it the flow's steps run
//! through the replay's own hands, on the copy. Arm A, without it, lists
//! nothing and refuses a call to it.

use std::sync::Arc;

use serde_json::{json, Value};

use super::hands::Minted;
use super::runloop::TEST_PROGRAMS;
use super::tests::{original, params, push, world, Sh, SERIAL};
use crate::flow::spec::Spec;
use crate::provider::Scripted;

fn check_it() -> Spec {
    Spec::parse(
        json!({
          "v": 1, "kind": "flow", "name": "check_it",
          "description": "Runs the repository's check and returns its result.",
          "scope": { "repos": ["r:abcdef0123"] },
          "steps": [
            {"id": "check", "tool": "proc.run", "args": {"argv": ["sh", "check.sh", "test"]},
             "on": {"err": "handback:check-red"}}
          ],
          "exits": { "ok": "the check passed" }
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap()
}

fn tool_names(req: &crate::provider::ProviderRequest) -> Vec<String> {
    req.tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// Arm B lists the flow, the model calls it, its step runs in the copy with
/// the recorded tool set's own hands, and the row says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arm_b_lists_the_minted_flow_and_runs_its_steps_on_the_copy() {
    let _g = SERIAL.lock().await;
    let sh = Arc::new(Sh::default());
    *TEST_PROGRAMS.lock().unwrap() = Some(sh.clone());
    let w = world(|_| {});
    let t = original(&w).await;
    let before = w.rig.model.requests().len();
    push(
        &w.rig,
        Scripted::tools(
            "",
            &[
                (
                    "e1",
                    "fs_edit",
                    crate::own_bench::whole::tests::edit(&w.work),
                ),
                ("f1", "flow__check_it", json!({})),
            ],
        ),
    );
    push(&w.rig, Scripted::text("The check passes."));
    let mut spent = 0;
    let r = w
        .rig
        .core
        .replay_turn_with(
            &params(&w, &t),
            "mint",
            None,
            &mut spent,
            Some(Minted::new(check_it())),
        )
        .await
        .unwrap();
    *TEST_PROGRAMS.lock().unwrap() = None;
    let whole = r.result.whole.expect("a whole replay");
    let how: Vec<(&str, &str)> = whole
        .calls
        .iter()
        .map(|c| (c.tool.as_str(), c.how.as_str()))
        .collect();
    assert_eq!(
        how,
        [
            ("fs.edit", "ran"),
            ("proc.run", "ran"),
            ("flow__check_it", "ran")
        ],
        "{how:?}"
    );
    assert_eq!(whole.calls[1].exit, Some(0));
    // The flow's step ran in the scratch copy, never the directory.
    let cwds = sh.cwds.lock().unwrap().clone();
    assert_eq!(cwds.len(), 1);
    assert!(!cwds[0].starts_with(&w.work), "{cwds:?}");
    assert_eq!(whole.loops, 2);
    // It was listed in the request, with the spec's own description, and in no
    // other words.
    let sent = w.rig.model.requests();
    let first = &sent[before];
    assert!(
        tool_names(first).contains(&"flow__check_it".to_string()),
        "{:?}",
        tool_names(first)
    );
    let def = first
        .tools
        .iter()
        .find(|t| t["name"] == "flow__check_it")
        .unwrap();
    assert!(def["description"]
        .as_str()
        .unwrap()
        .contains("Runs the repository's check"));
    // The model read the flow's own result.
    let last = serde_json::to_string(&sent.last().unwrap().messages).unwrap();
    assert!(last.contains("the check passed"), "{last}");
}

/// Arm A: no flow listed, and a call to one is refused, so the arms differ
/// only by the one tool.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arm_a_lists_no_flow_and_refuses_a_call_to_one() {
    let _g = SERIAL.lock().await;
    *TEST_PROGRAMS.lock().unwrap() = Some(Arc::new(Sh::default()));
    let w = world(|_| {});
    let t = original(&w).await;
    let before = w.rig.model.requests().len();
    push(
        &w.rig,
        Scripted::tools("", &[("f1", "flow__check_it", json!({}))]),
    );
    push(&w.rig, Scripted::text("It was not there."));
    let mut spent = 0;
    let r = w
        .rig
        .core
        .replay_turn_with(&params(&w, &t), "mint", None, &mut spent, None)
        .await
        .unwrap();
    *TEST_PROGRAMS.lock().unwrap() = None;
    let whole = r.result.whole.unwrap();
    assert_eq!(whole.calls.len(), 1);
    assert_eq!(
        (whole.calls[0].tool.as_str(), whole.calls[0].how.as_str()),
        ("flow__check_it", "refused")
    );
    let sent = w.rig.model.requests();
    assert!(!tool_names(&sent[before])
        .iter()
        .any(|n| n.starts_with("flow__")));
}

#[test]
fn a_programs_text_is_what_it_printed_without_the_hands_header_and_footer() {
    use super::hands::printed;
    assert_eq!(printed("$ git rerere status\na.txt\n[exit 0]\n"), "a.txt");
    assert_eq!(printed("$ true\n\n[exit 0]\n"), "");
    assert_eq!(
        printed("$ sh x\nout[stderr]\nerr\n[exit 2]\n"),
        "out[stderr]\nerr"
    );
    assert_eq!(printed("$ nope\n\n[did not run: no such file]\n"), "");
}

fn acting() -> super::tests::World {
    world(|c| c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act)
}

/// The A/B replay over one recorded turn, through the core.
async fn ab(
    w: &super::tests::World,
    turn: &str,
) -> anyhow::Result<crate::rsi::mint::validate::ab::AbReport> {
    let pending = crate::rsi::mint::run::Pending {
        placeholder: "mint_test".into(),
        kind: crate::rsi::mint::kind::Kind::Flow,
        item: "tsk_x".into(),
        motif: crate::rsi::mint::tests::land(),
    };
    let check = vec![crate::rsi::mine::Occurrence {
        session: w.sid.clone(),
        turn: turn.into(),
        day: "2026-10-17".into(),
        first: 0,
        last: 0,
    }];
    w.rig.core.mint_ab(&pending, &check_it(), &check).await
}

fn script_arm(w: &super::tests::World, calls_flow: bool) {
    let flow = if calls_flow {
        ("f1", "flow__check_it")
    } else {
        ("f1", "proc_run")
    };
    let args = if calls_flow {
        json!({})
    } else {
        crate::own_bench::whole::tests::check()
    };
    push(
        &w.rig,
        Scripted::tools(
            "",
            &[
                (
                    "e1",
                    "fs_edit",
                    crate::own_bench::whole::tests::edit(&w.work),
                ),
                flow_call(flow, args),
            ],
        ),
    );
    push(&w.rig, Scripted::text("The check passes."));
}

fn flow_call<'a>((id, name): (&'a str, &'a str), args: Value) -> (&'a str, &'a str, Value) {
    (id, name, args)
}

/// Both arms of a turn run under reservations on the self budget, are settled
/// to what they cost, and the report says the model called the tool.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ab_replay_reserves_on_the_self_budget_and_sees_the_tool_called() {
    let _g = SERIAL.lock().await;
    *TEST_PROGRAMS.lock().unwrap() = Some(Arc::new(Sh::default()));
    let w = acting();
    let t = original(&w).await;
    script_arm(&w, false); // arm A: the recorded tool set
    script_arm(&w, true); // arm B: the flow, loaded
    let r = ab(&w, &t).await.unwrap();
    *TEST_PROGRAMS.lock().unwrap() = None;
    assert_eq!((r.turns, r.b_called), (1, 1), "{r:?}");
    assert!(r.stopped.is_none(), "{r:?}");
    assert!(r.back_to_backlog.is_none());
    // Both arms were reserved and settled on the self budget: it spent, and
    // holds nothing open.
    let view = w
        .rig
        .core
        .runner
        .self_budget
        .view()
        .expect("the budget is on");
    assert!(view.day_spent_usd > 0.0, "{view:?}");
    assert_eq!(view.branches.len(), 1);
    assert_eq!(view.branches[0].branch, "mint_test");
    assert_eq!(view.branches[0].open_usd, 0.0, "{view:?}");
    assert!(
        (view.day_spent_usd - r.spent_usd).abs() < 1e-6,
        "{view:?} {r:?}"
    );
}

/// A model that never calls the tool in arm B sends the motif back to the
/// backlog with that evidence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_that_never_calls_the_tool_sends_the_motif_back_with_evidence() {
    let _g = SERIAL.lock().await;
    *TEST_PROGRAMS.lock().unwrap() = Some(Arc::new(Sh::default()));
    let w = acting();
    let t = original(&w).await;
    script_arm(&w, false);
    script_arm(&w, false);
    let r = ab(&w, &t).await.unwrap();
    *TEST_PROGRAMS.lock().unwrap() = None;
    assert!(!r.pass && r.b_called == 0, "{r:?}");
    let why = r.back_to_backlog.expect("evidence");
    assert!(why.contains("never called it"), "{why}");
}

/// With the self budget's day line too low for one arm, no model is called.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_day_line_stops_the_replay_before_any_arm_runs() {
    let _g = SERIAL.lock().await;
    *TEST_PROGRAMS.lock().unwrap() = Some(Arc::new(Sh::default()));
    let w = world(|c| {
        c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
        c.self_improve.budget.day_usd = 0.0001;
    });
    let t = original(&w).await;
    let before = w.rig.model.requests().len();
    let r = ab(&w, &t).await.unwrap();
    *TEST_PROGRAMS.lock().unwrap() = None;
    assert_eq!(r.turns, 0);
    assert!(
        r.stopped.as_deref().is_some_and(|s| s.contains("day line")),
        "{r:?}"
    );
    assert_eq!(w.rig.model.requests().len(), before, "no request was sent");
}
