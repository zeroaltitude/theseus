//! The prove's records (M5 L3, roadmap row 50; `learning::prove`), on
//! seeded stores: invented tasks, judgments, labels and spend give exact
//! records, every field, both arms, and every reason a task is left out;
//! an open window gives no outcome; fewer than the minimum says
//! "insufficient" with its counts; and cohorts with known outcomes give the
//! generator's exact rates per task and per dollar. `judge.prove` answers
//! the generator's report over the records it answers, byte for byte, reads
//! its default window from the canary's move (a declined move none), names
//! a learned loop version placed inside it, and writes no frame. A task a
//! learned loop version judged is left out as `learned_version`.

use serde_json::{json, Value};
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::prove::{
    markdown, parse_records, prove, ArmName, ProveMinimum, Stop, StopDecision, TaskRecord,
    VerdictKind,
};
use theseus_judge::{Judgment, Thresholds};
use theseus_kernel::{ExecState, Execution};
use theseus_protocol::judge_runs::JudgeProveResult;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::learning::labels::system_key;
use crate::learning::prove::{build, Ended, Input, JudgeSpend};
use crate::learning::system::FALSE_COMPLETION_MS;
use crate::learning::{LabelRow, Seen, LAST_RUN};
use crate::ledger::LedgerRow;
use crate::rpc::Core;
use crate::tests_judge::{rig_on, texts, turn, Rig};

const T: Thresholds = Thresholds {
    act: 0.9,
    confirm: 0.6,
};

const WORK: [&str; 6] = [
    "complete",
    "progressing",
    "blocked_needs_human",
    "thrashing",
    "off_task",
    "other",
];

/// A task's end, a day and more before the learning run.
const END: u64 = 1_800_000_000_000;
/// The last learning run: the 24 hours after `END` are read.
const SETTLED: u64 = END + FALSE_COMPLETION_MS + 60_000;

fn rig() -> Rig {
    rig_on(texts(1), None, |c| {
        c.judge.enabled = true;
        c.judge.api_base = "http://127.0.0.1:9".into();
        for (p, _) in crate::judge::WIRED {
            c.judge.packs.insert((*p).into(), crate::tests_judge::off());
        }
    })
}

fn answers(work: &str, conf: f64, unfinished: f64) -> Vec<AnswerRecord> {
    let rest = (1.0 - conf) / (WORK.len() as f64 - 1.0);
    let work = Answer::Choice {
        choice: work.into(),
        probabilities: WORK
            .iter()
            .map(|o| ((*o).to_string(), if *o == work { conf } else { rest }))
            .collect(),
        confidence: conf,
    };
    let unfinished = Answer::Noul { noul: unfinished };
    [("work_state", work), ("announced_unfinished", unfinished)]
        .into_iter()
        .map(|(q, a)| AnswerRecord {
            question: q.into(),
            def: q.into(),
            about: None,
            band: band(&a, T),
            answer: a,
        })
        .collect()
}

/// A judgment of `pack` in `session`, recorded in `arm`, costing `cost`.
fn judgment(id: &str, pack: &str, session: &str, arm: &str, work: &str, cost: u64) -> Judgment {
    let (conf, unfinished) = (0.95, 0.05);
    serde_json::from_value(json!({
        "id": id, "pack": pack, "version": 1, "pack_sha256": "00",
        "point": if pack.starts_with("loop") { "loop_end" } else { "gate" },
        "mode": "shadow", "model": "jev-1.13.0", "answered_by": "jev-1.13.0",
        "model_drift": false,
        "state": {"sha256": "00", "bytes": 10, "tokens": 3, "cap_tokens": 4000,
            "builder": "loop", "builder_version": 1, "truncated": [], "dropped": []},
        "questions": 2, "answers": answers(work, conf, unfinished), "call": null,
        "timing": {"queued_ms": 0, "http_ms": 9, "total_ms": 9},
        "usage": null, "cost_micros": cost, "reserve_micros": cost,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": {"session": session, "turn": format!("trn_{id}"), "decision": "no_tool_calls",
            "class": "task", "pack_arm": arm},
    }))
    .unwrap()
}

fn ledger(kind: LedgerKind, session: Option<&str>, data: Value, at_ms: u64) -> NewRecord {
    let mut row = LedgerRow::new(kind, session, None, data);
    row.at_unix_ms = at_ms;
    NewRecord::json(kinds::LEDGER, None, &row).unwrap()
}

