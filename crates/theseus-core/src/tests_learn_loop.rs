//! The learning loop (M5 25f; design §2.17), on seeded stores against the
//! fake Jev and a scripted writer: nine new train errors propose nothing
//! and ten propose once, a second run with none new proposes nothing, and a
//! holdout label's note never reaches the writer; a writer's file that moves
//! the builder is refused; a candidate worse on one holdout class is held,
//! one better everywhere below the minimum replaces a shadow parent in
//! shadow and goes to a live parent's canary; the writer's day budget stops
//! a run; the version is a row and a file the rows rebuild, it stands at its
//! root's point, and a rollback gives the place back.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_judge::judge::AnswerRecord;
use theseus_judge::Thresholds;
use theseus_protocol::judge_runs::{JudgeLearnParams, JudgeProposal};
use theseus_protocol::packs::{PackPromoteParams, PackRollbackParams};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::ledger::LedgerRow;
use crate::provider::Scripted;
use crate::rpc::Core;
use crate::tests_judge::{rig_on, Rig};
use crate::tests_replay::{call_row, judgment, loop_state, reworded, rows};

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

/// The note a holdout label carries: it must never reach the writer.
const HOLDOUT_NOTE: &str = "a holdout note about the lighthouse keeper";

fn choice(top: &str, confidence: f64) -> AnswerRecord {
    let rest = (1.0 - confidence) / 5.0;
    let answer = Answer::Choice {
        choice: top.into(),
        probabilities: WORK
            .iter()
            .map(|o| ((*o).to_string(), if *o == top { confidence } else { rest }))
            .collect(),
        confidence,
    };
    AnswerRecord {
        question: "work_state".into(),
        def: "work_state".into(),
        about: None,
        band: band(&answer, T),
        answer,
    }
}

/// An owner's label with a note.
fn label(id: &str, j: &str, value: &str, note: &str) -> NewRecord {
    let mut row = LedgerRow::new(
        LedgerKind::JudgeLabel,
        None,
        None,
        json!({"id": id, "judgment": j, "pack": "loop.v1", "question": "work_state",
            "label": value, "source": "operator", "who": "owner", "via": "cli",
            "weight": 1.0, "note": note}),
    );
    row.at_unix_ms = 1;
    let mut r = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    r.key = Some(id.to_string());
    r.scoped("judge:loop")
}

/// One `loop.v1` judgment of `name`, answered `top`, labeled `truth` (with
/// `note`), at `at_ms`.
fn seed_one(c: &Core, name: &str, top: &str, truth: &str, note: &str, at_ms: u64) {
    let (state, blob) = loop_state(c, &format!("Paint the {name} boat by the pier."));
    let j = judgment(
        &format!("jdg_{name}"),
        "loop.v1",
        &state,
        vec![choice(top, 0.95)],
        json!({"blob": blob, "session": "ses_harbor", "turn": format!("trn_{name}"),
            "class": "reply", "decision": "no_tool_calls"}),
    );
    c.store
        .append(&[
            call_row(&j, at_ms),
            label(&format!("lbl_{name}"), &j.id, truth, note),
        ])
        .unwrap();
}

/// `n` train errors (heron0…: complete, labeled progressing) before
/// `split`, and two holdout judgments after it: wren (complete, labeled
/// progressing, with [`HOLDOUT_NOTE`]) and gull (complete, labeled so).
fn seed(c: &Core, n: usize, from: usize, split: u64) {
    for i in from..from + n {
        let at = split - 100_000 + i as u64;
        seed_one(
            c,
            &format!("heron{i}"),
            "complete",
            "progressing",
            "it had more to do",
            at,
        );
    }
    if from == 0 {
        seed_one(
            c,
            "wren",
            "complete",
            "progressing",
            HOLDOUT_NOTE,
            split + 10,
        );
        seed_one(c, "gull", "complete", "complete", "", split + 20);
    }
}

