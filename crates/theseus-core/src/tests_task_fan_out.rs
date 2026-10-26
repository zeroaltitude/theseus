//! `task.fan_out` (theseus-z9mze.3): helpers that each read one area and
//! report in the turn that started them. The wait they end in is
//! `task_wait::wait_on_tasks`, proved in `tests_task_wait.rs`; what is proved
//! here is the opening, the split of the money and its floor, the reader set,
//! the end at the carve, and that nothing caps the count.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_kernel::{ExecState, Execution};
use theseus_protocol::Usage;

use crate::node::{Body, Node, Origin};
use crate::provider::{ProviderRequest, Scripted};
use crate::task_wait::{TurnRef, WaitAsk};
use crate::tests_tasks::{
    answers_a_call, exec, first_user, last_user, parent_exec, parent_session, rig, turn, until,
    Rig, ASK,
};
use crate::Core;

/// The call the parent's model makes, set by each test.
type Call = Arc<Mutex<Value>>;

/// A rig whose parent answers `ASK` with one `task_fan_out` call (id `f_1`),
/// then says it is done; each helper answers as `child` says from its request.
fn fan_rig(
    call: Call,
    child: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static,
    tweak: impl FnOnce(&mut crate::Config),
) -> Rig {
    rig(
        move |req| {
            let first = first_user(req);
            if first.contains("AREA:") {
                return child(req);
            }
            if answers_a_call(req) {
                return Scripted::text("Fanned out.");
            }
            match last_user(req).as_str() {
                l if l.starts_with("START") => Scripted::tools(
                    "Fanning out.",
                    &[("f_1", "task_fan_out", call.lock().unwrap().clone())],
                ),
                _ => Scripted::text("Nothing more."),
            }
        },
        tweak,
    )
}

fn helpers(n: usize) -> Vec<Value> {
    (1..=n)
        .map(|i| {
            json!({"title": format!("area {i}"), "brief": format!("AREA: read crate {i} for bugs"),
                   "paths": [format!("crates/c{i}")]})
        })
        .collect()
}

fn tasks_of(core: &Core, parent: &str) -> Vec<Execution> {
    let rec: crate::session::SessionRecord = core.store.get_session(parent).unwrap().unwrap();
    core.kernel.tasks(rec.execution_id.as_deref()).unwrap()
}

/// The text and meta of the call `id`'s result in a session.
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

fn briefs(core: &Core, e: &Execution) -> String {
    core.store
        .session_nodes(&e.session_id)
        .unwrap()
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some(text.clone()),
            Body::Arrangement { pieces, .. } => Some(
                pieces
                    .iter()
                    .map(|p| p.text.clone().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn nodes_of(core: &Core, sid: &str) -> Vec<Node> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .collect()
}

/// Three helpers open as `exe_Xh1..h3`, each grouped by the call's id, each
/// with the report format in its brief and the turn's own message as its
/// objective.
#[tokio::test]
async fn three_helpers_open_with_the_group_the_report_format_and_the_turns_objective() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(3), "budget_usd": 6.0, "wait": false}),
    ));
    let r = fan_rig(call, |_| Scripted::text("x.rs:1 — f — bug — fix"), |_| {});
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    assert_eq!(tasks.len(), 3);
    let group = tasks[0].helper_of().expect("a helper").to_string();
    let tail = group.split_once('_').unwrap().1;
    for (i, t) in tasks.iter().enumerate() {
        assert_eq!(t.helper_of(), Some(group.as_str()), "one group");
        assert_eq!(t.id, format!("exe_{tail}h{}", i + 1));
        assert_eq!(t.session_id, format!("ses_{tail}h{}", i + 1));
        let text = briefs(&r.core, t);
        assert!(text.contains(crate::task_fan_out::REPORT_FORMAT), "{text}");
        assert!(
            text.contains(&format!("AREA: read crate {}", i + 1)),
            "{text}"
        );
        assert!(text.contains("Your area: crates/c"), "{text}");
        // The objective is the operator's message that asked, quoted by id.
        assert!(text.contains(crate::tests_tasks::QUOTE), "{text}");
    }
    let (text, meta) = result_of(&r.core, &parent, "f_1");
    assert!(text.starts_with("3 helpers started"), "{text}");
    assert_eq!(meta["tasks"].as_array().unwrap().len(), 3);
    assert_eq!(meta["fan_out"], group.as_str());
}

