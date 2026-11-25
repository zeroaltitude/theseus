//! A change to the owner's task that ran without asking him (issue 57): his
//! objective or acceptance changed, or his task abandoned, runs at its tool's
//! own posture and is told, with an undo (`task.undo`) that writes the task's
//! record before it back exactly. His `approve` on the tool still asks with
//! the card's words, a press by anyone but him in a private place undoes
//! nothing, and a self-loop item's abandoning keeps its card.

use serde_json::{json, Value};

use crate::approval::Surface;
use crate::owner_note::told::Undo;
use crate::policy::Posture;
use crate::task_graph::{TaskRecord, TaskState};
use crate::tests_m3::{rpc_as, surface};
use crate::tests_task_graph::{rig_on, rows, session, turn, Rig, ASK, QUOTE};
use crate::Config;

/// The rig, with the owner's task (a session opened with a brief and its
/// arrangement, held open) made: the rig, the conversation and the task.
async fn with_his_task(tune: impl FnOnce(&mut Config)) -> (Rig, String, TaskRecord) {
    let r = rig_on(tempfile::tempdir().unwrap(), tune);
    let s = session(&r.core);
    r.model
        .hold_tasks
        .store(true, std::sync::atomic::Ordering::SeqCst);
    r.calls(&[(
        "task_create",
        json!({"brief": "Chart every buoy of the outer harbour and report the depths.",
               "title": "Chart the outer harbour", "acceptance": ["a chart exists"],
               "arrangement": {"pieces": [{"quote": QUOTE, "role": "objective"}]}}),
    )]);
    turn(&r.core, &s, ASK).await;
    let t = r.by_title("Chart the outer harbour");
    assert!(t.is_owners(), "{t:?}");
    (r, s, t)
}

/// A record as its content: every field but the version and the time of its
/// last change, which only count edits.
fn content(t: &TaskRecord) -> Value {
    let mut v = serde_json::to_value(t).unwrap();
    v["version"] = Value::Null;
    v["updated_at_ms"] = Value::Null;
    v
}

/// The told rows of a verb (`task.updated`, `task.closed`).
fn told(r: &Rig, kind: &str) -> Vec<Value> {
    rows(&r.core, kind)
        .into_iter()
        .map(|row| row.data)
        .filter(|d| d["told"] == true)
        .collect()
}

/// The notes told to the owner: each one's text and its undo.
pub(crate) fn notes(core: &crate::Core) -> Vec<(String, Undo)> {
    crate::tests_tasks::posts(core, crate::owner_note::told::KIND)
        .iter()
        .map(|a| {
            crate::owner_note::told::of(&core.kernel, &a.correlation_id)
                .unwrap()
                .unwrap()
        })
        .collect()
}

/// The post of the note that carries `undo`.
fn post_of(core: &crate::Core, undo: &Undo) -> String {
    crate::tests_tasks::posts(core, crate::owner_note::told::KIND)
        .into_iter()
        .find(|a| {
            crate::owner_note::told::of(&core.kernel, &a.correlation_id)
                .unwrap()
                .is_some_and(|(_, u)| u == *undo)
        })
        .expect("a note with that undo")
        .correlation_id
}

/// A note's Undo pressed by the owner, from his terminal: `told.undo` on its
/// post, the one door every press goes through.
pub(crate) async fn press(
    core: &std::sync::Arc<crate::Core>,
    undo: &Undo,
) -> Result<Value, theseus_protocol::RpcError> {
    let post = post_of(core, undo);
    rpc_as(
        core,
        surface("sock#1", Surface::Cli),
        "told.undo",
        json!({"post": post}),
    )
    .await
}

/// A note's Undo pressed by a person in a shared channel nobody bound as
/// private.
async fn press_by_a_stranger(
    core: &std::sync::Arc<crate::Core>,
    undo: &Undo,
) -> Result<Value, theseus_protocol::RpcError> {
    let post = post_of(core, undo);
    rpc_as(
        core,
        surface("discord:777", Surface::Discord),
        "told.undo",
        json!({"post": post, "author": "discord:777",
               "discord": {"user_id": "777", "channel_id": "9001", "guild_id": "7"}}),
    )
    .await
}

/// `task.undo` from the owner's own terminal.
async fn undo(r: &Rig, id: &str, version: u64) -> Result<Value, theseus_protocol::RpcError> {
    rpc_as(
        &r.core,
        surface("sock#1", Surface::Cli),
        "task.undo",
        json!({"id": id, "version": version}),
    )
    .await
}

