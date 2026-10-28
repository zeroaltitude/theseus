//! Graduation (theseus-pw1q.13), through the core: a loaded extension called
//! enough times with no failure becomes a self item to port it into core.
//! - `extension_usage` counts planted tool-call rows: calls, a failure, the
//!   latency, only this extension's, only inside the window;
//! - the rule fires at 20 calls and not at 19, not with one failure, not
//!   twice for one digest, not for a revoked extension, not with the mode
//!   off or the switch halted, and not on a read that missed rows;
//! - a network extension's item is keel-marked and never starts;
//! - the brief carries the source, the schemas and the declared tests;
//! - the item starts as any item does, one `"self"` session;
//! - the revoke after the join says `graduated`, and only for a filed port.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::rsi::{SelfBacklogParams, SelfHaltParams, SelfMode, SelfResumeParams};
use theseus_protocol::tasks::TaskState;

use super::tests::test_core;
use super::{Core, Parts};
use crate::approval::Answerer;
use crate::extend::load::{Loaded, LoadedSet};
use crate::extend::{Capabilities, Expect, Manifest, ProposedBy, TestRun, TestSpec, ToolInfo};
use crate::ledger::LedgerRow;
use crate::provider::FakeProvider;
use crate::test_support::rows;

const DAY: u64 = 86_400_000;
const HOUR: u64 = 3_600_000;
const DIGEST: &str = "3f2a1c9e0b7d5a4c8e1122334455667788990011";

fn core_with(mode: SelfMode, starts: u32) -> Arc<Core> {
    let base = test_core("the tide is out");
    let mut cfg = (*base.cfg).clone();
    cfg.self_improve.mode = mode;
    cfg.server.disk_warn_mb = 0;
    cfg.self_improve.backlog_starts = Some(starts);
    // A started item opens a tree (theseus-qh2tt): a throwaway repo, kept
    // for the process.
    let repo = crate::rsi::tests_tree::fixture();
    cfg.self_improve.repo = Some(repo.place.repo.display().to_string());
    std::mem::forget(repo);
    let core = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        base.store.clone(),
    ))
    .unwrap();
    core.tools
        .self_pr
        .set_env(crate::rsi::tests_tree::hermetic());
    core
}

fn cli() -> Answerer {
    Answerer::from("the CLI")
}

fn acting(starts: u32) -> Arc<Core> {
    let c = core_with(SelfMode::Act, starts);
    c.self_resume(&SelfResumeParams::default(), &cli()).unwrap();
    c
}

/// A call's row, `ago` ms before `at`: `ok` or not, in `ms`.
fn call_row(c: &Core, tool: &str, ok: bool, ms: u64, at: u64) {
    let kind = if ok {
        "action.succeeded"
    } else {
        "action.failed"
    };
    let mut row = LedgerRow::named(
        kind,
        None,
        None,
        json!({"correlation_id": format!("c{at}{ms}"), "producer": format!("async:mcp:{tool}"),
               "duration_ms": ms, "outcome": if ok { "succeeded" } else { "failed" }}),
    );
    row.at_unix_ms = at;
    c.store.append_ledger(&row).unwrap();
}

/// `n` working calls of `ext-wc/count`, hours apart, ending an hour ago.
fn calls(c: &Core, n: u64, now: u64) {
    for i in 0..n {
        call_row(c, "ext-wc/count", true, 10 + i, now - (i + 1) * HOUR);
    }
}

fn loaded(name: &str, digest: &str, network: &[&str], acked_at: u64) -> Loaded {
    Loaded {
        name: name.into(),
        digest: digest.into(),
        description: "counts words".into(),
        command: vec!["python3".into(), "server.py".into()],
        frozen: format!("/state/extensions/{name}/{digest}"),
        source: format!("/work/{name}"),
        tools: vec!["count".into()],
        capabilities: Capabilities {
            network: network.iter().map(|h| h.to_string()).collect(),
            hands: None,
            scratch: true,
        },
        acked_by: "cli".into(),
        acked_via: "cli".into(),
        acked_at_ms: acked_at,
        question: "q1".into(),
        proposed_by: ProposedBy::default(),
        place_name: None,
        ceiling: None,
        replaced: None,
    }
}

fn manifest(l: &Loaded) -> Manifest {
    Manifest {
        name: l.name.clone(),
        digest: l.digest.clone(),
        description: l.description.clone(),
        command: l.command.clone(),
        source: l.source.clone(),
        frozen: l.frozen.clone(),
        files: 2,
        bytes: 300,
        tools: vec![ToolInfo {
            name: "count".into(),
            description: Some("counts".into()),
            input_schema: json!({"type": "object", "properties": {"text": {"type": "string"}}}),
        }],
        tests: vec![TestRun {
            spec: TestSpec {
                tool: "count".into(),
                arguments: json!({"text": "three words here"}),
                expect: Expect::Contains("3".into()),
            },
            passed: true,
            got: Some("3".into()),
            why: None,
        }],
        error: None,
        capabilities: l.capabilities.clone(),
        proposed_by: l.proposed_by.clone(),
        proposed_at_ms: l.acked_at_ms,
        question: None,
        state: "acked".into(),
        answered_by: None,
        answered_at_ms: None,
        note: None,
    }
}