/// A plain task has no group.
#[tokio::test]
async fn a_plain_task_has_no_group() {
    let r = rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("done");
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            Scripted::tools(
                "Starting.",
                &[(
                    "t_1",
                    "task_create",
                    json!({"brief": "CHILD: plain", "budget_usd": 1.0,
                           "arrangement": crate::tests_tasks::arranged()}),
                )],
            )
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    assert!(tasks_of(&r.core, &parent)[0].helper_of().is_none());
}

/// With the budget given, the helpers split it equally; without it, half of
/// what the session has left. The floor refuses what the carve cannot afford,
/// in words that name how many it does; and a floor set far below one call's
/// worst-case reservation is raised to twice it.
#[tokio::test]
async fn the_budget_splits_equally_and_the_floor_refuses_a_carve_that_cannot_afford_it() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(3), "budget_usd": 6.0, "wait": false}),
    ));
    let r = fan_rig(
        call,
        |_| Scripted::text("ok"),
        |c| {
            c.kernel.spend_limit_usd = 8.0;
        },
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    for t in tasks_of(&r.core, &parent) {
        assert_eq!(t.budget.limit_micros, 2_000_000, "$6 over three helpers");
    }

    // Without `budget_usd`: half of what the session has left (a limit of $8).
    let call2: Call = Arc::new(Mutex::new(json!({"helpers": helpers(3), "wait": false})));
    let r2 = fan_rig(
        call2,
        |_| Scripted::text("ok"),
        |c| c.kernel.spend_limit_usd = 8.0,
    );
    let p2 = parent_session(&r2.core);
    turn(&r2.core, &p2, ASK).await;
    let limits: Vec<u64> = tasks_of(&r2.core, &p2)
        .iter()
        .map(|t| t.budget.limit_micros)
        .collect();
    assert_eq!(limits.len(), 3);
    assert!(
        limits.iter().all(|l| (1_300_000..=1_340_000).contains(l)),
        "half of $8, in three: {limits:?}"
    );

    // $4 for nine helpers: refused, in words that name the floor and the count.
    let call3: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(9), "budget_usd": 4.0, "wait": false}),
    ));
    let r3 = fan_rig(
        call3,
        |_| Scripted::text("ok"),
        |c| c.tools.helper_floor_usd = 1.0,
    );
    let p3 = parent_session(&r3.core);
    turn(&r3.core, &p3, ASK).await;
    let (text, _) = result_of(&r3.core, &p3, "f_1");
    let floor = crate::task_fan_out::call_floor(&r3.core, "claude-sonnet-5-5").max(1_000_000);
    assert!(text.contains("affords"), "{text}");
    assert!(text.contains("were asked for"), "{text}");
    assert!(text.contains(&theseus_kernel::usd(floor)), "{text}");
    assert!(tasks_of(&r3.core, &p3).is_empty(), "nothing was opened");

    // A floor of a cent is raised to twice one call's worst-case reservation.
    let call4: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(2), "budget_usd": 0.02, "wait": false}),
    ));
    let r4 = fan_rig(
        call4,
        |_| Scripted::text("ok"),
        |c| c.tools.helper_floor_usd = 0.01,
    );
    let p4 = parent_session(&r4.core);
    turn(&r4.core, &p4, ASK).await;
    let (text, _) = result_of(&r4.core, &p4, "f_1");
    let reserve_floor = crate::task_fan_out::call_floor(&r4.core, "claude-sonnet-5-5");
    assert!(
        reserve_floor > 10_000,
        "a call's reservation is more than a cent"
    );
    assert!(text.contains(&theseus_kernel::usd(reserve_floor)), "{text}");
    assert!(tasks_of(&r4.core, &p4).is_empty());
}

