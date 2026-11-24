use super::*;
use crate::record::kinds;

/// The synced mark (theseus-7nfj).
mod mark;
/// A failed sync (theseus-ljgm).
mod sync;
/// A found segment's name, vouched for (theseus-3q29).
mod vouch;

fn rec(kind: u16, key: Option<&str>, payload: &[u8]) -> NewRecord {
    NewRecord::bytes(kind, key, payload.to_vec())
}

#[test]
fn append_read_recover_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    let locs = wal
        .append(&[
            rec(kinds::SESSION, Some("s1"), b"hello"),
            rec(kinds::LEDGER, None, b"row"),
        ])
        .unwrap();
    assert_eq!(locs[0].0, 1);
    assert_eq!(locs[1].0, 2);
    let r = wal.read_at(locs[1].1).unwrap();
    assert_eq!(r.position, 2);
    assert_eq!(r.payload, b"row");
    drop(wal);
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    assert_eq!(wal.recovery().records, 2);
    assert_eq!(wal.last_position(), 2);
    let all = wal.replay_from(0).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].0.key.as_deref(), Some("s1"));
    // Appends continue the sequence.
    let more = wal.append(&[rec(kinds::META, Some("m"), b"x")]).unwrap();
    assert_eq!(more[0].0, 3);
}

#[test]
fn torn_tail_is_truncated_and_earlier_frames_survive() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    for i in 0..10u32 {
        wal.append(&[rec(kinds::LEDGER, None, &i.to_le_bytes())])
            .unwrap();
    }
    drop(wal);
    // Append garbage and a half frame.
    let path = segment_path(dir.path(), 1);
    let good_len = fs::metadata(&path).unwrap().len();
    {
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        let mut half = Vec::new();
        half.extend_from_slice(&MAGIC_MARKED.to_le_bytes());
        half.extend_from_slice(&(500u32).to_le_bytes());
        half.extend_from_slice(&(0u32).to_le_bytes());
        half.extend_from_slice(&[7u8; 40]);
        f.write_all(&half).unwrap();
    }
    // Every whole frame known synced: the torn one is past them all.
    let wal = Wal::open(
        dir.path(),
        WalConfig {
            synced_to: 10,
            ..WalConfig::default()
        },
    )
    .unwrap();
    assert_eq!(wal.recovery().records, 10);
    assert_eq!(wal.recovery().truncated_bytes, 52);
    let cut = wal.recovery().cut.expect("the cut is reported");
    assert_eq!((cut.offset, cut.bytes, cut.position), (good_len, 52, 11));
    assert_eq!(cut.whole_after, None, "nothing whole follows a torn tail");
    assert_eq!(fs::metadata(&path).unwrap().len(), good_len);
    assert_eq!(wal.last_position(), 10);
}

/// The offset of each frame in a segment's bytes.
fn frame_offsets(bytes: &[u8]) -> Vec<usize> {
    let mut offs = Vec::new();
    let mut off = 0usize;
    while off + FRAME_HEADER <= bytes.len() {
        offs.push(off);
        off += FRAME_HEADER + u32_at(bytes, off + 4) as usize;
    }
    offs
}

/// A log of five frames, one record each, written in `batches`: each
/// batch's frames back to back, then one sync, as the store's writer
/// writes them. So each frame's mark is the last position of the batch
/// before its own (theseus-7nfj).
fn five_frames_in(dir: &Path, batches: &[u8]) -> (PathBuf, Vec<u8>) {
    let wal = Wal::open(dir, WalConfig::default()).unwrap();
    let mut i = 0u8;
    for &n in batches {
        for _ in 0..n {
            i += 1;
            wal.write(&[rec(kinds::LEDGER, None, &[i; 40])]).unwrap();
        }
        wal.sync().unwrap();
    }
    assert_eq!(i, 5);
    drop(wal);
    let path = segment_path(dir, 1);
    let bytes = fs::read(&path).unwrap();
    assert_eq!(frame_offsets(&bytes).len(), 5);
    (path, bytes)
}

/// Five frames, each its own batch: each frame's mark is the position
/// before it.
fn five_frames(dir: &Path) -> (PathBuf, Vec<u8>) {
    five_frames_in(dir, &[1, 1, 1, 1, 1])
}