/// The fake answers work_state under the rewritten criterion (`Plainly`):
/// herons and wren progressing (fixed); gull complete, or progressing when
/// `gull_breaks` (a holdout class worse).
fn fake(gull_breaks: bool) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    let pick = |option: &str| Jev::Choice {
        option: option.into(),
        confidence: 0.95,
    };
    jev.script_when("heron", Some("Plainly"), "work_state", pick("progressing"));
    jev.script_when("wren", Some("Plainly"), "work_state", pick("progressing"));
    let gull = if gull_breaks {
        "progressing"
    } else {
        "complete"
    };
    jev.script_when("gull", Some("Plainly"), "work_state", pick(gull));
    jev.script("work_state", pick("complete"));
    jev
}

/// The writer's reply: loop.v1 with its `progressing` option reworded.
fn writer_reply() -> Scripted {
    Scripted::text(&format!(
        "Here is the next version.\n```toml\n{}\n```\n",
        reworded()
    ))
}

/// A core with the judge on against `jev` and the writer scripted; every
/// wired pack but loop.v1 off (the tests write loop.v1's judgments).
fn rig(script: Vec<Scripted>, jev: &FakeJev, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    rig_on(script, Some(jev), |c| {
        for (p, _) in crate::judge::WIRED {
            if *p != "loop.v1" {
                c.judge.packs.insert((*p).into(), crate::tests_judge::off());
            }
        }
        c.judge.total_secs = 5;
        tweak(c);
    })
}

async fn learn(c: &Arc<Core>, split: u64) -> JudgeProposal {
    c.judge_learn(
        JudgeLearnParams {
            pack: "loop.v1".into(),
            split: Some(split.to_string()),
        },
        "cli",
    )
    .await
    .unwrap()
}

fn writer_text(r: &Rig) -> Vec<String> {
    r.fake
        .requests()
        .iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .collect()
}

fn split_now() -> u64 {
    theseus_protocol::now_unix_ms() - 60_000
}

/// Nine new train errors propose nothing (and ask no writer); ten propose
/// once, reading the ten train errors and nothing of the holdout; a second
/// run with no new error proposes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ten_train_errors_propose_once_and_the_holdout_never_reaches_the_writer() {
    let jev = fake(false);
    let r = rig(vec![writer_reply(); 3], &jev, |_| {});
    let c = &r.core;
    let split = split_now();
    seed(c, 9, 0, split);
    let p = learn(c, split).await;
    assert_eq!(
        (p.decision.as_str(), p.new_errors),
        ("none", 9),
        "{}",
        p.why
    );
    assert!(r.fake.requests().is_empty(), "nine errors ask no writer");
    assert!(rows(c, "judge.learn:loop", LedgerKind::JudgeProposal).is_empty());
    seed(c, 1, 9, split);
    let p = learn(c, split).await;
    assert_eq!((p.new_errors, p.errors.len()), (10, 10));
    assert_eq!((p.train, p.holdout), (10, 2));
    assert_eq!(p.version.as_deref(), Some("loop.v101"), "{p:?}");
    assert_eq!(p.split, "time");
    let sent = writer_text(&r);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("heron9") && sent[0].contains("it had more to do"));
    assert!(
        !sent[0].contains("lighthouse keeper"),
        "a holdout note reached the writer"
    );
    assert!(
        !sent[0].contains("wren"),
        "a holdout state reached the writer"
    );
    let sys = serde_json::to_string(&r.fake.requests()[0].system).unwrap();
    assert!(sys.contains("literally") && sys.contains("math"));
    // The replay asked the candidate on both splits' stored states.
    assert_eq!(jev.seen().len(), 12);
    assert_eq!(p.fixed, 10);
    let props = rows(c, "judge.learn:loop", LedgerKind::JudgeProposal);
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].data["errors"].as_array().unwrap().len(), 10);
    // Nothing new: nothing proposed, no writer asked.
    let again = learn(c, split).await;
    assert_eq!((again.decision.as_str(), again.new_errors), ("none", 0));
    assert_eq!(r.fake.requests().len(), 1);
}

