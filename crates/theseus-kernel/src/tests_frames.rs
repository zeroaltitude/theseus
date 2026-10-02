//! The kernel's frames, as a golden (theseus-0owd, C6's move): every frame
//! the kernel commits through a scripted run of its transitions, each combined
//! one among them, as its observer sees it, record by record. Ids are aliased
//! (each uuid's tail becomes `#1`, `#2`, … by first appearance, so `act_#3`
//! and the wake `wak_#3` it set stay paired), and time is the virtual clock's,
//! so the transcript is the same on every run. It was written before the
//! kernel transaction, which must leave it byte-identical: the same frames,
//! the same records in the same order, the same payloads, and the same
//! results. `THESEUS_GOLDEN=write` rewrites it, for a change you mean.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_store::{kinds, NewRecord};

use crate::clock::Clock;
use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/kernel_frames.txt"
);

/// The transcript: each step's name and result, then each frame it wrote.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<String>>);

impl Log {
    fn line(&self, s: impl AsRef<str>) {
        let mut l = self.0.lock().unwrap();
        l.push_str(s.as_ref());
        l.push('\n');
    }

    fn step<T>(&self, name: &str, r: anyhow::Result<T>, show: impl Fn(&T) -> String) -> Option<T> {
        match r {
            Ok(v) => {
                self.line(format!("-- {name}: ok {}", show(&v)));
                Some(v)
            }
            Err(e) => {
                self.line(format!("-- {name}: err {e:#}"));
                None
            }
        }
    }

    fn text(&self) -> String {
        alias(&self.0.lock().unwrap())
    }
}

fn kind(k: u16) -> String {
    match k {
        kinds::EXECUTION => "EXECUTION".into(),
        kinds::ACTION => "ACTION".into(),
        kinds::COMPLETION => "COMPLETION".into(),
        kinds::LEDGER => "LEDGER".into(),
        kinds::NODE => "NODE".into(),
        k => format!("KIND{k}"),
    }
}

