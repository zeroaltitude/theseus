//! Replay (M5 25d; design §3, "25d"), on seeded stores against the fake
//! Jev: a candidate over a report's frozen holdout, its incumbent side the
//! report's own numbers; a reworded criterion, against a fake scripted per
//! state, fixing and breaking the judgments it should; a thresholds-only
//! candidate making no call; a state rebuilt when the builder's version
//! changed, and left out with the reason when it can't be; the run's limit;
//! a yes-or-no answer graded by its lean; and every refusal (a shared place,
//! a wired version, a name under another text).

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::band::band;
use theseus_judge::builders::{LoopInput, SessionKind as Kind};
use theseus_judge::client::Answer;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_judge::judge::{AnswerRecord, StateRecord};
use theseus_judge::learn::DAY_MS;
use theseus_judge::{Input, Judgment, NoScrub, Thresholds};
use theseus_protocol::judge_runs::{JudgeReplayParams, JudgeReplayResult};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::approval::{Answerer, Surface};
use crate::ledger::LedgerRow;
use crate::provider::Scripted;
use crate::rpc::Core;
use crate::tests_judge::{off, rig_on, texts, turn, Rig};

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

const LOOP_V1: &str = include_str!("../../theseus-judge/packs/loop.v1.toml");

/// A core with the judge on against `jev`, and every wired pack off: the
/// tests write the incumbent's judgments themselves.
pub(crate) fn rig(
    script: Vec<Scripted>,
    jev: &FakeJev,
    tweak: impl FnOnce(&mut crate::Config),
) -> Rig {
    rig_on(script, Some(jev), |c| {
        for (p, _) in crate::judge::WIRED {
            c.judge.packs.insert((*p).into(), off());
        }
        c.judge.total_secs = 5;
        tweak(c);
    })
}

/// `loop.v2`: loop.v1 with its `progressing` option reworded.
pub(crate) fn reworded() -> String {
    let at = LOOP_V1.find("{ id = \"progressing\", means = \"").unwrap()
        + "{ id = \"progressing\", means = \"".len();
    let mut s = LOOP_V1.replacen("version = 1", "version = 2", 1);
    s.insert_str(at, "Plainly, ");
    s
}

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

/// A `loop.v1` state for an ask, its blob written, and its record.
pub(crate) fn loop_state(c: &Core, ask: &str) -> (StateRecord, String) {
    let pack = theseus_judge::pack::by_name("loop.v1").unwrap();
    let input = Input::Loop(LoopInput {
        session_kind: Kind::Conversation,
        ask: ask.into(),
        final_text: "Done.".into(),
        tool_calls: vec![],
        loops: 1,
        spend_usd: 0.01,
        minutes_since_ask: 2,
    });
    let p = theseus_judge::prepare(&pack, &input, &NoScrub).unwrap();
    let blob = c.store.blobs().put(p.state.json.as_bytes()).unwrap();
    (StateRecord::from(p.state.as_ref()), blob)
}

pub(crate) fn judgment(
    id: &str,
    pack: &str,
    state: &StateRecord,
    answers: Vec<AnswerRecord>,
    context: Value,
) -> Judgment {
    let point = if pack.starts_with("security") {
        "gate"
    } else {
        "loop_end"
    };
    serde_json::from_value(json!({
        "id": id, "pack": pack, "version": 1, "pack_sha256": "00", "point": point,
        "mode": "shadow", "model": "jev-1.13.0", "answered_by": "jev-1.13.0",
        "model_drift": false, "state": state,
        "questions": answers.len(), "answers": answers, "call": null,
        "timing": {"queued_ms": 0, "http_ms": 300, "total_ms": 300},
        "usage": null, "cost_micros": 40, "reserve_micros": 50,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
        "context": context,
    }))
    .unwrap()
}

fn row_at(kind: LedgerKind, data: Value, at_ms: u64, key: &str, pack: &str) -> NewRecord {
    let mut row = LedgerRow::new(kind, None, None, data);
    row.at_unix_ms = at_ms;
    let mut r = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    r.key = Some(key.to_string());
    r.scoped(&crate::rpc::judge::scope_of(pack))
}