fn call_row(j: &Judgment) -> NewRecord {
    let f = crate::fact::judge::JudgeCall {
        judgment: j,
        budget: "shadow",
    };
    let s = j.context["session"].as_str();
    let mut r = ledger(
        LedgerKind::JudgeCall,
        s,
        crate::fact::Fact::row(&f),
        END - 60_000,
    );
    r.key = Some(j.id.clone());
    r.scoped(&crate::rpc::judge::scope_of(&j.pack))
}

fn label_row(id: &str, j: &str, label: Value, source: &str, weight: f64) -> NewRecord {
    let mut r = ledger(
        LedgerKind::JudgeLabel,
        None,
        json!({"id": id, "judgment": j, "pack": "loop.v1", "question": "work_state",
            "label": label, "source": source, "who": "test", "via": "cli", "weight": weight,
            "note": ""}),
        END,
    );
    r.key = Some(id.into());
    r.scoped("judge:loop")
}

/// One invented task: its execution (a copy of `like`'s, made a task),
/// its turns' rows, and its `task.ended` row in the parent's session.
struct Task<'a> {
    name: &'a str,
    state: ExecState,
    spent: u64,
    turns: u32,
    ended_at: u64,
}

fn seed(c: &Core, like: &Execution, t: &Task<'_>) -> NewRecord {
    let mut e = like.clone();
    e.id = format!("exe_{}", t.name);
    e.session_id = format!("ses_{}", t.name);
    e.kind = theseus_protocol::SessionKind::Task;
    e.parent = Some(like.id.clone());
    e.state = t.state;
    e.budget.spent_micros = t.spent;
    let mut recs = vec![NewRecord::json(kinds::EXECUTION, Some(&e.id), &e)
        .unwrap()
        .scoped(&e.session_id)];
    for i in 0..t.turns {
        let mut row = LedgerRow::new(
            LedgerKind::TurnEnded,
            Some(&e.session_id),
            Some(&format!("trn_{}_{i}", t.name)),
            json!({"loops": 1, "tool_calls": 0, "stop_reason": "no_tool_calls"}),
        );
        row.at_unix_ms = t.ended_at - 1_000;
        recs.push(NewRecord::json(kinds::LEDGER, None, &row).unwrap());
    }
    c.store.append(&recs).unwrap();
    ledger(
        LedgerKind::TaskEnded,
        Some(&like.session_id),
        json!({"execution_id": like.id, "task": e.id, "state": t.state}),
        t.ended_at,
    )
}

fn task(name: &str, state: ExecState, spent: u64, turns: u32) -> Task<'_> {
    Task {
        name,
        state,
        spent,
        turns,
        ended_at: END,
    }
}

/// The seeded judgments and their labels, each a case.
fn judged_rows() -> Vec<NewRecord> {
    let j = judgment;
    let mut calls = vec![
        // a: canary, called complete, no label: success once read.
        j("jdg_a1", "loop.v1", "ses_a", "canary", "complete", 40),
        // Another pack's judgment of the same session: its cost counts.
        j("jdg_a2", "security.v1", "ses_a", "canary", "complete", 25),
        // b: control, the system's near-identical rule: a false completion.
        j("jdg_b1", "loop.v1", "ses_b", "control", "complete", 40),
        // c: canary, Jev said progressing (continue), then complete.
        j("jdg_c1", "loop.v1", "ses_c", "canary", "progressing", 40),
        j("jdg_c2", "loop.v1", "ses_c", "canary", "complete", 40),
        // d: control, the execution failed.
        j("jdg_d1", "loop.v1", "ses_d", "control", "complete", 40),
        // f: cancelled, judged.
        j("jdg_f1", "loop.v1", "ses_f", "canary", "complete", 40),
        // g: canary; a candidate version's judgment in the other arm says
        // nothing of this canary.
        j("jdg_g1", "loop.v1", "ses_g", "canary", "complete", 40),
        j("jdg_g2", "loop.v2", "ses_g", "control", "complete", 40),
        // h: judged outside a canary only.
        j("jdg_h1", "loop.v1", "ses_h", "all", "complete", 40),
        // i: both arms.
        j("jdg_i1", "loop.v1", "ses_i", "canary", "complete", 40),
        j("jdg_i2", "loop.v1", "ses_i", "control", "complete", 40),
        // k: control, its 24 hours not yet read: no outcome.
        j("jdg_k1", "loop.v1", "ses_k", "control", "complete", 40),
    ];
    calls[3].answers = answers("progressing", 0.95, 0.05);
    let mut recs: Vec<NewRecord> = calls.iter().map(call_row).collect();
    recs.extend([
        label_row(
            &system_key("jdg_b1", "work_state", "false_completion"),
            "jdg_b1",
            json!({"not": "complete"}),
            "system",
            0.5,
        ),
        label_row("lbl_c1", "jdg_c1", json!("progressing"), "operator", 1.0),
        label_row("lbl_c2", "jdg_c2", json!("complete"), "operator", 1.0),
        // An audit says c2 was not done, but the operator's weighs more.
        label_row("lbl_c3", "jdg_c2", json!({"not": "complete"}), "audit", 0.5),
    ]);
    recs
}