/// Every uuid tail (32 hex digits after `_`) becomes `#n`, numbered by first
/// appearance.
fn alias(text: &str) -> String {
    let mut seen: Vec<String> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let tail = &b[i..b.len().min(i + 33)];
        let is_id = tail.len() == 33
            && tail[0] == b'_'
            && tail[1..]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
            && b.get(i + 33).is_none_or(|c| !c.is_ascii_hexdigit());
        if is_id {
            let id = String::from_utf8_lossy(&tail[1..]).into_owned();
            let n = match seen.iter().position(|s| *s == id) {
                Some(n) => n + 1,
                None => {
                    seen.push(id);
                    seen.len()
                }
            };
            out.push_str(&format!("_#{n}"));
            i += 33;
        } else {
            let c = text[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// A record of the caller's (a node), for the transitions that carry them.
fn node(tag: &str) -> Vec<NewRecord> {
    vec![NewRecord::json(kinds::NODE, Some(tag), &json!({"node": tag})).unwrap()]
}

fn act(a: &Action) -> String {
    format!("{} {} {}", a.correlation_id, a.tool, a.state.as_str())
}

fn exe(e: &Execution) -> String {
    format!("{} {}", e.id, e.state.as_str())
}

/// The scripted run. Each step is one call of the kernel's API; the frames it
/// writes follow its line.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn script(w: &World, log: &Log) {
    let k = &w.kernel;
    let tick = || {
        w.clock.advance(7);
    };
    let p = proposal("fs.read");
    let ask = proposal("proc.run");

    // A conversation's first turn: its input wakes and admits it in one frame.
    let e = log
        .step(
            "open_execution",
            k.open_execution(
                "ses_golden",
                SessionKind::Conversation,
                auth(),
                Some(100_000),
                None,
            ),
            exe,
        )
        .unwrap();
    tick();
    let g = log
        .step("admit_input", k.admit_input(&e.id), |g| g.turn.to_string())
        .unwrap();
    let turn = k.view(k.store().clone()).turn_of(&e.id);
    tick();
    // The provider call: planned, authorized, and dispatched in one frame, and
    // its answer the turn's own result.
    let call = log
        .step(
            "plan_and_dispatch (provider call)",
            turn.plan_and_dispatch(
                &g,
                &proposal(PROVIDER_TOOL),
                RetryClass::SafeToRepeat,
                Some(60_000),
                2_000,
                |a| Ok(node(&format!("call-{}", a.correlation_id))),
            ),
            act,
        )
        .unwrap();
    tick();
    log.step(
        "accept_completion_with (the turn's own)",
        turn.accept_completion_with(
            &completion(&call.correlation_id, Outcome::Succeeded, Some(1_500)),
            node("answer"),
        ),
        |a| format!("{a:?}"),
    );
    tick();
    // A tool call planned with its node, then authorized and dispatched alone.
    let read = log
        .step(
            "plan_action_with",
            k.plan_action_with(&g, &p, RetryClass::SafeToRepeat, None, 500, |a| {
                Ok(node(&format!("toolcall-{}", a.correlation_id)))
            }),
            act,
        )
        .unwrap();
    log.step(
        "authorize",
        k.authorize(&read.correlation_id, &p, None),
        act,
    );
    log.step(
        "dispatch",
        k.dispatch(&read.correlation_id, Some("op-1")),
        act,
    );
    tick();
    // Its result arrives from outside the turn: queued for the next.
    log.step(
        "accept_completion (queued)",
        k.accept_completion(&completion(
            &read.correlation_id,
            Outcome::Failed,
            Some(400),
        )),
        |a| format!("{a:?}"),
    );
    log.step(
        "take_results_with",
        k.take_results_with(&g, |taken| Ok(node(&format!("results-{}", taken.len())))),
        |v| v.iter().map(act).collect::<Vec<_>>().join(", "),
    );
    log.step(
        "take_results_with (none queued)",
        k.take_results_with(&g, |_| Ok(node("never"))),
        |v| v.len().to_string(),
    );
    tick();
    // A call that waits for the operator, and the turn parks on it.
    let q = log
        .step(
            "plan_confirm_with",
            k.plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, Some(60_000), |a| {
                Ok(node(&format!("question-{}", a.correlation_id)))
            }),
            act,
        )
        .unwrap();
    log.step(
        "end_turn_with (wait on the question)",
        k.end_turn_with(
            g,
            TurnEnd::Wait {
                wake: Wake::Confirm {
                    confirm_id: q.correlation_id.clone(),
                },
            },
            |e| Ok(node(&format!("end-{}", e.turns))),
        ),
        exe,
    );
    tick();
    // The answer, as the core writes it today: bind, then wake.
    log.step(
        "bind_confirm",
        k.bind_confirm(&q.correlation_id, "operator", &ask),
        act,
    );
    log.step("wake (confirmed)", k.wake(&e.id, "confirmed"), exe);
    tick();
    let g = log
        .step("admit", k.admit(&e.id), |g| g.turn.to_string())
        .unwrap();
    log.step(
        "authorize_and_dispatch (invalid: other arguments)",
        k.authorize_and_dispatch(&q.correlation_id, &p, Some("operator"), None),
        |r| match r {
            Ok(a) => act(a),
            Err(e) => format!("refused: {e:#}"),
        },
    );
    log.step(
        "authorize_and_dispatch",
        k.authorize_and_dispatch(&q.correlation_id, &ask, Some("operator"), Some("job-1")),
        |r| match r {
            Ok(a) => act(a),
            Err(e) => format!("refused: {e:#}"),
        },
    );
    tick();
    // A second question, declined.
    let q2 = log
        .step(
            "plan_confirm_with (second)",
            k.plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, None, |_| Ok(vec![])),
            act,
        )
        .unwrap();
    log.step(
        "end_turn_with (wait on the job and the question)",
        k.end_turn_with(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![q.correlation_id.clone()],
                },
            },
            |_| Ok(vec![]),
        ),
        exe,
    );
    tick();
    log.step(
        "decline_action",
        k.decline_action(&q2.correlation_id, "operator", "not now"),
        act,
    );
    log.step("wake (declined)", k.wake(&e.id, "declined"), exe);
    tick();
    // The job's result wakes nothing more (already queued), then the turn.
    log.step(
        "accept_completion_with (the job, outside the turn)",
        k.accept_completion_with(
            &completion(&q.correlation_id, Outcome::Succeeded, None),
            node("job-result"),
        ),
        |a| format!("{a:?}"),
    );
    log.step(
        "accept_completion (duplicate)",
        k.accept_completion(&completion(&q.correlation_id, Outcome::Succeeded, None)),
        |a| format!("{a:?}"),
    );
    log.step(
        "accept_completion (no such action)",
        k.accept_completion(&completion(
            "act_00000000000000000000000000000000",
            Outcome::Succeeded,
            None,
        )),
        |a| format!("{a:?}"),
    );
    tick();
    let g = log
        .step("admit (after the decline)", k.admit(&e.id), |g| {
            g.turn.to_string()
        })
        .unwrap();
    log.step(
        "take_results_with (two)",
        k.take_results_with(&g, |t| Ok(node(&format!("results-{}", t.len())))),
        |v| v.len().to_string(),
    );
    // Over budget: the question, and the turn parks on it; the reset wakes it.
    let b = log
        .step("ask_budget", k.ask_budget(&g, 250_000), act)
        .unwrap();
    log.step(
        "end_turn_with (wait on the budget)",
        k.end_turn_with(
            g,
            TurnEnd::Wait {
                wake: Wake::Budget {
                    correlation_id: b.correlation_id.clone(),
                },
            },
            |_| Ok(vec![]),
        ),
        exe,
    );
    tick();
    log.step(
        "reset_budget",
        k.reset_budget(&b.correlation_id, "operator"),
        |(e, before)| format!("{} {before}", exe(e)),
    );
    tick();
    let g = log
        .step("admit (after the reset)", k.admit(&e.id), |g| {
            g.turn.to_string()
        })
        .unwrap();
    log.step(
        "take_results_with (the reset)",
        k.take_results_with(&g, |_| Ok(vec![])),
        |v| v.len().to_string(),
    );
    // A wake it sets for itself, due while its turn runs.
    let wcall = new_id("act");
    log.step(
        "set_wake",
        k.set_wake(
            &g,
            &wcall,
            w.clock.now_ms() + 1_000,
            "check the build",
            None,
        ),
        |s| format!("{s:?}"),
    );
    let job = log
        .step(
            "plan_and_dispatch (a job)",
            k.plan_and_dispatch(&g, &ask, RetryClass::NonRepeatable, None, 300, |_| {
                Ok(vec![])
            }),
            act,
        )
        .unwrap();
    w.clock.advance(1_000);
    log.step(
        "end_turn_with (wait on input, a wake due)",
        k.end_turn_with(g, TurnEnd::Wait { wake: Wake::Input }, |e| {
            Ok(node(&format!("end-{}", e.turns)))
        }),
        exe,
    );
    tick();
    let g = log
        .step("admit (the wake's turn)", k.admit(&e.id), |g| {
            g.turn.to_string()
        })
        .unwrap();
    log.step(
        "take_wakes",
        k.take_wakes(&g, |f| Ok(node(&format!("wakes-{}", f.len())))),
        |f| f.len().to_string(),
    );
    log.step(
        "take_reports (none)",
        k.take_reports(&g, |_| Ok(vec![])),
        |t| format!("{t:?}"),
    );
    // A task, opened by this turn, with a wake for its parent.
    let tcall = new_id("act");
    let t = log
        .step(
            "open_task",
            k.open_task(
                &g,
                &tcall,
                20_000,
                Some("discord:dm:golden".into()),
                true,
                |t| Ok(node(&format!("brief-{}", t.id))),
            ),
            |t| format!("{} opened {}", exe(&t.task), t.opened),
        )
        .unwrap();
    log.step(
        "end_turn_with (the parent waits on input)",
        k.end_turn_with(g, TurnEnd::Wait { wake: Wake::Input }, |_| Ok(vec![])),
        exe,
    );
    tick();
    let tg = log
        .step("admit (the task)", k.admit(&t.task.id), |g| {
            g.turn.to_string()
        })
        .unwrap();
    let tturn = k.view(k.store().clone()).turn_of(&t.task.id);
    let tc = log
        .step(
            "plan_and_dispatch (the task's call)",
            tturn.plan_and_dispatch(
                &tg,
                &proposal(PROVIDER_TOOL),
                RetryClass::SafeToRepeat,
                None,
                3_000,
                |_| Ok(vec![]),
            ),
            act,
        )
        .unwrap();
    log.step(
        "accept_completion_with (the task's own, carried to the parent)",
        tturn.accept_completion_with(
            &completion(&tc.correlation_id, Outcome::Succeeded, Some(2_500)),
            node("task-answer"),
        ),
        |a| format!("{a:?}"),
    );
    tick();
    log.step(
        "end_turn_with (the task completes, reporting)",
        k.end_turn_with(
            tg,
            TurnEnd::Complete {
                reason: "done".into(),
            },
            |e| Ok(node(&format!("report-{}", e.id))),
        ),
        exe,
    );
    tick();
    let g = log
        .step("admit (the report's turn)", k.admit(&e.id), |g| {
            g.turn.to_string()
        })
        .unwrap();
    log.step(
        "take_reports",
        k.take_reports(&g, |r| Ok(node(&format!("reports-{}", r.len())))),
        |t| format!("{t:?}"),
    );
    log.step(
        "set_wake (later)",
        k.set_wake(&g, &new_id("act"), w.clock.now_ms() + 60_000, "later", None),
        |s| format!("{s:?}"),
    );
    // A stop during the turn: the running job is told to stop, the planned
    // call declined, and the turn's end parks on input.
    let planned = log
        .step(
            "plan_action_with (left planned)",
            k.plan_action_with(&g, &p, RetryClass::SafeToRepeat, None, 100, |_| Ok(vec![])),
            act,
        )
        .unwrap();
    log.step(
        "stop_execution",
        k.stop_execution(&e.id, "the operator"),
        |s| {
            format!(
                "{:?}",
                s.as_ref()
                    .map(|s| (s.to_kill.len(), s.declined.len(), s.turn_running))
            )
        },
    );
    log.step(
        "authorize (after the stop)",
        k.authorize(&planned.correlation_id, &p, None),
        act,
    );
    log.step(
        "end_turn_with (after the stop)",
        k.end_turn_with(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![],
                },
            },
            |_| Ok(node("stopped-end")),
        ),
        exe,
    );
    log.step(
        "cancel_acknowledged",
        k.cancel_acknowledged(&job.correlation_id),
        act,
    );
    log.step(
        "cancel_verified",
        k.cancel_verified(&job.correlation_id),
        act,
    );
    tick();
    // Another turn, which dies with a job out and a call planned: a cancel
    // settles the planned call and asks the job to stop.
    let g = log
        .step("admit_input (after the stop)", k.admit_input(&e.id), |g| {
            g.turn.to_string()
        })
        .unwrap();
    let job2 = log
        .step(
            "plan_and_dispatch (a second job)",
            k.plan_and_dispatch(&g, &ask, RetryClass::NonRepeatable, None, 0, |_| Ok(vec![])),
            act,
        )
        .unwrap();
    let left = log
        .step(
            "plan_confirm_with (left waiting)",
            k.plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, None, |_| Ok(vec![])),
            act,
        )
        .unwrap();
    log.step(
        "end_turn_with (wait on the job)",
        k.end_turn_with(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![job2.correlation_id.clone()],
                },
            },
            |_| Ok(vec![]),
        ),
        exe,
    );
    tick();
    log.step(
        "mark_unknown",
        k.mark_unknown(&job2.correlation_id, "deadline"),
        act,
    );
    log.step(
        "cancel_execution_with",
        k.cancel_execution_with(&e.id, "the operator", |end| {
            Ok(node(&format!(
                "cancelled-{}-{}-{}",
                end.execution.id,
                end.not_run.len(),
                end.turn_running
            )))
        }),
        |c| format!("to_kill {} not_run {}", c.to_kill.len(), c.not_run.len()),
    );
    log.step(
        "cancel_execution_with (again)",
        k.cancel_execution_with(&e.id, "the operator", |_| Ok(node("never"))),
        |c| format!("to_kill {} not_run {}", c.to_kill.len(), c.not_run.len()),
    );
    log.step(
        "accept_completion (late, after the cancel)",
        k.accept_completion(&completion(
            &job2.correlation_id,
            Outcome::Succeeded,
            Some(90),
        )),
        |a| format!("{a:?}"),
    );
    log.step(
        "decline_action (already settled)",
        k.decline_action(&left.correlation_id, "operator", "late"),
        act,
    );

    // A second conversation: a turn that completes with a job still out.
    let e2 = log
        .step(
            "open_execution (second)",
            k.open_execution(
                "ses_golden_two",
                SessionKind::Conversation,
                auth(),
                None,
                None,
            ),
            exe,
        )
        .unwrap();
    tick();
    let g2 = log
        .step("admit_input (second)", k.admit_input(&e2.id), |g| {
            g.turn.to_string()
        })
        .unwrap();
    log.step(
        "plan_and_dispatch (over budget)",
        k.plan_and_dispatch(
            &g2,
            &ask,
            RetryClass::NonRepeatable,
            None,
            1_000_000_000,
            |_| Ok(node("never")),
        ),
        act,
    );
    log.step(
        "plan_and_dispatch (second's job)",
        k.plan_and_dispatch(&g2, &ask, RetryClass::NonRepeatable, None, 0, |_| {
            Ok(vec![])
        }),
        act,
    );
    log.step(
        "end_turn_with (complete, a job out)",
        k.end_turn_with(
            g2,
            TurnEnd::Complete {
                reason: "finished".into(),
            },
            |e| Ok(node(&format!("end-{}", e.state.as_str()))),
        ),
        exe,
    );
    log.step("admit_input (ended)", k.admit_input(&e2.id), |g| {
        g.turn.to_string()
    });
}