pub(crate) fn call_row(j: &Judgment, at_ms: u64) -> NewRecord {
    let f = crate::fact::judge::JudgeCall {
        judgment: j,
        budget: "shadow",
    };
    row_at(
        LedgerKind::JudgeCall,
        crate::fact::Fact::row(&f),
        at_ms,
        &j.id,
        &j.pack,
    )
}

pub(crate) fn label_row(id: &str, j: &str, question: &str, label: Value) -> NewRecord {
    row_at(
        LedgerKind::JudgeLabel,
        json!({"id": id, "judgment": j, "pack": "loop.v1", "question": question, "label": label,
            "source": "operator", "who": "test", "via": "cli", "weight": 1.0, "note": ""}),
        1,
        id,
        "loop.v1",
    )
}

pub(crate) fn rows(c: &Core, scope: &str, kind: LedgerKind) -> Vec<LedgerRow> {
    c.store
        .scope_after(scope, 0)
        .unwrap()
        .into_iter()
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == kind.as_str())
        .collect()
}

/// Four `loop.v1` judgments in the holdout: heron (complete, labeled
/// progressing: wrong), wren (progressing, labeled so: right), gull
/// (complete, labeled so: right), tern (unlabeled); and the report that
/// freezes them. Returns the report's id.
fn seeded(c: &Arc<Core>) -> String {
    seeded_with(c, &[])
}

/// `seeded`, with more labels (judgment, question, label) written before the
/// report freezes them.
fn seeded_with(c: &Arc<Core>, extra: &[(&str, &str, Value)]) -> String {
    let now = theseus_protocol::now_unix_ms();
    let midnight = crate::learning::local_midnight(now);
    let mut records = Vec::new();
    for (i, (name, top)) in [
        ("heron", "complete"),
        ("wren", "progressing"),
        ("gull", "complete"),
        ("tern", "complete"),
    ]
    .iter()
    .enumerate()
    {
        let (state, blob) = loop_state(c, &format!("Count the {name}s on the pier."));
        let j = judgment(
            &format!("jdg_{name}"),
            "loop.v1",
            &state,
            vec![choice(top, 0.8), noul("announced_unfinished", 0.2)],
            json!({"blob": blob, "session": "ses_x", "turn": format!("trn_{name}"),
                "class": "reply", "decision": "no_tool_calls"}),
        );
        records.push(call_row(&j, midnight - DAY_MS + i as u64));
    }
    for (j, label) in [
        ("heron", "progressing"),
        ("wren", "progressing"),
        ("gull", "complete"),
    ] {
        records.push(label_row(
            &format!("lbl_{j}"),
            &format!("jdg_{j}"),
            "work_state",
            json!(label),
        ));
    }
    for (j, question, label) in extra {
        records.push(label_row(
            &format!("lbl_{j}_{question}"),
            &format!("jdg_{j}"),
            question,
            label.clone(),
        ));
    }
    c.store.append(&records).unwrap();
    let rep = c.run_learning(now, "on_demand", |_| {}).unwrap();
    format!("rpt_{}_loop.v1", rep.date)
}

/// The fake answers work_state per state: under the reworded criterion,
/// heron's progressing (fixed) and wren's complete (broken).
fn scripted_fake() -> FakeJev {
    let jev = FakeJev::start().unwrap();
    let pick = |option: &str| Jev::Choice {
        option: option.into(),
        confidence: 0.95,
    };
    jev.script_when("herons", Some("Plainly"), "work_state", pick("progressing"));
    jev.script_when("wrens", Some("Plainly"), "work_state", pick("complete"));
    jev.script("work_state", pick("complete"));
    jev
}

fn params(report: &str) -> JudgeReplayParams {
    JudgeReplayParams {
        pack_text: Some(reworded()),
        report: Some(report.into()),
        ..Default::default()
    }
}

