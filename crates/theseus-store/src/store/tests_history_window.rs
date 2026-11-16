//! The history check reads a window of a segment at a time, and keeps nothing
//! it has read (theseus-mem-daemon-rest).

use super::*;
use crate::record::kinds;

const SEGMENT: u64 = 4096;

/// A log of `n` frames of 20-byte payloads across several segments, one a
/// frame of `big` bytes, checkpointed and reopened.
fn log(dir: &Path, n: u32, big: Option<(u32, usize)>) -> WalStore {
    let cfg = WalConfig {
        segment_bytes: SEGMENT,
        ..Default::default()
    };
    let s = WalStore::open(dir, cfg.clone())
        .unwrap()
        .with_checkpoint_every(0);
    for i in 0..n {
        let len = big.filter(|b| b.0 == i).map_or(20, |b| b.1);
        s.append(&[NewRecord::json(kinds::LEDGER, None, &vec![i as u8; len]).unwrap()])
            .unwrap();
    }
    s.checkpoint().unwrap();
    drop(s);
    WalStore::open(dir, cfg).unwrap()
}

fn seg(dir: &Path, n: u32) -> PathBuf {
    dir.join("wal").join(format!("{n:09}.seg"))
}

/// Whatever the window, the check reaches the same verdict on the same
/// frames, and holds no more than a window (or the largest frame) at once.
#[test]
fn a_windowed_check_proves_the_same_log_and_holds_a_window() {
    let dir = tempfile::tempdir().unwrap();
    let s = log(dir.path(), 120, Some((60, 900)));
    let mark = |h: &crate::wal::History| {
        h.verified
            .map(|v| (v.segment, v.offset, v.first, v.position))
    };
    let whole = s
        .history_check()
        .with_window(usize::MAX / 2)
        .run(|_| {})
        .unwrap();
    assert_eq!(whole.records, 120);
    assert!(whole.segments >= 5, "{whole:?}");
    assert!(
        whole.peak_bytes >= SEGMENT / 2,
        "unwindowed holds a segment"
    );
    // A window smaller than every frame holds the largest frame, no more.
    let largest = s
        .history_check()
        .with_window(1)
        .run(|_| {})
        .unwrap()
        .peak_bytes;
    assert!(
        largest > 900,
        "the 900-byte frame is the largest: {largest}"
    );
    for window in [1, 64, 100, 700, 1000, 2000] {
        let h = s.history_check().with_window(window).run(|_| {}).unwrap();
        assert_eq!(
            (h.segments, h.frames, h.records, h.bytes, mark(&h)),
            (
                whole.segments,
                whole.frames,
                whole.records,
                whole.bytes,
                mark(&whole)
            ),
            "window {window}"
        );
        assert!(
            h.peak_bytes <= (window as u64).max(largest),
            "window {window}: held {}",
            h.peak_bytes
        );
        assert!(h.peak_bytes < whole.peak_bytes, "window {window}: {h:?}");
    }
    assert_eq!(s.history_check().run(|_| {}).unwrap().records, 120);
}

/// A mark in the middle of a segment starts the windowed check there, and
/// the check from a mark proves the same tail.
#[test]
fn a_windowed_check_from_a_mark_proves_the_same_tail() {
    let dir = tempfile::tempdir().unwrap();
    let s = log(dir.path(), 120, None);
    let first = s.history_check().run(|_| {}).unwrap();
    let mark = first.verified.unwrap();
    let from_mark = s.history_check().from_mark(Some(mark)).run(|_| {}).unwrap();
    for window in [1, 90, 300] {
        let h = s
            .history_check()
            .with_window(window)
            .from_mark(Some(mark))
            .run(|_| {})
            .unwrap();
        assert_eq!(
            (h.records, h.from_position),
            (from_mark.records, from_mark.from_position),
            "window {window}"
        );
        assert_eq!(
            h.verified.map(|v| (v.segment, v.offset, v.position)),
            from_mark
                .verified
                .map(|v| (v.segment, v.offset, v.position))
        );
    }
}

/// A corrupt frame is found at every window size, in the same segment and
/// at the same offset, and its reads are refused: a window never hides it,
/// whichever side of an edge it falls.
#[test]
fn a_windowed_check_finds_a_corrupt_frame_wherever_the_window_ends() {
    let dir = tempfile::tempdir().unwrap();
    let s = log(dir.path(), 120, None);
    drop(s);
    // Flip a payload byte of the third segment's second frame.
    let path = seg(dir.path(), 3);
    let mut b = std::fs::read(&path).unwrap();
    let first_len = 12 + u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    b[first_len + 12 + 8 + 4 + 28 + 3] ^= 0x01;
    std::fs::write(&path, &b).unwrap();
    let mut found = None;
    for window in [1, 64, 97, 130, 1000, 4096, usize::MAX / 2] {
        let s = WalStore::open(
            dir.path(),
            WalConfig {
                segment_bytes: SEGMENT,
                ..Default::default()
            },
        )
        .unwrap();
        let e = s
            .history_check()
            .with_window(window)
            .run(|_| {})
            .unwrap_err();
        let at = match &e {
            crate::wal::WalError::Corrupt {
                segment, offset, ..
            } => (*segment, *offset),
            other => panic!("window {window}: {other}"),
        };
        assert_eq!(*found.get_or_insert(at), at, "window {window}");
        assert_eq!(at.0, 3, "window {window}");
    }
}

/// A frame that states a length past its segment's end is torn however it
/// is read: the check says so without reading the length it states.
#[test]
fn a_windowed_check_refuses_a_length_past_the_segments_end() {
    let dir = tempfile::tempdir().unwrap();
    let s = log(dir.path(), 120, None);
    drop(s);
    let path = seg(dir.path(), 2);
    let mut b = std::fs::read(&path).unwrap();
    // The first frame's body length, made 3 GiB.
    b[4..8].copy_from_slice(&0xC000_0000u32.to_le_bytes());
    std::fs::write(&path, &b).unwrap();
    for window in [64, 1000, usize::MAX / 2] {
        let s = WalStore::open(
            dir.path(),
            WalConfig {
                segment_bytes: SEGMENT,
                ..Default::default()
            },
        )
        .unwrap();
        let check = s.history_check().with_window(window);
        let e = check.run(|_| {}).unwrap_err();
        assert!(
            matches!(&e, crate::wal::WalError::Corrupt { segment: 2, offset: 0, reason } if reason.contains("short frame body")),
            "window {window}: {e}"
        );
    }
}