/// theseus-gt12, rot: frame 3 of the last segment goes bad after the
/// whole log was synced (position 5 known synced, as a store's open
/// knows its checkpoint). Cutting it as a torn tail would lose frames 3
/// to 5, all acknowledged; the open refuses, names the position and the
/// repair, and cuts nothing.
#[test]
fn a_synced_frame_gone_bad_in_the_last_segment_is_refused_not_cut() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames(dir.path());
    let third = frame_offsets(&bytes)[2];
    bytes[third + FRAME_HEADER + 30] ^= 0x10;
    fs::write(&path, &bytes).unwrap();
    for synced_to in [3, 5] {
        let err = Wal::open(
            dir.path(),
            WalConfig {
                synced_to,
                ..WalConfig::default()
            },
        )
        .err()
        .expect("a synced frame gone bad is refused, not cut");
        let msg = err.to_string();
        assert!(
            matches!(err, WalError::Corrupt { segment: 1, offset, .. } if offset == third as u64),
            "{msg}"
        );
        assert!(msg.contains("position 3"), "{msg}");
        assert!(msg.contains("a whole frame follows it"), "{msg}");
        assert!(msg.contains("theseusd restore --repair"), "{msg}");
        assert_eq!(fs::read(&path).unwrap(), bytes, "nothing was cut");
    }
}

/// theseus-gt12, a torn batch: frames 3 to 5 were one batch, written back
/// to back, and the power went before its sync; frame 3's last page never
/// reached the disk, and frames 4 and 5 did. Nothing past position 2 was
/// acknowledged, and the batch's marks say only that much (theseus-7nfj),
/// so the batch is cut as a torn tail, with no index or with one, and the
/// cut says a whole frame followed it, and how far the log was synced.
#[test]
fn a_batch_torn_before_its_sync_is_cut_though_a_whole_frame_follows() {
    for synced_to in [0, 2] {
        let dir = tempfile::tempdir().unwrap();
        let (path, mut bytes) = five_frames_in(dir.path(), &[1, 1, 3]);
        let offs = frame_offsets(&bytes);
        let (third, fourth) = (offs[2], offs[3]);
        bytes[fourth - 16..fourth].fill(0);
        fs::write(&path, &bytes).unwrap();
        let wal = Wal::open(
            dir.path(),
            WalConfig {
                synced_to,
                ..WalConfig::default()
            },
        )
        .unwrap();
        let r = wal.recovery();
        assert_eq!(wal.last_position(), 2);
        assert_eq!(r.truncated_bytes, (bytes.len() - third) as u64);
        assert_eq!(
            r.cut,
            Some(Cut {
                segment: 1,
                offset: third as u64,
                bytes: (bytes.len() - third) as u64,
                position: 3,
                whole_after: Some(WholeAfter {
                    offset: fourth as u64,
                    first: 4
                }),
                synced_to: 2,
            }),
            "synced_to {synced_to}"
        );
        assert_eq!(fs::metadata(&path).unwrap().len(), third as u64);
    }
}

/// Where record `n` (1 to 5) of a `five_frames_in` log lies.
fn record_at(bytes: &[u8], n: usize) -> RecordLocation {
    let off = frame_offsets(bytes)[n - 1];
    RecordLocation {
        segment: 1,
        offset: (off + FRAME_HEADER + MARK + 4) as u64,
        len: (RECORD_HEADER + 40) as u32,
    }
}

/// theseus-gt12: the tail-only open (after the index's checkpoint, here
/// position 2) decides as the walk of every segment does: frame 3 gone
/// bad with position 5 known synced is refused, and torn with only
/// position 2 known synced is cut, with the whole frame after it said.
/// Frames 3 to 5 are one batch, so no mark says more.
#[test]
fn the_tail_only_open_refuses_rot_and_cuts_a_torn_batch_alike() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames_in(dir.path(), &[1, 1, 3]);
    let at = Some(record_at(&bytes, 2));
    let offs = frame_offsets(&bytes);
    bytes[offs[3] - 16..offs[3]].fill(0);
    fs::write(&path, &bytes).unwrap();
    let cfg = |synced_to| WalConfig {
        synced_to,
        ..WalConfig::default()
    };
    let err = Wal::open_from(dir.path(), cfg(5), 2, at)
        .err()
        .expect("refused");
    assert!(err.to_string().contains("position 3"), "{err}");
    assert_eq!(fs::read(&path).unwrap(), bytes, "nothing was cut");
    let (wal, tail) = Wal::open_from(dir.path(), cfg(2), 2, at).unwrap();
    assert!(tail.is_empty());
    let r = wal.recovery();
    assert!(r.checked_from.is_some(), "the tail-only open");
    assert_eq!(
        r.cut.map(|c| (c.whole_after, c.synced_to)),
        Some((
            Some(WholeAfter {
                offset: offs[3] as u64,
                first: 4
            }),
            2
        ))
    );
    assert_eq!(wal.last_position(), 2);
}