/// A `budget_usd` the model names is bounded by what the session can carve. The
/// owner (z9mze Q1): helpers share half of what is left, the parent keeps half.
/// An asked total far over the session's limit once passed the floor check at a
/// huge per-helper figure and carved several times the whole limit (the stack
/// review's F2), so the helpers together may never carve more than the session
/// has.
#[tokio::test]
async fn an_asked_budget_over_the_session_carves_no_more_than_the_session_has() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(3), "budget_usd": 600.0, "wait": false}),
    ));
    let r = fan_rig(
        call,
        |_| Scripted::text("ok"),
        |c| c.kernel.spend_limit_usd = 8.0,
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let carved: u64 = tasks_of(&r.core, &parent)
        .iter()
        .map(|t| t.budget.limit_micros)
        .sum();
    assert!(
        !tasks_of(&r.core, &parent).is_empty(),
        "an over-asked budget is clamped, not refused"
    );
    assert!(
        carved <= 8_000_000,
        "three helpers carved {carved} micro-dollars against a session limit of $8"
    );
}

/// A stop cancels the helpers a fan-out is waiting on, and only a fan-out's
/// wait does: the owner (z9mze Q3) "/stop cancels waiting helpers". A turn's
/// stop aborts the wait's future; the helpers the call opened end `Cancelled`,
/// while a plain `task.wait` of the same tasks leaves them running (the owner
/// started those separately). The stack review's F3.
#[tokio::test]
async fn a_stop_cancels_the_helpers_of_a_fan_out_wait_and_not_a_plain_waits_tasks() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(2), "budget_usd": 6.0, "wait": false}),
    ));
    let r = fan_rig(call, |_| Scripted::text("ok"), |_| {});
    // Each helper holds at the model's gate, so none ends by itself.
    *r.model.hold.lock().unwrap() = Some("AREA:".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    assert_eq!(tasks.len(), 2);
    let group = tasks[0].helper_of().expect("a helper").to_string();
    let ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let waiting = |fan_out: Option<String>| {
        let ask = WaitAsk {
            tasks: ids.clone(),
            timeout: std::time::Duration::from_secs(600),
            report_chars: 6000,
            fan_out,
            turn: TurnRef::for_exec(&r.core, &parent_exec(&r.core, &parent)),
        };
        tokio::spawn(crate::task_wait::wait_on_tasks(
            r.core.clone(),
            parent.clone(),
            ask,
        ))
    };

    // A plain wait is stopped: the tasks run on.
    let plain = waiting(None);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    plain.abort();
    let _ = plain.await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    for id in &ids {
        assert!(
            !exec(&r.core, id).state.is_terminal(),
            "a plain wait's stop leaves its tasks running: {:?}",
            exec(&r.core, id)
        );
    }

    // The fan-out's wait is stopped: its helpers are cancelled.
    let fan = waiting(Some(group));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    fan.abort();
    let _ = fan.await;
    until("the helpers are cancelled", || {
        ids.iter()
            .all(|id| exec(&r.core, id).state == ExecState::Cancelled)
    })
    .await;
}

/// There is no cap on the count. The owner (2026-10-24): "very very lightweight
/// on any limits to theseus tasking itself ... notify, work, and allow
/// feedback", and "All recs!" to the cap question: no per-turn child cap
/// beyond admission slots and spend. Twelve helpers at a budget that affords
/// the floor for all twelve open; those past the free slots queue, none is
/// refused for being wide.
#[tokio::test]
async fn twelve_helpers_open_and_those_past_the_free_slots_queue() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(12), "budget_usd": 36.0, "wait": false}),
    ));
    let r = fan_rig(
        call,
        |_| Scripted::text("ok"),
        |c| c.kernel.admission_ceiling = 3,
    );
    // Each helper holds its slot at the model's gate.
    *r.model.hold.lock().unwrap() = Some("AREA:".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    assert_eq!(tasks.len(), 12, "nothing refused it for width");
    let (text, _) = result_of(&r.core, &parent, "f_1");
    assert!(text.starts_with("12 helpers started"), "{text}");
    until("a slot fills", || {
        tasks
            .iter()
            .any(|t| exec(&r.core, &t.id).state == ExecState::Running)
    })
    .await;
    let states: Vec<ExecState> = tasks.iter().map(|t| exec(&r.core, &t.id).state).collect();
    assert!(
        states.contains(&ExecState::Queued),
        "past the slots they queue: {states:?}"
    );
    assert!(
        states.iter().all(|s| !s.is_terminal()),
        "none was refused: {states:?}"
    );
}