/// The seeded store's records, exact.
fn want() -> Vec<TaskRecord> {
    let stop = |should| Stop {
        decision: StopDecision::Stop,
        should_stop: should,
    };
    let rec = |task: &str, arm, success, spend, judge, turns, fc, stops| TaskRecord {
        task: format!("exe_{task}"),
        arm,
        success,
        spend_micros: spend,
        judge_micros: judge,
        turns,
        nudges: 0,
        unnecessary_nudges: 0,
        false_completion: fc,
        stops,
    };
    vec![
        rec(
            "a",
            ArmName::Canary,
            Some(true),
            300_065,
            65,
            2,
            None,
            vec![stop(None)],
        ),
        rec(
            "b",
            ArmName::Control,
            Some(false),
            200_040,
            40,
            1,
            Some(true),
            vec![stop(Some(false))],
        ),
        rec(
            "c",
            ArmName::Canary,
            Some(true),
            500_080,
            80,
            3,
            Some(false),
            vec![
                Stop {
                    decision: StopDecision::Continue,
                    should_stop: Some(false),
                },
                stop(Some(true)),
            ],
        ),
        rec(
            "d",
            ArmName::Control,
            Some(false),
            100_040,
            40,
            1,
            None,
            vec![stop(None)],
        ),
        rec(
            "g",
            ArmName::Canary,
            Some(true),
            150_080,
            80,
            1,
            None,
            vec![stop(None)],
        ),
        // k ended later than the rest: last.
        rec(
            "k",
            ArmName::Control,
            None,
            100_040,
            40,
            1,
            None,
            vec![stop(None)],
        ),
    ]
}

/// The seeded store: ten tasks, each a case.
async fn seeded() -> (Rig, Vec<TaskRecord>) {
    let r = rig();
    let c = &r.core;
    let res = turn(c, None, "open the parent").await;
    let like = c
        .kernel
        .execution(res.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    c.store.append(&judged_rows()).unwrap();
    let mut ends = vec![];
    for t in [
        task("a", ExecState::Complete, 300_000, 2),
        task("b", ExecState::Complete, 200_000, 1),
        task("c", ExecState::Complete, 500_000, 3),
        task("d", ExecState::Failed, 100_000, 1),
        task("e", ExecState::Complete, 100_000, 1),
        task("f", ExecState::Cancelled, 100_000, 1),
        task("g", ExecState::Complete, 150_000, 1),
        task("h", ExecState::Complete, 100_000, 1),
        task("i", ExecState::Complete, 100_000, 1),
        Task {
            ended_at: SETTLED - 3_600_000,
            ..task("k", ExecState::Complete, 100_000, 1)
        },
    ] {
        ends.push(seed(c, &like, &t));
    }
    // A second row naming a task counts once.
    ends.push(ledger(
        LedgerKind::TaskEnded,
        Some(&like.session_id),
        json!({"task": "exe_a", "state": "complete"}),
        END + 1,
    ));
    c.store.append(&ends).unwrap();
    c.store
        .put_meta(
            LAST_RUN,
            &json!({"at_unix_ms": SETTLED, "date": "2027-01-16"}),
        )
        .unwrap();
    (r, want())
}

/// The store's rows give exact records, every field: both arms, the judge's
/// spend (every pack's) inside the whole, the labels resolved heaviest
/// first, an operator's `complete` over an audit, an execution that failed
/// or ran out of budget, and each task left out counted by its reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ledger_gives_exact_records() {
    let (r, want) = seeded().await;
    let c = &r.core;
    let (input, unreadable) = c.prove_input(None, None).unwrap();
    assert_eq!(unreadable, 0);
    let built = build(&input);
    assert_eq!(built.records, want);
    assert_eq!(
        built.left_out,
        [
            ("both_arms".to_string(), 1),
            ("cancelled".to_string(), 1),
            ("never_judged".to_string(), 1),
            ("no_arm".to_string(), 1),
        ]
        .into()
    );
}

