//! The writer's time over each frame (theseus-w7dk).

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

/// One frame made slow on purpose is the slowest the store names, with the
/// frame's own first position: not the first frame, not the last.
#[test]
fn the_slowest_frame_since_a_mark_is_the_one_made_slow() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let before = s.append(&[rec("before")]).unwrap();
    let since = std::time::Instant::now();
    assert_eq!(
        s.slowest_frame_since(since),
        None,
        "no frame since the mark"
    );
    let one = s.append(&[rec("one")]).unwrap();
    s.inner.commit_delay_ms.store(40, Ordering::Relaxed);
    let slow = s.append(&[rec("slow-a"), rec("slow-b")]).unwrap();
    let last = s.append(&[rec("last")]).unwrap();
    let got = s.slowest_frame_since(since).unwrap();
    assert_eq!(got.first, slow[0], "the slow frame's first record");
    assert!(got.us >= 40_000, "{got:?}");
    assert_ne!(got.first, one[0]);
    assert_ne!(got.first, last[0]);
    assert!(
        s.slowest_frame_since(std::time::Instant::now()).is_none(),
        "a mark after them sees none"
    );
    assert_eq!(before.len(), 1);
}

/// The ring keeps the newest frames, and the slowest of those.
#[test]
fn the_ring_forgets_the_oldest_frames() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let since = std::time::Instant::now();
    s.inner.commit_delay_ms.store(20, Ordering::Relaxed);
    let slow = s.append(&[rec("slow")]).unwrap();
    assert_eq!(s.slowest_frame_since(since).unwrap().first, slow[0]);
    for i in 0..70 {
        s.append(&[rec(&format!("k{i}"))]).unwrap();
    }
    let got = s.slowest_frame_since(since).unwrap();
    assert_ne!(got.first, slow[0], "70 frames later it is out of the ring");
}
