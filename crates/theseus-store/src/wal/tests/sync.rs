//! A failed sync (theseus-ljgm): the frames it covered are cut back off,
//! durably, so no open reads them back and no later sync's mark claims them.

use super::*;

fn frame(payload: &[u8]) -> [NewRecord; 1] {
    [NewRecord::bytes(
        crate::record::kinds::LEDGER,
        None,
        payload.to_vec(),
    )]
}

/// Every record of the log, as an open reads it: (position, payload).
fn read_back(dir: &Path) -> Vec<(u64, Vec<u8>)> {
    let wal = Wal::open(dir, WalConfig::default()).unwrap();
    assert_eq!(wal.recovery().truncated_bytes, 0, "nothing torn to cut");
    wal.replay_from(0)
        .unwrap()
        .into_iter()
        .map(|(r, _)| (r.position, r.payload))
        .collect()
}

/// A batch whose sync failed was answered failed, so no open may read it
/// back: its frames are cut back off before the sync returns, the next frame
/// takes its first position, and the log reads back exactly the frames whose
/// syncs returned Ok, in order, with no gap.
#[test]
fn a_failed_syncs_frames_never_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    wal.append(&frame(b"kept")).unwrap();
    let len = fs::metadata(segment_path(dir.path(), 1)).unwrap().len();
    // A batch of two frames, then its one sync, which fails.
    wal.write(&frame(b"lost one")).unwrap();
    wal.write(&frame(b"lost two")).unwrap();
    wal.fail_next_sync();
    wal.sync().unwrap_err();
    assert_eq!(
        fs::metadata(segment_path(dir.path(), 1)).unwrap().len(),
        len,
        "the failed batch's frames are cut back off"
    );
    assert_eq!(wal.last_position(), 1);
    let after = wal.append(&frame(b"after")).unwrap();
    assert_eq!(after[0].0, 2, "the next frame takes the cut one's position");
    assert_eq!(wal.read_at(after[0].1).unwrap().payload, b"after");
    let more = wal.append(&frame(b"more")).unwrap();
    assert_eq!(more[0].0, 3);
    drop(wal);
    assert_eq!(
        read_back(dir.path()),
        vec![
            (1, b"kept".to_vec()),
            (2, b"after".to_vec()),
            (3, b"more".to_vec())
        ]
    );
}

/// A sync that fails may have lost its frames' pages for good, and a later
/// fdatasync can still return Ok (fsyncgate): `synced`, which each later
/// frame carries as its mark, never covers a frame no sync made durable.
#[test]
fn a_later_good_sync_never_claims_a_failed_batch() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    wal.append(&frame(b"kept")).unwrap();
    assert_eq!(wal.synced(), 1);
    wal.write(&frame(b"failed")).unwrap();
    wal.fail_next_sync();
    wal.sync().unwrap_err();
    assert_eq!(wal.synced(), 1, "a failed sync moves nothing");
    wal.write(&frame(b"good")).unwrap();
    wal.sync().unwrap();
    // Every frame at or before `synced` is one a sync that returned Ok
    // covered: the failed one is not among them.
    let synced = wal.synced();
    drop(wal);
    let covered: Vec<Vec<u8>> = read_back(dir.path())
        .into_iter()
        .filter(|(p, _)| *p <= synced)
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(covered, vec![b"kept".to_vec(), b"good".to_vec()]);
}