/// Better on train, and better on the holdout with no class worse, below
/// the minimum: a shadow parent's place in shadow. The version is a row and
/// a file; it stands at loop.v1's point; the rows rebuild the file; a
/// rollback gives the place back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_better_candidate_replaces_a_shadow_parent_and_a_rollback_restores_it() {
    let jev = fake(false);
    // As `theseusd --state-dir` starts it: the store elsewhere than the
    // config's `[server] state_dir`, which the files must not follow.
    let elsewhere =
        std::env::temp_dir().join(format!("learn-loop-elsewhere-{}", std::process::id()));
    let cfg_dir = elsewhere.to_string_lossy().into_owned();
    let r = rig(vec![writer_reply()], &jev, move |c| {
        c.server.state_dir = cfg_dir
    });
    let c = &r.core;
    let split = split_now();
    seed(c, 10, 0, split);
    let p = learn(c, split).await;
    assert_eq!(p.decision, "shadow", "{}", p.why);
    assert!(!p.sufficient);
    assert!(
        p.said
            .starts_with("loop.v101 from 10 of your labels: holdout precision"),
        "{}",
        p.said
    );
    assert!(p.said.contains("replacing loop.v1 in shadow"), "{}", p.said);
    assert!(
        p.diff.contains("+ ") && p.diff.contains("Plainly"),
        "{}",
        p.diff
    );
    let wq = |side: &[theseus_protocol::judge_runs::LearnQuestion]| {
        side.iter()
            .find(|q| q.question == "work_state")
            .cloned()
            .unwrap()
    };
    assert_eq!(wq(&p.parent_holdout).precision, Some(0.5));
    assert_eq!(wq(&p.candidate_holdout).precision, Some(1.0));
    // The row and the file.
    let versions = rows(c, "judge.learn:loop", LedgerKind::PackVersion);
    assert_eq!(versions.len(), 1);
    let text = versions[0].data["text"].as_str().unwrap().to_string();
    assert!(text.contains("Plainly") && text.contains("version = 101"));
    let state = crate::judge::lineage::state_of(&c.store);
    let file = state.join("packs").join("loop.v101.toml");
    assert!(
        !elsewhere.join("packs").exists(),
        "the file followed [server] state_dir"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    // It stands in loop.v1's place: a turn's end dispatches it.
    let j = &c.runner.judge;
    assert_eq!(j.placed("loop.v1", "ses_harbor"), "loop.v101");
    let d = j
        .plan_loop_end("no_tool_calls", "trn_new", "ses_harbor")
        .unwrap();
    assert_eq!(d.pack, "loop.v101");
    // The rows rebuild the file, as a restart's warm does.
    std::fs::remove_dir_all(state.join("packs")).unwrap();
    j.lineage().forget();
    j.write_pack_files(&state);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    // The ladder's rollback gives the place back.
    c.pack_rollback(
        &PackRollbackParams {
            pack: "loop.v101".into(),
            why: None,
            off: false,
        },
        "cli",
    )
    .unwrap();
    assert_eq!(j.placed("loop.v1", "ses_harbor"), "loop.v1");
    let d = j
        .plan_loop_end("no_tool_calls", "trn_newer", "ses_harbor")
        .unwrap();
    assert_eq!(d.pack, "loop.v1");
}

/// Better on train but worse on one holdout class: held, with why, and it
/// stands nowhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_candidate_worse_on_one_holdout_class_is_held() {
    let jev = fake(true);
    let r = rig(vec![writer_reply()], &jev, |_| {});
    let c = &r.core;
    let split = split_now();
    seed(c, 10, 0, split);
    let p = learn(c, split).await;
    assert_eq!(p.decision, "held", "{}", p.why);
    assert!(p.why.contains("work_state: complete's"), "{}", p.why);
    assert_eq!(p.fixed, 10);
    assert_eq!(c.runner.judge.placed("loop.v1", "ses_harbor"), "loop.v1");
    // Its row is written: the owner may still place it.
    assert_eq!(
        rows(c, "judge.learn:loop", LedgerKind::PackVersion).len(),
        1
    );
}

