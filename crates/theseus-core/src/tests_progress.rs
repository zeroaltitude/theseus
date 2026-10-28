//! A task's progress with no extra model call (theseus-xlias): the plan items
//! it lays out and closes, and its own line before each tool call, as the
//! work board shows them (`WorkView.progress`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::work::{WorkKind, WorkState, WorkView};

use crate::provider::{ProviderRequest, Scripted};
use crate::push::WorkScope;
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, parent_session, rig, turn, until, ASK,
};
use crate::tests_work::Watcher;

/// A token-shaped string, built from parts: the scrubber guesses no shapes,
/// so a say that carries one shows it as written.
fn token() -> String {
    format!("{}{}", "ghp_", "a1B2c3D4".repeat(5))
}

/// The task's own id, as its brief names it: `[Task ab12cd: …`.
fn own_id(req: &ProviderRequest) -> String {
    let first = first_user(req);
    let rest = first.strip_prefix("[Task ").expect("a task's brief");
    rest[..rest.find(':').unwrap()].to_string()
}

/// The ids of the plan items the last message's results recorded.
fn recorded(req: &ProviderRequest) -> Vec<String> {
    let last = req
        .messages
        .last()
        .map(|m| m.to_string())
        .unwrap_or_default();
    last.split("Recorded task ")
        .skip(1)
        .map(|s| s.split(' ').next().unwrap().to_string())
        .collect()
}

fn close(id: &str, n: usize) -> (String, &'static str, Value) {
    (
        format!("x_{n}"),
        "task_close",
        json!({"id": id, "version": 1, "outcome": "done",
               "evidence": [{"identity": "note", "note": "done"}]}),
    )
}

fn script(
    loops: Arc<AtomicUsize>,
) -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    move |req| {
        if !first_user(req).contains("CHILD:") {
            return match answers_a_call(req) {
                true => Scripted::text("Started it."),
                false => Scripted::tools(
                    "Starting it.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": "CHILD: lay out three steps and do them",
                               "budget_usd": 1.5, "arrangement": arranged()}),
                    )],
                ),
            };
        }
        let id = own_id(req);
        let n = loops.fetch_add(1, Ordering::SeqCst);
        let step = |i: usize| {
            (
                format!("s_{i}"),
                "task_create",
                json!({"title": format!("step {i}"), "parent": id}),
            )
        };
        let owned =
            |calls: Vec<(String, &'static str, Value)>| -> Vec<(String, &'static str, Value)> {
                calls
            };
        let run = |text: &str, calls: Vec<(String, &'static str, Value)>| {
            let calls: Vec<(&str, &str, Value)> = calls
                .iter()
                .map(|(a, b, c)| (a.as_str(), *b, c.clone()))
                .collect();
            Scripted::tools(text, &calls)
        };
        // The plan items' ids are in loop 1's results alone: kept for the rest.
        let mut kept = seen.lock().unwrap();
        if kept.is_empty() {
            *kept = recorded(req);
        }
        let ids = kept.clone();
        drop(kept);
        match n {
            0 => run(
                "Laying out the plan first.\nThis second line is not shown.",
                owned((1..=3).map(step).collect()),
            ),
            1 => run(
                &format!("Running the tests with {} again\nand more", token()),
                owned(vec![close(&ids[0], 0)]),
            ),
            2 => run(
                &"Checking every one of the remaining files for the stale name before closing it "
                    .repeat(3),
                owned(vec![close(&ids[1], 1)]),
            ),
            3 => run("", owned(vec![close(&ids[2], 2)])),
            _ => Scripted::text("All three steps are done."),
        }
    }
}

fn find_task(w: &mut Watcher) -> Option<WorkView> {
    w.held()
        .values()
        .find(|v| v.kind == WorkKind::Task)
        .cloned()
}