/// The records as JSON lines (`learning::prove::jsonl`) are what the
/// generator's binary reads: parsed back, they are the same records, and
/// their report is the same, byte for byte.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_records_file_gives_the_same_report() {
    let (r, want) = seeded().await;
    let (input, _) = r.core.prove_input(None, None).unwrap();
    let built = build(&input);
    let text = crate::learning::prove::jsonl(&built.records);
    assert_eq!(text.lines().count(), want.len());
    let parsed = parse_records(&text).unwrap();
    assert_eq!(parsed, built.records);
    let min = ProveMinimum::default();
    assert_eq!(
        markdown(&prove(&parsed, min)),
        markdown(&prove(&built.records, min))
    );
}

/// An open window: a task whose 24 hours no learning run has read yet has
/// no outcome (`null`), unless it already failed; the same task once a run
/// has read them is a success.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_window_is_no_outcome_yet() {
    let (r, _) = seeded().await;
    let c = &r.core;
    let (mut input, _) = c.prove_input(None, None).unwrap();
    input.settled_ms = Some(END + FALSE_COMPLETION_MS - 1);
    let built = build(&input);
    let of = |t: &str| {
        built
            .records
            .iter()
            .find(|r| r.task == format!("exe_{t}"))
            .unwrap()
            .success
    };
    assert_eq!(of("a"), None, "a's 24 hours are open");
    assert_eq!(
        of("c"),
        None,
        "an operator's complete waits for the window too"
    );
    assert_eq!(
        of("b"),
        Some(false),
        "a near-identical task already says not done"
    );
    assert_eq!(of("d"), Some(false), "a failed execution needs no window");
    input.settled_ms = None;
    assert_eq!(
        build(&input).records[0].success,
        None,
        "no learning run has read anything"
    );
}

/// The method answers the generator's report over the records it gives,
/// byte for byte as `theseus-judge prove` reads them from the JSON lines it
/// answers; counts the arms and the tasks left out; writes no frame; and
/// says "insufficient" with its counts below the minimum.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_method_is_the_generator_over_its_records() {
    let (r, want) = seeded().await;
    let c = &r.core;
    let before = c.store.last_position();
    let v = c.judge_prove(json!({"records": true})).await.unwrap();
    assert_eq!(c.store.last_position(), before, "the method wrote a frame");
    let jsonl = v.records.clone().unwrap();
    let records = parse_records(&jsonl).unwrap();
    assert_eq!(records, want);
    let report = prove(&records, ProveMinimum::default());
    assert_eq!(
        v.markdown,
        markdown(&report),
        "the report differs from the file's"
    );
    assert_eq!(v.report, serde_json::to_value(&report).unwrap());
    assert_eq!(v.verdict, "insufficient");
    assert_eq!(
        v.arms,
        [("canary".to_string(), 3), ("control".to_string(), 3)].into()
    );
    assert_eq!(v.left_out.values().sum::<u32>(), 4);
    assert_eq!(v.tasks, 10);
    assert!(v.window.contains("has not moved to canary"), "{}", v.window);
    let reasons = report.verdict.reasons.join("\n");
    assert!(
        reasons.contains("canary: 3 tasks, 3 labeled, 3 successes"),
        "{reasons}"
    );
    assert!(
        reasons.contains("canary arm: labeled tasks: 3 of 30"),
        "{reasons}"
    );
    assert!(v.notes.iter().any(|n| n.contains("26b")));
    assert_eq!(v.classification.len(), 1);
    assert_eq!(v.classification[0].verdict, "insufficient");
    // Without `records`, none are answered; the minimum is the caller's.
    let low = c
        .judge_prove(json!({"min_tasks": 2, "min_labeled": 1}))
        .await
        .unwrap();
    assert_eq!(low.records, None);
    let JudgeProveResult { report, .. } = low;
    assert_eq!(report["minimum"]["tasks_per_arm"], 2);
    assert_ne!(report["verdict"]["kind"], "insufficient");
}

