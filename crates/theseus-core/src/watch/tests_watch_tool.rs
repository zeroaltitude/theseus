//! The model's `watch` tool through the whole core (theseus-f2r2b): a
//! watch that runs a command is approved once, at its creation, with the
//! command shown, and the command that was approved is the command that runs,
//! every time, in the sandbox's place (a stand-in here); a watch is on the
//! commitments block with its last value; the model cannot move one; and a
//! met watch's `then: turn` gives its conversation a turn whose node reads
//! the message.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_kernel::probe_run::ProbeRun;

use super::tests_watch::{look, rows, rpc, until};
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{rig_scripted, session, turn, OWNER};

fn blocks_of(req: &ProviderRequest) -> Vec<String> {
    let last = req.messages.iter().rev().find(|m| m["role"] == "user");
    match last.map(|m| &m["content"]) {
        Some(Value::Array(b)) => b
            .iter()
            .filter_map(|b| b["text"].as_str().map(String::from))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

fn said(req: &ProviderRequest) -> String {
    blocks_of(req)
        .into_iter()
        .filter(|t| {
            !crate::task_graph::view::is_head(t) && !t.starts_with(crate::commitments::HEAD)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn block_of(req: &ProviderRequest) -> Option<String> {
    blocks_of(req)
        .pop()
        .filter(|t| t.starts_with(crate::commitments::HEAD))
}

fn first_id(req: &ProviderRequest) -> String {
    block_of(req)
        .unwrap_or_default()
        .lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("none")
        .to_string()
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

/// `WATCH RUN` (a command), `WATCH FILE <path> <then>` (a file), `MOVE` and
/// `CANCEL` act on the watch the block shows.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    let last = said(req);
    if last == "WATCH RUN" {
        return Scripted::tools(
            "Watching.",
            &[(
                "w1",
                "watch",
                json!({"title": "PR 123 merged?",
                       "check": {"kind": "run", "argv": ["gh", "pr", "view", "123"], "json": ".state"},
                       "until": "equals MERGED", "every": "5m", "for_at_most": "3d"}),
            )],
        );
    }
    if let Some(rest) = last.strip_prefix("WATCH FILE ") {
        let (path, then) = rest.split_once(' ').unwrap();
        return Scripted::tools(
            "Watching.",
            &[(
                "w1",
                "watch",
                json!({"title": "PR 123 merged?", "check": {"kind": "file", "path": path},
                       "until": "equals MERGED", "then": {"kind": then}}),
            )],
        );
    }
    if last == "MOVE" {
        return Scripted::tools(
            "Moving it.",
            &[(
                "m1",
                "wake_move",
                json!({"id": first_id(req), "after": "1h"}),
            )],
        );
    }
    if last == "CANCEL" {
        return Scripted::tools(
            "Cancelling.",
            &[("c1", "wake_cancel", json!({"id": first_id(req)}))],
        );
    }
    Scripted::text("Hello.")
}

fn exec_of(core: &crate::Core, sid: &str) -> theseus_kernel::Execution {
    let rec: crate::session::SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    core.kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
}

/// A command stands in for L1 and counts how it was asked.
fn hook(calls: &Arc<Mutex<Vec<Vec<String>>>>) -> crate::watch::check::RunHook {
    let calls = calls.clone();
    Arc::new(move |argv: &[String]| {
        calls.lock().unwrap().push(argv.to_vec());
        ProbeRun {
            code: Some(0),
            stdout: r#"{"state":"OPEN"}"#.into(),
            stderr: String::new(),
            failed: None,
        }
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_watch_asks_once_with_the_command_shown_and_the_command_never_changes() {
    let r = rig_scripted(|_| {}, script);
    let calls = Arc::new(Mutex::new(Vec::new()));
    r.core.watches.set_run_hook(hook(&calls));
    let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
    let res = turn(&r.core, &sid, "WATCH RUN").await;
    // It asks, whatever the posture says, and the card shows the command.
    let corr = res.awaiting_confirm.as_deref().unwrap_or_else(|| {
        panic!(
            "a command watch asks, but: {}",
            crate::tests_places::result_of(&r.core, &sid, "w1")
        )
    });
    assert_eq!(pending_watches(&r.core), 0, "nothing runs before the yes");
    let q = rpc(&r.core, "confirm.list", json!({})).await.unwrap();
    let card = q.to_string();
    assert!(
        card.contains("gh pr view 123")
            && card.contains("every 5m")
            && card.contains("equals MERGED"),
        "the card shows the command and when it stops: {card}"
    );
    assert!(calls.lock().unwrap().is_empty(), "not run before the yes");
    r.core.confirm_action(corr, true, None, "test").unwrap();
    // The driver takes the turn on.
    until("the watch", 15, || pending_watches(&r.core) == 1).await;
    let before = exec_of(&r.core, &sid).wakes;
    let id = before[0].id.clone();
    // It looks at once (the driver's tick), and a hand-made look is the same.
    until("the first look", 10, || {
        r.core.watches.get(&id).is_some_and(|e| e.checks >= 1)
    })
    .await;
    for _ in 0..3 {
        look(&r.core, &id).await;
    }
    let argvs = calls.lock().unwrap().clone();
    assert!(argvs.len() >= 4, "{argvs:?}");
    let approved: Vec<String> = ["gh", "pr", "view", "123"].map(String::from).to_vec();
    assert!(
        argvs.iter().all(|a| *a == approved),
        "the approved command, every time: {argvs:?}"
    );
    assert_eq!(rows(&r.core, "watch.checked").len(), 1, "OPEN, seen once");
    // The block shows it with its value, and the model cannot move it.
    turn(&r.core, &sid, "hello").await;
    let block = block_of(r.requests().last().unwrap()).expect("the block");
    assert!(
        block.contains(&format!(
            "👁 {} PR 123 merged? (OPEN at ",
            crate::task::short(&id)
        )),
        "{block}"
    );
    turn(&r.core, &sid, "MOVE").await;
    let text = crate::tests_places::result_of(&r.core, &sid, "m1");
    assert!(
        text.contains("is a watch") && text.contains("not moved"),
        "{text}"
    );
    assert_eq!(
        exec_of(&r.core, &sid).wakes,
        before,
        "the record is as it was"
    );
    // A cancel is the wake's own, and ends the checking.
    turn(&r.core, &sid, "CANCEL").await;
    assert_eq!(pending_watches(&r.core), 0);
    let n = calls.lock().unwrap().len();
    look_gone(&r.core, &id).await;
    assert_eq!(
        calls.lock().unwrap().len(),
        n,
        "a cancelled watch is not run again"
    );
    assert_eq!(
        rows(&r.core, "watch.checked").len(),
        1,
        "and a late check writes nothing"
    );
}

fn pending_watches(core: &crate::Core) -> usize {
    core.kernel.open_watches().unwrap().len()
}

/// A check that finishes after its watch was cancelled writes nothing and
/// leaves the board.
async fn look_gone(core: &Arc<crate::Core>, id: &str) {
    let c = core.clone();
    let late = id.to_string();
    tokio::task::spawn_blocking(move || {
        c.watch_finish(&late, Ok(crate::watch::cond::Observed::default()))
    })
    .await
    .unwrap();
    assert!(core.watches.get(id).is_none(), "it left the board");
}

/// `then: turn`: when it holds, its conversation gets a turn whose input
/// reads the message; nothing is posted besides, and the model is called only
/// for that turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_met_watch_with_then_turn_gives_its_conversation_a_turn() {
    let r = rig_scripted(|_| {}, script);
    let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
    let file = r.root.join("pr-state");
    turn(
        &r.core,
        &sid,
        &format!("WATCH FILE {} turn", file.display()),
    )
    .await;
    let before = r.requests().len();
    let id = exec_of(&r.core, &sid).wakes[0].id.clone();
    until("the first look", 10, || {
        r.core.watches.get(&id).is_some_and(|e| e.checks >= 1)
    })
    .await;
    std::fs::write(&file, "MERGED").unwrap();
    look(&r.core, &id).await;
    assert_eq!(rows(&r.core, "watch.met").len(), 1);
    until("the turn", 15, || r.requests().len() > before).await;
    let q = r.requests().pop().unwrap();
    let said = said(&q);
    assert!(
        said.starts_with("👁 watch met: PR 123 merged? → MERGED, after ")
            && said.contains(" checks"),
        "{said}"
    );
    assert_eq!(pending_watches(&r.core), 0);
    assert!(exec_of(&r.core, &sid).wakes.is_empty(), "the turn took it");
}
