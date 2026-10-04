//! Per-item answers in the learning ledger (M6 32d): the operator labels a
//! rerank's answer about one note by its own question (`helps.3`), the row
//! keeps the item's key, a question the judgment did not ask is refused,
//! the report grades every item's answer as a Noul by definition, and the
//! owner's memory label writes a system label once, which an operator's
//! label beats.

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::learning::{JudgeLabelParams, PackReport, QuestionReport};
use theseus_protocol::memory::MemoryLabelParams;

use crate::ledger::LedgerRow;
use crate::rpc::Core;
use crate::tests_rerank::{heron_rig, recalls, turn, until_reranked};

fn label(c: &Core, jdg: &str, q: Option<&str>, l: Value) -> anyhow::Result<String> {
    c.judge_label(
        &JudgeLabelParams {
            judgment: jdg.into(),
            question: q.map(str::to_string),
            label: l,
            note: None,
        },
        "cli",
    )
    .map(|r| r.about.unwrap_or_default())
}

fn memory_label(c: &Core, node: &str, word: &str, recall: Option<&str>) {
    c.memory_label(
        &MemoryLabelParams {
            node_id: node.into(),
            label: word.into(),
            recall_id: recall.map(str::to_string),
            note: None,
        },
        "cli",
    )
    .unwrap();
}

fn rerank_report(c: &Core) -> (PackReport, u32) {
    let r = c
        .run_learning(theseus_protocol::now_unix_ms(), "on_demand", |_| {})
        .unwrap();
    let p = r
        .packs
        .into_iter()
        .find(|p| p.pack == "rerank.v1")
        .expect("rerank.v1's report");
    (p, r.labels.system_written)
}

fn helps(p: &PackReport) -> &QuestionReport {
    p.questions
        .iter()
        .find(|q| q.question == "helps")
        .expect("the per-item definition's report")
}

fn label_rows(c: &Core) -> Vec<LedgerRow> {
    c.store
        .scope_after("judge:rerank", 0)
        .unwrap()
        .iter()
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == "judge.label")
        .collect()
}

/// A shadow rerank of three notes: the heron's third, which Jev says helps.
async fn reranked() -> (crate::tests_rerank::Rig, String, Vec<String>, String) {
    let jev = FakeJev::start().unwrap();
    jev.script("helps.1", Jev::Noul(0.10));
    jev.script("helps.2", Jev::Noul(0.30));
    jev.script("helps.3", Jev::Noul(0.97));
    let (r, here, order) = heron_rig(&jev, |_| {});
    turn(&r.core, &here, "Where does the grey heron nest?").await;
    let rows = until_reranked(&r.core.store, 1).await;
    let jdg = rows[0].data["id"].as_str().unwrap().to_string();
    // The fake's server goes with `jev`: every call has been made.
    (r, here, order, jdg)
}

/// `judge.label <jdg> true --question helps.3`: written with the item's
/// key; a question the judgment did not ask, the definition itself, or a
/// label that is not a Noul's, refused; and the report grades each item's
/// answer as a Noul under its definition.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_per_item_answer_takes_its_own_label_and_the_report_grades_it() {
    let (r, _, order, jdg) = reranked().await;
    let c = &r.core;
    let heron = c.store.session_nodes(&order[2]).unwrap()[0].1.id.clone();
    let about = label(c, &jdg, Some("helps.3"), json!("true")).unwrap();
    assert_eq!(about, format!("{heron}#0"));
    let rows = label_rows(c);
    assert_eq!(rows.len(), 1);
    let d = &rows[0].data;
    assert_eq!(
        (d["question"].as_str(), d["about"].as_str(), &d["label"]),
        (Some("helps.3"), Some(about.as_str()), &json!(true))
    );
    for (q, l, why) in [
        ("helps.7", json!("true"), "did not ask helps.7"),
        ("helps_more.1", json!("true"), "did not ask helps_more.1"),
        ("helps", json!("true"), "did not ask helps"),
        ("helps.2", json!("maybe"), "yes-or-no"),
    ] {
        let e = label(c, &jdg, Some(q), l).unwrap_err();
        assert!(format!("{e:#}").contains(why), "{q}: {e:#}");
    }
    assert_eq!(label_rows(c).len(), 1, "a refusal writes nothing");
    let (p, _) = rerank_report(c);
    let h = helps(&p);
    assert_eq!((h.kind.as_str(), h.answered, h.labeled), ("noul", 3, 1));
    let cal = h.calibration.as_ref().expect("graded");
    assert_eq!(cal.n, 1);
    assert!((cal.brier - 0.03f64.powi(2)).abs() < 1e-9, "{}", cal.brier);
    assert_eq!(p.labeled, 1, "the judgment counts as labeled");
    // A second label on another item, false: two graded.
    label(c, &jdg, Some("helps.1"), json!(false)).unwrap();
    let (p, _) = rerank_report(c);
    assert_eq!(helps(&p).labeled, 2);
}

/// The owner's `useful` on a recalled note, naming its recall, makes that
/// recall's rerank answer about it `true`: a system label at 0.5, keyed
/// with its item, written once. A `wrong` naming no recall grades the
/// newest rerank before it that asked about the node, `false`. An
/// operator's label on the same question (1.0) beats the system's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_memory_label_writes_a_system_label_once_and_the_owners_beats_it() {
    let (r, here, order, jdg) = reranked().await;
    let c = &r.core;
    let node = |i: usize| c.store.session_nodes(&order[i]).unwrap()[0].1.id.clone();
    let recall = recalls(c, &here)[0].recall_id.clone();
    memory_label(c, &node(2), "useful", Some(&recall));
    memory_label(c, &node(0), "wrong", None);
    let (p, written) = rerank_report(c);
    assert_eq!(written, 2);
    let mut rows = label_rows(c);
    rows.sort_by_key(|r| r.data["question"].as_str().unwrap_or_default().to_string());
    let got: Vec<(&str, &str, &Value, f64, &str)> = rows
        .iter()
        .map(|r| {
            (
                r.data["question"].as_str().unwrap(),
                r.data["about"].as_str().unwrap(),
                &r.data["label"],
                r.data["weight"].as_f64().unwrap(),
                r.data["rule"].as_str().unwrap(),
            )
        })
        .collect();
    let (k0, k2) = (format!("{}#0", node(0)), format!("{}#0", node(2)));
    assert_eq!(
        got,
        [
            ("helps.1", k0.as_str(), &json!(false), 0.5, "memory_label"),
            ("helps.3", k2.as_str(), &json!(true), 0.5, "memory_label"),
        ]
    );
    assert!(rows.iter().all(|r| r.data["judgment"] == jdg.as_str()));
    let h = helps(&p);
    assert_eq!(h.labeled, 2);
    // Written once: a second run writes none.
    let (_, written) = rerank_report(c);
    assert_eq!(written, 0);
    assert_eq!(label_rows(c).len(), 2);
    // The owner says the heron's note did not help: 1.0 beats 0.5.
    label(c, &jdg, Some("helps.3"), json!(false)).unwrap();
    let (p, _) = rerank_report(c);
    let cal = helps(&p).calibration.clone().unwrap();
    // helps.1 at 0.10, false; helps.3 at 0.97, now false.
    let want = (0.10f64.powi(2) + 0.97f64.powi(2)) / 2.0;
    assert!(
        (cal.brier - want).abs() < 1e-9,
        "{} against {want}",
        cal.brier
    );
}