/// The default window begins at `loop.v1`'s latest move to canary: a task
/// that ended before it is not read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_window_is_the_canarys() {
    let (r, _) = seeded().await;
    let c = &r.core;
    let ladder = c.runner.judge.ladder();
    let row = theseus_protocol::packs::PackModeRow {
        pack: "loop.v1".into(),
        mode: "canary".into(),
        from: "shadow".into(),
        share: Some(0.5),
        who: "owner".into(),
        why: "the prove's test".into(),
        forced: true,
        ..Default::default()
    };
    // Writes keep the kind's clock moving forward: the move lands after the
    // seeded rows.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    ladder.write(row).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let v = c.judge_prove(Value::Null).await.unwrap();
    assert_eq!(v.tasks, 0, "{v:?}");
    assert!(v.window.contains("move to canary 0.5"), "{}", v.window);
    let like = c.kernel.execution("exe_a").unwrap().unwrap();
    let parent = like.parent.clone().unwrap();
    let like = c.kernel.execution(&parent).unwrap().unwrap();
    let end = seed(c, &like, &task("late", ExecState::Complete, 1, 1));
    c.store.append(&[end]).unwrap();
    let v = c.judge_prove(Value::Null).await.unwrap();
    assert_eq!(
        (v.tasks, v.left_out.get("never_judged")),
        (1, Some(&1)),
        "{v:?}"
    );
    // A declined move to canary moves nothing (theseus-u4t3).
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    ladder
        .write(theseus_protocol::packs::PackModeRow {
            pack: "loop.v1".into(),
            mode: "canary".into(),
            from: "canary".into(),
            share: Some(0.2),
            who: "system".into(),
            why: "the prove's test: a move the bar declined".into(),
            declined: true,
            ..Default::default()
        })
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let after = c.judge_prove(Value::Null).await.unwrap();
    assert_eq!(
        (after.since_ms, after.window.as_str(), after.tasks),
        (v.since_ms, v.window.as_str(), 1),
        "{after:?}"
    );
    // A day given is the window, whatever the ladder says.
    let all = c.judge_prove(json!({"since": "2020-01-01"})).await.unwrap();
    assert_eq!(all.tasks, 11);
}

/// A learned version of loop.v1, `loop.v101`, in its lineage: its
/// `pack.version` row, read back by the next read of the lineage.
fn learned_loop(c: &Core) {
    let text = include_str!("../../theseus-judge/packs/loop.v1.toml").replacen(
        "version = 1",
        "version = 101",
        1,
    );
    let pack = theseus_judge::Pack::parse(&text).unwrap();
    assert_eq!(pack.name(), "loop.v101");
    let l = crate::judge::lineage::Learned {
        pack: std::sync::Arc::new(pack),
        text,
        parent: "loop.v1".into(),
        root: "loop.v1".into(),
        proposal: "prp_heron".into(),
        at_ms: END,
    };
    let mut r = ledger(LedgerKind::PackVersion, None, l.data(), END - 120_000);
    r.key = Some(crate::judge::lineage::key("loop.v101"));
    c.store
        .append(&[r.scoped(&crate::judge::lineage::scope("loop.v1"))])
        .unwrap();
    c.runner.judge.lineage().forget();
}

/// A task judged by a learned version standing in loop.v1's place is left
/// out as `learned_version`, never `never_judged` (theseus-ag0t); one judged
/// by both, its stops not all loop.v1's, is left out too; the others are as
/// before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_a_learned_version_judged_is_left_out_as_learned() {
    let (r, want) = seeded().await;
    let c = &r.core;
    learned_loop(c);
    // e, never judged before: judged by loop.v101 alone.
    c.store
        .append(&[call_row(&judgment(
            "jdg_e1",
            "loop.v101",
            "ses_e",
            "canary",
            "complete",
            40,
        ))])
        .unwrap();
    let (input, _) = c.prove_input(None, None).unwrap();
    assert_eq!(input.learned, ["loop.v101"]);
    let built = build(&input);
    assert_eq!(built.records, want, "the others as before");
    assert_eq!(
        built.left_out,
        [
            ("both_arms".to_string(), 1),
            ("cancelled".to_string(), 1),
            ("learned_version".to_string(), 1),
            ("no_arm".to_string(), 1),
        ]
        .into()
    );
    // g, judged by loop.v1 in its canary: a learned judgment too leaves it
    // out.
    c.store
        .append(&[call_row(&judgment(
            "jdg_g3",
            "loop.v101",
            "ses_g",
            "canary",
            "complete",
            40,
        ))])
        .unwrap();
    let (input, _) = c.prove_input(None, None).unwrap();
    let built = build(&input);
    let left: Vec<TaskRecord> = want.into_iter().filter(|t| t.task != "exe_g").collect();
    assert_eq!(built.records, left);
    assert_eq!(built.left_out.get("learned_version"), Some(&2));
    let v = c.judge_prove(Value::Null).await.unwrap();
    assert_eq!(v.left_out.get("learned_version"), Some(&2), "{v:?}");
}

