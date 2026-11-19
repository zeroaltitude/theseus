//! What `stats` says of the log, read where the writer keeps it, not under its lock (`wal::stand`): the same numbers
//! as the locked reads for a store at rest, through appends, a roll to a new segment and a cut after a failed sync;
//! and an answer while the writer is held inside a frame's write, as a disk that stalls holds it.

use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::record::kinds;

fn rec(i: usize) -> NewRecord {
    NewRecord::bytes(kinds::LEDGER, None, vec![i as u8; 40])
}

fn small_segments(dir: &Path) -> WalStore {
    let cfg = WalConfig {
        segment_bytes: 200,
        ..WalConfig::default()
    };
    WalStore::open(dir, cfg).unwrap().with_checkpoint_every(0)
}

/// The three numbers `stats` shows, and the locked reads of the same.
fn both(s: &WalStore) -> ((u64, u64, u32), (u64, u64, u32)) {
    let st = s.stats().unwrap();
    let w = &s.inner.wal;
    (
        (st.last_position, st.wal_bytes, st.wal_segments),
        (w.last_position(), w.total_bytes(), w.segment_count()),
    )
}

#[test]
fn stats_say_what_the_locked_reads_say_through_appends_and_a_roll() {
    let dir = tempfile::tempdir().unwrap();
    let s = small_segments(dir.path());
    let (shown, locked) = both(&s);
    assert_eq!(shown, locked, "a store just opened");
    for i in 0..12 {
        s.append(&[rec(i)]).unwrap();
        let (shown, locked) = both(&s);
        assert_eq!(shown, locked, "after append {i}");
    }
    let (shown, _) = both(&s);
    assert!(shown.2 > 1, "the log rolled to a new segment: {shown:?}");
    assert_eq!(shown.0, 12);
    drop(s);
    let s = small_segments(dir.path());
    let (shown, locked) = both(&s);
    assert_eq!(shown, locked, "after a reopen");
    assert_eq!(shown.0, 12);
}

/// A sync that fails cuts its frame back off the log, and the numbers go back with it. (In a segment with room: a
/// frame that rolls fails in the roll's own sync, before anything is written after the last durable frame.)
#[test]
fn stats_go_back_with_a_cut_after_a_failed_sync() {
    let dir = tempfile::tempdir().unwrap();
    let s = WalStore::open(dir.path(), WalConfig::default()).unwrap();
    for i in 0..3 {
        s.append(&[rec(i)]).unwrap();
    }
    let (before, _) = both(&s);
    s.inner.wal.fail_next_sync();
    // The answer comes before the sync (theseus-7yhp), so the append may say Ok: the writer is done when it can be
    // taken for writing, as a checkpoint takes it.
    let _ = s.append(&[rec(99)]);
    drop(s.inner.appending.write().unwrap());
    let (shown, locked) = both(&s);
    assert_eq!(shown, locked, "after a cut");
    assert_eq!(shown, before, "the cut frame is not counted");
}

/// The writer is held inside a frame's write (a gate it waits at, no clock), holding the lock a stalled disk would
/// hold it under: `stats` answers, with the numbers from before the frame. Let go, the frame lands and they move.
/// Without the fix `stats` waits for the lock and the answer never comes in this window (it comes only after the
/// gate is opened, which the test does by itself once the wait is past).
#[test]
fn stats_answer_while_the_writer_is_inside_its_write() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(small_segments(dir.path()));
    s.append(&[rec(0)]).unwrap();
    let gate = s.inner.wal.hold_writes();
    let appended = {
        let s = s.clone();
        std::thread::spawn(move || s.append(&[rec(1)]).unwrap())
    };
    gate.entered();
    let (said, heard) = mpsc::channel();
    {
        let s = s.clone();
        std::thread::spawn(move || {
            let _ = said.send(s.stats().unwrap().last_position);
        });
    }
    let answer = heard.recv_timeout(Duration::from_secs(20));
    gate.release();
    appended.join().unwrap();
    assert_eq!(
        answer,
        Ok(1),
        "stats answered while the write was held, with the position before it"
    );
    assert_eq!(
        s.stats().unwrap().last_position,
        2,
        "and moved when it landed"
    );
}