/// Load `l` the way the ack's frame leaves the store.
fn load(c: &Core, l: Loaded) {
    let mut set = LoadedSet::read(&c.store).unwrap();
    set.loaded.insert(l.name.clone(), l.clone());
    c.store
        .append(&[set.record().unwrap(), manifest(&l).record().unwrap()])
        .unwrap();
}

fn tick() -> u64 {
    theseus_protocol::now_unix_ms()
}

fn graduates(c: &Core) -> Vec<theseus_protocol::rsi::SelfBacklogItem> {
    c.self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items
        .into_iter()
        .filter(|i| i.cause.starts_with("graduate.ext"))
        .collect()
}

#[test]
fn usage_counts_calls_failures_and_latency_of_this_extension_in_the_window() {
    let c = acting(0);
    let t0 = tick();
    for (i, ms) in [10u64, 20, 30, 40, 50].into_iter().enumerate() {
        call_row(&c, "ext-wc/count", true, ms, t0 - (i as u64 + 1) * HOUR);
    }
    call_row(&c, "ext-wc/count", false, 100, t0 - 6 * HOUR);
    // Not this extension's: another extension, a configured server, a
    // built-in, and a call from before the window.
    call_row(&c, "ext-other/count", true, 5, t0 - HOUR);
    call_row(&c, "fake/echo", true, 5, t0 - HOUR);
    call_row(&c, "ext-wc/count", true, 7, t0 - 20 * DAY);
    let now = tick();
    let u = c.extension_usage("wc", now, 14).unwrap();
    assert_eq!((u.calls, u.failures), (6, 1), "{u:?}");
    assert_eq!((u.p50_ms, u.p95_ms), (30, 100), "{u:?}");
    assert_eq!(u.window_days, 14);
    assert!(!u.truncated);
    assert_eq!(
        u.line(),
        "used 6 times in 14 days, 1 failure, p50 30 ms, p95 100 ms"
    );
    let wide = c.extension_usage("wc", now, 30).unwrap();
    assert_eq!(wide.calls, 7, "the older call is in a wider window");
    assert_eq!(c.extension_usage("other", now, 14).unwrap().calls, 1);
    assert_eq!(c.extension_usage("fake", now, 14).unwrap().calls, 0);
}

#[test]
fn the_rule_fires_at_twenty_calls_and_not_at_nineteen() {
    let c = acting(0);
    let t0 = tick();
    load(&c, loaded("wc", DIGEST, &[], t0 - 10 * DAY));
    calls(&c, 19, t0);
    let n = c.backlog_night(tick()).unwrap();
    assert!(n.filed.is_empty() && graduates(&c).is_empty(), "{n:?}");
    call_row(&c, "ext-wc/count", true, 12, t0 - 30 * 60_000);
    let n = c.backlog_night(tick()).unwrap();
    assert_eq!(n.filed.len(), 1, "{n:?}");
    let items = graduates(&c);
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.cause, "graduate.ext:wc:3f2a1c9e0b7d");
    assert_eq!(item.state, "proposed");
    assert_eq!(item.keel, None);
    assert!(
        item.evidence.iter().any(|e| e.starts_with("graduate:wc:")),
        "{:?}",
        item.evidence
    );
    let task = crate::task_graph::get(&c.store, &item.id).unwrap().unwrap();
    assert_eq!(task.title, "graduate ext-wc");
    assert_eq!(task.origin.principal, "self");
    // The row: the counts, the digest, the window.
    let r = rows(&c, "self.graduate.proposed");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["extension"], "wc");
    assert_eq!(r[0]["digest"], DIGEST);
    assert_eq!(r[0]["calls"], 20);
    assert_eq!(r[0]["failures"], 0);
    assert_eq!(r[0]["window_days"], 14);
    assert_eq!(r[0]["item"], item.id.as_str());
}

#[test]
fn a_single_failure_files_nothing() {
    let c = acting(0);
    let t0 = tick();
    load(&c, loaded("wc", DIGEST, &[], t0 - 10 * DAY));
    calls(&c, 40, t0);
    call_row(&c, "ext-wc/count", false, 15, t0 - 90 * 60_000);
    // The failed call is a cause of its own for the backlog; the extension
    // is not proposed for a port.
    c.backlog_night(tick()).unwrap();
    assert!(graduates(&c).is_empty());
    assert!(rows(&c, "self.graduate.proposed").is_empty());
}