/// The window's line names a learned loop version placed inside the
/// window, by name and day; with none, or one placed before it, or a
/// declined move, the line is as before (theseus-ag0t).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_window_names_a_learned_version_placed_inside_it() {
    let (r, _) = seeded().await;
    let c = &r.core;
    learned_loop(c);
    let plain = "every task that ended: loop.v1 has not moved to canary";
    assert_eq!(c.judge_prove(Value::Null).await.unwrap().window, plain);
    let ladder = c.runner.judge.ladder();
    let row = |pack: &str, mode: &str, declined: bool| theseus_protocol::packs::PackModeRow {
        pack: pack.into(),
        mode: mode.into(),
        from: "shadow".into(),
        share: (mode == "canary").then_some(0.5),
        who: "owner".into(),
        why: "the prove's test".into(),
        forced: true,
        declined,
        ..Default::default()
    };
    // A declined move places nothing.
    ladder.write(row("loop.v101", "live", true)).unwrap();
    assert_eq!(c.judge_prove(Value::Null).await.unwrap().window, plain);
    let placed = ladder.write(row("loop.v101", "shadow", false)).unwrap();
    let today = crate::judge::spend::local_day(placed.at_unix_ms);
    let v = c.judge_prove(Value::Null).await.unwrap();
    assert_eq!(
        v.window,
        format!(
            "{plain}; a learned version stood in loop.v1's place: loop.v101 moved to shadow \
             on {today}"
        )
    );
    ladder.write(row("loop.v101", "canary", false)).unwrap();
    let v = c.judge_prove(Value::Null).await.unwrap();
    assert!(
        v.window.ends_with(&format!(
            "loop.v101 moved to canary 0.5 on {today}, the latest of 2 placements"
        )),
        "{}",
        v.window
    );
    // loop.v1's own canary, after both: the window opens there, and they
    // are before it.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    ladder.write(row("loop.v1", "canary", false)).unwrap();
    let v = c.judge_prove(Value::Null).await.unwrap();
    assert_eq!(
        v.window,
        format!("tasks that ended since loop.v1's move to canary 0.5 on {today}")
    );
}

// ------------------------------------------------------------ cohorts

fn seen(id: &str, session: &str, arm: &str, work: &str, position: u64) -> Seen {
    Seen {
        position,
        at_ms: position,
        judgment: judgment(id, "loop.v1", session, arm, work, 1_000),
    }
}

/// `n` tasks in `arm`, `wins` of them successes (the rest the system's
/// near-identical rule), each spending `spend` micros, the judge's 1,000
/// among them.
fn cohort(input: &mut Input, arm: &str, n: u32, wins: u32, spend: u64) {
    for i in 0..n {
        let name = format!("{arm}{i:02}");
        let session = format!("ses_{name}");
        let id = format!("jdg_{name}");
        input
            .loop_scope
            .judgments
            .entry("loop.v1".into())
            .or_default()
            .push(seen(&id, &session, arm, "complete", u64::from(i)));
        if i >= wins {
            input.loop_scope.labels.insert(
                id.clone(),
                vec![LabelRow {
                    position: 1,
                    at_ms: 1,
                    id: system_key(&id, "work_state", "false_completion"),
                    judgment: id.clone(),
                    pack: "loop.v1".into(),
                    question: Some("work_state".into()),
                    about: None,
                    label: json!({"not": "complete"}),
                    source: "system".into(),
                    weight: 0.5,
                }],
            );
        }
        input.tasks.push(Ended {
            task: name,
            session: session.clone(),
            state: ExecState::Complete,
            ended_at_ms: END,
            spent_micros: spend - 1_000,
        });
        input.judge_spend.insert(
            session.clone(),
            vec![JudgeSpend {
                micros: 1_000,
                outside: true,
            }],
        );
        input.turns.insert(session, 4);
    }
}

