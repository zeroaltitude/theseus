//! `budget.list` (step 42a, theseus-ext.7): each open execution's money as
//! its record holds it, with where its limit comes from, after a carve, a
//! reset, a place's limit, and the config's limit changed across a restart
//! (theseus-3pj); and the reset's time and approver from one indexed ledger
//! page, however much history lies after it.

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use theseus_kernel::{micros_to_usd as usd, usd_to_micros, Execution};
use theseus_protocol::{BudgetListResult, BudgetRow, PlaceCeiling, SessionOpenParams};

use crate::places::BoundPlace;
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::Config;

const PIER: u64 = 141_421_356_237_309_504;

fn core_at(dir: &Path, limit_usd: f64) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.kernel.spend_limit_usd = limit_usd;
    let store = Store::open(&dir.join("store")).unwrap();
    Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap()
}

fn open(core: &Core, label: &str) -> (String, String) {
    let rec = core
        .open_session(SessionOpenParams {
            label: Some(label.into()),
            ..Default::default()
        })
        .unwrap();
    (rec.session_id, rec.execution_id.unwrap())
}

fn exec(core: &Core, id: &str) -> Execution {
    core.kernel.execution(id).unwrap().unwrap()
}

/// Every row, its tasks included.
fn every(r: &BudgetListResult) -> Vec<&BudgetRow> {
    r.executions
        .iter()
        .flat_map(|e| std::iter::once(e).chain(&e.tasks))
        .collect()
}

/// Each row agrees with its execution's record.
fn agrees(core: &Core, r: &BudgetListResult) {
    for row in every(r) {
        let e = exec(core, &row.execution_id);
        let b = &e.budget;
        assert_eq!(row.session_id, e.session_id);
        assert_eq!(row.state, e.state.as_str());
        assert_eq!(row.limit_usd, usd(b.limit_micros), "{row:?}");
        assert_eq!(row.spent_usd, usd(b.spent_micros));
        assert_eq!(row.reserved_usd, usd(b.reserved_micros));
        assert_eq!(row.held_unknown_usd, usd(b.held_unknown_micros));
        assert_eq!(row.available_usd, usd(b.available()));
        assert_eq!(row.resets, b.resets);
        assert_eq!(
            row.question.as_ref().map(|q| &q.correlation_id),
            b.question.as_ref()
        );
        assert_eq!(row.parent, e.parent);
    }
    // The totals add the top rows' money, and every session's lifetime.
    let top = &r.executions;
    let sum = |f: fn(&BudgetRow) -> f64| usd(top.iter().map(|x| usd_to_micros(f(x))).sum());
    assert_eq!(r.totals.limit_usd, sum(|x| x.limit_usd));
    assert_eq!(r.totals.spent_usd, sum(|x| x.spent_usd));
    assert_eq!(r.totals.reserved_usd, sum(|x| x.reserved_usd));
    assert_eq!(r.totals.available_usd, sum(|x| x.available_usd));
    assert_eq!(r.totals.executions as usize, top.len());
    assert_eq!(
        r.totals.tasks as usize,
        top.iter().map(|x| x.tasks.len()).sum::<usize>()
    );
}

/// A conversation with two tasks: each task under its parent, its limit
/// its carve and its parent's reservation for it; the parent's limit the
/// config's; then a reset, read back with who approved it and when; then a
/// place's limit, which pins the parent's while it caps it.
#[tokio::test]
async fn budget_list_after_a_carve_a_reset_and_a_place_limit_agrees_with_each_record() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_at(dir.path(), 100.0);
    let (sid, xid) = open(&core, "the lighthouse");
    core.kernel.wake_input(&xid).unwrap();
    let g = core.kernel.admit(&xid).unwrap();
    let a = core
        .kernel
        .open_task(&g, "act_lamp1", usd_to_micros(3.0), None, false, |_| {
            Ok(vec![])
        })
        .unwrap()
        .task;
    let b = core
        .kernel
        .open_task(&g, "act_lens22", usd_to_micros(2.0), None, false, |_| {
            Ok(vec![])
        })
        .unwrap()
        .task;

    let r = core.budget_list().unwrap();
    agrees(&core, &r);
    assert_eq!(
        r.executions.len(),
        1,
        "the tasks are under their parent: {r:?}"
    );
    let p = &r.executions[0];
    assert_eq!(p.execution_id, xid);
    assert_eq!(p.title.as_deref(), Some("the lighthouse"));
    assert_eq!(
        (p.limit_from.as_str(), p.limit_by.as_deref()),
        ("config", None)
    );
    assert_eq!(
        p.reserved_usd, 5.0,
        "both carves are the parent's reservations"
    );
    let ids: Vec<&str> = p.tasks.iter().map(|t| t.execution_id.as_str()).collect();
    assert_eq!(ids, [a.id.as_str(), b.id.as_str()]);
    for (t, carve) in p.tasks.iter().zip([3.0, 2.0]) {
        assert_eq!(t.kind, "task");
        assert_eq!(
            (t.limit_from.as_str(), t.limit_by.as_deref()),
            ("carve", Some(sid.as_str()))
        );
        assert_eq!(t.limit_usd, carve);
        assert_eq!(t.carve_held_usd, Some(carve));
    }
    assert_eq!(r.totals.reserved_usd, 5.0);
    assert_eq!(r.config_limit_usd, 100.0);
    let j = r.judge.as_ref().unwrap();
    assert_eq!(j.limit_usd, core.cfg.judge.shadow_limit_usd_per_day);

    // A question, and its reset approved.
    let q = core.kernel.ask_budget(&g, usd_to_micros(1.0)).unwrap();
    let r = core.budget_list().unwrap();
    agrees(&core, &r);
    let asked = r.executions[0].question.as_ref().unwrap();
    assert_eq!(asked.correlation_id, q.correlation_id);
    assert_eq!(asked.needs_usd, 1.0);
    assert_eq!(r.totals.questions, 1);
    core.kernel.reset_budget(&q.correlation_id, "cli").unwrap();
    let r = core.budget_list().unwrap();
    agrees(&core, &r);
    let p = &r.executions[0];
    assert_eq!(p.resets, 1);
    assert!(p.question.is_none());
    let reset = p.last_reset.as_ref().expect("its row is read");
    assert_eq!(reset.by, "cli");
    let rows = core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(50)
        .unwrap();
    let row = rows
        .iter()
        .rev()
        .find(|(_, x)| x.kind == "budget.reset")
        .unwrap();
    assert_eq!(reset.at_ms, row.1.at_unix_ms);

    a_place_limit_pins_it_while_it_caps_it(&core, &sid);
}