/// A replay over a frozen holdout: every judgment asked again with its
/// stored state, both sides graded by the report's frozen labels, the
/// incumbent's numbers the report's own, heron fixed and wren broken; its
/// calls and its run in `judge.replay:loop`, never in the scope the report
/// reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replay_over_a_frozen_holdout() {
    let jev = scripted_fake();
    let r = rig(texts(0), &jev, |_| {});
    let c = &r.core;
    let report = seeded(c);
    let stored: theseus_protocol::learning::PackReport = serde_json::from_value(
        c.store
            .ledger_by_key(&report)
            .unwrap()
            .unwrap()
            .decode::<LedgerRow>()
            .unwrap()
            .data["report"]
            .clone(),
    )
    .unwrap();
    assert_eq!(stored.holdout.judgments.len(), 4);
    let out: JudgeReplayResult = c.judge_replay(params(&report), "cli").await.unwrap();
    assert_eq!(
        (
            out.candidate.as_str(),
            out.incumbent.as_str(),
            out.change.as_str()
        ),
        ("loop.v2", "loop.v1", "asks")
    );
    assert_eq!(
        (out.set.as_str(), out.labels.as_str(), out.judgments),
        ("holdout", "frozen", 4)
    );
    assert_eq!(
        (out.called, out.stored, out.rebuilt, out.rebanded),
        (4, 4, 0, 0)
    );
    assert!(out.left_out.is_empty(), "{:?}", out.left_out);
    // The incumbent's side is the report's own numbers on its holdout.
    assert_eq!(out.incumbent_report.questions, stored.holdout.questions);
    let fixed: Vec<(&str, &[String], &[String])> = out
        .per_judgment
        .iter()
        .map(|j| (j.judgment.as_str(), &j.fixed[..], &j.broken[..]))
        .collect();
    assert!(fixed.contains(&("jdg_heron", &["work_state".to_string()][..], &[][..])));
    assert!(fixed.contains(&("jdg_wren", &[][..], &["work_state".to_string()][..])));
    assert_eq!((out.fixed, out.broken), (1, 1));
    // Each call sent the candidate's criterion and the judgment's own state.
    let seen = jev.seen();
    assert_eq!(seen.len(), 4);
    assert!(seen.iter().all(|s| s.body.to_string().contains("Plainly")));
    // The rows: four calls and the run, in the replay's scope; nothing new
    // where the report reads.
    let calls = rows(c, "judge.replay:loop", LedgerKind::JudgeCall);
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|r| r.data["budget"] == "replay"
        && r.data["context"]["purpose"] == "replay"
        && r.data["context"]["run"] == json!(out.id)
        && r.data["pack"] == "loop.v2"));
    let runs = rows(c, "judge.replay:loop", LedgerKind::JudgeReplay);
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].data["candidate_sha256"],
        json!(out.candidate_sha256)
    );
    assert!(c
        .store
        .blobs()
        .base64(runs[0].data["text_blob"].as_str().unwrap())
        .is_some());
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeCall).len(), 4);
    let again = c
        .run_learning(theseus_protocol::now_unix_ms(), "on_demand", |_| {})
        .unwrap();
    let packs: Vec<(&str, u32)> = again
        .packs
        .iter()
        .map(|p| (p.pack.as_str(), p.calls))
        .collect();
    assert_eq!(packs, [("loop.v1", 4)], "the report never counts a replay");
    // Only the incumbent's errors: heron alone.
    let errs = c
        .judge_replay(
            JudgeReplayParams {
                errors: true,
                ..params(&report)
            },
            "cli",
        )
        .await
        .unwrap();
    assert_eq!(errs.judgments, 1);
    assert_eq!(errs.per_judgment[0].judgment, "jdg_heron");
    assert_eq!(errs.fixed, 1);
}

/// A yes-or-no answer is right when its lean meets the label (theseus-m5az):
/// gull's incumbent leaned no (0.2) on `announced_unfinished`, labeled yes,
/// so it is an error, which `errors: true` selects, and the candidate
/// leaning yes (0.8) fixes it; tern's, leaning no on a no label, is right
/// on both sides.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_noul_that_leaned_against_its_label_is_an_error_the_candidate_fixes() {
    let jev = scripted_fake();
    jev.script_when("gulls", None, "announced_unfinished", Jev::Noul(0.8));
    jev.script("announced_unfinished", Jev::Noul(0.2));
    let r = rig(texts(0), &jev, |_| {});
    let c = &r.core;
    let report = seeded_with(
        c,
        &[
            ("gull", "announced_unfinished", json!(true)),
            ("tern", "announced_unfinished", json!(false)),
        ],
    );
    let out = c.judge_replay(params(&report), "cli").await.unwrap();
    let of = |id: &str| {
        let j = out.per_judgment.iter().find(|j| j.judgment == id).unwrap();
        (j.fixed.clone(), j.broken.clone())
    };
    let (none, au) = (
        Vec::<String>::new(),
        vec!["announced_unfinished".to_string()],
    );
    assert_eq!(of("jdg_gull"), (au, none.clone()));
    assert_eq!(of("jdg_tern"), (none.clone(), none));
    // heron's work_state fixed and wren's broken, as before; gull's Noul fixed.
    assert_eq!((out.fixed, out.broken), (2, 1));
    let errs = c
        .judge_replay(
            JudgeReplayParams {
                errors: true,
                ..params(&report)
            },
            "cli",
        )
        .await
        .unwrap();
    let mut ids: Vec<&str> = errs
        .per_judgment
        .iter()
        .map(|j| j.judgment.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!((errs.judgments, ids), (2, vec!["jdg_gull", "jdg_heron"]));
    assert_eq!(errs.fixed, 2);
}