/// The sync a roll makes of the segment it leaves fails: that segment's
/// unsynced frames are cut as a failed `sync`'s are, no segment is created,
/// and the next frame rolls as it would have.
#[test]
fn a_failed_sync_at_a_roll_cuts_the_segment_it_leaves() {
    let dir = tempfile::tempdir().unwrap();
    // Three 108-byte frames a segment.
    let cfg = WalConfig {
        segment_bytes: 400,
        ..WalConfig::default()
    };
    let wal = Wal::open(dir.path(), cfg).unwrap();
    wal.append(&frame(&[1u8; 64])).unwrap();
    wal.write(&frame(&[2u8; 64])).unwrap();
    wal.write(&frame(&[3u8; 64])).unwrap();
    wal.fail_next_sync();
    wal.write(&frame(&[4u8; 64])).unwrap_err();
    assert_eq!(list_segments(dir.path()).unwrap(), vec![1], "no roll");
    assert_eq!(wal.last_position(), 1, "the unsynced frames are cut");
    let placed = wal.append(&frame(&[5u8; 64])).unwrap();
    assert_eq!((placed[0].0, placed[0].1.segment), (2, 1));
    drop(wal);
    let back: Vec<u8> = read_back(dir.path())
        .into_iter()
        .map(|(_, p)| p[0])
        .collect();
    assert_eq!(back, vec![1, 5]);
}

/// A cut that fails leaves frames no sync made durable where the next frame
/// would go: the log takes no more, and says why.
#[test]
fn a_failed_cut_leaves_the_log_broken() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    wal.append(&frame(b"kept")).unwrap();
    wal.write(&frame(b"failed")).unwrap();
    wal.fail_next_sync();
    wal.fail_next_cut();
    wal.sync().unwrap_err();
    let err = wal.append(&frame(b"refused")).unwrap_err().to_string();
    assert!(err.contains("takes no more frames"), "{err}");
    assert!(err.contains("could not be cut back off"), "{err}");
    assert!(err.contains("a sync that failed"), "{err}");
    assert_eq!(wal.synced(), 1);
}

/// Frames an open found past the last position known synced may be ones its
/// writer answered Ok, so a failed sync cannot cut them, and no later sync
/// may claim them either: the log takes no more frames until a restart.
#[test]
fn a_failed_sync_over_frames_the_open_found_unsynced_breaks_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    wal.append(&frame(b"synced")).unwrap();
    wal.write(&frame(b"found unsynced")).unwrap();
    drop(wal);
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    assert_eq!((wal.last_position(), wal.synced()), (2, 1));
    wal.write(&frame(b"this one's")).unwrap();
    wal.fail_next_sync();
    wal.sync().unwrap_err();
    assert_eq!(wal.last_position(), 2, "its own frame is cut");
    let err = wal.append(&frame(b"refused")).unwrap_err().to_string();
    assert!(err.contains("found at its open"), "{err}");
    assert_eq!(wal.synced(), 1);
    drop(wal);
    // Once a sync of this log covers them, they are its to cut back to.
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    wal.write(&frame(b"next")).unwrap();
    wal.sync().unwrap();
    wal.write(&frame(b"failed")).unwrap();
    wal.fail_next_sync();
    wal.sync().unwrap_err();
    assert_eq!(wal.append(&frame(b"goes on")).unwrap()[0].0, 4);
}

/// `append` is a frame and its sync. When another caller's sync fails
/// between the two, and cuts this frame with its own, this append's sync can
/// return Ok over a log that no longer holds the frame: the append fails. A
/// frame a cut left whole stays answered Ok.
#[test]
fn an_append_whose_frame_another_failed_sync_cut_fails() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    let (kept, _, before) = wal.write_counted(&frame(b"kept")).unwrap();
    wal.sync().unwrap();
    // Another caller's frame, then this one's, then the other's sync, which
    // fails: both are cut.
    wal.write(&frame(b"another's")).unwrap();
    let (this, _, cuts) = wal.write_counted(&frame(b"this one")).unwrap();
    wal.fail_next_sync();
    wal.sync().unwrap_err();
    // This one's own sync returns Ok, and covers nothing of it.
    wal.sync().unwrap();
    assert_eq!(
        wal.cut_since(cuts, this[0].0),
        Some(2),
        "this frame is gone"
    );
    assert_eq!(
        wal.cut_since(before, kept[0].0),
        None,
        "the cut began past it"
    );
    let after = wal.append(&frame(b"after")).unwrap();
    assert_eq!(after[0].0, 2);
}
