//! A found segment's name, vouched for (theseus-3q29): an open that knows a
//! position in the segment it appends to was synced (the index's checkpoint,
//! or a frame's mark at or past the segment's first position) knows a sync
//! covering the segment's first frame returned Ok, and such a sync syncs the
//! log's directory before it returns (theseus-xprd). So it leaves the
//! directory out of its first sync. Only a segment nothing vouches for pays
//! c67g's sync, once.

use super::*;

/// The open under test: segments large enough that its frames roll none.
fn cfg(synced_to: u64) -> WalConfig {
    WalConfig {
        synced_to,
        ..WalConfig::default()
    }
}

/// A log of two segments: segment 1 full, then `batches` synced batches of
/// one frame each in segment 2. Each frame's location, by position.
fn two_segments(dir: &Path, batches: usize) -> Vec<RecordLocation> {
    // Segments of two frames: a frame of a 64-byte payload is 108 bytes.
    let wal = Wal::open(
        dir,
        WalConfig {
            segment_bytes: 300,
            ..WalConfig::default()
        },
    )
    .unwrap();
    let mut locs = Vec::new();
    while wal.segment_count() < 2 {
        locs.push(wal.append(&[rec(kinds::LEDGER, None, &[1u8; 64])]).unwrap()[0].1);
    }
    for _ in 1..batches {
        locs.push(wal.append(&[rec(kinds::LEDGER, None, &[2u8; 64])]).unwrap()[0].1);
    }
    assert_eq!(locs.last().unwrap().segment, 2);
    assert_eq!(locs.iter().filter(|l| l.segment == 2).count(), batches);
    locs
}

/// The directory syncs of this open's first frame, and of its second.
fn first_frames_dir_syncs(wal: &Wal) -> (u64, u64) {
    wal.append(&[rec(kinds::LEDGER, None, b"first")]).unwrap();
    let first = wal.dir_syncs();
    wal.append(&[rec(kinds::LEDGER, None, b"second")]).unwrap();
    (first, wal.dir_syncs() - first)
}

/// Two synced batches in the found segment: the second's mark is the
/// first's position, in the segment, so the open skips the sync, with no
/// checkpoint at all.
#[test]
fn a_mark_in_the_found_segment_vouches_for_its_name() {
    let dir = tempfile::tempdir().unwrap();
    two_segments(dir.path(), 2);
    let wal = Wal::open(dir.path(), cfg(0)).unwrap();
    assert_eq!(wal.recovery().vouched, Some(Vouch::Mark));
    assert!(wal.unsynced_dirs.lock().unwrap().is_empty());
    assert_eq!(first_frames_dir_syncs(&wal), (0, 0));
}

/// One batch in the found segment, whose mark is segment 1's last position:
/// the checkpoint at its position vouches instead.
#[test]
fn a_checkpoint_in_the_found_segment_vouches_for_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let locs = two_segments(dir.path(), 1);
    let last = locs.len() as u64;
    let wal = Wal::open(dir.path(), cfg(last)).unwrap();
    assert_eq!(wal.recovery().vouched, Some(Vouch::Checkpoint));
    assert_eq!(first_frames_dir_syncs(&wal), (0, 0));
}

/// The tail-only open: the checkpoint's record in the found segment vouches
/// by itself, whatever `synced_to` says; with it in segment 1, a mark in the
/// found segment vouches.
#[test]
fn the_tail_only_open_is_vouched_for_as_the_full_walk_is() {
    let dir = tempfile::tempdir().unwrap();
    let locs = two_segments(dir.path(), 2);
    let last = locs.len() as u64;
    let (wal, _) = Wal::open_from(dir.path(), cfg(0), last, Some(locs[last as usize - 1])).unwrap();
    assert!(wal.recovery().checked_from.is_some(), "the tail-only open");
    assert_eq!(wal.recovery().vouched, Some(Vouch::Checkpoint));
    assert_eq!(first_frames_dir_syncs(&wal), (0, 0));
    drop(wal);

    let dir = tempfile::tempdir().unwrap();
    let locs = two_segments(dir.path(), 2);
    let in_one = locs.iter().filter(|l| l.segment == 1).count();
    let (wal, tail) = Wal::open_from(
        dir.path(),
        cfg(in_one as u64),
        in_one as u64,
        Some(locs[in_one - 1]),
    )
    .unwrap();
    assert_eq!(
        wal.recovery().checked_from,
        Some((1, locs[in_one - 1].offset + u64::from(locs[in_one - 1].len)))
    );
    assert_eq!(tail.len(), 2, "segment 2's two frames");
    assert_eq!(wal.recovery().vouched, Some(Vouch::Mark));
    assert_eq!(first_frames_dir_syncs(&wal), (0, 0));
}

/// The found segment's one frame carries segment 1's last position as its
/// mark, and the checkpoint is there too: they vouch for segment 1's name,
/// not segment 2's, so the first frame pays the sync, once. Both walks.
#[test]
fn a_mark_for_the_segment_before_vouches_for_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let locs = two_segments(dir.path(), 1);
    let in_one = locs.len() - 1;
    let wal = Wal::open(dir.path(), cfg(in_one as u64)).unwrap();
    assert_eq!(wal.synced(), in_one as u64);
    assert_eq!(wal.recovery().vouched, None);
    assert_eq!(
        *wal.unsynced_dirs.lock().unwrap(),
        vec![dir.path().to_path_buf()]
    );
    assert_eq!(first_frames_dir_syncs(&wal), (1, 0));
    drop(wal);

    let dir = tempfile::tempdir().unwrap();
    let locs = two_segments(dir.path(), 1);
    let (wal, _) = Wal::open_from(
        dir.path(),
        cfg(in_one as u64),
        in_one as u64,
        Some(locs[in_one - 1]),
    )
    .unwrap();
    assert!(wal.recovery().checked_from.is_some(), "the tail-only open");
    assert_eq!(wal.recovery().vouched, None);
    assert_eq!(first_frames_dir_syncs(&wal), (1, 0));
}

/// A found segment with no frame (its creator died between the roll and its
/// first frame) has no position to vouch for: the first frame pays, even
/// with every position checkpointed, in both walks.
#[test]
fn an_empty_found_segment_is_never_vouched_for() {
    for tail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let locs = two_segments(dir.path(), 2);
        let last = locs.len() as u64;
        File::create(segment_path(dir.path(), 3)).unwrap();
        let at = tail.then(|| locs[last as usize - 1]);
        let (wal, _) = Wal::open_from(dir.path(), cfg(last), last, at).unwrap();
        assert_eq!(wal.recovery().checked_from.is_some(), tail);
        assert_eq!(wal.recovery().segments, 3);
        assert_eq!(wal.recovery().vouched, None, "tail-only {tail}");
        assert_eq!(first_frames_dir_syncs(&wal), (1, 0), "tail-only {tail}");
    }
}