#[test]
fn calls_from_before_it_was_loaded_do_not_count_for_this_version() {
    let c = acting(0);
    let t0 = tick();
    // Loaded two hours ago: the calls of the day before were a version
    // that is gone.
    load(&c, loaded("wc", DIGEST, &[], t0 - 2 * HOUR));
    calls(&c, 30, t0);
    let n = c.backlog_night(tick()).unwrap();
    assert!(n.filed.is_empty(), "{n:?}");
}

#[test]
fn one_digest_is_proposed_once_and_a_new_digest_counts_afresh() {
    let c = acting(0);
    let t0 = tick();
    load(&c, loaded("wc", DIGEST, &[], t0 - 10 * DAY));
    calls(&c, 25, t0);
    let n1 = c.backlog_night(tick()).unwrap();
    assert_eq!(n1.filed.len(), 1);
    let id = graduates(&c)[0].id.clone();
    let again = c.backlog_night(tick() + DAY).unwrap();
    assert!(
        again.filed.is_empty() && again.updated.is_empty(),
        "{again:?}"
    );
    // Its item ends (done): it is still not proposed again.
    let mut t = crate::task_graph::get(&c.store, &id).unwrap().unwrap();
    t.state = TaskState::Done;
    t.version += 1;
    c.store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    let after = c.backlog_night(tick() + 2 * DAY).unwrap();
    assert!(after.filed.is_empty(), "{after:?}");
    assert_eq!(rows(&c, "self.graduate.proposed").len(), 1);
    // A new version is a new digest, and counts its own calls.
    let newer = "aa11bb22cc33dd44ee55ff66778899aabbccddee";
    load(&c, loaded("wc", newer, &[], tick() - 5 * HOUR));
    let n = c.backlog_night(tick() + 3 * DAY).unwrap();
    assert!(n.filed.is_empty(), "no calls of its own yet: {n:?}");
}

#[test]
fn a_revoked_extension_is_not_proposed() {
    let c = acting(0);
    let t0 = tick();
    load(&c, loaded("wc", DIGEST, &[], t0 - 10 * DAY));
    calls(&c, 25, t0);
    // The owner revokes it before the night: it leaves the loaded set.
    let mut set = LoadedSet::read(&c.store).unwrap();
    set.loaded.remove("wc");
    c.store.append(&[set.record().unwrap()]).unwrap();
    let n = c.backlog_night(tick()).unwrap();
    assert!(n.filed.is_empty(), "{n:?}");
    assert!(graduates(&c).is_empty());
}

#[test]
fn mode_off_and_a_halted_switch_file_and_write_nothing() {
    let t0 = tick();
    let setup = |c: &Core| {
        load(c, loaded("wc", DIGEST, &[], t0 - 10 * DAY));
        calls(c, 25, t0);
    };
    let off = core_with(SelfMode::Off, 1);
    setup(&off);
    let n = off.backlog_night(tick()).unwrap();
    assert_eq!(n.gate, "off");
    assert!(n.filed.is_empty() && n.started.is_empty());
    assert!(rows(&off, "self.graduate.proposed").is_empty());
    assert!(crate::task_graph::all(&off.store).unwrap().is_empty());

    let halted = acting(1);
    setup(&halted);
    halted
        .self_halt(&SelfHaltParams::default(), &cli())
        .unwrap();
    let n = halted.backlog_night(tick()).unwrap();
    assert_eq!(n.gate, "halted");
    assert!(n.filed.is_empty() && n.started.is_empty());
    assert!(rows(&halted, "self.graduate.proposed").is_empty());
    assert!(crate::task_graph::all(&halted.store).unwrap().is_empty());
}

#[test]
fn a_network_extensions_item_is_keel_marked_and_never_starts() {
    let c = acting(1);
    let t0 = tick();
    load(
        &c,
        loaded("feed", DIGEST, &["api.wordlist.invalid:443"], t0 - 10 * DAY),
    );
    for i in 0..22 {
        call_row(&c, "ext-feed/count", true, 30, t0 - (i + 1) * HOUR);
    }
    let n = c.backlog_night(tick()).unwrap();
    assert_eq!(n.filed.len(), 1, "{n:?}");
    assert!(n.started.is_empty(), "a keel item never starts: {n:?}");
    let item = &graduates(&c)[0];
    assert_eq!(item.cause, "graduate.ext.net:feed:3f2a1c9e0b7d");
    assert_eq!(item.state, "waiting_human");
    assert_eq!(item.keel.as_deref(), Some("security"));
    assert!(item.session.is_none());
    let r = rows(&c, "self.graduate.proposed");
    assert_eq!(r[0]["keel"], "security");
    // The brief says what the grant was.
    let t = crate::task_graph::get(&c.store, &item.id).unwrap().unwrap();
    assert!(
        t.objective.contains("network to api.wordlist.invalid:443"),
        "{}",
        t.objective
    );
}

