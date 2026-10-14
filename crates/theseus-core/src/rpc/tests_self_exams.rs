//! Self-exams through the core (theseus-pw1q.11): a seeded day makes four
//! candidates in the right families, the pass reads and writes nothing with
//! the mode off or the switch thrown, `exams_day_max` caps the day, and
//! `self.exams` shows counts and never a sealed item. The pure parts
//! (the split, the scrub, the proposer rule) are `rsi/exams/tests.rs`.

use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::{Judgment, Thresholds};
use theseus_protocol::rsi::{SelfExamsParams, SelfMode, SelfResumeParams};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use super::tests::test_core;
use super::{Core, Parts};
use crate::approval::Answerer;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rsi::exams::{Family, Item, Reviewer, SplitKey, Verdict, KEY_META};

const DAY: u64 = 86_400_000;
const T: Thresholds = Thresholds {
    act: 0.9,
    confirm: 0.6,
};

/// A reviewer that says every item is fair and counts what it was asked.
struct Kind(Mutex<Vec<String>>);

impl Reviewer for Kind {
    fn review<'a>(&'a self, item: &'a Item) -> BoxFuture<'a, anyhow::Result<Verdict>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(item.id.0.clone());
            Ok(Verdict {
                tests_behaviour: true,
                answer_right: true,
                not_gamed: true,
                reason: "fair".into(),
                reviewer: "stub-reviewer".into(),
            })
        })
    }
}

fn core(mode: SelfMode, day_max: u32) -> Arc<Core> {
    let c = test_core("the tide is out");
    let mut cfg = (*c.cfg).clone();
    cfg.self_improve.mode = mode;
    cfg.self_improve.exams_day_max = day_max;
    let c = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        c.store.clone(),
    ))
    .unwrap();
    // A key of the test's own, so the split is the same every run.
    c.store
        .put_meta(KEY_META, &json!({"key": SplitKey([9; 32]).hex()}))
        .unwrap();
    c
}

fn resume(c: &Core) {
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
}

fn ledger(c: &Core, kind: LedgerKind, session: Option<&str>, data: Value, at_ms: u64) {
    let mut row = LedgerRow::new(kind, session, None, data);
    row.at_unix_ms = at_ms;
    c.store.append_ledger(&row).unwrap();
}

/// A loop.v1 judgment that leaned `complete`, 20 days old, labeled `wrong`
/// by the operator today.
fn wrong_label(c: &Core, now: u64, age_days: u64) {
    let answer = Answer::Choice {
        choice: "complete".into(),
        probabilities: [
            ("complete".to_string(), 0.9),
            ("progressing".to_string(), 0.1),
        ]
        .into(),
        confidence: 0.9,
    };
    let record = AnswerRecord {
        question: "work_state".into(),
        def: "work_state".into(),
        about: None,
        band: band(&answer, T),
        answer,
    };
    let j: Judgment = serde_json::from_value(json!({
        "id": "jdg_tern", "pack": "loop.v1", "version": 1, "pack_sha256": "00",
        "point": "loop_end", "mode": "shadow", "model": "jev-1.13.0",
        "answered_by": "jev-1.13.0", "model_drift": false,
        "state": {"sha256": "00", "bytes": 10, "tokens": 3, "cap_tokens": 4000,
            "builder": "loop", "builder_version": 1, "truncated": [], "dropped": []},
        "questions": 1, "answers": [record], "call": null,
        "timing": {"queued_ms": 0, "http_ms": 9, "total_ms": 9},
        "usage": null, "cost_micros": 40, "reserve_micros": 50,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": {"session": "ses_plover", "stop": "no_tool_calls"},
    }))
    .unwrap();
    let scoped = |kind, data: Value, at: u64, key: &str| {
        let mut row = LedgerRow::new(kind, None, None, data);
        row.at_unix_ms = at;
        let mut r = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
        r.key = Some(key.to_string());
        r.scoped(&super::judge::scope_of("loop.v1"))
    };
    let call = crate::fact::judge::JudgeCall {
        judgment: &j,
        budget: "shadow",
    };
    c.store
        .append(&[
            scoped(
                LedgerKind::JudgeCall,
                crate::fact::Fact::row(&call),
                now - age_days * DAY,
                &j.id,
            ),
            scoped(
                LedgerKind::JudgeLabel,
                json!({"id": "lbl_1", "judgment": "jdg_tern", "pack": "loop.v1",
                    "question": "work_state", "label": "wrong", "source": "operator",
                    "who": "test", "via": "cli", "weight": 1.0, "note": ""}),
                now,
                "lbl_1",
            ),
        ])
        .unwrap();
}