/// A task lays out three plan items and closes them, with a line before each
/// call: the board reads 0/3 with the first line, then 3/3 with the last
/// one, the secret in a line never shown, a long line cut, and the loop that
/// made no call writes none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_with_three_plan_items_reads_0_of_3_then_3_of_3_with_its_own_lines() {
    let loops = Arc::new(AtomicUsize::new(0));
    let r = rig(script(loops.clone()), |_| {});
    let parent = parent_session(&r.core);
    let (mut w, _) = Watcher::start(&r.core, WorkScope::default()).await;
    turn(&r.core, &parent, ASK).await;
    until("the task is done", || {
        find_task(&mut w).is_some_and(|t| t.state == WorkState::Done)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    w.drain();
    // Every progress the task's view went through, in order.
    let task_id = find_task(&mut w).unwrap().id;
    let mut seen: Vec<(u32, u32, Option<String>)> = Vec::new();
    for c in &w.changed {
        for v in c.views.iter().filter(|v| v.id == task_id) {
            if let Some(p) = &v.progress {
                let row = (p.steps_done, p.steps_total, p.said.clone());
                if seen.last() != Some(&row) {
                    seen.push(row);
                }
            }
        }
    }
    let said: Vec<String> = seen.iter().filter_map(|s| s.2.clone()).collect();
    assert!(
        seen.iter().any(|s| (s.0, s.1) == (0, 3)),
        "0/3 was read: {seen:?}"
    );
    let last = find_task(&mut w).unwrap().progress.unwrap();
    assert_eq!((last.steps_done, last.steps_total), (3, 3), "{last:?}");
    assert_eq!(said[0], "Laying out the plan first.", "{said:?}");
    assert!(said.iter().all(|s| !s.contains("second line")), "{said:?}");
    let all = said.join("\n");
    assert!(all.contains(&token()), "shown as written: {all}");
    assert!(all.contains("Running the tests with"), "{all}");
    let long = said
        .iter()
        .find(|s| s.starts_with("Checking every one"))
        .expect("the long line");
    assert!(long.chars().count() <= 120 && long.ends_with('…'), "{long}");
    // The last loop made no call and said nothing new: the board keeps the
    // newest line it had, and the final text is no `say`.
    assert_eq!(last.said.as_deref(), Some(long.as_str()), "{last:?}");
    assert!(!all.contains("All three steps are done"), "{all}");
    assert_eq!(loops.load(Ordering::SeqCst), 5);
    // The task's items are the task's: the conversation above it counts none.
    let held = w.held();
    let root = held
        .values()
        .find(|v| v.kind == WorkKind::Conversation)
        .expect("the conversation");
    let rp = root
        .progress
        .as_ref()
        .map_or((0, 0), |p| (p.steps_done, p.steps_total));
    assert_eq!(
        rp,
        (0, 0),
        "the task's plan items are not the conversation's: {root:?}"
    );
}

/// The row a loop with tool calls writes carries `say`; one without writes
/// none (checked on the ledger, not the board).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_a_loop_with_a_tool_call_writes_a_say() {
    let loops = Arc::new(AtomicUsize::new(0));
    let r = rig(script(loops), |_| {});
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    until("the task is done", || {
        let tail: Vec<(u64, crate::ledger::LedgerRow)> = r.core.store.ledger_tail(2000).unwrap();
        tail.iter().any(|(_, row)| row.kind == "task.ended")
    })
    .await;
    let tail: Vec<(u64, crate::ledger::LedgerRow)> = r.core.store.ledger_tail(2000).unwrap();
    let rows: Vec<crate::ledger::LedgerRow> = tail
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| row.kind == "loop.ended")
        .collect();
    let with: Vec<&crate::ledger::LedgerRow> = rows
        .iter()
        .filter(|row| row.data["outcome"]["tool_calls"].as_u64().unwrap_or(0) > 0)
        .collect();
    assert!(with.len() >= 4, "{}", rows.len());
    for row in &rows {
        let calls = row.data["outcome"]["tool_calls"].as_u64().unwrap_or(0);
        let say = row.data.get("say");
        match (calls > 0, say) {
            (true, Some(s)) => assert!(s.as_str().unwrap().chars().count() <= 120),
            (true, None) => {
                // The loop that wrote no text before its call has no say.
                assert_eq!(
                    row.data["outcome"]["output_chars"].as_u64(),
                    Some(0),
                    "{row:?}"
                )
            }
            (false, s) => assert!(s.is_none(), "a loop with no call writes no say: {row:?}"),
        }
    }
}

/// The guidance is in the task's brief and the system note, each as its own
/// sentence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_plan_sentence_is_in_the_brief_and_the_system_note() {
    let brief = crate::task::brief_text_for_tests("tsk_abcdef", "ses_parent", "do it");
    assert!(brief.contains("list them first as plan items"), "{brief}");
    assert!(
        brief.contains("abcdef") && brief.contains("`task_close`"),
        "{brief}"
    );
    let r = rig(|_| Scripted::text("ok"), |_| {});
    let note = r.core.tools.system_note_for(crate::ceiling::PlaceView {
        class: crate::places::PlaceClass::Private,
        ceiling: None,
        voice: false,
        draft: None,
    });
    assert!(note.contains(crate::task_graph::PLAN_NOTE), "{note}");
}
