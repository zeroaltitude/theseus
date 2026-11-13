//! The sync comes after the batch's answers (theseus-7yhp): a frame's caller
//! goes on — a turn's model request leaves — while the disk takes the frame,
//! and the index answers that frame already. The owner's call, 2026-11-12
//! 19:24, asked of exactly this: "YES, OPTION (A) — the index may show a
//! provider call's plan frame before that frame's sync has landed."
//!
//! What it costs is here too: a crash in that window loses the frames the
//! answer promised, and a sync that *fails* in it stops the store, because
//! the index holds frames the log has cut and no position of theirs may be
//! written again.

use super::*;
use crate::record::kinds;

fn open(dir: &Path) -> WalStore {
    WalStore::open(dir, WalConfig::default())
        .unwrap()
        .with_checkpoint_every(0)
}

fn rec(key: &str) -> NewRecord {
    NewRecord::json(kinds::META, Some(key), &key).unwrap()
}

/// Wait until the writer has finished the batch it holds: it takes
/// `appending` for reading from its first write through its sync, so taking
/// it alone is the barrier a checkpoint takes.
fn writer_idle(s: &WalStore) {
    drop(s.inner.appending.write().unwrap());
}

/// Counted and ordered, not timed: with the batch's sync held as a disk under
/// writeback holds one, the append is answered, the index answers the record
/// it wrote, and the log still knows no position past the frame before it.
/// Then the sync is let go and the frame is durable.
#[test]
fn a_frames_answer_comes_before_its_sync_and_after_its_index() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(open(dir.path()));
    s.append(&[rec("before")]).unwrap();
    writer_idle(&s);
    let synced_before = s.stats().unwrap().syncs;
    assert_eq!(s.inner.wal.synced(), 1, "the first frame is durable");
    let (release, hold) = mpsc::channel::<()>();
    *s.inner.sync_hold.lock().unwrap() = Some(hold);
    let appended = {
        let s = s.clone();
        std::thread::spawn(move || s.append(&[rec("one")]).unwrap())
    };
    // The answer comes while the sync is still held: a test that waited for
    // the sync instead would hang here, and the assertions below say what
    // the answer did and did not mean.
    assert_eq!(appended.join().unwrap(), vec![2], "answered, at position 2");
    assert_eq!(
        s.stats().unwrap().syncs,
        synced_before,
        "its sync has not run"
    );
    assert_eq!(
        s.inner.wal.synced(),
        1,
        "no position past the first frame is known synced"
    );
    assert_eq!(
        s.latest_by_key(kinds::META, "one")
            .unwrap()
            .expect("the index answers the frame already (K1)")
            .decode::<String>()
            .unwrap(),
        "one"
    );
    drop(release);
    writer_idle(&s);
    assert_eq!(s.stats().unwrap().syncs, synced_before + 1, "then it syncs");
    assert_eq!(s.inner.wal.synced(), 2, "and the frame is durable");
}

/// A later frame's sync covers the one before it, so the frame a turn's reply
/// writes is durable only once every frame of the turn is: the answer that
/// came early bought the turn its request, not a hole in the log. Here the
/// second batch's sync is the first batch's too (one fdatasync for both),
/// and a reopen holds them both.
#[test]
fn a_later_frames_sync_covers_the_frame_answered_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(open(dir.path()));
    let (release, hold) = mpsc::channel::<()>();
    *s.inner.sync_hold.lock().unwrap() = Some(hold);
    let first = {
        let s = s.clone();
        std::thread::spawn(move || s.append(&[rec("plan")]).unwrap())
    };
    assert_eq!(first.join().unwrap(), vec![1]);
    assert_eq!(s.inner.wal.synced(), 0, "nothing is durable yet");
    drop(release);
    s.append(&[rec("reply")]).unwrap();
    writer_idle(&s);
    assert_eq!(
        s.inner.wal.synced(),
        2,
        "the reply's sync covers the plan frame too"
    );
    drop(s);
    let s = open(dir.path());
    assert_eq!(s.last_position(), 2, "both frames came back");
    assert!(s.latest_by_key(kinds::META, "plan").unwrap().is_some());
}