#[test]
fn corrupt_middle_frame_is_refused_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(
        dir.path(),
        WalConfig {
            segment_bytes: 200,
            ..Default::default()
        },
    )
    .unwrap();
    for i in 0..20u32 {
        wal.append(&[rec(kinds::LEDGER, None, &[i as u8; 40])])
            .unwrap();
    }
    assert!(wal.segment_count() > 1);
    drop(wal);
    // Flip a byte in the first segment's first frame body.
    let path = segment_path(dir.path(), 1);
    let mut bytes = fs::read(&path).unwrap();
    bytes[FRAME_HEADER + 10] ^= 0xFF;
    fs::write(&path, &bytes).unwrap();
    let err = Wal::open(dir.path(), WalConfig::default())
        .err()
        .expect("must refuse");
    assert!(matches!(err, WalError::Corrupt { segment: 1, .. }), "{err}");
}

#[test]
fn frame_is_atomic_two_records_or_none() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    wal.append(&[rec(kinds::LEDGER, None, b"a")]).unwrap();
    wal.append(&[
        rec(kinds::COMPLETION, Some("c1"), b"done"),
        rec(kinds::EXECUTION, Some("e1"), b"runnable"),
    ])
    .unwrap();
    drop(wal);
    // Truncate the file inside the second frame's body.
    let path = segment_path(dir.path(), 1);
    let len = fs::metadata(&path).unwrap().len();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(len - 3)
        .unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    // The pair is gone together; the first frame survives.
    assert_eq!(wal.last_position(), 1);
    assert_eq!(wal.recovery().records, 1);
}

/// A write cut short (a full disk) is cut back off: the next frame starts
/// where it did, and an open finds every frame written before and after,
/// where it used to end the log at the torn bytes and drop the rest.
#[test]
fn a_write_cut_short_is_cut_back_off_and_the_log_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    wal.append(&[rec(kinds::LEDGER, None, b"before")]).unwrap();
    let len = fs::metadata(segment_path(dir.path(), 1)).unwrap().len();
    wal.cut_next_write(20);
    let err = wal
        .append(&[rec(kinds::LEDGER, None, &[9u8; 64])])
        .unwrap_err();
    assert!(err.to_string().contains("cut short"), "{err}");
    assert_eq!(
        fs::metadata(segment_path(dir.path(), 1)).unwrap().len(),
        len,
        "the torn bytes are gone"
    );
    let after = wal.append(&[rec(kinds::LEDGER, None, b"after")]).unwrap();
    assert_eq!(after[0].0, 2, "the refused frame took no position");
    assert_eq!(wal.read_at(after[0].1).unwrap().payload, b"after");
    drop(wal);
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    assert_eq!(wal.recovery().records, 2);
    assert_eq!(wal.recovery().truncated_bytes, 0);
}

/// A segment's name is as durable as its frames (theseus-xprd). A kill
/// keeps the page cache, so only the syncs themselves show it: the sync
/// that makes a new segment's first frame durable syncs the log's
/// directory before the append returns, once a segment, and a new log's
/// first syncs the directory that holds the log too.
#[test]
fn a_new_segments_name_is_synced_before_its_first_frame_is_reported_durable() {
    let parent = tempfile::tempdir().unwrap();
    let dir = parent.path().join("wal");
    // Three 108-byte frames a segment.
    let cfg = WalConfig {
        segment_bytes: 400,
        ..WalConfig::default()
    };
    let dir_syncs = |w: &Wal| w.dir_syncs.load(std::sync::atomic::Ordering::Relaxed);
    let wal = Wal::open(&dir, cfg.clone()).unwrap();
    assert_eq!(dir_syncs(&wal), 0, "nothing is synced before a frame is");
    wal.append(&[rec(kinds::LEDGER, None, &[1u8; 64])]).unwrap();
    assert_eq!(
        dir_syncs(&wal),
        2,
        "segment 1's name, and the log directory's own"
    );
    for roll in 1..=3u64 {
        let segments = wal.segment_count();
        while wal.segment_count() == segments {
            let before = dir_syncs(&wal);
            wal.append(&[rec(kinds::LEDGER, None, &[2u8; 64])]).unwrap();
            if wal.segment_count() == segments {
                assert_eq!(dir_syncs(&wal), before, "a frame in the same segment");
            }
        }
        assert_eq!(
            dir_syncs(&wal),
            2 + roll,
            "roll {roll}: the new segment's name synced before its first frame's append returned"
        );
    }
    assert_eq!(wal.segment_count(), 4);
    drop(wal);
    // A log opened again creates no name until it rolls, but syncs the
    // name of the segment it appends to once, with its first frame: the
    // process that created it may have died before it did (theseus-c67g).
    let wal = Wal::open(&dir, cfg).unwrap();
    assert_eq!(dir_syncs(&wal), 0, "the open syncs nothing");
    wal.append(&[rec(kinds::LEDGER, None, &[3u8; 64])]).unwrap();
    assert_eq!(dir_syncs(&wal), 1, "the found segment's name, once");
    wal.append(&[rec(kinds::LEDGER, None, &[3u8; 64])]).unwrap();
    assert_eq!(dir_syncs(&wal), 1, "and not again");
    assert_eq!(wal.recovery().records, 10);
}

