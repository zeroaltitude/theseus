//! The learning ledger (M5 25c; design §3, "25c"), on seeded stores: a
//! label through the protocol, refused from a shared place and from an
//! unnamed connection; each system label from a scripted history, and a
//! second run writing none again; the report's numbers equal
//! `theseus_judge::learn`'s on the same pairs; and a holdout frozen into the
//! report, unchanged by later judgments. The tender's timing and priority
//! are `learning::tender`'s tests.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::learn::{self, DAY_MS};
use theseus_judge::{Judgment, Thresholds};
use theseus_protocol::learning::{JudgeLabelParams, LearningReport};
use theseus_protocol::{LedgerKind, SessionKind};
use theseus_store::{kinds, NewRecord};
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Answerer, Client, Surface};
use crate::ledger::LedgerRow;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::rpc::Core;
use crate::session::{SessionRecord, TaskOf};
use crate::tests_judge::{off, rig_on, texts, turn, Rig};

const T: Thresholds = Thresholds {
    act: 0.9,
    confirm: 0.6,
};

/// A core with the judge on and every pack off: nothing reaches a Jev, and
/// the tests write the judgments themselves.
fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    rig_on(script, None, |c| {
        c.judge.enabled = true;
        c.judge.api_base = "http://127.0.0.1:9".into();
        for (p, _) in crate::judge::WIRED {
            c.judge.packs.insert((*p).into(), off());
        }
        c.secrets
            .entry("jev_api_key".into())
            .or_insert_with(|| "env:THESEUS_TEST_JEV".into());
        tweak(c);
    })
}

fn noul(q: &str, p: f64) -> AnswerRecord {
    let answer = Answer::Noul { noul: p };
    AnswerRecord {
        question: q.into(),
        def: q.into(),
        about: None,
        band: band(&answer, T),
        answer,
    }
}

fn choice(q: &str, top: &str, confidence: f64, options: &[&str]) -> AnswerRecord {
    let rest = (1.0 - confidence) / (options.len() as f64 - 1.0).max(1.0);
    let answer = Answer::Choice {
        choice: top.into(),
        probabilities: options
            .iter()
            .map(|o| ((*o).to_string(), if *o == top { confidence } else { rest }))
            .collect(),
        confidence,
    };
    AnswerRecord {
        question: q.into(),
        def: q.into(),
        about: None,
        band: band(&answer, T),
        answer,
    }
}

const WORK: [&str; 6] = [
    "complete",
    "progressing",
    "blocked_needs_human",
    "thrashing",
    "off_task",
    "other",
];

fn judgment(id: &str, pack: &str, answers: Vec<AnswerRecord>, context: Value, ms: u64) -> Judgment {
    let point = match pack.split('.').next() {
        Some("loop") => "loop_end",
        Some("security") => "gate",
        _ => "inbound",
    };
    serde_json::from_value(json!({
        "id": id, "pack": pack, "version": 1, "pack_sha256": "00", "point": point,
        "mode": "shadow", "model": "jev-1.13.0", "answered_by": "jev-1.13.0",
        "model_drift": false,
        "state": {"sha256": "00", "bytes": 10, "tokens": 3, "cap_tokens": 4000,
            "builder": "loop", "builder_version": 1, "truncated": [], "dropped": []},
        "questions": answers.len(), "answers": answers, "call": null,
        "timing": {"queued_ms": 0, "http_ms": ms, "total_ms": ms},
        "usage": null, "cost_micros": 40, "reserve_micros": 50,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": context,
    }))
    .unwrap()
}

/// A judgment's `judge.call` row, as the sink writes it, at `at_ms`.
fn call_row(j: &Judgment, at_ms: u64) -> NewRecord {
    let f = crate::fact::judge::JudgeCall {
        judgment: j,
        budget: "shadow",
    };
    let s = |k: &str| j.context.get(k).and_then(Value::as_str);
    row_at(
        LedgerKind::JudgeCall,
        s("session"),
        crate::fact::Fact::row(&f),
        at_ms,
        &j.id,
        &j.pack,
    )
}