/// The day's four sources.
fn seed(c: &Core, now: u64) {
    ledger(
        c,
        LedgerKind::RouteCorrected,
        Some("ses_plover"),
        json!({"id": "rcx_1", "turn": "trn_1", "session": "ses_plover", "mode": "chat",
            "ran": "haiku", "to": "fable", "words": ["harbor", "tide", "gauge"],
            "provenance": "msg_1", "via": "message", "at_ms": now}),
        now - 1000,
    );
    wrong_label(c, now, 20);
    ledger(
        c,
        LedgerKind::TaskEnded,
        Some("ses_plover"),
        json!({"execution_id": "exe_p", "task": "exe_wren", "state": "failed",
            "reason": "the build failed twice", "spent_usd": 0.4, "limit_usd": 1.0}),
        now - 900,
    );
    ledger(
        c,
        LedgerKind::BudgetReached,
        Some("ses_plover"),
        json!({"mode": "notify", "spent_usd": 2.2, "limit_usd": 1.0, "multiple": 2,
            "next_usd": 3.0, "lifetime_usd": 2.2, "posted": false}),
        now - 800,
    );
}

fn count(c: &Core, kind: &str) -> usize {
    c.store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .count()
}

/// A correction, a wrong label, a failed task and an over-budget turn make
/// four candidates, one in each family; the open ones are reviewed, counted
/// and shelved, the sealed ones (no sealed store yet) are dropped with a
/// count, and a second pass finds nothing new.
#[tokio::test]
async fn a_seeded_day_makes_four_candidates_in_the_right_families() {
    let c = core(SelfMode::Act, 10);
    resume(&c);
    let now = theseus_protocol::now_unix_ms();
    seed(&c, now);
    // The pass runs after the day's rows are written, as a night's does.
    let now = theseus_protocol::now_unix_ms();
    let raws = c.exam_raws(now - DAY, now, now).unwrap();
    let check = |f: Family| raws.iter().find(|r| r.family == f).unwrap().check.clone();
    assert!(check(Family::Correction).contains("reply has word \"fable\""));
    assert_eq!(check(Family::WrongLabel), "reply lacks word \"complete\"");
    assert!(check(Family::FailedTask).contains("retry"));
    assert!(raws
        .iter()
        .all(|r| r.source_rows.iter().all(|s| s.starts_with("ledger:"))));
    let reviewer = Kind(Mutex::new(Vec::new()));
    let rep = c.self_exams_pass(now, &reviewer).await.unwrap().unwrap();
    assert_eq!(rep.candidates, 4, "{rep:?}");
    let mut families: Vec<Family> = rep.outcomes.iter().map(|o| o.family).collect();
    families.sort();
    assert_eq!(
        families,
        vec![
            Family::Correction,
            Family::WrongLabel,
            Family::FailedTask,
            Family::OverBudget
        ]
    );
    let open = rep.outcomes.iter().filter(|o| !o.sealed).count();
    assert_eq!(rep.counted() as usize, open);
    assert_eq!(rep.sealed_dropped() as usize, 4 - open);
    assert_eq!(
        reviewer.0.lock().unwrap().len(),
        open,
        "only open items are reviewed"
    );
    assert_eq!(count(&c, "self.exam.counted"), open);
    assert_eq!(count(&c, "self.exam.pass"), 1);
    let shelved = crate::rsi::exams::Shelf::new(&c.cfg.state_dir()).items();
    assert_eq!(shelved.len(), open);
    assert!(shelved
        .iter()
        .all(|i| i.provenance.iter().all(|p| p.starts_with("ledger:"))));
    // The wrong label's item holds the right answer and no ids.
    let again = c.self_exams_pass(now + 1000, &reviewer).await.unwrap();
    assert!(again.is_none_or(|r| r.candidates == 0), "nothing new");
    let shown = c.self_exams(&SelfExamsParams::default()).unwrap();
    let total: u64 = shown.families.iter().map(|f| f.open).sum();
    assert_eq!(total as usize, open);
    assert_eq!(shown.sealed_dropped as usize, 4 - open);
    assert!(shown.today.iter().all(|t| t.sealed || t.task.is_some()));
}

