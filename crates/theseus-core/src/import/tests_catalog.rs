//! `import.sessions`' catalog bounded (theseus-9lxe): a read after the
//! catalog was dropped answers as the reads before it did; one catalog is
//! held however many reads and imports come, so a stale one never stays
//! beside the new; and the daemon's tender drops it after its idle stretch
//! and gives the allocator's free pages back after a quiet one, which the
//! resident set shows.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use theseus_protocol::import::{ImportSessionsParams, ImportSessionsResult};

use super::catalog::Catalog;
use crate::books::tests::import;
use crate::config::MemoryMode;
use crate::tests_recall::{rig, Rig};
use crate::Core;

const TAG: &str = "marsh-2026-01";
const LATER: &str = "marsh-2026-02";

fn rigged(n: usize) -> Rig {
    let r = rig(MemoryMode::Off);
    import(&r.core, TAG, 0, n);
    r
}

/// The queries the Context page makes: its count, a page of each sort with
/// summaries, the filters, and a search of words.
fn queries() -> Vec<ImportSessionsParams> {
    let p = |v: serde_json::Value| serde_json::from_value::<ImportSessionsParams>(v).unwrap();
    vec![
        p(serde_json::json!({"limit": 1})),
        p(serde_json::json!({"limit": 50, "summaries": true})),
        p(serde_json::json!({"limit": 20, "offset": 40, "sort": "oldest"})),
        p(serde_json::json!({"book": "diary", "topic": "heron"})),
        p(serde_json::json!({"source": "wiki", "limit": 500})),
        p(serde_json::json!({"q": "wren", "summaries": true})),
    ]
}

/// Each query's answer, without its timings.
fn answers(core: &Core) -> Vec<(ImportSessionsResult, bool)> {
    queries()
        .iter()
        .map(|q| {
            let mut r = core.import_sessions(q, true).unwrap();
            let built = r.built_ms.is_some();
            (r.built_ms, r.ms) = (None, 0.0);
            (r, built)
        })
        .collect()
}

fn same(a: &[(ImportSessionsResult, bool)], b: &[(ImportSessionsResult, bool)]) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        assert_eq!(
            serde_json::to_value(&x.0).unwrap(),
            serde_json::to_value(&y.0).unwrap()
        );
    }
}

#[test]
fn a_read_after_the_catalog_was_dropped_answers_as_before() {
    let r = rigged(300);
    let c = &r.core;
    let before = answers(c);
    assert!(before[0].1, "the first read builds the catalog");
    assert!(
        before[1..].iter().all(|(_, built)| !built),
        "the rest read it"
    );
    assert_eq!(before[0].0.all, 300);
    let h = c.episodes.health(Duration::from_secs(600));
    assert_eq!(h.entries, 300);
    assert!(
        h.bytes > 300 * 200,
        "its estimate counts its rows' text: {h:?}"
    );

    // Read just now: an idle bound of a minute keeps it.
    assert_eq!(c.episodes.drop_if_idle(Duration::from_secs(60)), None);
    assert!(c.episodes.read_at().is_some());
    // Idle past its bound: it goes, and says so.
    assert_eq!(c.episodes.drop_if_idle(Duration::ZERO), Some(300));
    assert_eq!(c.episodes.read_at(), None, "none is kept, so none is read");
    let h = c.episodes.health(Duration::from_secs(600));
    assert_eq!((h.entries, h.bytes), (0, 0));
    assert!(
        h.note.contains("not built") && h.note.contains("(1 so far)"),
        "{}",
        h.note
    );
    assert_eq!(
        c.episodes.drop_if_idle(Duration::ZERO),
        None,
        "nothing left to drop"
    );

    let after = answers(c);
    assert!(after[0].1, "the next read builds it again");
    same(&before, &after);
}