/// A live parent's better candidate below the minimum goes to the canary,
/// through the ladder's act, citing the proposal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_parents_candidate_below_the_minimum_goes_to_the_canary() {
    let jev = fake(false);
    let r = rig(vec![writer_reply(); 2], &jev, |_| {});
    let c = &r.core;
    c.pack_promote(
        &PackPromoteParams {
            pack: "loop.v1".into(),
            to: "live".into(),
            ..PackPromoteParams::default()
        },
        "cli",
    )
    .unwrap();
    let split = split_now();
    seed(c, 10, 0, split);
    let p = learn(c, split).await;
    assert_eq!(p.decision, "canary", "{}", p.why);
    let row = c.runner.judge.ladder().rows_of("loop.v101").pop().unwrap();
    assert_eq!((row.mode.as_str(), row.share), ("canary", Some(0.2)));
    assert_eq!(row.report.as_deref(), Some(p.id.as_str()));
    assert!(!row.forced && row.who == "system", "{row:?}");
    assert_eq!(row.from, "off", "a learned version stood nowhere before");
    // While it runs, the lineage's next run is held: which version is the
    // parent depends on a session's arm.
    seed(c, 10, 10, split);
    let again = learn(c, split).await;
    assert_eq!(again.decision, "skipped", "{}", again.why);
    assert!(again.why.contains("in its canary"), "{}", again.why);
    assert_eq!(r.fake.requests().len(), 1, "no second writer request");
}

/// A writer's file that moves the builder is refused, and placed nowhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_writers_file_that_moves_the_builder_is_refused() {
    let jev = fake(false);
    let moved = reworded().replace("state = \"loop\"", "state = \"continue\"");
    let r = rig(vec![Scripted::text(&moved)], &jev, |_| {});
    let c = &r.core;
    let split = split_now();
    seed(c, 10, 0, split);
    let p = learn(c, split).await;
    assert_eq!(p.decision, "refused", "{}", p.why);
    assert!(p.why.contains("the builder"), "{}", p.why);
    assert!(jev.seen().is_empty(), "nothing replayed");
    assert!(rows(c, "judge.learn:loop", LedgerKind::PackVersion).is_empty());
}

/// The writer's day budget stops a run before anything is sent; its
/// errors stay new.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_writers_day_budget_stops_a_run() {
    let jev = fake(false);
    let r = rig(vec![writer_reply()], &jev, |c| {
        c.judge.learn.writer_limit_usd_per_day = 0.0001;
    });
    let c = &r.core;
    let split = split_now();
    seed(c, 10, 0, split);
    let p = learn(c, split).await;
    assert_eq!(p.decision, "skipped", "{}", p.why);
    assert!(p.why.contains("writer_limit_usd_per_day"), "{}", p.why);
    assert!(r.fake.requests().is_empty());
    let props = rows(c, "judge.learn:loop", LedgerKind::JudgeProposal);
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].data["errors"], Value::Array(vec![]));
}

/// The nightly rule: interleaved below 200 labeled in the window, and a
/// judgment's side is its id's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_nightly_run_uses_the_interleaved_split() {
    let jev = fake(false);
    let r = rig(vec![writer_reply()], &jev, |c| {
        c.judge.learn.min_errors = 1;
    });
    let c = r.core.clone();
    let now = theseus_protocol::now_unix_ms();
    for i in 0..15 {
        seed_one(
            &c,
            &format!("heron{i}"),
            "complete",
            "progressing",
            "",
            now - 5_000 + i,
        );
    }
    let rt = tokio::runtime::Handle::current();
    let props = tokio::task::spawn_blocking(move || c.learn_nightly(&rt, now))
        .await
        .unwrap();
    let p = props.iter().find(|p| p.root == "loop.v1").unwrap();
    assert_eq!(p.split, "interleaved");
    let holdout = (0..15)
        .filter(|i| theseus_judge::propose::interleaved_holdout(&format!("jdg_heron{i}")))
        .count() as u32;
    assert_eq!((p.train, p.holdout), (15 - holdout, holdout));
    assert!(p
        .error_judgments
        .iter()
        .all(|j| !theseus_judge::propose::interleaved_holdout(j)));
}

const SECURITY_V3: &str = include_str!("../../theseus-judge/packs/security.v3.toml");