/// `task.undo` pressed by a person in a shared channel nobody bound as private.
async fn undo_by_a_stranger(
    r: &Rig,
    id: &str,
    version: u64,
) -> Result<Value, theseus_protocol::RpcError> {
    rpc_as(
        &r.core,
        surface("discord:777", Surface::Discord),
        "task.undo",
        json!({"id": id, "version": version, "author": "discord:777",
               "discord": {"user_id": "777", "channel_id": "9001", "guild_id": "7"}}),
    )
    .await
}

fn stop_holding(r: &Rig) {
    r.model
        .hold_tasks
        .store(false, std::sync::atomic::Ordering::SeqCst);
}

/// His objective changed with nothing set on `task.update`: no question, the
/// change applies, its row is told and keeps the record before it; a
/// stranger's undo changes nothing; his undo writes that record back exactly,
/// and a second undo finds the task moved and changes nothing.
#[tokio::test]
async fn his_objective_changed_runs_unasked_and_his_undo_puts_it_back_exactly() {
    let (r, s, t) = with_his_task(|_| {}).await;
    let before = r.task(&t.id);
    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": before.version,
               "patch": {"objective": "chart the inner harbour instead",
                         "acceptance": ["the inner chart exists"]}}),
    )]);
    turn(&r.core, &s, "make it the inner harbour").await;
    assert!(
        r.core.confirm_list().unwrap().is_empty(),
        "nothing is asked"
    );
    let changed = r.task(&t.id);
    assert_eq!(changed.objective, "chart the inner harbour instead");
    assert_eq!(changed.version, before.version + 1);
    let said = told(&r, "task.updated");
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0]["before"]["objective"], json!(before.objective));
    assert!(
        rows(&r.core, "task.change_accepted").is_empty(),
        "nobody accepted it"
    );
    let told = notes(&r.core);
    assert_eq!(told.len(), 1, "told once: {told:?}");
    let (text, how) = &told[0];
    assert!(
        text.starts_with(
            "ℹ️ FYI · Your task's objective and acceptance were changed\nIn conversation "
        ),
        "{text}"
    );
    assert!(
        text.contains("without asking you") && text.contains(&t.id),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "Undo: `theseus task undo {} {}`",
            t.id, changed.version
        )),
        "{text}"
    );
    assert!(
        !text.contains("inner"),
        "it quotes nothing the change wrote: {text}"
    );
    assert_eq!(
        *how,
        Undo::TaskUndo {
            task: t.id.clone(),
            version: changed.version
        }
    );

    let refused = undo_by_a_stranger(&r, &t.id, changed.version).await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(r.task(&t.id), changed, "a stranger's undo undoes nothing");
    let refused = press_by_a_stranger(&r.core, how).await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(r.task(&t.id), changed, "a stranger's press undoes nothing");

    let back = press(&r.core, how).await.unwrap();
    assert!(
        back["text"]
            .as_str()
            .is_some_and(|t| t.contains("\n↩️ Undone by ") && !t.contains("Undo: `")),
        "{back}"
    );
    let now = r.task(&t.id);
    assert_eq!(content(&now), content(&before), "exactly as it was");
    assert_eq!(now.version, changed.version + 1);
    let undid = rows(&r.core, "task.updated");
    assert_eq!(undid.last().unwrap().data["undid"], json!(changed.version));

    let again = press(&r.core, how).await;
    assert!(again.is_err(), "{again:?}");
    let again = undo(&r, &t.id, changed.version).await;
    assert!(again.is_err(), "{again:?}");
    assert_eq!(r.task(&t.id), now, "a second undo changes nothing");
    stop_holding(&r);
}

/// A crash between the undo and the note's record of it: the task is back,
/// and the next press finds that, marks the note and writes nothing again.
#[tokio::test]
async fn a_task_undo_whose_receipt_was_lost_counts_once() {
    use std::sync::atomic::Ordering;
    let (r, s, t) = with_his_task(|_| {}).await;
    let before = r.task(&t.id);
    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": before.version,
               "patch": {"objective": "chart the inner harbour instead"}}),
    )]);
    turn(&r.core, &s, "make it the inner harbour").await;
    let told = notes(&r.core);
    assert_eq!(told.len(), 1, "told once: {told:?}");
    let how = &told[0].1;

    crate::rpc::told::FAULT_RECEIPT.store(true, Ordering::SeqCst);
    let lost = press(&r.core, how).await;
    assert!(lost.is_err(), "the receipt was lost: {lost:?}");
    let back = r.task(&t.id);
    assert_eq!(content(&back), content(&before), "the undo itself was done");

    let again = press(&r.core, how).await.unwrap();
    assert_eq!(again["found"], json!(true), "{again}");
    assert!(
        again["text"]
            .as_str()
            .is_some_and(|t| t.ends_with("Nothing left to undo: the task is back as it was.")),
        "{again}"
    );
    assert_eq!(r.task(&t.id), back, "nothing ran twice");
    assert!(press(&r.core, how).await.is_err(), "a third press is refused");
    stop_holding(&r);
}

