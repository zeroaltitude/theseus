//! The ladder's first read off every point's path (theseus-289c): before the
//! warm read (`Core::warm_ladder`), a judged turn's points answer from the
//! wired lines under the config, a pack that would act in shadow, and the
//! roots, reading and writing nothing of the ladder or the lineage; after
//! it, a stored rollback applies; and a first ask after a local midnight
//! reads nothing.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::packs::PackRollbackParams;
use theseus_protocol::LedgerKind;

use crate::config::PackMode;
use crate::judge::ladder::Rung;
use crate::ledger::LedgerRow;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::tests_judge::{board, judge_config, turn};

/// A core on `dir`'s store, built as the daemon builds one: the judge on at
/// `jev` (or off), every pack as the build wires it, and its model asking
/// for one `proc.run` that acts and then answering.
fn core_at(dir: &Path, work: &Path, jev: Option<&FakeJev>) -> Arc<Core> {
    let mut cfg = judge_config(dir, jev);
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Open;
    cfg.validate().unwrap();
    let store = Store::open(&dir.join("store")).unwrap();
    let script = vec![
        Scripted::tools("", &[("tu_1", "proc_run", json!({"argv": ["echo", "hi"]}))]),
        Scripted::text("Done: said hi."),
    ];
    let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(script)), store);
    p.secrets = board();
    Core::build(p).unwrap()
}

fn pack_modes(core: &Core) -> usize {
    core.store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == LedgerKind::PackMode.as_str())
        .count()
}

fn frames(res: &theseus_protocol::TurnSubmitResult) -> Value {
    serde_json::to_value(res).unwrap()["trace"]["attrs"]["frames"].clone()
}

/// Wait, on the runtime's timer, until `f` holds.
async fn until(what: &str, f: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A store whose owner rolled `route.v1` back. Before the warm read, a
/// turn judged at every point (the inbound point's three packs, the gate's
/// two, and the loop's end, with the compile's on) leaves the ladder and the
/// lineage unread, writes no `pack.mode` row, and writes the judge-off
/// turn's frames; what would act judges in shadow. The warm read then
/// reads once, and the rollback applies; a first ask after a local midnight
/// reads nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn before_the_warm_read_a_judged_turn_reads_and_writes_nothing_of_the_ladder() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.02));
    let (dir, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    {
        // The owner's rollback, through the RPC's own first read (which
        // writes the adoptions before it).
        let c = core_at(dir.path(), work.path(), Some(&jev));
        let p = PackRollbackParams {
            pack: "route.v3".into(),
            why: Some("too many switches".into()),
            off: false,
        };
        c.pack_rollback(&p, "cli").unwrap();
    }
    let c = core_at(dir.path(), work.path(), Some(&jev));
    let j = &c.runner.judge;
    let before = pack_modes(&c);
    assert_eq!(before, 4, "three adoptions and the rollback");

    let res = turn(&c, None, "Say hi, then tell me.").await;
    assert_eq!(res.tool_calls, 1, "{res:?}");
    // Every point judged: the inbound batch's three, the gate's two, and
    // the loop's end. The compile's point is on too; continue.v1 judges
    // only a compile whose signal fired, and this turn's fires none.
    let want = [
        "classify.v1",
        "role.v1",
        "route.v3",
        "security.v1",
        "security.v3",
        "loop.v1",
    ];
    let judged = || -> Vec<String> {
        crate::tests_judge::kinds(&c.store, "judge.call")
            .iter()
            .map(|r| r.data["pack"].as_str().unwrap_or_default().to_string())
            .collect()
    };
    let t0 = Instant::now();
    while !want.iter().all(|p| judged().iter().any(|q| q == p)) {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "judged so far: {:?}",
            judged()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!j.ladder().is_loaded(), "the ladder is unread");
    assert!(!j.lineage().is_loaded(), "the lineage is unread");
    assert_eq!(j.ladder().reads(), 0, "nothing of the ladder was read");
    assert_eq!(pack_modes(&c), before, "no pack.mode row written");
    // What would act judges in shadow until the read.
    for p in ["route.v3", "rerank.v1", "security.v3"] {
        assert_eq!(j.mode_for(p, &res.session_id).mode, PackMode::Shadow, "{p}");
    }
    assert!(
        j.pack_lines()
            .contains(&"rerank.v1: shadow (until the ladder is read; wired live)".to_string()),
        "{:?}",
        j.pack_lines()
    );
    // The judge-off turn's frames.
    let off_dir = tempfile::tempdir().unwrap();
    let off = core_at(off_dir.path(), work.path(), None);
    let base = turn(&off, None, "Say hi, then tell me.").await;
    assert_eq!(frames(&res), frames(&base), "the judge-off turn's frames");
    assert!(frames(&res).as_u64().is_some_and(|n| n > 0));

    // The warm read: once, and the rollback applies.
    c.warm_ladder();
    until("the warm read", || j.ladder_read()).await;
    assert_eq!(j.ladder().reads(), 1);
    assert_eq!(j.ladder().standing("route.v3").rung, Rung::RolledBack);
    assert_eq!(
        j.mode_for("route.v3", &res.session_id).mode,
        PackMode::Shadow,
        "rolled back"
    );
    assert_eq!(
        j.mode_for("rerank.v1", &res.session_id).mode,
        PackMode::Live,
        "read, the adopted pack acts"
    );
    assert_eq!(pack_modes(&c), before, "every adoption was there");

    // A first ask after a local midnight: a new day, read from nothing.
    let tomorrow = theseus_protocol::now_unix_ms() + 86_400_000;
    j.ladder().set_clock(Arc::new(move || tomorrow));
    assert_eq!(
        j.mode_for("rerank.v1", &res.session_id).mode,
        PackMode::Live
    );
    assert_eq!(j.ladder().reads(), 1, "the new day read nothing");
}

/// On a fresh store the warm read writes the three adoptions, between
/// turns: none while a turn runs, all once it has ended and a quiet stretch
/// has passed. The turn is held well past the warm read's own quiet stretch
/// after serving (its 500 ms sleep), so a read that only slept, and never
/// waited for the turn, writes inside it and fails here (theseus-3bl9).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_warm_reads_adoptions_wait_for_a_moment_between_turns() {
    let dir = tempfile::tempdir().unwrap();
    // The judge on with Jev nowhere: nothing here is judged.
    let mut cfg = judge_config(dir.path(), None);
    cfg.judge.enabled = true;
    cfg.judge.api_base = "http://127.0.0.1:9".into();
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut p = Parts::for_tests(cfg, Arc::new(FakeProvider::scripted(vec![])), store);
    p.secrets = board();
    let c = Core::build(p).unwrap();
    let running = c.runner.pass.turns().begin().await;
    c.warm_ladder();
    until("the warm read", || c.runner.judge.ladder_read()).await;
    let held = crate::memory_pass::QUIET * 4;
    tokio::time::sleep(held).await;
    assert_eq!(
        pack_modes(&c),
        0,
        "no adoption while a turn runs, {held:?} past the warm read's sleep"
    );
    let ended = Instant::now();
    drop(running);
    until("the adoptions", || pack_modes(&c) == 3).await;
    assert!(
        ended.elapsed() >= crate::memory_pass::QUIET,
        "written a quiet stretch after the turn ended, not before"
    );
}