/// Cohorts with known outcomes give the generator's exact rates: per task,
/// successes over labeled tasks; per dollar, successes over the labeled
/// tasks' whole spend, the judge's included.
#[test]
fn known_cohorts_give_exact_rates() {
    let mut input = Input {
        settled_ms: Some(SETTLED),
        ..Input::default()
    };
    cohort(&mut input, "canary", 40, 36, 400_000);
    cohort(&mut input, "control", 40, 24, 500_000);
    let built = build(&input);
    assert_eq!(built.records.len(), 80);
    assert!(built.left_out.is_empty());
    let r = prove(&built.records, ProveMinimum::default());
    let rate = |e: &theseus_judge::prove::Estimate| e.value.unwrap();
    assert!((rate(&r.canary.completion) - 0.9).abs() < 1e-12);
    assert!((rate(&r.control.completion) - 0.6).abs() < 1e-12);
    // 36 successes over $16.00; 24 over $20.00.
    assert!((rate(&r.canary.completions_per_usd) - 2.25).abs() < 1e-12);
    assert!((rate(&r.control.completions_per_usd) - 1.2).abs() < 1e-12);
    assert_eq!(r.canary.spend_micros, 16_000_000);
    assert_eq!(r.canary.judge_micros, 40_000);
    // Only the false completions carry a label here: 4 and 16, short of the
    // minimum, so no rate is stated.
    assert_eq!(r.canary.false_completion.n, 4);
    assert_eq!(r.control.false_completion.n, 16);
    assert!(r.control.false_completion.insufficient.is_some());
    assert_eq!(r.verdict.kind, VerdictKind::CanaryBetter);
    // Below the minimum in one arm: insufficient, with its counts.
    let mut few = Input {
        settled_ms: Some(SETTLED),
        ..Input::default()
    };
    cohort(&mut few, "canary", 40, 36, 400_000);
    cohort(&mut few, "control", 12, 8, 500_000);
    let r = prove(&build(&few).records, ProveMinimum::default());
    assert_eq!(r.verdict.kind, VerdictKind::Insufficient);
    assert!(
        r.verdict
            .reasons
            .iter()
            .any(|l| l == "control arm: labeled tasks: 12 of 30"),
        "{:?}",
        r.verdict.reasons
    );
}

/// The judge's spend is in each record's whole: a record without it would
/// compare the arms at unequal budgets.
#[test]
fn judge_spend_counts_in_the_whole() {
    let mut input = Input {
        settled_ms: Some(SETTLED),
        ..Input::default()
    };
    cohort(&mut input, "canary", 1, 1, 10_000);
    let r = &build(&input).records[0];
    assert_eq!((r.spend_micros, r.judge_micros), (10_000, 1_000));
    // A judgment the execution paid is in its spend already.
    input.judge_spend.insert(
        "ses_canary00".into(),
        vec![JudgeSpend {
            micros: 1_000,
            outside: false,
        }],
    );
    let r = &build(&input).records[0];
    assert_eq!((r.spend_micros, r.judge_micros), (9_000, 1_000));
}

