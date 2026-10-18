//! A mint's reach (theseus-pw1q.21.8): a flow whose deploy or retire is on
//! the record is never deployed again, and a mint never takes the name of a
//! tool the owner loaded.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::rsi::{SelfMode, SelfResumeParams};

use super::kind::Kind;
use super::ValidateReport;
use crate::approval::Answerer;
use crate::config::Config;
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;

const DAY: u64 = 86_400_000;

fn acting() -> Arc<Core> {
    let dir = std::env::temp_dir().join(format!("theseus-owned-{}", crate::new_id("t")));
    let store = Store::open(&dir.join("store")).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.self_improve.mode = SelfMode::Act;
    let c = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap();
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    c
}

fn spec(name: &str) -> serde_json::Value {
    json!({
        "v": 1, "kind": "flow", "name": name,
        "description": "Counts the lines of the notes.",
        "scope": {"repos": ["r:abc123def456"], "places": ["cli"]},
        "inputs": {},
        "steps": [{"id": "count", "tool": "proc.run", "args": {"argv": ["wc", "-l", "notes.txt"]}}],
        "exits": {"ok": "counted"},
        "evidence": {"occurrences": 6, "sessions": 3, "days": 2, "usd_per_occ": 0.1}
    })
}

/// A passing report for `digest`, kept under `placeholder`.
fn keep_passing(c: &Core, placeholder: &str, digest: &str) {
    std::fs::create_dir_all(c.tools.flows.root().join(placeholder)).unwrap();
    let mut r = ValidateReport::assemble(("counts", Kind::Flow, digest), (None, None, None), true);
    r.pass = true;
    c.mint_keep_report(placeholder, &r).unwrap();
}

fn row(c: &Core, kind: &str, digest: &str, age_ms: u64) {
    let mut row = LedgerRow::named(
        kind,
        None,
        None,
        json!({"name": "counts", "digest": digest}),
    );
    row.at_unix_ms = theseus_protocol::now_unix_ms() - age_ms;
    c.store.append_ledger(&row).unwrap();
}

/// A passing report waits for a deploy; a deploy row 91 days old still holds
/// it back, and so does a retire row written as bare hex.
#[test]
fn a_flow_on_the_record_is_never_deployed_again() {
    let c = acting();
    keep_passing(&c, "mint_a", "sha256:aaaa");
    keep_passing(&c, "mint_b", "sha256:bbbb");
    keep_passing(&c, "mint_c", "sha256:cccc");
    assert_eq!(c.mint_waiting().unwrap().len(), 3);
    row(&c, "self.mint.deployed", "sha256:aaaa", 91 * DAY);
    row(&c, "self.mint.retired", "bbbb", DAY);
    assert_eq!(c.mint_waiting().unwrap(), ["mint_c"]);
    assert!(c.mint_deploy_waiting().unwrap().is_empty());
}

/// A mint refuses the name of a flow the owner loaded, and takes that of one
/// a mint loaded (its update).
#[test]
fn a_mint_does_not_replace_an_owners_flow() {
    let c = acting();
    for (name, by) in [("owned", "owner"), ("minted", "self")] {
        let path = std::env::temp_dir().join(format!("{name}-{}.flow.json", crate::new_id("f")));
        std::fs::write(&path, spec(name).to_string()).unwrap();
        c.flow_load(&path, by, "cli").unwrap();
    }
    let e = c.mint_not_owned(Kind::Flow, "owned").unwrap_err();
    assert!(format!("{e:#}").contains("the owner's card"), "{e:#}");
    assert!(c.mint_not_owned(Kind::Flow, "minted").is_ok());
    assert!(c.mint_not_owned(Kind::Flow, "unknown").is_ok());
    assert!(c.mint_not_owned(Kind::Mcp, "owned").is_ok());
}
