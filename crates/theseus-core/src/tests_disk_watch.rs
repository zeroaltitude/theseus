//! The disk's crossings (theseus-f337): one `disk.*` row and one notice on
//! the operator's lane each time free space crosses a line or comes back,
//! over many heartbeats, with `FixedSpace` for `statvfs`.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::disk::FixedSpace;
use crate::ledger::LedgerRow;
use crate::outbox::{body_of, kind_of, OPERATOR_TARGET};
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

/// A core whose warning is 5,120 MB and whose floor is 1,024 MB, over a
/// disk of 100,000 MB with `free_mb` free.
fn rig(free_mb: u64) -> (Rig, Arc<FixedSpace>) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.server.disk_warn_mb = 5120;
    cfg.server.disk_floor_mb = 1024;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    let space = FixedSpace::new(free_mb, 100_000);
    core.tools.disk.set_probe(space.clone());
    (Rig { core, _dir: dir }, space)
}

/// The `disk.*` rows, oldest first: kind and data.
fn rows(core: &Core) -> Vec<(String, Value)> {
    core.store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind.starts_with("disk."))
        .map(|(_, r)| (r.kind, r.data))
        .collect()
}

/// The `disk` posts to the operator, written and not yet delivered (no
/// binding runs here), by their bodies.
fn notices(core: &Core) -> Vec<Value> {
    core.outbox
        .open_for(OPERATOR_TARGET)
        .iter()
        .filter(|a| kind_of(a) == "disk")
        .map(|a| body_of(a).clone())
        .collect()
}

fn beats(core: &Core, n: usize) {
    for _ in 0..n {
        core.heartbeat("timer");
    }
}

/// ok → low → below the floor → ok, over many beats, with free space that
/// wobbles at each line: exactly one row and one notice per crossing, and
/// none in a steady state, for a wobble inside a margin, or for a notice's
/// beat.
#[test]
fn a_crossing_writes_one_row_and_one_notice_and_a_steady_state_none() {
    let (r, space) = rig(80_000);
    beats(&r.core, 5);
    assert!(
        rows(&r.core).is_empty(),
        "a start that reads ok says nothing"
    );
    let seen = |kinds: &[&str]| {
        let got: Vec<String> = rows(&r.core).into_iter().map(|(k, _)| k).collect();
        assert_eq!(got, kinds, "rows");
        assert_eq!(notices(&r.core).len(), kinds.len(), "notices");
    };
    // Under the warning: one row.
    space.set_free_mb(5000);
    beats(&r.core, 5);
    seen(&["disk.low"]);
    let low = &rows(&r.core)[0].1;
    assert_eq!(
        (
            low["free_mb"].as_u64(),
            low["total_mb"].as_u64(),
            low["warn_mb"].as_u64()
        ),
        (Some(5000), Some(100_000), Some(5120))
    );
    assert_eq!(
        (low["floor_mb"].as_u64(), low["left"].as_str()),
        (Some(1024), Some("ok"))
    );
    // A wobble over the warning, inside its margin (256 MB): still one.
    for free in [5200, 5000, 5375, 5100, 5300] {
        space.set_free_mb(free);
        beats(&r.core, 2);
    }
    seen(&["disk.low"]);
    // A wrapper's notice checks nothing: it comes in bursts.
    space.set_free_mb(800);
    for _ in 0..4 {
        r.core.heartbeat("notify");
    }
    seen(&["disk.low"]);
    // Under the floor: one row, and steady beats add none.
    beats(&r.core, 5);
    seen(&["disk.low", "disk.below_floor"]);
    // A wobble at the floor, inside its margin (51 MB): none.
    for free in [1030, 900, 1074, 800] {
        space.set_free_mb(free);
        beats(&r.core, 2);
    }
    seen(&["disk.low", "disk.below_floor"]);
    // An unreadable disk is no crossing, and coming back to the state it
    // held is none either.
    space.fail("invented failure");
    beats(&r.core, 3);
    space.set_free_mb(800);
    beats(&r.core, 3);
    seen(&["disk.low", "disk.below_floor"]);
    // Clear of the floor by its margin: low again, then ok past the warning's.
    space.set_free_mb(1200);
    beats(&r.core, 3);
    seen(&["disk.low", "disk.below_floor", "disk.low"]);
    space.set_free_mb(90_000);
    beats(&r.core, 3);
    seen(&["disk.low", "disk.below_floor", "disk.low", "disk.ok"]);
    let all = rows(&r.core);
    assert_eq!(all[3].1["left"], "low");
    assert_eq!(all[2].1["left"], "below_floor");
    let posted = notices(&r.core);
    let states: Vec<&str> = posted
        .iter()
        .map(|b| b["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["low", "below_floor", "low", "ok"]);
    assert_eq!(posted[1]["free_mb"], 800);
    assert_eq!(posted[1]["floor_mb"], 1024);
    assert_eq!(posted[1]["kind"], json!("disk"));
}

/// A start whose first read is not ok writes one row, which leaves no state
/// (`left` null); a steady low after it writes none; space that comes all
/// the way back writes `disk.ok`.
#[test]
fn a_first_read_below_the_warning_is_one_crossing_from_nothing() {
    let (r, space) = rig(4000);
    beats(&r.core, 4);
    let got = rows(&r.core);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, "disk.low");
    assert!(got[0].1["left"].is_null());
    assert_eq!(notices(&r.core).len(), 1);
    space.set_free_mb(100_000);
    beats(&r.core, 2);
    assert_eq!(rows(&r.core).len(), 2);
    assert_eq!(rows(&r.core)[1].0, "disk.ok");
}

/// A first read that cannot be read is nothing, and the first that can is
/// judged as a first.
#[test]
fn an_unreadable_first_read_is_no_crossing() {
    let (r, space) = rig(80_000);
    space.fail("invented failure");
    beats(&r.core, 3);
    assert!(rows(&r.core).is_empty() && notices(&r.core).is_empty());
    space.set_free_mb(80_000);
    beats(&r.core, 3);
    assert!(rows(&r.core).is_empty());
}