/// With `task.update = "approve"` set, the same change asks, with the words
/// of the card, the proposal on the task and the task as it was.
#[tokio::test]
async fn his_approve_on_task_update_still_asks_with_the_cards_words() {
    let (r, s, t) = with_his_task(|cfg| {
        cfg.policy
            .tools
            .insert("task.update".into(), Posture::Approve);
    })
    .await;
    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": t.version,
               "patch": {"objective": "chart the inner harbour instead"}}),
    )]);
    turn(&r.core, &s, "make it the inner harbour").await;
    let asked = r.core.confirm_list().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].reason.contains("layer 1"), "{}", asked[0].reason);
    assert!(
        asked[0]
            .reason
            .contains("objective after: \"chart the inner harbour instead\""),
        "{}",
        asked[0].reason
    );
    let now = r.task(&t.id);
    assert_eq!(now.objective, t.objective);
    assert!(now.proposal.is_some());
    assert!(told(&r, "task.updated").is_empty());
    assert!(notes(&r.core).is_empty(), "asked, not told");
    stop_holding(&r);
}

/// His task abandoned with nothing set on `task.close`: no question, the task
/// is abandoned and its close row is told; his undo reopens it exactly.
#[tokio::test]
async fn his_task_abandoned_runs_unasked_and_his_undo_reopens_it_exactly() {
    let (r, s, t) = with_his_task(|_| {}).await;
    let before = r.task(&t.id);
    r.calls(&[(
        "task_close",
        json!({"id": t.id, "version": before.version, "outcome": "abandoned"}),
    )]);
    turn(&r.core, &s, "drop it").await;
    assert!(
        r.core.confirm_list().unwrap().is_empty(),
        "nothing is asked"
    );
    let closed = r.task(&t.id);
    assert_eq!(closed.state, TaskState::Abandoned);
    let said = told(&r, "task.closed");
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(rows(&r.core, "task.change_accepted").is_empty());
    let told = notes(&r.core);
    assert_eq!(told.len(), 1, "told once: {told:?}");
    assert!(
        told[0].0.starts_with("ℹ️ FYI · Your task was abandoned\n"),
        "{}",
        told[0].0
    );

    press(&r.core, &told[0].1).await.unwrap();
    let now = r.task(&t.id);
    assert_eq!(content(&now), content(&before), "reopened exactly");
    assert_ne!(now.state, TaskState::Abandoned);
    stop_holding(&r);
}

/// With `task.close = "approve"` set, abandoning his task asks.
#[tokio::test]
async fn his_approve_on_task_close_still_asks_to_abandon() {
    let (r, s, t) = with_his_task(|cfg| {
        cfg.policy
            .tools
            .insert("task.close".into(), Posture::Approve);
    })
    .await;
    r.calls(&[(
        "task_close",
        json!({"id": t.id, "version": t.version, "outcome": "abandoned"}),
    )]);
    turn(&r.core, &s, "drop it").await;
    let asked = r.core.confirm_list().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].reason.contains("abandoning"),
        "{}",
        asked[0].reason
    );
    assert_ne!(r.task(&t.id).state, TaskState::Abandoned);
    stop_holding(&r);
}

/// A self-loop item is the owner's too, but abandoning it stops its build
/// and closes its trees, which no record written back restarts: with nothing
/// set on `task.close` it keeps its card, as before.
#[tokio::test]
async fn abandoning_a_self_loop_item_keeps_its_card() {
    let r = rig_on(tempfile::tempdir().unwrap(), |_| {});
    let s = session(&r.core);
    let item = crate::task_graph::NewTask {
        id: "tsk_selfitem01".into(),
        title: "Fix the parser",
        objective: "fix it".into(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: None,
        origin: crate::task_graph::TaskOrigin {
            session: "self:filed".into(),
            principal: "self".into(),
            ..Default::default()
        },
        state: TaskState::Proposed,
    }
    .build(theseus_protocol::now_unix_ms());
    r.core
        .store
        .append(&[crate::task_graph::record(&item).unwrap()])
        .unwrap();
    r.calls(&[(
        "task_close",
        json!({"id": item.id, "version": 1, "outcome": "abandoned"}),
    )]);
    turn(&r.core, &s, "drop the parser item").await;
    let asked = r.core.confirm_list().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0].reason.contains("abandoning"),
        "{}",
        asked[0].reason
    );
    assert_eq!(r.task(&item.id).state, TaskState::Proposed);
    assert!(notes(&r.core).is_empty());
}