fn row_at(
    kind: LedgerKind,
    session: Option<&str>,
    data: Value,
    at_ms: u64,
    key: &str,
    pack: &str,
) -> NewRecord {
    let mut row = LedgerRow::new(kind, session, None, data);
    row.at_unix_ms = at_ms;
    let mut r = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    r.key = Some(key.to_string());
    r.scoped(&crate::rpc::judge::scope_of(pack))
}

/// An operator's (or the system's) label row, at `position`'s order.
fn label_row(
    id: &str,
    j: &str,
    pack: &str,
    question: Option<&str>,
    label: Value,
    weight: f64,
) -> NewRecord {
    let source = if weight < 1.0 { "system" } else { "operator" };
    row_at(
        LedgerKind::JudgeLabel,
        None,
        json!({"id": id, "judgment": j, "pack": pack, "question": question, "label": label,
            "source": source, "who": "test", "via": "cli", "weight": weight, "note": ""}),
        1,
        id,
        pack,
    )
}

fn rows(core: &Core, scope: &str, kind: LedgerKind) -> Vec<(u64, LedgerRow)> {
    core.store
        .scope_after(scope, 0)
        .unwrap()
        .into_iter()
        .map(|r| (r.position, r.decode::<LedgerRow>().unwrap()))
        .filter(|(_, r)| r.kind == kind.as_str())
        .collect()
}

fn loop_answers(work: &str, conf: f64, unfinished: f64) -> Vec<AnswerRecord> {
    vec![
        choice("work_state", work, conf, &WORK),
        noul("announced_unfinished", unfinished),
    ]
}

/// A label through `judge.label`: written keyed `lbl_…`, scoped as its
/// judgment, at the operator's weight, and said; refused from a shared
/// place, with its refusal ledgered and nothing written; and a label the
/// pack cannot take, or a judgment that is not one, is invalid.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_is_the_operators_from_a_private_place() {
    let r = rig(texts(0), |_| {});
    let c = &r.core;
    let j = judgment(
        "jdg_heron",
        "loop.v1",
        loop_answers("complete", 0.8, 0.3),
        json!({"session": "ses_x"}),
        900,
    );
    c.store.append(&[call_row(&j, 1_000)]).unwrap();
    let p = |q: Option<&str>, l: Value| JudgeLabelParams {
        judgment: "jdg_heron".into(),
        question: q.map(str::to_string),
        label: l,
        note: Some("it was still going".into()),
    };
    let out = c
        .judge_label(&p(Some("work_state"), json!("progressing")), "cli")
        .unwrap();
    assert_eq!((out.pack.as_str(), out.weight), ("loop.v1", 1.0));
    let written = rows(c, "judge:loop", LedgerKind::JudgeLabel);
    assert_eq!(written.len(), 1);
    let d = &written[0].1.data;
    assert_eq!(
        (
            d["id"].as_str(),
            d["question"].as_str(),
            d["label"].clone(),
            d["source"].as_str(),
            d["via"].as_str()
        ),
        (
            Some(out.id.as_str()),
            Some("work_state"),
            json!("progressing"),
            Some("operator"),
            Some("cli")
        )
    );
    let by_key = c.store.ledger_by_key(&out.id).unwrap().unwrap();
    assert_eq!(by_key.position, written[0].0);
    // A shared place's word does not count.
    let stranger = Answerer {
        label: "discord".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "42".into(),
            channel_id: "1001".into(),
            guild_id: Some("7".into()),
        }),
    };
    let e = c
        .judge_label(&p(None, json!("wrong")), stranger)
        .unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    assert_eq!(
        rows(c, "judge:loop", LedgerKind::JudgeLabel).len(),
        1,
        "nothing written"
    );
    let refused = c.store.ledger_tail::<LedgerRow>(50).unwrap();
    assert!(refused
        .iter()
        .any(|(_, r)| r.kind == "approval.refused" && r.data["act"] == "judge.label"));
    // Labels the pack cannot take, and a judgment that is none.
    assert!(c
        .judge_label(&p(Some("work_state"), json!("sideways")), "cli")
        .is_err());
    assert!(c
        .judge_label(&p(None, json!("progressing")), "cli")
        .is_err());
    let mut ghost = p(None, json!("wrong"));
    ghost.judgment = "jdg_nobody".into();
    assert!(c.judge_label(&ghost, "cli").is_err());
}