/// Bad input: one helper, an empty brief, a title past 80 characters and a
/// brief past the limit each fail in words that name what was wrong; inside a
/// task session the call is refused with the depth words.
#[tokio::test]
async fn bad_input_and_depth_are_refused_in_words() {
    let long_title = "t".repeat(81);
    let long_brief = "b".repeat(crate::task::MAX_BRIEF_CHARS + 1);
    let r = rig(
        move |req| {
            let first = first_user(req);
            if first.contains("CHILD: nested") {
                if answers_a_call(req) {
                    return Scripted::text("Refused.");
                }
                return Scripted::tools(
                    "Fanning.",
                    &[("n_1", "task_fan_out", json!({"helpers": helpers(2)}))],
                );
            }
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            let bad = |h: Vec<Value>| json!({"helpers": h, "budget_usd": 6.0, "wait": false});
            let ok = json!({"title": "b", "brief": "x"});
            Scripted::tools(
                "Trying.",
                &[
                    ("b_1", "task_fan_out", bad(helpers(1))),
                    (
                        "b_2",
                        "task_fan_out",
                        bad(vec![json!({"title": "a", "brief": "  "}), ok.clone()]),
                    ),
                    (
                        "b_3",
                        "task_fan_out",
                        bad(vec![json!({"title": long_title, "brief": "x"}), ok.clone()]),
                    ),
                    (
                        "b_4",
                        "task_fan_out",
                        bad(vec![json!({"title": "a", "brief": long_brief}), ok]),
                    ),
                    (
                        "b_5",
                        "task_create",
                        json!({"brief": "CHILD: nested", "budget_usd": 1.0,
                               "arrangement": crate::tests_tasks::arranged()}),
                    ),
                ],
            )
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let said = |id: &str| result_of(&r.core, &parent, id).0;
    assert!(
        said("b_1").contains("at least 2 helpers"),
        "{}",
        said("b_1")
    );
    assert!(said("b_2").contains("empty brief"), "{}", said("b_2"));
    assert!(said("b_3").contains("title"), "{}", said("b_3"));
    assert!(said("b_4").contains("over the 32000"), "{}", said("b_4"));
    let nested = tasks_of(&r.core, &parent).remove(0);
    until("the nested task ended", || {
        exec(&r.core, &nested.id).state.is_terminal()
    })
    .await;
    assert_eq!(
        result_of(&r.core, &nested.session_id, "n_1").0,
        crate::task::DEPTH_REFUSAL
    );
}

/// A helper sees the reader set and no more: no write, no run, no task tool; a
/// call to one it was not offered is refused in a helper's words. A plain task
/// keeps its full set.
#[tokio::test]
async fn a_helper_reads_and_a_plain_task_keeps_its_full_set() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(2), "budget_usd": 6.0, "wait": false}),
    ));
    let r = rig(
        move |req| {
            let first = first_user(req);
            if first.contains("AREA:") {
                if answers_a_call(req) {
                    return Scripted::text("x.rs:1 — f — bug — fix");
                }
                return Scripted::tools(
                    "Trying to write.",
                    &[("h_1", "fs_write", json!({"path": "x.txt", "content": "no"}))],
                );
            }
            if first.contains("CHILD: plain") {
                return Scripted::text("done");
            }
            if answers_a_call(req) {
                return Scripted::text("Started.");
            }
            Scripted::tools(
                "Starting.",
                &[
                    ("f_1", "task_fan_out", call.lock().unwrap().clone()),
                    (
                        "t_1",
                        "task_create",
                        json!({"brief": "CHILD: plain", "budget_usd": 1.5,
                               "arrangement": crate::tests_tasks::arranged()}),
                    ),
                ],
            )
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    until("every task ended", || {
        tasks
            .iter()
            .all(|t| exec(&r.core, &t.id).state.is_terminal())
    })
    .await;
    let names = |req: &ProviderRequest| -> Vec<String> {
        req.tools
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect()
    };
    let reqs = r.model.requests.lock().unwrap().clone();
    let helper_req = reqs
        .iter()
        .find(|q| first_user(q).contains("AREA: read crate 1"))
        .unwrap();
    let plain_req = reqs
        .iter()
        .find(|q| first_user(q).contains("CHILD: plain"))
        .unwrap();
    let (h, p) = (names(helper_req), names(plain_req));
    for read in [
        "fs_read", "fs_grep", "fs_glob", "fs_list", "git_diff", "git_log",
    ] {
        assert!(
            h.iter().any(|n| n == read),
            "a helper reads with {read}: {h:?}"
        );
    }
    for no in [
        "fs_write",
        "fs_edit",
        "fs_patch",
        "proc_run",
        "task_create",
        "task_wait",
        "task_fan_out",
    ] {
        assert!(
            !h.iter().any(|n| n == no),
            "a helper is not offered {no}: {h:?}"
        );
    }
    assert_eq!(
        p.iter().any(|n| n == "tools_find"),
        h.iter().any(|n| n == "tools_find"),
        "tools_find is kept where there is one"
    );
    assert!(
        p.iter().any(|n| n == "fs_write") && p.iter().any(|n| n == "proc_run"),
        "{p:?}"
    );
    // The call it was not offered is refused in a helper's words.
    let helper = tasks
        .iter()
        .find(|t| briefs(&r.core, t).contains("AREA: read crate 1"))
        .unwrap();
    let (text, _) = result_of(&r.core, &helper.session_id, "h_1");
    assert!(text.contains("is not for a helper"), "{text}");
}

/// Usage that bills `out` output tokens and nothing else.
fn billed(out: u64) -> Usage {
    Usage {
        output_tokens: out,
        ..Default::default()
    }
}

/// Output tokens that cost `usd` on the rig's model, found from the catalog.
fn tokens_for(core: &Core, usd: f64) -> u64 {
    let per_million = core
        .catalog
        .cost_usd("claude-sonnet-5-5", &billed(1_000_000))
        .expect("the model is priced");
    (usd / per_million * 1_000_000.0) as u64
}

/// A helper at 75 % of its carve is told once to write its report; at its carve
/// it ends with the reason `budget` and posts no question, its findings so far
/// its report. A plain task at its carve still asks (`tests_tasks`).
#[tokio::test]
async fn a_helper_is_told_once_at_three_quarters_and_ends_at_its_carve() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(2), "budget_usd": 3.0, "wait": false}),
    ));
    // Output tokens per dollar, set once the rig's catalog can be asked.
    let outs: Arc<Mutex<[u64; 3]>> = Arc::default();
    let seen = outs.clone();
    let r = fan_rig(
        call,
        move |req| {
            let loops = req
                .messages
                .iter()
                .filter(|m| m["role"] == "assistant")
                .count();
            let id = format!("d_{loops}");
            let out = seen.lock().unwrap()[loops.min(2)];
            let say = |t: &str| Scripted::Billed {
                usage: billed(out),
                then: Box::new(Scripted::tools(
                    t,
                    &[(&id, "text_diff", json!({"a": "x\n", "b": "y\n"}))],
                )),
            };
            match loops {
                0 => say("a.rs:1 — f — first finding — fix"),
                1 => say("b.rs:2 — g — second finding — fix"),
                _ => say("c.rs:3 — h — third finding — fix"),
            }
        },
        |c| {
            c.tools.helper_floor_usd = 0.0001;
            // Under the default, a limit that notifies, a carve is a notice and
            // no call is refused at it: only a limit that asks is a line.
            c.kernel.spend_limit_mode = crate::config::SpendLimitMode::Ask;
        },
    );
    // A carve of $1.50: the first loop costs $1.15 (past 75 %), the second
    // $0.05, the third $0.20, and a fourth call's reservation no longer fits.
    *outs.lock().unwrap() = [
        tokens_for(&r.core, 1.15),
        tokens_for(&r.core, 0.05),
        tokens_for(&r.core, 0.20),
    ];
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let tasks = tasks_of(&r.core, &parent);
    until("both helpers end", || {
        tasks
            .iter()
            .all(|t| exec(&r.core, &t.id).state.is_terminal())
    })
    .await;
    for t in &tasks {
        let e = exec(&r.core, &t.id);
        assert_eq!(e.state, ExecState::Complete, "{e:?}");
        assert_eq!(e.ended_reason.as_deref(), Some("budget"), "{e:?}");
        assert!(e.budget.question.is_none(), "it asked nothing");
        // The note, once.
        let said = nodes_of(&r.core, &t.session_id)
            .iter()
            .filter(|n| n.author.as_deref() == Some(crate::turn::helper_step::NOTE_AUTHOR))
            .count();
        assert_eq!(said, 1, "told once");
    }
    assert!(
        crate::tests_tasks::posts(&r.core, "card").is_empty(),
        "no budget question was posted"
    );
    // The wait names the state, and the report carries what it found.
    let ask = WaitAsk {
        tasks: tasks.iter().map(|t| t.id.clone()).collect(),
        timeout: std::time::Duration::from_secs(2),
        report_chars: 6000,
        fan_out: Some("act_test".into()),
        turn: TurnRef::for_exec(&r.core, &parent_exec(&r.core, &parent)),
    };
    let out = crate::task_wait::wait_on_tasks(r.core.clone(), parent.clone(), ask)
        .await
        .unwrap();
    assert!(out.0.text.contains("ended at its budget"), "{}", out.0.text);
    let reports: String = nodes_of(&r.core, &parent)
        .iter()
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .map(|n| match &n.body {
            Body::UserMessage { text, .. } => text.clone(),
            _ => String::new(),
        })
        .collect();
    assert!(reports.contains("first finding"), "{reports}");
}