/// A candidate that changes only a threshold makes no call: the stored
/// answers are re-banded under its thresholds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thresholds_only_candidate_makes_no_call() {
    let jev = scripted_fake();
    let r = rig(texts(0), &jev, |_| {});
    let c = &r.core;
    let report = seeded(c);
    let lowered =
        LOOP_V1
            .replacen("version = 1", "version = 2", 1)
            .replacen("act = 0.90", "act = 0.75", 1);
    let out = c
        .judge_replay(
            JudgeReplayParams {
                pack_text: Some(lowered),
                report: Some(report),
                ..Default::default()
            },
            "cli",
        )
        .await
        .unwrap();
    assert_eq!(out.change, "thresholds_only");
    assert_eq!((out.called, out.rebanded), (0, 4));
    assert_eq!(jev.seen().len(), 0, "no call");
    assert_eq!(out.cost_usd, 0.0);
    let act = |r: &theseus_protocol::learning::PackReport| {
        r.questions
            .iter()
            .find(|q| q.question == "work_state")
            .unwrap()
            .bands
            .iter()
            .find(|b| b.band == "act")
            .unwrap()
            .n
    };
    // At 0.8 confidence: confirm under 0.90, act under 0.75.
    assert_eq!(
        (act(&out.incumbent_report), act(&out.candidate_report)),
        (0, 4)
    );
    assert_eq!((out.fixed, out.broken), (0, 0));
    assert!(rows(c, "judge.replay:loop", LedgerKind::JudgeCall).is_empty());
}

/// A judgment whose builder version differs from the candidate's is asked
/// with its state rebuilt from the turn's record; a gate judgment, whose
/// input can't be rebuilt, is left out with the reason; and a security
/// candidate runs the planted-injection set beside its incumbent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_state_is_rebuilt_when_the_builder_changed_and_left_out_when_it_cant_be() {
    let jev = scripted_fake();
    let r = rig(texts(2), &jev, |_| {});
    let c = &r.core;
    let res = turn(c, None, "Draft the release notes for the herons.").await;
    let (mut state, _) = loop_state(c, "anything");
    state.builder_version = 0;
    let j = judgment(
        "jdg_old_builder",
        "loop.v1",
        &state,
        vec![choice("complete", 0.8)],
        json!({"session": res.session_id, "turn": res.turn_id, "class": "reply",
            "decision": "no_tool_calls", "blob": "gone"}),
    );
    c.store.append(&[call_row(&j, 10)]).unwrap();
    let out = c
        .judge_replay(
            JudgeReplayParams {
                pack_text: Some(reworded()),
                judgments: vec!["jdg_old_builder".into()],
                ..Default::default()
            },
            "cli",
        )
        .await
        .unwrap();
    assert!(out.left_out.is_empty(), "{:?}", out.left_out);
    assert_eq!((out.stored, out.rebuilt), (0, 1));
    // The rebuilt state is what loop.v1's builder makes of the turn.
    let sent = jev.seen()[0].body["state"].to_string();
    assert!(
        sent.contains("Draft the release notes for the herons."),
        "{sent}"
    );
    assert!(sent.contains("Done: answer 0."), "{sent}");
    // A gate judgment for security.v2, whose builder differs: left out.
    let mut sec = state.clone();
    sec.builder = "security".into();
    sec.builder_version = 1;
    let g = judgment(
        "jdg_gate",
        "security.v1",
        &sec,
        vec![noul("risky", 0.3)],
        json!({"session": res.session_id, "blob": "gone"}),
    );
    c.store.append(&[call_row(&g, 11)]).unwrap();
    let before = jev.seen().len();
    let out = c
        .judge_replay(
            JudgeReplayParams {
                candidate: Some("security.v2".into()),
                judgments: vec!["jdg_gate".into()],
                ..Default::default()
            },
            "cli",
        )
        .await
        .unwrap();
    assert_eq!(out.called, 0);
    assert_eq!(out.left_out.len(), 1);
    let why = &out.left_out[0].reason;
    assert!(
        why.contains("built by security, the candidate's by security2")
            && why.contains("can't be rebuilt: a gate state's input"),
        "{why}"
    );
    let eval = out.eval.expect("a security candidate runs the planted set");
    assert!(eval.cases > 0);
    assert_eq!(
        (eval.incumbent.pack.as_str(), eval.candidate.pack.as_str()),
        ("security.v1", "security.v2")
    );
    assert!(eval.candidate.met + eval.candidate.missed > 0);
    assert_eq!(jev.seen().len() - before, 2 * eval.cases as usize);
}