/// Send one request on a connection the core serves as `client`.
async fn request(core: &Arc<Core>, client: Client, method: &str, params: Value) -> Value {
    let (a, b) = duplex(1 << 20);
    let (sr, sw) = tokio::io::split(b);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(a);
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    cw.write_all(format!("{line}\n").as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let answer = loop {
        let l: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        if l.get("id").is_some() {
            break l;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    answer
}

/// Through the protocol: the CLI's socket labels and runs the report; an
/// unnamed connection's label is refused (`REFUSED`) and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_and_a_report_through_the_protocol() {
    let r = rig(texts(0), |_| {});
    let c = &r.core;
    let j = judgment(
        "jdg_wren",
        "loop.v1",
        loop_answers("complete", 0.8, 0.3),
        json!({}),
        900,
    );
    c.store
        .append(&[call_row(&j, theseus_protocol::now_unix_ms())])
        .unwrap();
    let params = json!({"judgment": "jdg_wren", "label": "wrong"});
    let ok = request(
        c,
        Client::new("cli", Surface::Cli),
        "judge.label",
        params.clone(),
    )
    .await;
    assert_eq!(ok["result"]["label"], "wrong", "{ok}");
    let no = request(
        c,
        Client::new("test", Surface::Unnamed),
        "judge.label",
        params,
    )
    .await;
    assert_eq!(
        no["error"]["code"].as_i64(),
        Some(theseus_protocol::error_code::REFUSED),
        "{no}"
    );
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeLabel).len(), 1);
    let rep = request(
        c,
        Client::new("cli", Surface::Cli),
        "learning.report",
        Value::Null,
    )
    .await;
    let rep: LearningReport = serde_json::from_value(rep["result"].clone()).unwrap();
    assert_eq!(rep.trigger, "on_demand");
    let lp = rep.packs.iter().find(|p| p.pack == "loop.v1").unwrap();
    assert_eq!((lp.calls, lp.labeled, rep.labels.operator), (1, 1, 1));
    assert!(lp
        .holdout
        .insufficient
        .as_deref()
        .unwrap()
        .starts_with("insufficient: "));
    // By date: the stored one, and its file.
    let again = request(
        c,
        Client::new("cli", Surface::Cli),
        "learning.report",
        json!({"date": rep.date}),
    )
    .await;
    let again: LearningReport = serde_json::from_value(again["result"].clone()).unwrap();
    assert_eq!(again.packs, rep.packs);
    let file = c
        .cfg
        .state_dir()
        .join("learning")
        .join(format!("{}.json", rep.date));
    let on_disk: LearningReport = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(on_disk.packs, rep.packs);
    // The file is derived: gone, a read by date writes it again from the rows.
    std::fs::remove_file(&file).unwrap();
    c.stored_report(&rep.date).unwrap().unwrap();
    assert!(file.exists());
}

/// A small deterministic generator (xorshift64*), for synthetic data.
struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// What the synthetic store's pairs are, as the test computes them.
struct Want {
    n: u64,
    noul_pairs: Vec<(f64, bool)>,
    top_pairs: Vec<(f64, bool)>,
    class_pairs: Vec<(String, String)>,
    latency: BTreeMap<&'static str, Vec<u64>>,
    disagree: u32,
}