#[test]
fn the_brief_carries_the_source_the_schemas_and_the_declared_tests() {
    let c = acting(0);
    let t0 = tick();
    let l = loaded("wc", DIGEST, &[], t0 - 10 * DAY);
    load(&c, l.clone());
    calls(&c, 21, t0);
    c.backlog_night(tick()).unwrap();
    let item = &graduates(&c)[0];
    let o = crate::task_graph::get(&c.store, &item.id)
        .unwrap()
        .unwrap()
        .objective;
    for part in [
        l.frozen.as_str(),
        DIGEST,
        "21 calls",
        "0 failures",
        "- count {\"properties\":{\"text\":{\"type\":\"string\"}},\"type\":\"object\"}",
        "- count {\"text\":\"three words here\"} contains \"3\"",
        "Grant: no network, no secret.",
        "as a Rust test beside the tool",
        "fails without the port",
        "revokes it with the reason graduated",
        "widens nothing the extension had",
    ] {
        assert!(o.contains(part), "missing {part:?} in:\n{o}");
    }
    assert!(
        o.contains(&format!("item: \"{}\"", item.id)),
        "the tool call names the item: {o}"
    );
    // The declared tests are those of the manifest the extension was loaded
    // with, as the trial ran them.
    let tests = rows(&c, "self.graduate.proposed");
    assert_eq!(tests.len(), 1);
}

#[test]
fn a_graduate_item_starts_in_one_self_session_a_night() {
    let c = acting(1);
    let t0 = tick();
    for (name, d) in [
        ("wc", DIGEST),
        ("tally", "bb22cc33dd44ee55ff66778899aabbccddeeff00"),
    ] {
        load(&c, loaded(name, d, &[], t0 - 10 * DAY));
        for i in 0..22 {
            call_row(
                &c,
                &format!("ext-{name}/count"),
                true,
                20,
                t0 - (i + 1) * HOUR,
            );
        }
    }
    let n = c.backlog_night(tick()).unwrap();
    assert_eq!(n.filed.len(), 2, "{n:?}");
    assert_eq!(n.started.len(), 1, "backlog_starts = 1: {n:?}");
    let started = &n.started[0];
    assert!(
        started.title.starts_with("graduate ext-"),
        "{}",
        started.title
    );
    assert!(started.objective.contains("self.propose_change"));
    let exec = c
        .kernel
        .open_executions()
        .unwrap()
        .into_iter()
        .find(|e| e.session_id == started.session_id)
        .expect("the item's session has an execution");
    assert_eq!(exec.authority.principal, "self");
}

#[test]
fn the_revoke_after_a_join_says_graduated_and_only_for_a_filed_port() {
    let c = acting(0);
    let t0 = tick();
    load(&c, loaded("wc", DIGEST, &[], t0 - 10 * DAY));
    load(
        &c,
        loaded(
            "lone",
            "cc33dd44ee55ff66778899aabbccddeeff001122",
            &[],
            t0 - 10 * DAY,
        ),
    );
    calls(&c, 25, t0);
    c.backlog_night(tick()).unwrap();
    // No port was filed for `lone`; a stale digest is not the loaded one.
    assert!(c
        .extension_revoke_graduated("lone", "cc33dd44ee55ff66778899aabbccddeeff001122", "abc123")
        .is_err());
    assert!(c
        .extension_revoke_graduated("wc", "ffff", "abc123")
        .is_err());
    assert_eq!(LoadedSet::read(&c.store).unwrap().loaded.len(), 2);
    // Halted: nothing is revoked.
    c.self_halt(&SelfHaltParams::default(), &cli()).unwrap();
    assert!(c
        .extension_revoke_graduated("wc", DIGEST, "abc123")
        .is_err());
    c.self_resume(&SelfResumeParams::default(), &cli()).unwrap();
    let done = c
        .extension_revoke_graduated("wc", DIGEST, "abc123")
        .unwrap();
    assert_eq!(done.name, "wc");
    let r = rows(&c, "extend.revoked");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["reason"], "graduated");
    assert_eq!(r[0]["by"], "self");
    assert_eq!(r[0]["via"], "join abc123");
    assert_eq!(r[0]["digest"], DIGEST);
    let left = LoadedSet::read(&c.store).unwrap();
    assert!(left.loaded.contains_key("lone") && !left.loaded.contains_key("wc"));
    // The owner's own revoke leaves the key out.
    c.extension_revoke(
        &theseus_protocol::extend::ExtensionRevokeParams {
            name: "lone".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let r = rows(&c, "extend.revoked");
    assert!(r[1].get("reason").is_none(), "{}", r[1]);
}
