//! A trim that finds the resident set over the floor names what ran before
//! it (issue 21, step one): one `resident.peak` row and one log line, built
//! from the record every entry point of work feeds; any other trim writes
//! nothing and drops the window.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::config::MemoryMode;
use crate::ledger::LedgerRow;
use crate::resident::activity::{self, Kind};
use crate::resident::{act, rss_bytes, PEAK_FLOOR};
use crate::tests_recall::rig;
use crate::Core;

const TAG: &str = "marsh-2026-01";

/// The `resident.peak` rows' data, oldest first.
fn peaks(core: &Core) -> Vec<Value> {
    core.store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == "resident.peak")
        .map(|(_, r)| r.data)
        .collect()
}

/// One request over a connection, as a client sends it: the answer's line.
async fn ask(core: &Arc<Core>, method: &str, params: Value) -> String {
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(theirs);
    let client = crate::approval::Client::new("sock#1", crate::approval::Surface::Cli);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    cw.write_all(format!("{}\n", serde_json::to_string(&req).unwrap()).as_bytes())
        .await
        .unwrap();
    let mut lines = BufReader::new(cr).lines();
    let line = lines.next_line().await.unwrap().expect("an answer");
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    line
}

/// A trim's turn, now: quiet is zero, so the mark just made is due.
fn trim_turn(core: &Arc<Core>) {
    core.resident
        .set_bounds(Duration::ZERO, Duration::from_secs(600));
    core.resident.mark();
    act(&Arc::downgrade(core), false, true);
}

/// The entry in a row's `ran` list that is `name`.
fn entry<'a>(row: &'a Value, name: &str) -> &'a Value {
    row["ran"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not in the row: {row}"))
}

/// A method that builds a large transient (`import.sessions` builds the
/// imported sessions' catalog), served over a connection as every client's
/// is, and a trim over the floor after it: the row names the method, its
/// count, its time, and the size of what it answered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peak_names_the_method_that_ran_before_it() {
    let r = rig(MemoryMode::Off);
    let c = r.core.clone();
    crate::books::tests::import(&c, TAG, 0, 300);
    // Any process is over a floor of one MiB: the test is of the record.
    c.resident.set_peak_floor(1 << 20);
    let line = ask(
        &c,
        theseus_protocol::method::IMPORT_SESSIONS,
        serde_json::json!({"limit": 50, "summaries": true}),
    )
    .await;
    trim_turn(&c);
    let rows = peaks(&c);
    assert_eq!(rows.len(), 1, "one row for one peak: {rows:?}");
    let e = entry(&rows[0], "import.sessions");
    assert_eq!(e["kind"], "rpc");
    assert_eq!(e["count"], 1);
    assert!(e["ms"].is_u64() && e["max_ms"].is_u64(), "{e}");
    // The answer's size is its result's, not its envelope's.
    let size = e["bytes"].as_u64().expect("its bytes");
    assert!(
        size > 1000 && size < line.len() as u64,
        "{size} of {}",
        line.len()
    );
    assert!(rows[0]["rss_before_bytes"].as_u64().unwrap() > 1 << 20);
    assert_eq!(rows[0]["floor_bytes"], 1 << 20);
    assert!(rows[0]["since_ms"].as_u64() <= rows[0]["until_ms"].as_u64());
    // Nothing of what was asked or answered is in the row.
    let text = rows[0].to_string();
    assert!(
        !text.contains("marsh") && !text.contains("summaries"),
        "{text}"
    );
}

/// A pass that has not ended when the trim comes holds memory the trim
/// cannot return: the row names it, still running. The floor is 64 MiB above
/// the process's own resident set and the pass holds 128 MiB, so it is crossed
/// by what the pass holds and by nothing else.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_large_transient_in_a_pass_still_running_is_named() {
    let r = rig(MemoryMode::Off);
    let c = &r.core;
    let floor = rss_bytes() + (64 << 20);
    c.resident.set_peak_floor(floor);
    let ran = c.resident.activity().ran(Kind::Job, "a.big.pass");
    // Touched, so it is resident: one page in each 4 KiB.
    let held = vec![1u8; 128 << 20];
    assert!(held.iter().step_by(4096).all(|&b| b == 1));
    trim_turn(c);
    drop(ran);
    drop(held);
    let rows = peaks(c);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let e = entry(&rows[0], "a.big.pass");
    assert_eq!(e["running"], true);
    assert!(rows[0]["rss_before_bytes"].as_u64().unwrap() > floor);
}

/// An ordinary trim, at the real floor, writes no row, and its window is
/// dropped: the next one begins empty.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ordinary_trim_writes_nothing() {
    let r = rig(MemoryMode::Off);
    let c = r.core.clone();
    assert!(
        rss_bytes() < PEAK_FLOOR,
        "this test's own process is over the floor"
    );
    ask(&c, theseus_protocol::method::HEALTH, serde_json::json!({})).await;
    trim_turn(&c);
    assert!(peaks(&c).is_empty(), "{:?}", peaks(&c));
    assert!(
        c.resident.activity().drain().entries.is_empty(),
        "the trim's turn took the window"
    );
}

/// The floor is the owner's number from the issue, and sits between the
/// daemon's rest and its peaks.
#[test]
fn the_floor_is_512_mib() {
    assert_eq!(PEAK_FLOOR, 512 << 20);
}

/// A nightly pass is named by the function that built it, with no closure
/// path before it.
#[tokio::test(start_paused = true)]
async fn a_nightly_pass_is_named_by_its_function() {
    fn the_nightly_pass_under_test() -> impl FnMut(&'static str) -> std::future::Ready<()> {
        |_| std::future::ready(())
    }
    let mut left = 1;
    let next = move || {
        (left > 0).then(|| {
            left -= 1;
            (Duration::from_secs(1), "timer")
        })
    };
    crate::learning::tender::tend(
        tokio::time::Instant::now(),
        next,
        the_nightly_pass_under_test(),
    )
    .await;
    let w = activity::Activity::global().drain();
    let e = w
        .entries
        .iter()
        .find(|e| e.name == "the_nightly_pass_under_test")
        .unwrap_or_else(|| panic!("{:?}", w.entries));
    assert_eq!((e.kind, e.count), (Kind::Job, 1));
}