/// 120 `loop.v1` judgments over the last day, three in four labeled (each
/// with a lighter system label that disagrees), and the pairs they make.
fn synthetic(now: u64) -> (Vec<NewRecord>, Want) {
    let mut rng = Rng(0x5EED_1234_ABCD_0042);
    let mut records = Vec::new();
    let (mut noul_pairs, mut top_pairs, mut class_pairs) = (vec![], vec![], vec![]);
    let mut latency: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    let mut disagree = 0u32;
    let n: u64 = 120;
    for i in 0..n {
        let id = format!("jdg_syn{i:04}");
        let top = WORK[(rng.unit() * 6.0) as usize];
        let conf = 0.5 + rng.unit() * 0.5;
        let p = rng.unit();
        let class = if i % 3 == 0 { "task" } else { "reply" };
        let ms = 100 + (rng.unit() * 900.0) as u64;
        latency.entry(class).or_default().push(ms);
        let j = judgment(
            &id,
            "loop.v1",
            loop_answers(top, conf, p),
            json!({"class": class}),
            ms,
        );
        if crate::fact::judge::disagrees(&j) {
            disagree += 1;
        }
        records.push(call_row(&j, now - DAY_MS + i));
        if i % 4 == 3 {
            continue; // unlabeled
        }
        let truth = WORK[(rng.unit() * 6.0) as usize];
        let yes = rng.unit() < p;
        // A system label says otherwise, at a lower weight: the operator's counts.
        records.push(label_row(
            &format!("lbl_s{i}"),
            &id,
            "loop.v1",
            Some("work_state"),
            json!("other"),
            0.5,
        ));
        records.push(label_row(
            &format!("lbl_o{i}"),
            &id,
            "loop.v1",
            Some("work_state"),
            json!(truth),
            1.0,
        ));
        records.push(label_row(
            &format!("lbl_n{i}"),
            &id,
            "loop.v1",
            Some("announced_unfinished"),
            json!(yes),
            1.0,
        ));
        noul_pairs.push((p, yes));
        top_pairs.push((conf, top == truth));
        class_pairs.push((top.to_string(), truth.to_string()));
    }
    let want = Want {
        n,
        noul_pairs,
        top_pairs,
        class_pairs,
        latency,
        disagree,
    };
    (records, want)
}

/// The report's numbers on a synthetic store equal `learn`'s on the same
/// pairs: calibration (Brier, ECE, every bin) for a Noul and a top choice,
/// precision and recall per class, latency percentiles per class, and
/// agreement; the heavier label counts where two disagree.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_reports_numbers_equal_learns_on_the_same_pairs() {
    let r = rig(texts(0), |_| {});
    let c = &r.core;
    let now = theseus_protocol::now_unix_ms();
    let (records, want) = synthetic(now);
    let Want {
        n,
        noul_pairs,
        top_pairs,
        class_pairs,
        latency,
        disagree,
    } = want;
    c.store.append(&records).unwrap();
    let rep = c.run_learning(now, "on_demand", |_| {}).unwrap();
    let lp = rep.packs.iter().find(|p| p.pack == "loop.v1").unwrap();
    assert_eq!(
        (lp.calls, lp.answered, lp.labeled),
        (n as u32, n as u32, 90)
    );
    assert_eq!(lp.disagreements, disagree);
    let q = |id: &str| lp.questions.iter().find(|q| q.question == id).unwrap();
    let same = |got: &theseus_protocol::learning::Calibration, want: &learn::Calibration| {
        assert_eq!(got.n as usize, want.n);
        assert_eq!((got.brier, got.ece), (want.brier, want.ece));
        assert_eq!(got.bins.len(), want.bins.len());
        for (g, w) in got.bins.iter().zip(&want.bins) {
            assert_eq!(
                (g.n as usize, g.mean_p, g.frequency),
                (w.n, w.mean_p, w.frequency)
            );
        }
    };
    same(
        q("announced_unfinished").calibration.as_ref().unwrap(),
        &learn::calibration(&noul_pairs, 10),
    );
    same(
        q("work_state").calibration.as_ref().unwrap(),
        &learn::calibration(&top_pairs, 10),
    );
    for class in WORK {
        let row = q("work_state")
            .classes
            .iter()
            .find(|r| r.class == class)
            .unwrap();
        assert_eq!(
            (row.precision, row.recall),
            learn::precision_recall(&class_pairs, class),
            "{class}"
        );
    }
    for (class, ms) in &latency {
        let row = lp.latency.iter().find(|l| l.class == *class).unwrap();
        assert_eq!(
            (row.p50_ms, row.p95_ms, row.p99_ms),
            (
                learn::percentile(ms, 50.0),
                learn::percentile(ms, 95.0),
                learn::percentile(ms, 99.0)
            )
        );
    }
    assert_eq!(
        lp.agreement,
        Some(f64::from(n as u32 - disagree) / f64::from(n as u32))
    );
    // Bands, as shares of the answers.
    let bands: u32 = q("work_state").bands.iter().map(|b| b.n).sum();
    assert_eq!(bands, n as u32);
    assert_eq!(rep.labels.operator, 180);
    assert_eq!(rep.labels.system, 90);
}