/// A place's limit: lower than the config's, so the parent's is the
/// place's, pinned, and named by the place; without the cap, the config's.
fn a_place_limit_pins_it_while_it_caps_it(core: &Core, sid: &str) {
    core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{PIER}"),
        name: "#pier".into(),
        private: false,
        guild: Some("100000000000000002".into()),
        ceiling: Some(PlaceCeiling {
            spend_limit_usd: Some(40.0),
            ..Default::default()
        }),
    }]);
    core.outbox
        .bind_place(&format!("channel:{PIER}"), sid)
        .unwrap();
    core.place_spend(sid, "#pier", Some(40.0));
    let r = core.budget_list().unwrap();
    agrees(core, &r);
    let p = &r.executions[0];
    assert_eq!(p.limit_usd, 40.0);
    assert_eq!(
        (p.limit_from.as_str(), p.limit_by.as_deref()),
        ("place", Some("#pier"))
    );
    // Without the cap it is the config's again.
    core.place_spend(sid, "#pier", None);
    let r = core.budget_list().unwrap();
    agrees(core, &r);
    assert_eq!(r.executions[0].limit_usd, 100.0);
    assert_eq!(r.executions[0].limit_from, "config");
}

/// The config's limit changed across a restart (theseus-3pj): an open
/// conversation follows it, and a task keeps its carve.
#[tokio::test]
async fn budget_list_follows_a_changed_config_limit_and_a_carve_keeps_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let (xid, task) = {
        let core = core_at(dir.path(), 100.0);
        let (_, xid) = open(&core, "the harbour");
        core.kernel.wake_input(&xid).unwrap();
        let g = core.kernel.admit(&xid).unwrap();
        let t = core
            .kernel
            .open_task(&g, "act_buoy3", usd_to_micros(4.0), None, false, |_| {
                Ok(vec![])
            })
            .unwrap()
            .task;
        core.kernel
            .end_turn(
                g,
                theseus_kernel::TurnEnd::Wait {
                    wake: theseus_kernel::Wake::Input,
                },
            )
            .unwrap();
        (xid, t.id)
    };
    let core = core_at(dir.path(), 60.0);
    let r = core.budget_list().unwrap();
    agrees(&core, &r);
    let p = r.executions.iter().find(|e| e.execution_id == xid).unwrap();
    assert_eq!((p.limit_usd, p.limit_from.as_str()), (60.0, "config"));
    let t = &p.tasks[0];
    assert_eq!(t.execution_id, task);
    assert_eq!((t.limit_usd, t.limit_from.as_str()), (4.0, "carve"));
    assert_eq!(r.config_limit_usd, 60.0);
}

/// The last reset is one ledger page by kind and session: found behind
/// 5,000 later rows of other kinds and sessions, where a window of the
/// newest rows (what `ledger.tail` scans while the index is built) would
/// not reach it; and a reset of the session's earlier execution is not
/// this one's: its own, beneath it, is.
#[tokio::test]
async fn the_last_reset_is_read_by_one_page_however_much_history_follows() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_at(dir.path(), 100.0);
    let (sid, xid) = open(&core, "the quay");
    core.kernel.wake_input(&xid).unwrap();
    let g = core.kernel.admit(&xid).unwrap();
    let q = core.kernel.ask_budget(&g, usd_to_micros(1.0)).unwrap();
    core.kernel
        .reset_budget(&q.correlation_id, "discord:271828")
        .unwrap();
    for i in 0..5_000u64 {
        core.store
            .append_ledger(&json!({"at_unix_ms": i, "kind": "turn.started",
                                   "session_id": format!("ses_other{}", i % 7), "data": {"i": i}}))
            .unwrap();
    }
    let r = core.budget_list().unwrap();
    let p = &r.executions[0];
    assert_eq!(
        p.last_reset.as_ref().map(|x| x.by.as_str()),
        Some("discord:271828")
    );
    assert!(p.last_reset_unread.is_none());
    // A newer row of an earlier execution of the same session is not its.
    core.store
        .append_ledger(
            &json!({"at_unix_ms": 9, "kind": "budget.reset", "session_id": sid,
                               "data": {"execution_id": "exe_before", "by": "someone"}}),
        )
        .unwrap();
    let r = core.budget_list().unwrap();
    assert_eq!(
        r.executions[0].last_reset.as_ref().map(|x| x.by.as_str()),
        Some("discord:271828"),
        "{r:?}"
    );
}