/// A third execution waits while the ceiling is full: its input is written,
/// and admission's refusal returned.
fn ceiling(log: &Log) {
    let w = world_with(KernelConfig {
        admission_ceiling: 1,
        ..KernelConfig::default()
    });
    let l = log.clone();
    w.kernel
        .observe(Arc::new(move |c: Committed<'_>| frame(&l, &c)));
    let k = &w.kernel;
    let a = k
        .open_execution(
            "ses_golden_a",
            SessionKind::Conversation,
            auth(),
            None,
            None,
        )
        .unwrap();
    let b = k
        .open_execution(
            "ses_golden_b",
            SessionKind::Conversation,
            auth(),
            None,
            None,
        )
        .unwrap();
    let _held = log.step(
        "admit_input (a, the ceiling's one)",
        k.admit_input(&a.id),
        |g| g.turn.to_string(),
    );
    log.step(
        "admit_input (b, the ceiling full)",
        k.admit_input(&b.id),
        |g| g.turn.to_string(),
    );
    log.step("admit_input (b, again)", k.admit_input(&b.id), |g| {
        g.turn.to_string()
    });
}

/// A wake the driver's scan fires, and one cancelled; and a call an older
/// build's cancel left planned in a cancelled execution: answering it writes
/// its authorization and its cancel, and is refused.
fn legacy(log: &Log) {
    let w = world();
    let l = log.clone();
    w.kernel
        .observe(Arc::new(move |c: Committed<'_>| frame(&l, &c)));
    let k = &w.kernel;
    let ask = proposal("proc.run");
    let e1 = k
        .open_execution(
            "ses_golden_wakes",
            SessionKind::Conversation,
            auth(),
            None,
            None,
        )
        .unwrap();
    let g = k.admit_input(&e1.id).unwrap();
    let now = w.clock.now_ms();
    log.step(
        "set_wake (soon)",
        k.set_wake(&g, &new_id("act"), now + 1_000, "soon", None),
        |s| s.wake.id.clone(),
    );
    let later = new_id("act");
    log.step(
        "set_wake (later)",
        k.set_wake(&g, &later, now + 90_000, "later", None),
        |s| s.wake.id.clone(),
    );
    log.step(
        "end_turn_with (wait on input)",
        k.end_turn_with(g, TurnEnd::Wait { wake: Wake::Input }, |_| Ok(vec![])),
        exe,
    );
    log.step(
        "cancel_wake",
        k.cancel_wake(&e1.id, &crate::wakes::wake_id(&later), "the operator"),
        |r| format!("{:?}", r.as_ref().map(|(e, w)| (exe(e), w.id.clone()))),
    );
    w.clock.advance(1_000);
    log.step("fire_due", k.fire_due(&e1.id), |r| {
        format!("{:?}", r.as_ref().map(exe))
    });

    let e2 = k
        .open_execution(
            "ses_golden_old",
            SessionKind::Conversation,
            auth(),
            None,
            None,
        )
        .unwrap();
    let g = k.admit_input(&e2.id).unwrap();
    let q = k
        .plan_confirm_with(&g, &ask, RetryClass::NonRepeatable, None, |_| Ok(vec![]))
        .unwrap();
    log.step(
        "end_turn_with (wait on the question)",
        k.end_turn_with(
            g,
            TurnEnd::Wait {
                wake: Wake::Confirm {
                    confirm_id: q.correlation_id.clone(),
                },
            },
            |_| Ok(vec![]),
        ),
        exe,
    );
    // An older build's cancel: the execution ends, and the question stays.
    let mut old = k.execution(&e2.id).unwrap().unwrap();
    old.state = ExecState::Cancelled;
    old.ended_reason = Some("cancelled by an older build".into());
    old.wake = None;
    log.line("-- an older build's cancel (written directly)");
    k.commit(&[exec_record(&old).unwrap()]).unwrap();
    log.step(
        "bind_confirm (legacy)",
        k.bind_confirm(&q.correlation_id, "operator", &ask),
        act,
    );
    log.step(
        "authorize_and_dispatch (legacy: refused)",
        k.authorize_and_dispatch(&q.correlation_id, &ask, Some("operator"), None),
        |r| match r {
            Ok(a) => act(a),
            Err(e) => format!("invalid: {e:#}"),
        },
    );
    log.step(
        "dispatch (after the refusal)",
        k.dispatch(&q.correlation_id, None),
        act,
    );
}

fn frame(log: &Log, c: &Committed<'_>) {
    log.line(format!("== frame: {} records", c.records.len()));
    for r in c.records {
        log.line(format!(
            "{} key={} scope={} schema={} {}",
            kind(r.kind),
            r.key.as_deref().unwrap_or("-"),
            r.scope.as_deref().unwrap_or("-"),
            r.schema,
            String::from_utf8_lossy(&r.payload)
        ));
    }
}

#[test]
fn the_kernels_frames_match_their_golden() {
    let log = Log::default();
    let w = world();
    let l = log.clone();
    w.kernel
        .observe(Arc::new(move |c: Committed<'_>| frame(&l, &c)));
    script(&w, &log);
    log.line("== a second world: the admission ceiling");
    ceiling(&log);
    log.line("== a third world: wakes, and an older build's cancel");
    legacy(&log);
    let got = log.text();
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(GOLDEN).unwrap_or_default();
    if got != want {
        let first = got
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(got.lines().count().min(want.lines().count()));
        panic!(
            "the kernel's frames differ from {GOLDEN} at line {}:\n  got:  {}\n  want: {}\n({} lines got, {} want; \
             THESEUS_GOLDEN=write rewrites it, for a change you mean)",
            first + 1,
            got.lines().nth(first).unwrap_or("<end>"),
            want.lines().nth(first).unwrap_or("<end>"),
            got.lines().count(),
            want.lines().count()
        );
    }
}
