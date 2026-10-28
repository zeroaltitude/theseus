//! `task.wait` (theseus-z9mze.2): the turn waits in itself for the tasks it
//! started and reads their reports as relayed harness nodes, in the turn that
//! waited. The stand-in model reports only a few tokens a call, so what a
//! clearing pass would do is checked on the nodes' kind and the compiled
//! request, not by watching a stand-in run climb to the line.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution};

use crate::node::{Body, Node, Origin};
use crate::provider::{ProviderError, ProviderRequest, Scripted};
use crate::task_wait::{clamp_timeout, TurnRef, WaitAsk};
use crate::tests_tasks::{
    answers_a_call, arranged, exec, first_user, last_user, parent_exec, parent_session, rig, turn,
    until, Rig, ASK,
};
use crate::Core;

/// What the parent's model calls to wait, set by each test.
type WaitCall = Arc<Mutex<Value>>;

/// Whether the request's history already holds a call of `name`.
fn called(req: &ProviderRequest, name: &str) -> bool {
    req.messages.iter().any(|m| {
        m["content"].as_array().is_some_and(|c| {
            c.iter()
                .any(|b| b["type"] == "tool_use" && b["name"] == name)
        })
    })
}

/// `task_create` calls for `briefs`, each with the arrangement and a budget.
fn starts(briefs: &[&str]) -> Scripted {
    let calls: Vec<(String, Value)> = briefs
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (
                format!("t_{i}"),
                json!({"brief": b, "budget_usd": 1.0, "arrangement": arranged()}),
            )
        })
        .collect();
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, v)| (id.as_str(), "task_create", v.clone()))
        .collect();
    Scripted::tools("Starting.", &calls)
}

/// A rig whose parent starts `briefs` on `START` and then calls `task_wait`
/// with whatever `wait` holds; each child answers as `child` says.
fn waiting_rig(
    briefs: &'static [&'static str],
    wait: WaitCall,
    child: impl Fn(&str) -> Scripted + Send + Sync + 'static,
    tweak: impl FnOnce(&mut crate::Config),
) -> Rig {
    rig(
        move |req| {
            let first = first_user(req);
            if first.contains("CHILD:") {
                return child(&first);
            }
            if answers_a_call(req) && called(req, "task_wait") {
                return Scripted::text("The reports are in.");
            }
            if answers_a_call(req) {
                return Scripted::tools(
                    "Waiting.",
                    &[("w_1", "task_wait", wait.lock().unwrap().clone())],
                );
            }
            match last_user(req).as_str() {
                l if l.starts_with("START") => starts(briefs),
                _ => Scripted::text("Nothing more."),
            }
        },
        tweak,
    )
}

/// The relayed report nodes of a session.
fn reports_in(core: &Core, sid: &str) -> Vec<Node> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .collect()
}

fn text_of(n: &Node) -> String {
    match &n.body {
        Body::UserMessage { text, .. } => text.clone(),
        _ => String::new(),
    }
}

/// The result of the call `id` in a session: its text and meta.
fn result_of(core: &Core, sid: &str, id: &str) -> (String, Value) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool_use_id,
                content,
                meta,
                late: false,
                ..
            } if tool_use_id == id => Some((content, meta)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result for {id}"))
}

fn tasks_of(core: &Core, parent: &str) -> Vec<Execution> {
    let rec: crate::session::SessionRecord = core.store.get_session(parent).unwrap().unwrap();
    core.kernel.tasks(rec.execution_id.as_deref()).unwrap()
}