/// With `wait` (the default), the call opens the helpers and then waits for them
/// by the one wait `task.wait` runs: its result is the wait's index, grouped
/// by the call's id, and their reports are in the turn.
#[tokio::test]
async fn a_fan_out_waits_for_its_helpers_and_brings_their_reports_in_the_turn() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(3), "budget_usd": 6.0}),
    ));
    let r = fan_rig(
        call,
        |req| {
            let n = ["1", "2", "3"]
                .iter()
                .find(|n| first_user(req).contains(&format!("crate {n} ")))
                .unwrap();
            Scripted::text(&format!(
                "c{n}.rs:1 — f — bug in {n} — fix\nRead whole: c{n}.rs"
            ))
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    let done = turn(&r.core, &parent, ASK).await;
    assert!(done.output.ends_with("Fanned out."), "{}", done.output);
    let (text, meta) = result_of(&r.core, &parent, "f_1");
    assert!(text.starts_with("3 of 3 helpers reported"), "{text}");
    let group = tasks_of(&r.core, &parent)[0]
        .helper_of()
        .unwrap()
        .to_string();
    assert_eq!(meta["fan_out"], group.as_str());
    let reports: Vec<String> = nodes_of(&r.core, &parent)
        .iter()
        .filter(|n| n.author.as_deref().is_some_and(|a| a.starts_with("task:")))
        .map(|n| match &n.body {
            Body::UserMessage { text, .. } => text.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(reports.len(), 3, "{reports:?}");
    for n in ["1", "2", "3"] {
        assert!(
            reports.iter().any(|t| t.contains(&format!("bug in {n}"))),
            "{reports:?}"
        );
    }
    assert!(parent_exec(&r.core, &parent).reports.is_empty());
}

/// Under a limit that notifies (the default), a session past its limit still
/// fans out: the share is half of the greater of what it has left and its
/// limit, as `task_create`'s quarter is.
#[tokio::test]
async fn under_a_notify_limit_the_half_is_of_the_greater_of_the_rest_and_the_limit() {
    let call: Call = Arc::new(Mutex::new(json!({"helpers": helpers(2), "wait": false})));
    let spent = Arc::new(Mutex::new(0u64));
    let seen = spent.clone();
    let r = rig(
        move |req| {
            if first_user(req).contains("AREA:") {
                return Scripted::text("ok");
            }
            if answers_a_call(req) {
                return Scripted::text("Fanned out.");
            }
            // The first answer costs more than the whole limit.
            Scripted::Billed {
                usage: billed(*seen.lock().unwrap()),
                then: Box::new(Scripted::tools(
                    "Fanning out.",
                    &[("f_1", "task_fan_out", call.lock().unwrap().clone())],
                )),
            }
        },
        |c| c.kernel.spend_limit_usd = 12.0,
    );
    *spent.lock().unwrap() = tokens_for(&r.core, 13.0);
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let pe = parent_exec(&r.core, &parent);
    assert!(
        pe.budget.spent_micros > pe.budget.limit_micros,
        "the session is past its limit: {:?}",
        pe.budget
    );
    let limits: Vec<u64> = tasks_of(&r.core, &parent)
        .iter()
        .map(|t| t.budget.limit_micros)
        .collect();
    assert_eq!(limits, vec![3_000_000, 3_000_000], "half of $12, in two");
}

/// A crash between the call's opening of its helpers and its settle opens no
/// second set: the async call is not run again (its outcome is told as unknown,
/// as any async call's is), the helpers stand, and `task_wait` with no tasks
/// named collects them. A helper's ids come from the call's, so the same call
/// could only ever find the same helpers (`Opening::opened`).
#[tokio::test]
async fn a_crash_after_the_helpers_opened_leaves_one_set_and_the_wait_finds_it() {
    let call: Call = Arc::new(Mutex::new(
        json!({"helpers": helpers(2), "budget_usd": 6.0, "wait": false}),
    ));
    let r = fan_rig(call, |_| Scripted::text("x.rs:1 — f — bug — fix"), |_| {});
    let parent = parent_session(&r.core);
    let settles = |records: &[theseus_store::NewRecord]| {
        records.iter().any(|rec| {
            rec.kind == theseus_store::kinds::ACTION
                && serde_json::from_slice::<Value>(&rec.payload)
                    .is_ok_and(|a| a["tool"] == crate::task::FAN_OUT && a["state"] == "succeeded")
        })
    };
    r.core.store.fail_turn_frame(settles);
    let first = crate::test_support::try_turn(&r.core, &parent, ASK).await;
    assert!(first.is_err(), "the turn faults at the call's settle");
    assert_eq!(tasks_of(&r.core, &parent).len(), 2, "opened once");
    let e = parent_exec(&r.core, &parent);
    r.core.continue_execution(&e.id).await.unwrap();
    let tasks = tasks_of(&r.core, &parent);
    assert_eq!(tasks.len(), 2, "no second set");
    let (text, _) = result_of(&r.core, &parent, "f_1");
    assert!(text.contains("cannot be established"), "{text}");
    // The wait with nothing named finds both.
    until("both helpers end", || {
        tasks
            .iter()
            .all(|t| exec(&r.core, &t.id).state.is_terminal())
    })
    .await;
    let ask = WaitAsk {
        tasks: tasks.iter().map(|t| t.id.clone()).collect(),
        timeout: std::time::Duration::from_secs(2),
        report_chars: 6000,
        fan_out: None,
        turn: TurnRef::for_exec(&r.core, &parent_exec(&r.core, &parent)),
    };
    let out = crate::task_wait::wait_on_tasks(r.core.clone(), parent.clone(), ask)
        .await
        .unwrap();
    assert!(
        out.0.text.starts_with("2 of 2 tasks reported"),
        "{}",
        out.0.text
    );
}

/// The objective each helper quotes is the report that started this turn, not
/// the earlier exchange's operator message (the stack review's N6).
fn the_objective_is_the_report_that_started_the_turn(core: &Core, parent: &str) {
    for t in tasks_of(core, parent)
        .iter()
        .filter(|t| t.helper_of().is_some())
    {
        let text = briefs(core, t);
        assert!(text.contains("[Report from task"), "{text}");
        assert!(!text.contains(crate::tests_tasks::QUOTE), "{text}");
    }
}

/// A fan-out a turn makes by itself (a wake, a report) says why, as a task it
/// starts does (theseus-o1h3f.1): without `because` it is refused in the words
/// that name it and opens nothing; with it, each helper is a self-started task
/// carrying the reason.
#[tokio::test]
async fn a_fan_out_a_wake_makes_says_why_and_each_helper_carries_it() {
    let because: Call = Arc::new(Mutex::new(Value::Null));
    let seen = because.clone();
    let r = rig(
        move |req| {
            let first = first_user(req);
            if first.contains("CHILD:") {
                return Scripted::text("Surveyed.");
            }
            if first.contains("AREA:") {
                return Scripted::text("ok");
            }
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            let text = serde_json::to_string(&req.messages).unwrap();
            if text.contains("[Report from task") {
                let mut call = json!({"helpers": helpers(2), "budget_usd": 6.0, "wait": false});
                if let Some(b) = seen.lock().unwrap().as_str() {
                    call["because"] = json!(b);
                }
                return Scripted::tools("Following up.", &[("f_wake", "task_fan_out", call)]);
            }
            Scripted::tools(
                "Starting.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: survey", "budget_usd": 1.5, "wake_parent": true,
                           "arrangement": crate::tests_tasks::arranged()}),
                )],
            )
        },
        |_| {},
    );
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    until("the wake's refusal", || {
        nodes_of(&r.core, &parent).iter().any(
            |n| matches!(&n.body, Body::ToolResult { tool_use_id, .. } if tool_use_id == "f_wake"),
        )
    })
    .await;
    let (text, _) = result_of(&r.core, &parent, "f_wake");
    assert!(text.contains("`because`"), "{text}");
    assert_eq!(tasks_of(&r.core, &parent).len(), 1, "only the survey");

    // The same turn with a reason opens the helpers, each marked.
    *because.lock().unwrap() = json!("the survey named two areas it missed");
    let r2 = rig(
        {
            let seen = because.clone();
            move |req| {
                let first = first_user(req);
                if first.contains("CHILD:") || first.contains("AREA:") {
                    return Scripted::text("ok");
                }
                if answers_a_call(req) {
                    return Scripted::text("Done.");
                }
                let text = serde_json::to_string(&req.messages).unwrap();
                if text.contains("[Report from task") {
                    let call = json!({"helpers": helpers(2), "budget_usd": 6.0, "wait": false,
                                      "because": seen.lock().unwrap().as_str()});
                    return Scripted::tools("Following up.", &[("f_wake", "task_fan_out", call)]);
                }
                Scripted::tools(
                    "Starting.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": "CHILD: survey", "budget_usd": 1.5, "wake_parent": true,
                               "arrangement": crate::tests_tasks::arranged()}),
                    )],
                )
            }
        },
        |_| {},
    );
    let p2 = parent_session(&r2.core);
    turn(&r2.core, &p2, ASK).await;
    until("both helpers are recorded", || {
        tasks_of(&r2.core, &p2).len() == 3
    })
    .await;
    let marked: Vec<_> = crate::task_graph::all(&r2.core.store)
        .unwrap()
        .into_iter()
        .filter(|t| t.title.starts_with("area "))
        .collect();
    assert_eq!(marked.len(), 2);
    for t in marked {
        assert!(t.origin.self_started && t.origin.by_model, "{:?}", t.origin);
        assert_eq!(
            t.origin.because.as_deref(),
            Some("the survey named two areas it missed")
        );
    }
    the_objective_is_the_report_that_started_the_turn(&r2.core, &p2);
}

