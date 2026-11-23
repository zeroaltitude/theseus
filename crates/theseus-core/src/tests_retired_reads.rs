//! A store from before issue 44 retired `role.v1` and `continue.v1`, the last
//! versions of their families: their judgments, labels, learned versions and
//! mode rows are all still read. What this build can RUN is the embedded
//! packs; what it READS is every family it ever had (`pack::readable_ids`).

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::judge::JudgeListParams;
use theseus_protocol::packs::PackModeRow;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use crate::judge::ladder::Rung;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, judge_config, texts};

fn core_at(dir: &Path) -> Arc<Core> {
    let mut cfg = judge_config(dir, None);
    cfg.judge.enabled = true;
    cfg.judge.api_base = "http://127.0.0.1:9".into();
    cfg.validate().unwrap();
    let store = Store::open(&dir.join("store")).unwrap();
    let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(texts(1))), store);
    p.secrets = board();
    Core::build(p).unwrap()
}

fn row(kind: LedgerKind, scope: &str, data: Value, at_ms: u64, key: &str) -> NewRecord {
    let mut row = LedgerRow::new(kind, Some("ses_heron"), None, data);
    row.at_unix_ms = at_ms;
    let mut r = NewRecord::json(kinds::LEDGER, None, &row).unwrap();
    r.key = Some(key.to_string());
    r.scoped(scope)
}

/// Each retired family's judgment (whole, as the judge wrote one) and its
/// label, and a learned `role` version, as an older build wrote them.
fn old_rows(c: &Core, now: u64) -> Vec<NewRecord> {
    let (state, _) = crate::tests_replay::loop_state(c, "anything");
    let mut out = Vec::new();
    for (pack, i) in [
        ("role.v1", 0u64),
        ("continue.v1", 1),
        ("recall_cross.v1", 2),
    ] {
        let id = format!("jdg_old_{i}");
        let j = crate::tests_replay::judgment(
            &id,
            pack,
            &state,
            Vec::new(),
            json!({"session": "ses_heron"}),
        );
        out.push(crate::tests_replay::call_row(&j, now - 2_000 - i));
        let scope = crate::rpc::judge::scope_of(pack);
        let label = json!({"id": format!("lbl_old_{i}"), "judgment": id, "pack": pack,
            "label": "right", "source": "operator"});
        out.push(row(
            LedgerKind::JudgeLabel,
            &scope,
            label,
            now - 1_000 - i,
            &format!("lbl_old_{i}"),
        ));
    }
    let text = include_str!("../../theseus-judge/packs/retired/role.v1.toml").replacen(
        "version = 1",
        "version = 101",
        1,
    );
    let learned = json!({"name": "role.v101", "text": text, "parent": "role.v1",
        "root": "role.v1", "proposal": "prop_old"});
    out.push(row(
        LedgerKind::PackVersion,
        &crate::judge::lineage::scope("role.v1"),
        learned,
        now - 500,
        &crate::judge::lineage::key("role.v101"),
    ));
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retired_familys_records_are_still_listed_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let now = theseus_protocol::now_unix_ms();
    let c = core_at(dir.path());
    c.store.append(&old_rows(&c, now)).unwrap();
    c.runner
        .judge
        .ladder()
        .write(PackModeRow {
            pack: "role.v1".into(),
            mode: Rung::Shadow.as_str().into(),
            from: Rung::Off.as_str().into(),
            who: "owner".into(),
            by: "owner".into(),
            via: "promote".into(),
            why: "an older build's row".into(),
            ..PackModeRow::default()
        })
        .unwrap();
    // And the cross judge's, retired with its service (XD3): its day brake still reads the row.
    c.runner
        .judge
        .ladder()
        .write(PackModeRow {
            pack: "recall_cross.v1".into(),
            mode: Rung::Live.as_str().into(),
            from: Rung::Shadow.as_str().into(),
            who: "owner".into(),
            by: "owner".into(),
            via: "promote".into(),
            why: "an older build's row".into(),
            ..PackModeRow::default()
        })
        .unwrap();
    drop(c);
    // The upgrade: a fresh start on the store.
    let c = core_at(dir.path());
    // judge.list, by default, lists both families' judgments.
    let listed = c.judge_list(JudgeListParams::default()).unwrap();
    let packs: Vec<&str> = listed
        .judgments
        .iter()
        .filter_map(|e| e.data["pack"].as_str())
        .collect();
    for pack in ["role.v1", "continue.v1", "recall_cross.v1"] {
        assert!(packs.contains(&pack), "{pack}: {packs:?}");
    }
    // The learning report, run now, reports both.
    let report = c.learning_report(Value::Null).await.unwrap();
    let reported: Vec<&str> = report.packs.iter().map(|p| p.pack.as_str()).collect();
    for pack in ["role.v1", "continue.v1", "recall_cross.v1"] {
        assert!(reported.contains(&pack), "{pack}: {reported:?}");
    }
    // The learned version is in its lineage.
    let learned = crate::judge::lineage::read(&c.store).unwrap();
    assert!(learned.contains_key("role.v101"), "{:?}", learned.keys());
    // The mode row is on the ladder after the restart.
    let rows = c.runner.judge.ladder().rows_of("role.v1");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let cross = c.runner.judge.ladder().rows_of("recall_cross.v1");
    assert_eq!(cross.len(), 1, "{cross:?}");
    // And nothing can run them.
    assert!(theseus_judge::pack::by_name("role.v1").is_none());
    assert!(theseus_judge::pack::by_name("continue.v1").is_none());
    assert!(theseus_judge::pack::by_name("recall_cross.v1").is_none());
}