/// Imports and reads interleaved, from four threads: every answer counts
/// the import as it was when read, and no catalog outlives the next one
/// built, so what is held is one catalog's rows, never the history's.
#[test]
fn under_a_load_of_reads_and_imports_one_catalog_is_held() {
    let r = rigged(200);
    let c = &r.core;
    let mut seen: Vec<Weak<Catalog>> = Vec::new();
    for round in 0..5usize {
        import(c, LATER, round * 40, 40);
        let want = 200 + (round as u64 + 1) * 40;
        let readers: Vec<_> = (0..4)
            .map(|_| {
                let c = c.clone();
                std::thread::spawn(move || {
                    let list = super::write::list(&c.store).unwrap();
                    (0..10)
                        .map(|_| {
                            let (cat, _) = c.episodes.at(&c.store, &list).unwrap();
                            assert_eq!(cat.rows.len() as u64, want);
                            Arc::downgrade(&cat)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in readers {
            seen.extend(h.join().unwrap());
        }
        let held: Vec<_> = seen.iter().filter_map(Weak::upgrade).collect();
        assert!(
            held.iter().all(|h| Arc::ptr_eq(h, &held[0])),
            "round {round}: one catalog held, not {}",
            held.len()
        );
        let h = c.episodes.health(Duration::from_secs(600));
        assert_eq!(h.entries, want);
        assert_eq!(h.bytes, held[0].bytes(), "health's size is the held one's");
    }
    let distinct = {
        let mut v: Vec<_> = seen.iter().map(Weak::as_ptr).collect();
        v.sort();
        v.dedup();
        v.len()
    };
    assert_eq!(distinct, 5, "one build per import, however many reads");
}

/// Wait up to `bound` for `f`, on tokio's timer.
async fn until(bound: Duration, f: impl Fn() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < bound {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    f()
}

/// The tender, with short bounds: a catalog read and left alone is dropped
/// after its idle stretch, and the next read builds it with the same
/// answers; one read again and again is kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tender_drops_an_idle_catalog_and_keeps_one_still_read() {
    let r = rigged(150);
    let c = r.core.clone();
    c.resident
        .set_bounds(Duration::from_millis(50), Duration::from_millis(400));
    c.tend_memory_after_serving();
    let cc = c.clone();
    let before = tokio::task::spawn_blocking(move || answers(&cc))
        .await
        .unwrap();

    // Read every 100 ms for a second: never idle for 400, so kept.
    for _ in 0..10 {
        let cc = c.clone();
        let fresh = tokio::task::spawn_blocking(move || answers(&cc))
            .await
            .unwrap();
        assert!(fresh.iter().all(|(_, built)| !built), "kept while read");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(c.episodes.read_at().is_some());

    // Left alone, it goes.
    assert!(
        until(Duration::from_secs(5), || c.episodes.read_at().is_none()).await,
        "the idle catalog was dropped"
    );
    let note = c.resident_health().caches[1].note.clone();
    assert!(note.contains("(1 so far)"), "{note}");

    let cc = c.clone();
    let after = tokio::task::spawn_blocking(move || answers(&cc))
        .await
        .unwrap();
    assert!(after[0].1, "built again by the next read");
    same(&before, &after);
}

/// Free pages a thread's arena holds are given back once the daemon has
/// been quiet after work: the trim runs, and the heap it leaves holds less
/// free than before.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quiet_stretch_after_work_gives_the_free_pages_back() {
    use crate::resident::{heap, TRIM_FLOOR};
    let r = rig(MemoryMode::Off);
    let c = r.core.clone();
    // Garbage as a catalog's build leaves it: a thread allocates many small
    // blocks, keeps one in 64, and frees the rest, so its arena holds the
    // freed pages between the kept blocks and gives none back itself.
    let kept = std::thread::spawn(|| {
        let mut all: Vec<Box<[u8; 1024]>> = (0..65_536).map(|_| Box::new([7u8; 1024])).collect();
        let kept: Vec<_> = all.drain(..).step_by(64).collect();
        kept
    })
    .join()
    .unwrap();
    let h = heap().unwrap();
    let free = h.held_bytes - h.in_use_bytes;
    assert!(free >= TRIM_FLOOR, "the garbage is held: {h:?}");

    c.resident
        .set_bounds(Duration::from_millis(200), Duration::from_secs(600));
    c.tend_memory_after_serving();
    // Work keeps marking: no trim while it does.
    for _ in 0..5 {
        c.resident.mark();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(c.resident_health().trims, 0, "no trim while work goes on");
    assert!(
        until(Duration::from_secs(5), || c.resident_health().trims == 1).await,
        "one trim after the quiet stretch"
    );
    // The trim gives the pages back to the system; the allocator still
    // counts them as its own (mallinfo2's `arena`), so the resident set is
    // the measure.
    let t = c.resident_health().last_trim.unwrap();
    assert!(
        t.rss_before_bytes >= t.rss_after_bytes + TRIM_FLOOR,
        "free {free}: {t:?}"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        c.resident_health().trims,
        1,
        "no mark since: no second trim"
    );
    drop(kept);
}

/// Work that never stops (the cockpit's `health` every 2 s) never leaves a
/// quiet stretch, so the trim is put off at most `MAX_DEFER` after the first
/// mark, and then runs (review of theseus-9lxe).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn marks_that_never_stop_still_trim_after_the_longest_deferral() {
    use crate::resident::{heap, TRIM_FLOOR};
    let r = rig(MemoryMode::Off);
    let c = r.core.clone();
    let kept = std::thread::spawn(|| {
        let mut all: Vec<Box<[u8; 1024]>> = (0..65_536).map(|_| Box::new([7u8; 1024])).collect();
        let kept: Vec<_> = all.drain(..).step_by(64).collect();
        kept
    })
    .join()
    .unwrap();
    let h = heap().unwrap();
    assert!(
        h.held_bytes - h.in_use_bytes >= TRIM_FLOOR,
        "the garbage is held: {h:?}"
    );

    c.resident.set_max_defer(
        Duration::from_millis(300),
        Duration::from_millis(800),
        Duration::from_secs(600),
    );
    c.tend_memory_after_serving();
    // A mark every 50 ms: never 300 ms quiet.
    let t0 = Instant::now();
    let mut first_trim = None;
    while first_trim.is_none() && t0.elapsed() < Duration::from_secs(5) {
        c.resident.mark();
        if c.resident_health().trims > 0 {
            first_trim = Some(t0.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let at = first_trim.expect("a trim while the marks went on");
    assert!(
        at >= Duration::from_millis(700),
        "not before the deferral's bound: {at:?}"
    );
    drop(kept);
}

/// `health` and the tender read the catalog's state on a runtime worker: a
/// build in progress never holds them (review of theseus-9lxe).
#[test]
fn health_answers_while_a_catalog_builds() {
    let r = rigged(50);
    let building = r.core.episodes.building();
    let (tx, rx) = std::sync::mpsc::channel();
    let cc = r.core.clone();
    std::thread::spawn(move || {
        let read = cc.episodes.read_at();
        let h = cc.resident_health();
        tx.send((read, h.caches[1].entries)).unwrap();
    });
    let got = rx.recv_timeout(Duration::from_secs(5));
    drop(building);
    assert_eq!(got, Ok((None, 0)), "health waited for the build");
}

/// A method's answer is work's end: it marks, so a trim follows the quiet
/// stretch after it (review of theseus-9lxe).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_methods_answer_marks_work() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let r = rig(MemoryMode::Off);
    let c = r.core.clone();
    assert_eq!(c.resident.marked(), None, "nothing has answered yet");
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let client = crate::approval::Client::new("sock#1", crate::approval::Surface::Cli);
    let srv = tokio::spawn(c.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(
        theseus_protocol::Id::Num(1),
        theseus_protocol::method::HEALTH,
        serde_json::json!({}),
    );
    let before = Instant::now();
    cw.write_all(format!("{}\n", serde_json::to_string(&req).unwrap()).as_bytes())
        .await
        .unwrap();
    let mut lines = BufReader::new(cr).lines();
    lines.next_line().await.unwrap().expect("an answer");
    assert!(
        c.resident.marked().is_some_and(|m| m >= before),
        "the answer marked work"
    );
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
}

/// `health` never reads the heap itself: `mallinfo2` walks every free chunk
/// under each arena's lock (10 ms over 100 MiB free), and the cockpit asks
/// for health every 2 s. It shows the tender's last read (review of
/// theseus-9lxe).
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_shows_the_heap_the_tender_last_read() {
    let r = rig(MemoryMode::Off);
    let c = r.core.clone();
    assert_eq!(c.resident_health().heap, None, "nothing has read it yet");
    c.resident
        .set_bounds(Duration::from_millis(50), Duration::from_secs(600));
    c.tend_memory_after_serving();
    assert!(
        until(Duration::from_secs(5), || c
            .resident_health()
            .heap
            .is_some())
        .await,
        "the tender's read, at the start's trim"
    );
}