/// The holdout is the 14 days before the report's local midnight, frozen:
/// the judgments inside it, and nothing after; a later judgment changes
/// neither the stored report nor a run again for the same day; and short
/// of the minimum it says "insufficient" with its counts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_holdout_is_frozen_into_the_report() {
    let r = rig(texts(0), |_| {});
    let c = &r.core;
    let now = theseus_protocol::now_unix_ms();
    let midnight = crate::learning::local_midnight(now);
    let at = |days_before: u64, ms: u64| midnight - days_before * DAY_MS + ms;
    // 20 days before (train), 10 and 1 day before (holdout), and today
    // (after the window: later).
    let mut records = Vec::new();
    for (id, when) in [
        ("jdg_old", at(20, 0)),
        ("jdg_in1", at(10, 5)),
        ("jdg_in2", at(1, 5)),
        ("jdg_edge", at(14, 0)),
        ("jdg_today", midnight + 1),
    ] {
        let j = judgment(
            id,
            "loop.v1",
            loop_answers("progressing", 0.95, 0.2),
            json!({}),
            300,
        );
        records.push(call_row(&j, when));
    }
    records.push(label_row(
        "lbl_in1",
        "jdg_in1",
        "loop.v1",
        Some("work_state"),
        json!("progressing"),
        1.0,
    ));
    records.push(label_row(
        "lbl_old",
        "jdg_old",
        "loop.v1",
        None,
        json!("wrong"),
        1.0,
    ));
    c.store.append(&records).unwrap();
    let rep = c.run_learning(now, "nightly", |_| {}).unwrap();
    let h = &rep
        .packs
        .iter()
        .find(|p| p.pack == "loop.v1")
        .unwrap()
        .holdout;
    assert_eq!((h.start_ms, h.end_ms), (midnight - 14 * DAY_MS, midnight));
    assert_eq!(h.judgments, ["jdg_in1", "jdg_in2", "jdg_edge"]);
    assert_eq!(h.labels, ["lbl_in1"]);
    assert_eq!((h.train, h.later), (1, 1));
    assert_eq!(h.labeled_per_question["work_state"], 1);
    assert_eq!(h.labeled_per_acting_class["progressing"], 1);
    assert!(!h.sufficient);
    assert_eq!(
        h.insufficient.as_deref(),
        Some(
            "insufficient: announced_unfinished: labeled 0 of 200; work_state: labeled 1 of 200; \
             progressing: labeled 1 of 30"
        )
    );
    // Later judgments: the stored report is unchanged, and a run again for
    // the same day freezes the same holdout.
    let late = judgment(
        "jdg_late",
        "loop.v1",
        loop_answers("complete", 0.9, 0.1),
        json!({}),
        300,
    );
    c.store.append(&[call_row(&late, now + 1)]).unwrap();
    let stored = c.stored_report(&rep.date).unwrap().unwrap();
    assert_eq!(stored.packs, rep.packs);
    let again = c.run_learning(now + 2, "on_demand", |_| {}).unwrap();
    let h2 = &again
        .packs
        .iter()
        .find(|p| p.pack == "loop.v1")
        .unwrap()
        .holdout;
    assert_eq!(h2.judgments, h.judgments);
    assert_eq!(h2.later, 2);
}