/// A segment whose creator died before its first frame's sync is synced
/// into the log's directory by the next process to sync a frame in it
/// (theseus-c67g): an open that finds a last segment puts the log's
/// directory in the first sync, once, and only the log's own.
#[test]
fn an_open_that_finds_its_last_segment_syncs_its_name_with_the_first_frame() {
    let parent = tempfile::tempdir().unwrap();
    let dir = parent.path().join("wal");
    let dir_syncs = |w: &Wal| w.dir_syncs.load(std::sync::atomic::Ordering::Relaxed);
    // The creator writes and dies before any sync: its segment exists,
    // and nothing synced the log's directory.
    let wal = Wal::open(&dir, WalConfig::default()).unwrap();
    wal.write(&[rec(kinds::LEDGER, None, b"unsynced")]).unwrap();
    assert_eq!(dir_syncs(&wal), 0);
    drop(wal);
    assert_eq!(list_segments(&dir).unwrap(), vec![1]);
    let wal = Wal::open(&dir, WalConfig::default()).unwrap();
    assert_eq!(
        *wal.unsynced_dirs.lock().unwrap(),
        vec![dir],
        "the log's directory, not the one holding it"
    );
    wal.write(&[rec(kinds::LEDGER, None, b"one")]).unwrap();
    wal.sync().unwrap();
    assert_eq!(
        dir_syncs(&wal),
        1,
        "synced before the first frame is durable"
    );
    wal.write(&[rec(kinds::LEDGER, None, b"two")]).unwrap();
    wal.sync().unwrap();
    assert_eq!(dir_syncs(&wal), 1, "once an open");
    drop(wal);
    // An open that creates segment 1 in a directory it finds: that one
    // name, as before.
    let fresh = parent.path().join("fresh");
    fs::create_dir(&fresh).unwrap();
    let wal = Wal::open(&fresh, WalConfig::default()).unwrap();
    wal.append(&[rec(kinds::LEDGER, None, b"first")]).unwrap();
    assert_eq!(dir_syncs(&wal), 1);
}

#[test]
fn scope_roundtrips_and_concurrent_appends_are_each_durable() {
    let dir = tempfile::tempdir().unwrap();
    let wal = std::sync::Arc::new(Wal::open(dir.path(), WalConfig::default()).unwrap());
    let locs = wal
        .append(&[rec(kinds::LEDGER, None, b"x").scoped("ses_1")])
        .unwrap();
    assert_eq!(
        wal.read_at(locs[0].1).unwrap().scope.as_deref(),
        Some("ses_1")
    );
    // 8 threads x 50 appends: every append durable when it returns. The
    // log alone syncs each; the store's writer is what batches them.
    let mut hs = Vec::new();
    for t in 0..8u8 {
        let w = wal.clone();
        hs.push(std::thread::spawn(move || {
            for i in 0..50u32 {
                w.append(&[rec(kinds::LEDGER, None, &[t, i as u8])])
                    .unwrap();
            }
        }));
    }
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(wal.frames_appended(), 401);
    assert!(wal.syncs() <= wal.frames_appended());
    assert!(wal.syncs() >= 1);
    drop(wal);
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    assert_eq!(wal.recovery().records, 401);
}

#[test]
fn full_wal_refuses_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(
        dir.path(),
        WalConfig {
            max_total_bytes: Some(300),
            ..Default::default()
        },
    )
    .unwrap();
    let mut ok = 0;
    for _ in 0..20 {
        match wal.append(&[rec(kinds::LEDGER, None, &[1u8; 64])]) {
            Ok(_) => ok += 1,
            Err(WalError::Full { .. }) => break,
            Err(e) => panic!("{e}"),
        }
    }
    assert!((2..20).contains(&ok));
    // Still readable and consistent after the refusal.
    let all = wal.replay_from(0).unwrap();
    assert_eq!(all.len(), ok);
}