/// The run's estimate is checked first against `[judge] replay_limit_usd`:
/// past it, the replay is refused with the numbers, and nothing is sent or
/// written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replay_past_its_limit_is_refused_with_the_numbers() {
    let jev = scripted_fake();
    let r = rig(texts(0), &jev, |c| c.judge.replay_limit_usd = 0.000_01);
    let c = &r.core;
    let report = seeded(c);
    let e = c
        .judge_replay(params(&report), "cli")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("over 4 judgments would reserve $")
            && e.contains("past [judge] replay_limit_usd ($0.00001); nothing was sent"),
        "{e}"
    );
    assert!(jev.seen().is_empty());
    assert!(rows(c, "judge.replay:loop", LedgerKind::JudgeReplay).is_empty());
}

/// Every refusal: from a shared place (its refusal ledgered), a wired
/// version (by name, or a pack file under its name), another pack's
/// version, and a name a replay already holds under another text.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replay_is_the_owners_and_a_name_means_one_text() {
    let jev = scripted_fake();
    let r = rig(texts(0), &jev, |_| {});
    let c = &r.core;
    let report = seeded(c);
    let stranger = Answerer {
        label: "discord".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "42".into(),
            channel_id: "1001".into(),
            guild_id: Some("7".into()),
        }),
    };
    let e = c.judge_replay(params(&report), stranger).await.unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    let refused = c.store.ledger_tail::<LedgerRow>(50).unwrap();
    assert!(refused
        .iter()
        .any(|(_, r)| r.kind == "approval.refused" && r.data["act"] == "judge.replay"));
    assert!(jev.seen().is_empty());
    let refuse = |p: JudgeReplayParams| {
        let c = c.clone();
        async move { c.judge_replay(p, "cli").await.unwrap_err().to_string() }
    };
    let wired = refuse(JudgeReplayParams {
        candidate: Some("loop.v1".into()),
        report: Some(report.clone()),
        ..Default::default()
    })
    .await;
    assert!(wired.contains("loop.v1 is wired"), "{wired}");
    let other = refuse(JudgeReplayParams {
        candidate: Some("security.v2".into()),
        report: Some(report.clone()),
        ..Default::default()
    })
    .await;
    assert!(other.contains("is not a version of loop"), "{other}");
    let posing = LOOP_V1.replace("act = 0.90", "act = 0.95");
    let posing = refuse(JudgeReplayParams {
        pack_text: Some(posing),
        report: Some(report.clone()),
        ..Default::default()
    })
    .await;
    assert!(posing.contains("loop.v1 is wired"), "{posing}");
    c.judge_replay(params(&report), "cli").await.unwrap();
    let twin = reworded().replace("Plainly, ", "Simply, ");
    let twin = refuse(JudgeReplayParams {
        pack_text: Some(twin),
        report: Some(report),
        ..Default::default()
    })
    .await;
    assert!(
        twin.contains("loop.v2 is already recorded under another text"),
        "{twin}"
    );
}