fn loop_context(res: &theseus_protocol::TurnSubmitResult, decision: &str, class: &str) -> Value {
    json!({"session": res.session_id, "turn": res.turn_id, "decision": decision, "class": class})
}

/// The history `loop.v1`'s rules read: a reply then "Go on."; a turn the
/// budget ended, then "continue"; a task, then a near-identical one. Each
/// judged turn's judgment, as the sink would write it.
async fn loop_history(c: &Arc<Core>) -> Vec<Judgment> {
    // A conversation: an answer, then "go on".
    let a = turn(c, None, "Draft the release notes.").await;
    let a2 = turn(c, Some(&a.session_id), "Go on.").await;
    // A conversation whose budget ended its turn, then "continue".
    let b = turn(c, None, "Count the herons.").await;
    turn(c, Some(&b.session_id), "continue").await;
    // A task, and a near-identical one after it.
    let task = |brief: &str| {
        let mut rec = SessionRecord::new(SessionKind::Task, None);
        rec.task = Some(TaskOf {
            parent_session: "ses_parent".into(),
            parent_execution: "exe_parent".into(),
            by: "corr_parent".into(),
            target: None,
            arrangement: None,
            check: None,
        });
        c.store.put_session(&rec.session_id, &rec).unwrap();
        let _ = brief;
        rec.session_id
    };
    let t1 = task("x");
    let tr = turn(
        c,
        Some(&t1),
        "Rename the parser module to lexer and fix the imports.",
    )
    .await;
    let t2 = task("y");
    turn(
        c,
        Some(&t2),
        "Rename the parser module to lexer and fix the imports!",
    )
    .await;
    vec![
        judgment(
            "jdg_a",
            "loop.v1",
            loop_answers("complete", 0.9, 0.1),
            loop_context(&a, "no_tool_calls", "reply"),
            300,
        ),
        judgment(
            "jdg_a2",
            "loop.v1",
            loop_answers("complete", 0.9, 0.1),
            loop_context(&a2, "no_tool_calls", "reply"),
            300,
        ),
        judgment(
            "jdg_b",
            "loop.v1",
            loop_answers("complete", 0.9, 0.1),
            loop_context(&b, "budget_exhausted", "reply"),
            300,
        ),
        judgment(
            "jdg_t",
            "loop.v1",
            loop_answers("complete", 0.9, 0.1),
            loop_context(&tr, "no_tool_calls", "task"),
            300,
        ),
    ]
}

/// `loop.v1`'s system labels from a scripted history: "go on" within 10
/// minutes is stopped too early; a near-identical task brief is a false
/// completion; a turn the budget ended takes none; and a second run writes
/// nothing again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn loops_system_labels_come_from_the_history_and_are_written_once() {
    let r = rig(texts(8), |_| {});
    let c = &r.core;
    let js = loop_history(c).await;
    let now = theseus_protocol::now_unix_ms();
    let recs: Vec<NewRecord> = js.iter().map(|j| call_row(j, now)).collect();
    c.store.append(&recs).unwrap();
    let first = c.run_learning(now, "on_demand", |_| {}).unwrap();
    let labels = rows(c, "judge:loop", LedgerKind::JudgeLabel);
    let got: Vec<(String, String, Value, String)> = labels
        .iter()
        .map(|(_, r)| {
            let d = &r.data;
            (
                d["judgment"].as_str().unwrap().into(),
                d["rule"].as_str().unwrap().into(),
                d["label"].clone(),
                d["source"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                "jdg_a".into(),
                "continuation".into(),
                json!("progressing"),
                "system".into()
            ),
            (
                "jdg_t".into(),
                "false_completion".into(),
                json!({"not": "complete"}),
                "system".into()
            ),
        ],
        "{got:#?}"
    );
    assert!(labels.iter().all(|(_, r)| r.data["weight"] == 0.5));
    assert_eq!(first.labels.system_written, 2);
    let lp = first.packs.iter().find(|p| p.pack == "loop.v1").unwrap();
    assert_eq!(lp.labeled, 2, "both grade work_state's `complete`");
    // A second run: nothing written again.
    let second = c.run_learning(now + 1, "on_demand", |_| {}).unwrap();
    assert_eq!(second.labels.system_written, 0);
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeLabel).len(), 2);
}

