//! A re-read of the ladder (`Ladder::reread`, which `reload` and the nightly
//! `recheck` take), in the forced orders a race needs. A re-read scans the
//! store and puts what it scanned in the place of what is loaded; a writer
//! that lands while it scans must be in the ladder afterwards, and a reader
//! that asks while it scans must be told the ladder is loaded.
//!
//! Each test holds one re-read where it is (`Ladder::on_scan`), lets a second
//! thread arrive beside it, and only then lets it go on. Nothing here is a
//! rate: whichever way the re-read is written, the arrival is inside its scan.
//! A re-read that scans without holding the ladder's lock lets the arrival in
//! at once, and the test waits for it to finish before it lets the scan go on;
//! one that scans holding the lock keeps the arrival out until the swap, and
//! the test lets the scan go on first. The lock is asked with `try_lock`, which
//! never waits, from the scanning thread itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use theseus_judge::learn::CanaryEvent;
use theseus_protocol::packs::PackModeRow;
use theseus_protocol::LedgerKind;

use super::*;

/// A pack wired live whose own rule rolls it back on one event.
const PACK: &str = "loop.v1";

/// The event that rule counts: a nudged turn that did nothing new.
fn nudge() -> CanaryEvent {
    CanaryEvent::NudgedTurnEnded {
        task: "tsk_heron".into(),
        new_tool_calls: 0,
        final_before: "I will run the tests next.".into(),
        final_after: "I will run the tests next.".into(),
    }
}

/// A ladder on a store of its own, read once and with its adoptions
/// written, on a clock that does not move: every re-read below starts from a
/// loaded ladder with nothing left to write.
fn loaded() -> (tempfile::TempDir, Arc<Ladder>) {
    let dir = tempfile::tempdir().unwrap();
    let l = Arc::new(Ladder::new(Store::open(dir.path()).unwrap()));
    l.set_clock(Arc::new(|| 1_790_000_000_000));
    l.reload();
    assert!(l.is_loaded(), "read");
    assert_eq!(l.standing(PACK).rung, Rung::Live, "wired live, no row yet");
    (dir, l)
}

/// The modes of a pack's `pack.mode` rows in the store, oldest first: what is
/// durable, whatever the ladder holds.
fn stored(l: &Ladder, pack: &str) -> Vec<String> {
    l.store()
        .scope_after(&scope(pack), 0)
        .unwrap()
        .into_iter()
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == LedgerKind::PackMode.as_str())
        .map(|r| serde_json::from_value::<PackModeRow>(r.data).unwrap())
        .filter(|r| r.pack == pack)
        .map(|r| r.mode)
        .collect()
}

/// A re-read held at a point of its scan.
struct Held {
    /// Whether the ladder's lock was free at that point: the re-read scans
    /// without holding it, or does not.
    free: bool,
    go: Sender<()>,
    reread: JoinHandle<()>,
}

impl Held {
    /// Let the scan go on, and wait for the re-read to end.
    fn release(self) {
        self.go.send(()).unwrap();
        self.reread.join().unwrap();
    }
}

/// Start a re-read of `l`, and hold it the first time it passes `at`.
fn hold(l: &Arc<Ladder>, at: Scan) -> Held {
    let (reached_tx, reached) = channel();
    let (go, go_rx) = channel::<()>();
    let (go_rx, fired) = (Mutex::new(go_rx), AtomicBool::new(false));
    l.on_scan(Some(Arc::new(move |l: &Ladder, now: Scan| {
        if now != at || fired.swap(true, Ordering::SeqCst) {
            return;
        }
        reached_tx.send(l.loaded.try_lock().is_ok()).unwrap();
        // Dropped by a test that failed first: the scan goes on.
        go_rx.lock().unwrap().recv().ok();
    })));
    let reread = {
        let l = l.clone();
        thread::spawn(move || l.reload())
    };
    let free = reached
        .recv_timeout(Duration::from_secs(30))
        .expect("the re-read passed its scan point");
    Held { free, go, reread }
}

/// A thread that arrived beside a held re-read.
enum Beside<T> {
    /// It ran to its end inside the scan.
    Landed(T),
    /// It waits for the ladder's lock, which the scan holds.
    Waiting(JoinHandle<T>),
}

impl<T: Send + 'static> Beside<T> {
    /// Start `work` beside `held`: with the lock free, wait for it to end
    /// before the scan goes on; with the lock held, never wait (it cannot end
    /// before the scan does).
    fn arrive(held: &Held, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let h = thread::spawn(work);
        match held.free {
            true => Beside::Landed(h.join().unwrap()),
            false => Beside::Waiting(h),
        }
    }

    /// What it came to, once the re-read is let go.
    fn finish(self) -> T {
        match self {
            Beside::Landed(t) => t,
            Beside::Waiting(h) => h.join().unwrap(),
        }
    }
}

