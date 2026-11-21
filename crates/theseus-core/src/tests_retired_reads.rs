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

/// Each retired family's judgment and its label, and a learned `role`
/// version, as an older build wrote them.
fn old_rows(now: u64) -> Vec<NewRecord> {
    let mut out = Vec::new();
    for (pack, q, i) in [("role.v1", "role", 0u64), ("continue.v1", "decision", 1)] {
        let id = format!("jdg_old_{i}");
        let scope = crate::rpc::judge::scope_of(pack);
        let data = json!({"id": id, "pack": pack, "version": 1, "mode": "shadow",
            "outcome": {"outcome": "answered"}, "context": {"session": "ses_heron"},
            "answers": [{"question": q, "answer": {"choice": "other", "confidence": 0.8}}]});
        out.push(row(
            LedgerKind::JudgeCall,
            &scope,
            data,
            now - 2_000 - i,
            &id,
        ));
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
    c.store.append(&old_rows(now)).unwrap();
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
    for pack in ["role.v1", "continue.v1"] {
        assert!(packs.contains(&pack), "{pack}: {packs:?}");
    }
    // The learning report, run now, reports both.
    let report = c.learning_report(Value::Null).await.unwrap();
    let reported: Vec<&str> = report.packs.iter().map(|p| p.pack.as_str()).collect();
    for pack in ["role.v1", "continue.v1"] {
        assert!(reported.contains(&pack), "{pack}: {reported:?}");
    }
    // The learned version is in its lineage.
    let learned = crate::judge::lineage::read(&c.store).unwrap();
    assert!(learned.contains_key("role.v101"), "{:?}", learned.keys());
    // The mode row is on the ladder after the restart.
    let rows = c.runner.judge.ladder().rows_of("role.v1");
    assert_eq!(rows.len(), 1, "{rows:?}");
    // And nothing can run them.
    assert!(theseus_judge::pack::by_name("role.v1").is_none());
    assert!(theseus_judge::pack::by_name("continue.v1").is_none());
}