/// Mode off, and a switch never resumed, make nothing: no row, no file, no
/// new position in the store.
#[tokio::test]
async fn mode_off_and_a_thrown_switch_make_nothing() {
    for (mode, resumed) in [(SelfMode::Off, true), (SelfMode::Act, false)] {
        let c = core(mode, 10);
        if resumed {
            resume(&c);
        }
        let now = theseus_protocol::now_unix_ms();
        seed(&c, now);
        let before = c.store.last_position();
        assert!(c.self_exams_nightly().await.is_none(), "{mode:?}");
        assert_eq!(
            c.store.last_position(),
            before,
            "{mode:?}: nothing was written"
        );
        assert!(!c.cfg.state_dir().join("self").exists());
    }
}

/// `exams_day_max` caps the reviews a day, across passes.
#[tokio::test]
async fn exams_day_max_caps_the_day() {
    let c = core(SelfMode::Act, 2);
    resume(&c);
    let now = theseus_protocol::now_unix_ms();
    for n in 0..30u32 {
        ledger(
            &c,
            LedgerKind::RouteCorrected,
            Some("ses_plover"),
            json!({"id": format!("rcx_{n}"), "session": "ses_plover", "ran": "haiku",
                "to": "fable", "words": [format!("word{n}"), "tide"], "via": "message"}),
            now - 5000 + u64::from(n),
        );
    }
    let now = theseus_protocol::now_unix_ms();
    let reviewer = Kind(Mutex::new(Vec::new()));
    let rep = c.self_exams_pass(now, &reviewer).await.unwrap().unwrap();
    assert_eq!(rep.counted() + rep.rejected(), 2, "{rep:?}");
    assert!(rep.capped > 0);
    assert_eq!(count(&c, "self.exam.counted"), 2);
    let second = c.self_exams_pass(now + 1000, &reviewer).await.unwrap();
    assert!(second.is_none(), "the day's room is spent");
    assert_eq!(count(&c, "self.exam.counted"), 2);
}

/// `self.exams` counts a sealed item and shows nothing of it.
#[tokio::test]
async fn self_exams_shows_a_sealed_item_as_a_number_alone() {
    let c = core(SelfMode::Act, 10);
    let f = crate::fact::rsi_exams::ExamCounted {
        item: "0123456789abcdef",
        family: "self_correction",
        sealed: true,
        reviewer: "stub-reviewer",
    };
    c.store
        .append(&[crate::fact::row(&f, None, None).unwrap()])
        .unwrap();
    let shown = c.self_exams(&SelfExamsParams::default()).unwrap();
    let fam = shown
        .families
        .iter()
        .find(|f| f.family == "self_correction")
        .unwrap();
    assert_eq!((fam.open, fam.sealed), (0, 1));
    assert_eq!(shown.today.len(), 1);
    assert!(shown.today[0].sealed && shown.today[0].task.is_none());
}

/// A source's mistake made by a `"self"` session is the item's proposer.
#[test]
fn an_item_records_the_self_session_whose_mistake_made_it() {
    use crate::rsi::exams::harvest;
    let mk = |kind: LedgerKind, session: &str, data: Value| {
        let mut r = LedgerRow::new(kind, Some(session), None, data);
        r.at_unix_ms = 5;
        (7u64, r)
    };
    let rows = vec![
        mk(
            LedgerKind::BudgetReached,
            "ses_self1",
            json!({"spent_usd": 3.0, "limit_usd": 1.0, "multiple": 3}),
        ),
        mk(
            LedgerKind::BudgetReached,
            "ses_owner1",
            json!({"spent_usd": 3.0, "limit_usd": 1.0, "multiple": 3}),
        ),
    ];
    let raws = harvest::from_rows(&rows, &|id| id == "ses_self1");
    assert_eq!(raws.len(), 2);
    assert_eq!(raws[0].proposer.as_deref(), Some("ses_self1"));
    assert_eq!(raws[1].proposer, None);
}

/// A label on a judgment inside the learning loop's holdout window is not
/// the train split: it makes no candidate, so the loop's yardstick is not
/// learned from its own held-out data.
#[tokio::test]
async fn a_wrong_label_inside_the_holdout_window_makes_no_candidate() {
    let c = core(SelfMode::Act, 10);
    let now = theseus_protocol::now_unix_ms();
    wrong_label(&c, now, 3);
    let now = theseus_protocol::now_unix_ms();
    assert!(c.exam_raws(now - DAY, now, now).unwrap().is_empty());
    let c = core(SelfMode::Act, 10);
    let now = theseus_protocol::now_unix_ms();
    wrong_label(&c, now, 20);
    let now = theseus_protocol::now_unix_ms();
    assert_eq!(c.exam_raws(now - DAY, now, now).unwrap().len(), 1);
}