/// Three tasks reply while the parent waits: their reports are relayed
/// harness nodes titled `task:<short>`, never tool results, written in the
/// turn that waited; the parent's next turn writes none of them again.
#[tokio::test]
async fn three_helpers_reply_and_their_reports_arrive_in_the_turn_that_waited() {
    let wait: WaitCall = Arc::new(Mutex::new(json!({})));
    let r = waiting_rig(
        &["CHILD: one", "CHILD: two", "CHILD: three"],
        wait,
        |first| {
            let n = ["one", "two", "three"]
                .iter()
                .find(|n| first.contains(&format!("CHILD: {n}")))
                .unwrap();
            Scripted::text(&format!("a.rs:1 — f — bug {n} — fix {n}\nRead whole: a.rs"))
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    let t0 = Instant::now();
    let done = turn(&r.core, &parent, ASK).await;
    assert!(
        done.output.ends_with("The reports are in."),
        "{}",
        done.output
    );
    assert!(t0.elapsed() < Duration::from_secs(25), "{:?}", t0.elapsed());
    let nodes = reports_in(&r.core, &parent);
    assert_eq!(nodes.len(), 3, "one per task, in the turn that waited");
    for n in &nodes {
        assert_eq!(n.origin, Origin::Harness);
        assert_ne!(
            n.kind_str(),
            "tool_result",
            "the clearing pass stubs only those"
        );
    }
    let all: String = nodes.iter().map(text_of).collect();
    for n in ["one", "two", "three"] {
        assert!(all.contains(&format!("bug {n}")), "{all}");
    }
    let (text, meta) = result_of(&r.core, &parent, "w_1");
    assert!(text.starts_with("3 of 3 tasks reported"), "{text}");
    assert!(
        text.contains("Each report follows as its own note."),
        "{text}"
    );
    assert!(
        text.chars().count() <= crate::task_wait::INDEX_MAX_CHARS,
        "{text}"
    );
    assert_eq!(meta["tasks"].as_array().unwrap().len(), 3);
    assert_eq!(meta["timed_out"], 0);
    assert!(parent_exec(&r.core, &parent).reports.is_empty());
    // The parent's next turn writes none of them again.
    turn(&r.core, &parent, "What came back?").await;
    assert_eq!(reports_in(&r.core, &parent).len(), 3, "read once");
}

/// A fan-in past the bound is cut to it, each report at its edge; the cut
/// line names the file, which holds the whole report.
#[tokio::test]
async fn a_fan_in_past_the_bound_is_cut_and_the_whole_report_is_kept() {
    let wait: WaitCall = Arc::new(Mutex::new(json!({"report_chars": 2000})));
    let r = waiting_rig(
        &["CHILD: one", "CHILD: two"],
        wait,
        |_| {
            let lines: Vec<String> = (0..400)
                .map(|i| format!("src/m{i}.rs:{i} — f{i} — the problem {i} — the fix {i}"))
                .collect();
            Scripted::text(&lines.join("\n"))
        },
        |c| c.tools.fan_in_chars = 3000,
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let (text, meta) = result_of(&r.core, &parent, "w_1");
    assert!(text.starts_with("2 of 2 tasks reported"), "{text}");
    assert!(["fan_in_chars", "room"].contains(&meta["fan_in"]["bound"].as_str().unwrap()));
    let nodes = reports_in(&r.core, &parent);
    assert_eq!(nodes.len(), 2);
    for n in &nodes {
        let t = text_of(n);
        assert!(t.chars().count() < 2_700, "{} chars", t.chars().count());
        assert!(
            t.contains("findings cut; the whole report: fs_read "),
            "{t}"
        );
        let path = t
            .split("fs_read ")
            .nth(1)
            .and_then(|s| s.split(|c: char| c.is_whitespace() || c == ']').next())
            .unwrap();
        let whole = std::fs::read_to_string(path).expect("the kept file");
        assert_eq!(whole.lines().count(), 400);
        assert!(whole.contains("the problem 399"));
    }
    let tasks = meta["tasks"].as_array().unwrap();
    assert!(tasks
        .iter()
        .all(|t| t["cut"] == true && t["outputs"].is_string()));
}

/// What the reports and the running notes may take is `total` characters among
/// them, and the least a text keeps does not defeat it (the stack review's N5):
/// an equal share when that is no less than the least; else the first
/// `total / least` keep the least and the rest take none.
#[test]
fn the_least_a_report_keeps_does_not_defeat_the_bound() {
    use crate::task_wait::{split_room, REPORT_CHARS_MIN as LEAST};
    assert_eq!(
        split_room(36_000, 4, 6_000),
        vec![6_000; 4],
        "room to spare"
    );
    assert_eq!(split_room(3_000, 2, 6_000), vec![1_500; 2], "equal shares");
    assert_eq!(split_room(3_500, 5, 6_000), vec![LEAST, LEAST, LEAST, 0, 0]);
    assert_eq!(split_room(0, 3, 6_000), vec![0; 3], "past the line: none");
    assert_eq!(
        split_room(400, 2, 6_000),
        vec![0; 2],
        "under the least: none"
    );
    for (total, n) in [(3_500, 5), (20_000, 40), (999, 7), (0, 12)] {
        let sum: usize = split_room(total, n, 6_000).iter().sum();
        assert!(sum <= total, "{n} texts took {sum} of {total}");
    }
}

/// Twelve reports in a fan-in of 2,500 characters: the room holds two at the
/// least a report keeps; the other ten arrive as a pointer to their whole text
/// in a kept file, so the bound holds however many reports there are.
#[tokio::test]
async fn many_reports_past_the_bound_arrive_as_pointers_after_the_ones_that_fit() {
    let wait: WaitCall = Arc::new(Mutex::new(json!({"report_chars": 2000})));
    let r = waiting_rig(
        &[
            "CHILD: a", "CHILD: b", "CHILD: c", "CHILD: d", "CHILD: e", "CHILD: f",
        ],
        wait,
        |_| {
            let lines: Vec<String> = (0..200)
                .map(|i| format!("src/m{i}.rs:{i} — f{i} — the problem {i} — the fix {i}"))
                .collect();
            Scripted::text(&lines.join("\n"))
        },
        |c| c.tools.fan_in_chars = 2_500,
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let nodes = reports_in(&r.core, &parent);
    assert_eq!(nodes.len(), 6, "{nodes:?}");
    let held: Vec<&Node> = nodes
        .iter()
        .filter(|n| text_of(n).contains("held back"))
        .collect();
    assert_eq!(held.len(), 4, "two fit, the other four are pointers");
    for n in &held {
        let t = text_of(n);
        assert!(t.contains("fs_read "), "{t}");
        assert!(t.chars().count() < 600, "a pointer is short: {t}");
    }
    let inline: usize = nodes
        .iter()
        .filter(|n| !text_of(n).contains("held back"))
        .map(|n| text_of(n).chars().count())
        .sum();
    assert!(inline < 2_500 + 2 * 400, "{inline} characters inline");
}

/// The room is half of what is left before the clearing line, never more than
/// `fan_in_chars`; with none known, `fan_in_chars` alone.
#[tokio::test]
async fn the_fan_in_bound_is_half_the_room_and_falls_back_without_one() {
    let r = rig(|_| Scripted::text("ok"), |c| c.tools.fan_in_chars = 36_000);
    let bound = |room| crate::task_wait::fan_in_bound(&r.core, room);
    assert_eq!(bound(None), (36_000, "fan_in_chars"));
    // A 1M window: the line is 50 %. 480K used leaves 20K, half is 10K
    // tokens, about 40K characters: `fan_in_chars` binds.
    assert_eq!(bound(Some((480_000, 1_000_000))), (36_000, "fan_in_chars"));
    // 490K used leaves 10K tokens: half is 5K, 20K characters.
    assert_eq!(bound(Some((490_000, 1_000_000))), (20_000, "room"));
    // Past the line there is no room.
    assert_eq!(bound(Some((600_000, 1_000_000))), (0, "room"));
}

/// One task never ends, one fails, one comes back empty-handed: the result at
/// the timeout has the stuck one `running`, the failure `failed`, the empty
/// one `done` with 0 findings, and the call succeeds. The stuck task keeps
/// running and is delivered later.
#[tokio::test]
async fn a_task_that_never_ends_a_failure_and_an_empty_report_are_told_apart() {
    let r = rig(
        |req| {
            let first = first_user(req);
            if first.contains("CHILD: stuck") {
                return Scripted::text("x.rs:9 — late — bug — fix");
            }
            if first.contains("CHILD: empty") {
                return Scripted::text("Read whole: x.rs");
            }
            if first.contains("CHILD: fails") {
                return Scripted::Fail(ProviderError::InvalidRequest {
                    status: 400,
                    message: "the stand-in refuses".into(),
                });
            }
            if first.contains("CHILD: good") {
                return Scripted::text("a.rs:1 — f — bug — fix\nb.rs:2 — g — bug — fix");
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            starts(&[
                "CHILD: good",
                "CHILD: empty",
                "CHILD: fails",
                "CHILD: stuck",
            ])
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD: stuck".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    assert_eq!(tasks.len(), 4);
    let stuck = task_named(&r.core, &tasks, "CHILD: stuck");
    until("the others ended", || {
        tasks
            .iter()
            .filter(|t| t.id != stuck.id)
            .all(|t| exec(&r.core, &t.id).state.is_terminal())
    })
    .await;
    let parent_ex = parent_exec(&r.core, &parent);
    let ask = WaitAsk {
        tasks: tasks.iter().map(|t| t.id.clone()).collect(),
        timeout: Duration::from_secs(2),
        report_chars: 6000,
        fan_out: None,
        turn: TurnRef::for_exec(&r.core, &parent_ex),
    };
    let out = crate::task_wait::wait_on_tasks(r.core.clone(), parent.clone(), ask)
        .await
        .expect("a failing task never fails the call");
    let (text, meta) = (out.0.text, out.0.meta);
    assert!(
        text.starts_with("2 of 4 tasks reported, 1 still running, 1 failed"),
        "{text}"
    );
    assert!(
        text.contains("0 findings: it came back empty-handed"),
        "{text}"
    );
    assert!(text.contains("task_wait "), "{text}");
    assert_eq!(meta["timed_out"], 1);
    let rows = meta["tasks"].as_array().unwrap();
    let state = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap()["state"].clone();
    assert_eq!(state(&stuck.id), "running");
    assert_eq!(rows.iter().filter(|r| r["state"] == "done").count(), 2);
    assert_eq!(rows.iter().filter(|r| r["state"] == "failed").count(), 1);
    // Three reports and a note for the one still running.
    let nodes = reports_in(&r.core, &parent);
    assert_eq!(nodes.len(), 4, "{nodes:?}");
    assert!(nodes
        .iter()
        .any(|n| text_of(n).contains("still running: its latest findings")));
    // It keeps running, and its report is delivered when it ends.
    assert!(!exec(&r.core, &stuck.id).state.is_terminal());
    r.model.gate.add_permits(1);
    until("the stuck task ends", || {
        exec(&r.core, &stuck.id).state == ExecState::Complete
    })
    .await;
    assert_eq!(
        parent_exec(&r.core, &parent).reports,
        vec![stuck.id.clone()]
    );
    turn(&r.core, &parent, "Anything else?").await;
    assert!(
        reports_in(&r.core, &parent)
            .iter()
            .any(|n| text_of(n).contains("late — bug")),
        "delivered at the next turn"
    );
}

/// A wait takes the tasks it names and leaves the rest: the report of a task it
/// did not name stays on the parent for its next turn, which delivers it once.
#[tokio::test]
async fn a_wait_takes_the_tasks_it_names_and_leaves_the_rest_for_the_next_turn() {
    let r = rig(
        |req| {
            let first = first_user(req);
            if let Some(n) = ["one", "two", "three"]
                .iter()
                .find(|n| first.contains(&format!("CHILD: {n}")))
            {
                return Scripted::text(&format!("a.rs:1 — f — bug {n} — fix"));
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            match last_user(req).as_str() {
                l if l.starts_with("START") => {
                    starts(&["CHILD: one", "CHILD: two", "CHILD: three"])
                }
                _ => Scripted::text("Nothing more."),
            }
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    until("all three ended", || {
        tasks
            .iter()
            .all(|t| exec(&r.core, &t.id).state.is_terminal())
    })
    .await;
    let named = &tasks[..2];
    let ask = WaitAsk {
        tasks: named.iter().map(|t| t.id.clone()).collect(),
        timeout: Duration::from_secs(2),
        report_chars: 6000,
        fan_out: Some("act_test".into()),
        turn: TurnRef::for_exec(&r.core, &parent_exec(&r.core, &parent)),
    };
    let out = crate::task_wait::wait_on_tasks(r.core.clone(), parent.clone(), ask)
        .await
        .unwrap();
    assert!(
        out.0.text.starts_with("2 of 2 helpers reported"),
        "{}",
        out.0.text
    );
    assert_eq!(out.0.meta["fan_out"], "act_test");
    assert_eq!(reports_in(&r.core, &parent).len(), 2);
    assert_eq!(
        parent_exec(&r.core, &parent).reports,
        vec![tasks[2].id.clone()],
        "the one it did not name waits for the next turn"
    );
    turn(&r.core, &parent, "What came back?").await;
    let all: String = reports_in(&r.core, &parent).iter().map(text_of).collect();
    assert_eq!(
        reports_in(&r.core, &parent).len(),
        3,
        "one each, none twice"
    );
    assert_eq!(all.matches("bug three").count(), 1, "{all}");
}

/// The task among `tasks` whose brief says `brief`.
fn task_named(core: &Core, tasks: &[Execution], brief: &str) -> Execution {
    tasks
        .iter()
        .find(|t| {
            let nodes = core.store.session_nodes(&t.session_id).unwrap();
            nodes.iter().any(|(_, n)| text_of(n).contains(brief))
        })
        .unwrap()
        .clone()
}

/// The clamp: 10 s goes up to 30, 3,600 down to 1,800, and a turn deadline 70
/// s away leaves 10 s (the greater of 60 s and 15 % kept back).
#[test]
fn the_timeout_is_clamped_to_the_range_and_to_the_turns_deadline() {
    let cfg = crate::Config::example();
    let secs = |a: Option<u64>, left: Option<u64>| {
        clamp_timeout(&cfg, a, left.map(Duration::from_secs)).as_secs()
    };
    assert_eq!(secs(Some(10), None), 30);
    assert_eq!(secs(Some(3_600), None), 1_800);
    assert_eq!(secs(None, None), 900);
    assert_eq!(secs(Some(900), Some(70)), 10);
    assert_eq!(
        secs(Some(900), Some(1_000)),
        850,
        "15 % of 1,000 is 150, over 60"
    );
    assert_eq!(
        secs(Some(900), Some(50)),
        0,
        "less left than the time kept back"
    );
    assert_eq!(secs(Some(200), Some(10_000)), 200);
}

/// `task_wait` in a task session at the depth limit (here 1) is refused with the depth words, and it runs
/// in a group of its own beside other calls.
#[tokio::test]
async fn a_task_session_cannot_wait_and_a_wait_runs_alone() {
    assert!(crate::task_wait::alone("task.wait"));
    assert!(crate::task_wait::alone("task.fan_out"));
    assert!(!crate::task_wait::alone("fs.read"));
    let r = rig(
        |req| {
            let first = first_user(req);
            if first.contains("CHILD: nested") {
                if answers_a_call(req) {
                    return Scripted::text("I could not wait.");
                }
                return Scripted::tools("Waiting.", &[("n_1", "task_wait", json!({}))]);
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            starts(&["CHILD: nested"])
        },
        |c| c.kernel.task_depth = 1,
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let t = tasks_of(&r.core, &parent).remove(0);
    until("the task ended", || {
        exec(&r.core, &t.id).state.is_terminal()
    })
    .await;
    let (text, _) = result_of(&r.core, &t.session_id, "n_1");
    assert_eq!(text, crate::task_lineage::depth_refusal(1, 1));
}

/// A person's message ends the wait at once, the tasks untouched.
#[tokio::test]
async fn a_message_ends_the_wait_early_and_the_tasks_go_on() {
    let wait: WaitCall = Arc::new(Mutex::new(json!({"timeout_secs": 900})));
    let r = waiting_rig(&["CHILD: stuck"], wait, |_| Scripted::text("late."), |_| {});
    *r.model.hold.lock().unwrap() = Some("CHILD: stuck".into());
    let parent = parent_session(&r.core);
    let (core, p) = (r.core.clone(), parent.clone());
    let first = tokio::spawn(async move { turn(&core, &p, ASK).await });
    // The wait is under way once the call is made.
    until("the wait began", || {
        r.core
            .store
            .session_nodes(&parent)
            .unwrap()
            .iter()
            .any(|(_, n)| {
                matches!(&n.body, Body::AssistantMessage { blocks, .. }
                    if serde_json::to_string(blocks).unwrap().contains("task_wait"))
            })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let t0 = Instant::now();
    let (core, p) = (r.core.clone(), parent.clone());
    let second = tokio::spawn(async move { turn(&core, &p, "Stop waiting, please.").await });
    let (a, b) = (first.await.unwrap(), second.await.unwrap());
    assert!(t0.elapsed() < Duration::from_secs(20), "{:?}", t0.elapsed());
    assert_eq!(a.turn_id, b.turn_id, "the running turn answered both");
    let (text, meta) = result_of(&r.core, &parent, "w_1");
    assert_eq!(meta["interrupted_by"], "message", "{text}");
    assert!(text.contains("1 still running"), "{text}");
    let task = tasks_of(&r.core, &parent).remove(0);
    assert!(!exec(&r.core, &task.id).state.is_terminal(), "untouched");
}

/// A parent that waits lends its admission slot after `LEND_AFTER`, or the
/// tasks it waits on are never admitted: with a ceiling of one slot the
/// parent holds it, and this deadlocks without the lend. It ends at 20 s
/// here, never at a hang.
#[tokio::test]
async fn a_waiting_parent_lends_its_slot_so_its_tasks_are_admitted() {
    let wait: WaitCall = Arc::new(Mutex::new(json!({})));
    let r = waiting_rig(
        &["CHILD: one"],
        wait,
        |_| Scripted::text("a.rs:1 — f — bug — fix"),
        |c| c.kernel.admission_ceiling = 1,
    );
    let parent = parent_session(&r.core);
    let (core, p) = (r.core.clone(), parent.clone());
    let turn = tokio::spawn(async move { turn(&core, &p, ASK).await });
    let done = tokio::time::timeout(Duration::from_secs(20), turn).await;
    let done = done
        .expect("the wait deadlocked: the parent kept the only slot")
        .unwrap();
    assert!(
        done.output.ends_with("The reports are in."),
        "{}",
        done.output
    );
    assert_eq!(reports_in(&r.core, &parent).len(), 1);
}

/// The parent's request after the wait has the same prefix bytes as the one
/// before it, only its tail grows, and no clearing pass ran: the wait compiled
/// nothing and appended no node while it waited, and the reports it brought
/// are harness notes, which clearing leaves whole.
#[tokio::test]
async fn the_request_after_a_wait_keeps_the_prefix_and_no_clearing_ran() {
    let wait: WaitCall = Arc::new(Mutex::new(json!({"report_chars": 2000})));
    let r = waiting_rig(
        &["CHILD: one", "CHILD: two", "CHILD: three"],
        wait,
        |_| {
            let lines: Vec<String> = (0..300)
                .map(|i| format!("src/m{i}.rs:{i} — f{i} — the problem {i} — the fix {i}"))
                .collect();
            Scripted::text(&lines.join("\n"))
        },
        |c| c.tools.fan_in_chars = 6000,
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let asked: Vec<ProviderRequest> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| !first_user(q).contains("CHILD:"))
        .cloned()
        .collect();
    assert_eq!(asked.len(), 3, "start, wait, answer");
    let (before, after) = (&asked[1].messages, &asked[2].messages);
    assert!(after.len() > before.len());
    // The last message of a request carries the turn's tail (the task graph's
    // note) and the cache breakpoint, which move on; everything before it is
    // the bytes it was.
    let bare = |m: &[Value]| {
        let mut v = serde_json::to_string(m).unwrap();
        while let Some(i) = v.find(r#""cache_control":{"type":"ephemeral"},"#) {
            v.replace_range(i..i + r#""cache_control":{"type":"ephemeral"},"#.len(), "");
        }
        v
    };
    let n = before.len() - 1;
    assert_eq!(
        bare(&after[..n]),
        bare(&before[..n]),
        "the prefix is the bytes it was"
    );
    let results = |m: &Value| {
        m["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["type"] == "tool_result")
            .map(|b| b["content"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        results(&after[n]),
        results(&before[n]),
        "its results are the same"
    );
    // The tail: the wait's call, then one message with its result first and
    // each report after it.
    let tail = serde_json::to_string(&after[before.len()..]).unwrap();
    assert_eq!(tail.matches("[Report from task").count(), 3, "{tail}");
    assert!(
        crate::test_support::rows_100k(&r.core, "context.cleared").is_empty(),
        "no clearing pass ran"
    );
}

/// Keep-warm: a wait past a five-minute cache's lifetime reads the request
/// again before it lapses, each read a `keep_warm` row marked `during_wait`,
/// and the index counts them. A task nothing drives never ends, so the wait
/// runs its whole 900 s on tokio's paused clock.
mod warm {
    use super::*;
    use crate::keep_warm::MAX_TOKENS;
    use crate::provider::{DeltaSink, FakeProvider, Provider, ProviderFuture};
    use crate::store::Store;

    struct Split {
        turns: Arc<FakeProvider>,
        reads: Arc<FakeProvider>,
    }

    impl Provider for Split {
        fn name(&self) -> &str {
            "fake"
        }
        fn stream_message<'a>(
            &'a self,
            req: &'a ProviderRequest,
            on_delta: DeltaSink<'a>,
        ) -> ProviderFuture<'a> {
            match req.max_tokens == MAX_TOKENS {
                true => self.reads.stream_message(req, on_delta),
                false => self.turns.stream_message(req, on_delta),
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_task_wait_past_the_cache_lifetime_reads_the_cache_before_it_lapses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        std::fs::create_dir_all(&root).unwrap();
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
        cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        cfg.policy.enforcement = crate::policy::Posture::Notify;
        cfg.memory.mode = crate::config::MemoryMode::Off;
        cfg.cache.keep_warm_min_tokens = 0;
        let store = Store::open(&dir.path().join("store")).unwrap();
        let turns = Arc::new(FakeProvider::scripted(vec![
            starts(&["CHILD: stuck"]),
            Scripted::tools(
                "Waiting.",
                &[("w_1", "task_wait", json!({"timeout_secs": 900}))],
            ),
            Scripted::text("Done."),
        ]));
        let reads = Arc::new(FakeProvider::default());
        let split = Arc::new(Split {
            turns: turns.clone(),
            reads: reads.clone(),
        });
        let core = crate::Core::build(crate::rpc::Parts::for_tests(cfg, split, store)).unwrap();
        let sid = parent_session(&core);
        let (c, s2) = (core.clone(), sid.clone());
        let run = tokio::spawn(async move { turn(&c, &s2, ASK).await });
        let t0 = tokio::time::Instant::now();
        while !run.is_finished() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            assert!(
                t0.elapsed() < Duration::from_secs(1200),
                "the wait never ended"
            );
        }
        run.await.unwrap();
        let said = result_of(&core, &sid, "w_1").0;
        assert_eq!(
            reads.requests().len(),
            3,
            "reads at 240, 480 and 720 s: {said}"
        );
        let rows = crate::test_support::rows_100k(&core, "keep_warm");
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(rows.iter().all(|w| w["during_wait"] == true), "{rows:?}");
        let (text, meta) = result_of(&core, &sid, "w_1");
        assert_eq!(meta["warm_reads"], 3, "{text}");
        assert!(text.contains("1 still running"), "{text}");
        assert_eq!(
            turns.requests().len(),
            3,
            "the turn's own calls are the script's"
        );
    }
}