/// Which message a fan-out's helpers quote is the turn's own: the operator's,
/// else the last user-role message the turn holds. A turn with none (a retry,
/// a late result's continuation) is refused, in words that point at
/// `task_create`, and is never given an earlier exchange's message: a helper's
/// objective is an arrangement piece, `task.create` resolves a piece to a node
/// of the transcript and will not start without one, so `objective` and
/// `because` cannot stand in for it. Fan-out is unavailable on such a turn; the
/// stack review's N6 asked for a fallback and this is the decision instead.
#[test]
fn a_fan_out_quotes_the_turns_own_message_and_a_turn_with_none_is_refused() {
    use crate::stub::Stub;
    let user = |turn: &str, text: &str| Node::user("ses_n6", Some(turn), "owner", text);
    let report = |turn: &str| {
        let author = "task:ab12";
        Node::relayed(
            "ses_n6",
            Some(turn),
            Origin::Harness,
            author,
            "[Report from task ab12]",
        )
    };
    let transcript = |nodes: Vec<Node>| -> Vec<(u64, Stub)> {
        let at = |(i, n): (usize, Node)| (i as u64, Stub::from(n));
        nodes.into_iter().enumerate().map(at).collect()
    };
    let pick = |nodes: Vec<Node>| crate::task_fan_out::pick_input(&transcript(nodes), "trn_2");

    // No message of its own: refused, though an earlier exchange has one.
    let earlier = user("trn_1", "survey the repository");
    let refused = pick(vec![earlier.clone()]).unwrap_err();
    assert!(refused.contains("task_create"), "{refused}");
    // A report started the turn: it is the objective, not the earlier ask.
    let woken = report("trn_2");
    assert_eq!(
        pick(vec![earlier.clone(), woken.clone()]),
        Ok(woken.id.clone())
    );
    // Both in one turn: the operator's wins over the report beside it.
    let asked = user("trn_2", "fan it out");
    let both = pick(vec![earlier, asked.clone(), woken]);
    assert_eq!(both, Ok(asked.id));
}