/// A writer that lands while a re-read is scanning is in the ladder after
/// it. The nudge rolls `loop.v1` back at once: its row is appended to the
/// store and pushed to the loaded rows, inside the scan, after the scan read
/// the pack's scope. A re-read that swaps in what it read without holding the
/// lock drops the row (and the event) from the ladder, and the pack, rolled
/// back and so recorded as such in the store, stands live in memory until
/// something else reads the store again.
#[test]
fn a_write_that_lands_inside_a_re_read_is_not_lost_to_it() {
    let (_dir, l) = loaded();
    let held = hold(&l, Scan::Read);
    let free = held.free;
    let writer = {
        let l = l.clone();
        Beside::arrive(&held, move || l.land(PACK, nudge()))
    };
    held.release();
    let written = writer.finish();
    assert_eq!(written.len(), 1, "the rule fired: a rollback was written");
    assert_eq!(
        stored(&l, PACK).last().map(String::as_str),
        Some("rolled_back"),
        "durable, whatever the ladder says"
    );
    assert_eq!(
        l.standing(PACK).rung,
        Rung::RolledBack,
        "the rollback is in the ladder (the writer landed {} the scan)",
        if free { "inside" } else { "after" }
    );
    assert_eq!(
        l.rows_of(PACK).last().map(|r| r.mode.as_str()),
        Some("rolled_back")
    );
}

/// A row a caller wrote in a frame of its own, outside the ladder, and then
/// asked the ladder to read again (`rpc::packs`, `judge::notice`), is in the
/// ladder after that re-read, even when another re-read had begun scanning
/// before the row was written and swaps in what it scanned after the second
/// has finished. The caller did what the ladder asks of it, so the older scan
/// must not put its answer over the newer one.
#[test]
fn a_re_read_that_began_earlier_does_not_put_its_scan_over_a_later_one() {
    let (_dir, l) = loaded();
    let held = hold(&l, Scan::Read);
    let free = held.free;
    // The caller's frame, outside the ladder, after the first scan read the store.
    let row = PackModeRow {
        pack: PACK.into(),
        mode: Rung::RolledBack.as_str().into(),
        from: Rung::Live.as_str().into(),
        who: "owner".into(),
        by: "owner".into(),
        via: "cli".into(),
        why: "the owner rolled it back".into(),
        ..PackModeRow::default()
    };
    l.store().append(&[l.row_record(&row).unwrap()]).unwrap();
    // And then it asks for the ladder to be read again.
    let second = {
        let l = l.clone();
        Beside::arrive(&held, move || l.reload())
    };
    held.release();
    second.finish();
    assert_eq!(
        stored(&l, PACK).last().map(String::as_str),
        Some("rolled_back")
    );
    assert_eq!(
        l.standing(PACK).rung,
        Rung::RolledBack,
        "the row its caller asked to be read is in the ladder (the second re-read ran {} the first's scan)",
        if free { "inside" } else { "after" }
    );
}

/// A ladder that has been read is loaded to whoever asks while it is read
/// again, and the re-read that cannot read leaves what it had: a rollback
/// held in memory is still held, not a ladder emptied back to every pack's
/// wired line. (Asked the same way, with the same arrival, whichever way the
/// re-read is written: an asker either answers at once or waits for the
/// re-read, and is told `true` in both.)
#[test]
fn a_ladder_is_loaded_to_whoever_asks_while_it_is_read_again() {
    for unreadable in [false, true] {
        let (_dir, l) = loaded();
        let rolled = l.land(PACK, nudge());
        assert_eq!(rolled.len(), 1, "a rollback, to be held in memory");
        let before = l.rows_of(PACK);
        assert_eq!(l.standing(PACK).rung, Rung::RolledBack);
        l.store().fail_scope_reads(unreadable);
        let held = hold(&l, Scan::Begun);
        let asker = {
            let l = l.clone();
            Beside::arrive(&held, move || l.is_loaded())
        };
        held.release();
        l.store().fail_scope_reads(false);
        assert!(
            asker.finish(),
            "unreadable: {unreadable}: asked during the re-read"
        );
        assert!(l.is_loaded(), "unreadable: {unreadable}: asked after");
        assert_eq!(
            l.standing(PACK).rung,
            Rung::RolledBack,
            "unreadable: {unreadable}: what was held is held, not emptied"
        );
        assert_eq!(
            l.rows_of(PACK),
            before,
            "unreadable: {unreadable}: its rows"
        );
    }
}