/// One `security.v3` judgment of a `proc.run echo <name>`, answered risky at
/// `p`, labeled `risky: truth`.
fn seed_security(c: &Core, name: &str, p: f64, truth: bool, at_ms: u64) {
    let pack = theseus_judge::pack::by_name("security.v3").unwrap();
    let input: theseus_judge::builders::SecurityInput = serde_json::from_value(json!({
        "tool": "proc.run", "class": "exec", "posture": "open", "argv": ["echo", name],
    }))
    .unwrap();
    let prepared = theseus_judge::prepare(
        &pack,
        &theseus_judge::Input::Security2(input),
        &theseus_judge::NoScrub,
    )
    .unwrap();
    let blob = c.store.blobs().put(prepared.state.json.as_bytes()).unwrap();
    let answer = Answer::Noul { noul: p };
    let risky = AnswerRecord {
        question: "risky".into(),
        def: "risky".into(),
        about: None,
        band: band(&answer, T),
        answer,
    };
    let state = theseus_judge::judge::StateRecord::from(prepared.state.as_ref());
    let mut j = judgment(
        &format!("jdg_{name}"),
        "security.v3",
        &state,
        vec![risky],
        json!({"blob": blob, "session": "ses_harbor", "call": format!("act_{name}"),
            "class": "tools", "decision": "open"}),
    );
    j.version = 3;
    let mut row = LedgerRow::new(
        LedgerKind::JudgeLabel,
        None,
        None,
        json!({"id": format!("lbl_{name}"), "judgment": j.id, "pack": "security.v3",
            "question": "risky", "label": truth, "source": "operator", "who": "owner",
            "via": "cli", "weight": 1.0, "note": ""}),
    );
    row.at_unix_ms = 1;
    let mut label = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    label.key = Some(format!("lbl_{name}"));
    c.store
        .append(&[call_row(&j, at_ms), label.scoped("judge:security")])
        .unwrap();
}

/// A security pack's better candidate waits on the owner's card: nothing
/// is placed until it is answered, and the lineage's next run is held as
/// one open proposal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_security_candidate_waits_on_the_owners_card() {
    let jev = FakeJev::start().unwrap();
    jev.script_when("heron", Some("Plainly"), "risky", Jev::Noul(0.05));
    jev.script_when("wren", Some("Plainly"), "risky", Jev::Noul(0.05));
    jev.script_when("gull", Some("Plainly"), "risky", Jev::Noul(0.05));
    jev.script("risky", Jev::Noul(0.95));
    let reply = SECURITY_V3.replacen(
        "when_true = \"The call could destroy",
        "when_true = \"Plainly, the call could destroy",
        1,
    );
    let r = rig(vec![Scripted::text(&reply); 2], &jev, |_| {});
    let c = &r.core;
    let split = split_now();
    for i in 0..10 {
        seed_security(c, &format!("heron{i}"), 0.95, false, split - 100_000 + i);
    }
    seed_security(c, "wren", 0.95, false, split + 10);
    seed_security(c, "gull", 0.05, false, split + 20);
    let learn_v3 = || {
        c.judge_learn(
            JudgeLearnParams {
                pack: "security.v3".into(),
                split: Some(split.to_string()),
            },
            "cli",
        )
    };
    let p = learn_v3().await.unwrap();
    assert_eq!(p.decision, "card", "{}", p.why);
    // The Noul's errors: its lean against the label, not the label alone.
    assert_eq!((p.fixed, p.broken), (10, 0));
    assert_eq!(p.version.as_deref(), Some("security.v101"));
    assert!(p.question.is_some(), "{p:?}");
    assert!(
        p.said.contains("waiting on your approval card"),
        "{}",
        p.said
    );
    // Nothing placed until the owner answers.
    assert!(c.runner.judge.ladder().rows_of("security.v101").is_empty());
    assert_eq!(
        c.runner.judge.placed("security.v3", "ses_harbor"),
        "security.v3"
    );
    // A new error, but the card is open: the next run is held.
    seed_security(c, "heron10", 0.95, false, split - 1_000);
    let again = learn_v3().await.unwrap();
    assert_eq!(again.decision, "skipped", "{}", again.why);
    assert!(again.why.contains("one open proposal"), "{}", again.why);
    assert_eq!(r.fake.requests().len(), 1);
}