/// `security.*`'s system labels: a declined waiting call is risky, an
/// approved one is not, and an approved one the operator labeled takes
/// none; `classify.v1`'s `should_promote` is whether the turn called
/// `task.create`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn security_and_classify_labels_come_from_the_record() {
    let run = |id: &str| Scripted::tools("", &[(id, "proc_run", json!({"argv": ["echo", "hi"]}))]);
    let r = rig(
        vec![
            run("r1"),
            run("r2"),
            run("r3"),
            Scripted::tools(
                "Starting it.",
                &[("t_task", "task_create", json!({"brief": "Sort the shelf."}))],
            ),
            Scripted::text("Started."),
            Scripted::text("Hello."),
        ],
        |c| {
            c.policy.tools.insert("proc.run".into(), Posture::Approve);
        },
    );
    let c = &r.core;
    let mut calls = Vec::new();
    for _ in 0..3 {
        let res = turn(c, None, "say hi").await;
        calls.push((
            res.session_id.clone(),
            res.awaiting_confirm.clone().expect("it waits"),
        ));
    }
    c.confirm_action(&calls[0].1, false, Some("no"), "cli")
        .unwrap();
    c.confirm_action(&calls[1].1, true, None, "cli").unwrap();
    c.confirm_action(&calls[2].1, true, None, "cli").unwrap();
    let promoted = turn(c, None, "Sort the shelf, as a task.").await;
    let chat = turn(c, None, "Hello there.").await;
    let now = theseus_protocol::now_unix_ms();
    let sec = |i: usize| {
        judgment(
            &format!("jdg_sec{i}"),
            "security.v1",
            vec![noul("risky", 0.3)],
            json!({"session": calls[i].0, "call": calls[i].1}),
            200,
        )
    };
    let cls = |id: &str, res: &theseus_protocol::TurnSubmitResult| {
        judgment(
            id,
            "classify.v1",
            vec![noul("should_promote", 0.7)],
            json!({"session": res.session_id, "turn": res.turn_id}),
            200,
        )
    };
    let mut recs: Vec<NewRecord> = (0..3).map(|i| call_row(&sec(i), now)).collect();
    recs.push(label_row(
        "lbl_press",
        "jdg_sec2",
        "security.v1",
        Some("risky"),
        json!(true),
        1.0,
    ));
    recs.push(call_row(&cls("jdg_cls_task", &promoted), now));
    recs.push(call_row(&cls("jdg_cls_chat", &chat), now + DAY_MS));
    c.store.append(&recs).unwrap();
    // An hour on, the chat's turn counts as ended.
    c.run_learning(now + 2 * 3_600_000, "on_demand", |_| {})
        .unwrap();
    let said = |scope: &str| -> Vec<(String, String, Value)> {
        rows(c, scope, LedgerKind::JudgeLabel)
            .into_iter()
            .filter(|(_, r)| r.data["source"] == "system")
            .map(|(_, r)| {
                (
                    r.data["judgment"].as_str().unwrap().into(),
                    r.data["rule"].as_str().unwrap().into(),
                    r.data["label"].clone(),
                )
            })
            .collect()
    };
    assert_eq!(
        said("judge:security"),
        [
            ("jdg_sec0".into(), "declined".into(), json!(true)),
            ("jdg_sec1".into(), "approved".into(), json!(false)),
        ]
    );
    assert_eq!(
        said("judge:classify"),
        [
            ("jdg_cls_task".into(), "task_create".into(), json!(true)),
            ("jdg_cls_chat".into(), "task_create".into(), json!(false)),
        ]
    );
}