/// A `judge.report` row as an older build wrote it for `pack` on `date` (`Core::write_run`'s shape).
fn report_row(date: &str, pack: &str, at_ms: u64) -> NewRecord {
    let labels = theseus_protocol::learning::LabelCounts::default();
    let p = theseus_protocol::learning::PackReport {
        pack: pack.into(),
        calls: 3,
        ..Default::default()
    };
    let f = crate::fact::judge::JudgeReport {
        date,
        trigger: "nightly",
        at_unix_ms: at_ms,
        labels: &labels,
        pack: &p,
    };
    let mut rec = crate::fact::row(&f, None, None).unwrap();
    rec.key = Some(crate::rpc::learning::report_key(date, pack));
    rec.scoped(&crate::rpc::judge::scope_of(pack))
}

fn packs_of(r: &theseus_protocol::learning::LearningReport) -> Vec<&str> {
    r.packs.iter().map(|p| p.pack.as_str()).collect()
}

/// A dated learning report reads a retired version's stored row (XD3 round 2): alone, it is the day's
/// report, not "no learning report is stored"; beside a live pack's, both are in it, and the day's file,
/// missing, is written again whole. `role.v1` was retired before `recall_cross.v1`, by the same catalog.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dated_report_reads_a_retired_versions_stored_row() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path());
    c.store
        .append(&[report_row("2026-10-01", "recall_cross.v1", 10)])
        .unwrap();
    let alone = c
        .learning_report(json!({"date": "2026-10-01"}))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(packs_of(&alone), ["recall_cross.v1"]);
    c.store
        .append(&[
            report_row("2026-10-02", "loop.v1", 20),
            report_row("2026-10-02", "role.v1", 21),
        ])
        .unwrap();
    let file = c.cfg.state_dir().join("learning").join("2026-10-02.json");
    let _ = std::fs::remove_file(&file);
    let beside = c
        .learning_report(json!({"date": "2026-10-02"}))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut got = packs_of(&beside);
    got.sort_unstable();
    assert_eq!(got, ["loop.v1", "role.v1"]);
    let written: theseus_protocol::learning::LearningReport =
        serde_json::from_slice(&std::fs::read(&file).expect("the day's file is written again"))
            .unwrap();
    let mut filed = packs_of(&written);
    filed.sort_unstable();
    assert_eq!(
        filed,
        ["loop.v1", "role.v1"],
        "the file is the whole report"
    );
}

/// The memory pass's shadow packs, retired by D8a, keep their dated reports: a stored `memory.v1` day is
/// the day's report on its own, and an `attribution.v1` row is read beside a live pack's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dated_report_reads_the_memory_passs_retired_packs() {
    let dir = tempfile::tempdir().unwrap();
    let c = core_at(dir.path());
    c.store
        .append(&[report_row("2026-10-05", "memory.v1", 30)])
        .unwrap();
    let alone = c
        .learning_report(json!({"date": "2026-10-05"}))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(packs_of(&alone), ["memory.v1"]);
    c.store
        .append(&[
            report_row("2026-10-06", "loop.v1", 40),
            report_row("2026-10-06", "attribution.v1", 41),
        ])
        .unwrap();
    let beside = c
        .learning_report(json!({"date": "2026-10-06"}))
        .await
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut got = packs_of(&beside);
    got.sort_unstable();
    assert_eq!(got, ["attribution.v1", "loop.v1"]);
}