/// The read's time on a store of 10,000 finished tasks, each with a turn,
/// a judgment, and its end: printed, not held (the owner's machine is the
/// budgets'). `cargo nextest run -E 'test(ten_thousand)' --run-ignored all
/// --no-capture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a timing on a large store, run by hand"]
async fn ten_thousand_tasks_read_in_time() {
    let r = rig();
    let c = &r.core;
    let res = turn(c, None, "open the parent").await;
    let like = c
        .kernel
        .execution(res.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    let mut recs = Vec::new();
    for i in 0..10_000 {
        let name = format!("n{i:05}");
        let arm = if i % 2 == 0 { "canary" } else { "control" };
        recs.push(call_row(&judgment(
            &format!("jdg_{name}"),
            "loop.v1",
            &format!("ses_{name}"),
            arm,
            "complete",
            40,
        )));
        let t = task(&name, ExecState::Complete, 100_000, 1);
        recs.push(seed(c, &like, &t));
        if recs.len() >= 400 {
            c.store.append(&recs).unwrap();
            recs.clear();
        }
    }
    c.store.append(&recs).unwrap();
    let began = std::time::Instant::now();
    let v = c.judge_prove(Value::Null).await.unwrap();
    let took = began.elapsed();
    assert_eq!(v.arms["canary"] + v.arms["control"], 10_000);
    println!(
        "judge.prove over 10,000 tasks: {} ms (its own count: {} ms)",
        took.as_millis(),
        v.elapsed_ms
    );
}

// ------------------------------------------------------- classification

fn promote(id: &str, p: f64, at_ms: u64) -> Seen {
    let a = Answer::Noul { noul: p };
    let mut j = judgment(id, "classify.v1", "ses_talk", "all", "complete", 30);
    j.answers = vec![AnswerRecord {
        question: "should_promote".into(),
        def: "should_promote".into(),
        about: None,
        band: band(&a, T),
        answer: a,
    }];
    Seen {
        position: at_ms,
        at_ms,
        judgment: j,
    }
}

fn label(id: &str, j: &str, value: bool, source: &str, weight: f64, position: u64) -> LabelRow {
    LabelRow {
        position,
        at_ms: position,
        id: id.into(),
        judgment: j.into(),
        pack: "classify.v1".into(),
        question: Some("should_promote".into()),
        about: None,
        label: json!(value),
        source: source.into(),
        weight,
    }
}

/// One `classify.v1` judgment with its labels: the system's `task_create`
/// (`called`) and an operator's or an audit's truth.
fn add(
    scope: &mut crate::learning::Scope,
    i: u32,
    lean: f64,
    called: Option<bool>,
    truth: Option<(bool, &str)>,
) {
    let id = format!("jdg_m{i:02}");
    scope
        .judgments
        .entry("classify.v1".into())
        .or_default()
        .push(promote(&id, lean, 1_000 + u64::from(i)));
    let mut ls = vec![];
    if let Some(c) = called {
        let key = system_key(&id, "should_promote", "task_create");
        ls.push(label(&key, &id, c, "system", 0.5, 1));
    }
    if let Some((t, source)) = truth {
        let w = if source == "operator" { 1.0 } else { 0.5 };
        ls.push(label(&format!("lbl_m{i}"), &id, t, source, w, 2));
    }
    scope.labels.insert(id, ls);
}

/// Classification's decision quality: Jev's lean against the model's own
/// `task.create` (the system's label, by its key), on the messages an
/// operator or an audit labeled; the system's label is never the truth, a
/// judgment the system has not labeled is counted and not compared, the
/// window holds, and the verdict is McNemar's on the pairs that disagree.
#[test]
fn classification_is_jev_against_the_models_own_task_create() {
    use crate::learning::prove::classify_quality;
    let mut scope = crate::learning::Scope::default();
    // 30 compared: both right on 20; Jev alone right on 8; the baseline
    // alone on 2.
    for i in 0..20 {
        add(&mut scope, i, 0.95, Some(true), Some((true, "operator")));
    }
    for i in 20..28 {
        add(&mut scope, i, 0.05, Some(true), Some((false, "audit")));
    }
    for i in 28..30 {
        add(&mut scope, i, 0.95, Some(false), Some((false, "operator")));
    }
    // Labeled, its turn not yet labeled by the system: counted only.
    add(&mut scope, 30, 0.95, None, Some((true, "operator")));
    // The system's label alone: not a truth.
    add(&mut scope, 31, 0.95, Some(false), None);
    // Outside the window.
    add(&mut scope, 40, 0.95, Some(false), Some((true, "operator")));
    let q = classify_quality(&scope, Some(1_000), Some(1_031), 30);
    assert_eq!(
        (q.labeled, q.compared, q.jev_right, q.baseline_right),
        (31, 30, 28, 22)
    );
    assert_eq!(q.jev_rate, Some(28.0 / 30.0));
    assert_eq!(q.baseline_rate, Some(22.0 / 30.0));
    // (8 - 2) / sqrt(10) = 1.90: not past chance.
    assert_eq!(q.verdict, "no_difference");
    let q = classify_quality(&scope, Some(1_000), Some(1_031), 31);
    assert_eq!(q.verdict, "insufficient");
    assert_eq!(q.insufficient.as_deref(), Some("compared 30 of 31"));
    assert_eq!(q.jev_rate, None);
    // One more pair Jev alone gets right: (9 - 2) / sqrt(11) = 2.11.
    let q = {
        add(&mut scope, 41, 0.05, Some(true), Some((false, "operator")));
        classify_quality(&scope, None, None, 30)
    };
    assert_eq!((q.compared, q.verdict.as_str()), (32, "jev_better"));
}
