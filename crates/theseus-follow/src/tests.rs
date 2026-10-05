use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{Duration, Instant};

use theseus_store::{kinds, NewRecord, Wal, WalConfig};

use super::*;

fn cfg(segment_bytes: u64) -> WalConfig {
    WalConfig {
        segment_bytes,
        fsync: false,
        ..WalConfig::default()
    }
}

fn row(i: u32) -> NewRecord {
    NewRecord::bytes(kinds::LEDGER, None, format!("row {i}").into_bytes())
}

/// Every record a follower reads until it stops, with the stops' kinds.
fn drain(f: &mut WalFollower, max_bytes: usize) -> Vec<u64> {
    let mut out = Vec::new();
    loop {
        let b = f.read(max_bytes).unwrap();
        out.extend(b.records.iter().map(|r| r.position));
        if *f.stop() != Stop::Budget {
            return out;
        }
    }
}

fn seg(dir: &Path, n: u32) -> PathBuf {
    wal::segment_path(dir, n)
}

#[test]
fn it_reads_every_frame_in_order_from_the_start() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(1 << 20)).unwrap();
    for i in 0..5 {
        w.append(&[row(i), row(i + 100)]).unwrap();
    }
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    let b = f.read(1 << 20).unwrap();
    assert_eq!(b.frames, 5);
    assert_eq!(
        b.records.iter().map(|r| r.position).collect::<Vec<_>>(),
        (1..=10).collect::<Vec<_>>()
    );
    assert_eq!(b.records[1].payload, b"row 100");
    assert_eq!(*f.stop(), Stop::CaughtUp);
    assert_eq!(f.cursor().position, 10);
    let len = fs::metadata(seg(dir.path(), 1)).unwrap().len();
    assert_eq!(f.cursor().offset, len);
    // The bytes read, as a shipper would copy them: the whole segment so far.
    assert_eq!(b.spans, vec![(1, 0, len)]);
    // And each span's last position, which says where a record lives.
    assert_eq!(b.ends, vec![10]);
    assert_eq!(b.bytes, len);
    // Caught up: an empty read, and the cursor stays.
    let c = f.cursor().clone();
    assert!(f.read(1 << 20).unwrap().is_empty());
    assert_eq!(*f.cursor(), c);
    // A later frame's span starts where the last one ended.
    w.append(&[row(9)]).unwrap();
    let b = f.read(1 << 20).unwrap();
    let now = fs::metadata(seg(dir.path(), 1)).unwrap().len();
    assert_eq!(b.spans, vec![(1, len, now)]);
    assert_eq!(b.ends, vec![11]);
}

#[test]
fn an_empty_or_missing_log_is_caught_up() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("wal");
    let mut f = WalFollower::open(&missing, Cursor::start()).unwrap();
    assert!(f.read(1024).unwrap().is_empty());
    assert_eq!(*f.stop(), Stop::CaughtUp);
    // The store creates its first segment; the follower then reads it.
    let w = Wal::open(&missing, cfg(1 << 20)).unwrap();
    assert!(f.read(1024).unwrap().is_empty());
    w.append(&[row(1)]).unwrap();
    assert_eq!(f.read(1024).unwrap().records.len(), 1);
}

#[test]
fn it_stops_at_a_torn_tail_and_resumes_after_the_cores_repair() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(1 << 20)).unwrap();
    for i in 0..5 {
        w.append(&[row(i)]).unwrap();
    }
    drop(w);
    // A crash in the middle of a frame: its header, and part of its body.
    let mut torn = Vec::new();
    torn.extend_from_slice(&theseus_store::wal::MAGIC_MARKED.to_le_bytes());
    torn.extend_from_slice(&500u32.to_le_bytes());
    torn.extend_from_slice(&0u32.to_le_bytes());
    torn.extend_from_slice(&[7u8; 40]);
    OpenOptions::new()
        .append(true)
        .open(seg(dir.path(), 1))
        .unwrap()
        .write_all(&torn)
        .unwrap();

    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    assert_eq!(drain(&mut f, 1 << 20), vec![1, 2, 3, 4, 5]);
    let at = f.cursor().clone();
    assert!(
        matches!(f.stop(), Stop::Partial { segment: 1, offset, .. } if *offset == at.offset),
        "{:?}",
        f.stop()
    );
    // It waits there: never past the torn frame, never cutting it.
    let len = fs::metadata(seg(dir.path(), 1)).unwrap().len();
    assert!(f.read(1 << 20).unwrap().is_empty());
    assert_eq!(*f.cursor(), at);
    assert_eq!(fs::metadata(seg(dir.path(), 1)).unwrap().len(), len);

    // The core's open cuts the torn tail and appends on: the follower reads
    // the new frames from where it stopped.
    let w = Wal::open(dir.path(), cfg(1 << 20)).unwrap();
    assert_eq!(w.recovery().truncated_bytes, torn.len() as u64);
    w.append(&[row(6)]).unwrap();
    w.append(&[row(7), row(8)]).unwrap();
    assert_eq!(drain(&mut f, 1 << 20), vec![6, 7, 8]);
    assert_eq!(*f.stop(), Stop::CaughtUp);
    let back = f.read(1 << 20).unwrap();
    assert!(back.is_empty());
    // And a cursor saved at the torn tail reopens against the repaired log.
    WalFollower::open(dir.path(), at).unwrap();
}

#[test]
fn it_waits_on_a_frame_being_written_and_reads_it_once_whole() {
    let a = tempfile::tempdir().unwrap();
    let w = Wal::open(a.path(), cfg(1 << 20)).unwrap();
    for i in 0..3 {
        w.append(&[row(i)]).unwrap();
    }
    drop(w);
    // The next frame's bytes, as the writer would write them: from a copy
    // of the log that appended it.
    let b = tempfile::tempdir().unwrap();
    fs::copy(seg(a.path(), 1), seg(b.path(), 1)).unwrap();
    let before = fs::metadata(seg(a.path(), 1)).unwrap().len() as usize;
    Wal::open(b.path(), cfg(1 << 20))
        .unwrap()
        .append(&[row(3), row(4)])
        .unwrap();
    let frame = fs::read(seg(b.path(), 1)).unwrap()[before..].to_vec();

    let mut f = WalFollower::open(a.path(), Cursor::start()).unwrap();
    assert_eq!(drain(&mut f, 1 << 20), vec![1, 2, 3]);
    let mut file = OpenOptions::new()
        .append(true)
        .open(seg(a.path(), 1))
        .unwrap();
    for cut in [5, 20, frame.len() - 1] {
        let have = fs::metadata(seg(a.path(), 1)).unwrap().len() as usize - before;
        file.write_all(&frame[have..cut]).unwrap();
        assert!(
            f.read(1 << 20).unwrap().is_empty(),
            "a frame cut at {cut} was read"
        );
        assert!(matches!(f.stop(), Stop::Partial { .. }));
    }
    file.write_all(&frame[frame.len() - 1..]).unwrap();
    assert_eq!(drain(&mut f, 1 << 20), vec![4, 5]);
    assert_eq!(*f.stop(), Stop::CaughtUp);
}

#[test]
fn it_crosses_segment_rotations() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(256)).unwrap();
    for i in 0..30 {
        w.append(&[row(i)]).unwrap();
    }
    assert!(w.segment_count() >= 4, "{} segments", w.segment_count());
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    // Small reads, so the budget, a segment's end, and a rotation all fall
    // between reads and inside them.
    let mut seen = Vec::new();
    let mut sealed = Vec::new();
    let mut spans = Vec::new();
    loop {
        let b = f.read(90).unwrap();
        // One last position per span, rising, the batch's last its last.
        assert_eq!(b.ends.len(), b.spans.len());
        assert!(b.ends.windows(2).all(|w| w[0] < w[1]), "{:?}", b.ends);
        assert_eq!(b.ends.last().copied(), b.records.last().map(|r| r.position));
        seen.extend(b.records.iter().map(|r| r.position));
        sealed.extend(b.sealed);
        spans.extend(b.spans);
        if *f.stop() == Stop::CaughtUp {
            break;
        }
    }
    assert_eq!(seen, (1..=30).collect::<Vec<_>>());
    let last = w.segment_count();
    assert_eq!(sealed, (1..last).collect::<Vec<_>>());
    assert_eq!(f.cursor().segment, last);
    // The spans tile every segment once, in order: a shipper that copies
    // them copies each byte once.
    for s in 1..=last {
        let mut at = 0;
        for (_, from, to) in spans.iter().filter(|(seg, ..)| *seg == s) {
            assert_eq!(*from, at, "segment {s}: a gap or an overlap at {at}");
            at = *to;
        }
        assert_eq!(
            at,
            fs::metadata(seg(dir.path(), s)).unwrap().len(),
            "segment {s}"
        );
    }

    // The writer rolls again while the follower is caught up.
    for i in 30..60 {
        w.append(&[row(i)]).unwrap();
    }
    assert!(w.segment_count() > last);
    assert_eq!(drain(&mut f, 1 << 20), (31..=60).collect::<Vec<_>>());
    assert_eq!(f.cursor().segment, w.segment_count());
}

#[test]
fn a_frame_larger_than_the_budget_is_read_whole() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(1 << 20)).unwrap();
    w.append(&[NewRecord::bytes(kinds::NODE, Some("n"), vec![b'x'; 10_000])])
        .unwrap();
    w.append(&[row(2)]).unwrap();
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    let b = f.read(100).unwrap();
    assert_eq!(b.records.len(), 1);
    assert_eq!(b.records[0].payload.len(), 10_000);
    assert_eq!(*f.stop(), Stop::Budget);
    assert_eq!(drain(&mut f, 100), vec![2]);
}

#[test]
fn a_saved_cursor_reopens_where_it_left_off() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(300)).unwrap();
    for i in 0..10 {
        w.append(&[row(i)]).unwrap();
    }
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    assert_eq!(drain(&mut f, 1 << 20), (1..=10).collect::<Vec<_>>());
    let saved = serde_json::to_string(f.cursor()).unwrap();
    for i in 10..20 {
        w.append(&[row(i)]).unwrap();
    }
    let cursor: Cursor = serde_json::from_str(&saved).unwrap();
    let mut again = WalFollower::open(dir.path(), cursor).unwrap();
    assert_eq!(drain(&mut again, 1 << 20), (11..=20).collect::<Vec<_>>());
}

#[test]
fn a_rewound_or_replaced_log_is_noticed() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(1 << 20)).unwrap();
    for i in 0..4 {
        w.append(&[row(i)]).unwrap();
    }
    drop(w);
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    drain(&mut f, 1 << 20);
    let at = f.cursor().clone();

    // A tail lost before its sync (a machine crash): the segment is shorter
    // than the cursor.
    let path = seg(dir.path(), 1);
    let whole = fs::read(&path).unwrap();
    fs::write(&path, &whole[..whole.len() - 5]).unwrap();
    assert!(matches!(
        WalFollower::open(dir.path(), at.clone()),
        Err(FollowError::Rewound(_))
    ));
    // And a running follower sees it at its next read.
    assert!(matches!(f.read(1 << 20), Err(FollowError::Rewound(_))));

    // Another log of the same length and positions (a restore of another
    // copy): the frame before the cursor is not the one it read.
    let other = tempfile::tempdir().unwrap();
    let w = Wal::open(other.path(), cfg(1 << 20)).unwrap();
    for i in 0..4 {
        w.append(&[NewRecord::bytes(
            kinds::LEDGER,
            None,
            format!("ROW {i}").into_bytes(),
        )])
        .unwrap();
    }
    drop(w);
    fs::copy(seg(other.path(), 1), &path).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), at.offset);
    assert!(matches!(
        WalFollower::open(dir.path(), at.clone()),
        Err(FollowError::Rewound(_))
    ));

    // The segment gone altogether.
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        WalFollower::open(dir.path(), at),
        Err(FollowError::Rewound(_))
    ));
}

/// A read up to a position (theseus-mgw.12): it stops before the first
/// frame whose records pass it, the cursor stays before that frame, and a
/// segment the read stopped inside is not named sealed, though a later one
/// exists; a read with a higher bound takes the rest, and names it then.
#[test]
fn a_read_up_to_a_position_holds_the_frames_past_it() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(200)).unwrap();
    for i in 0..8 {
        w.append(&[row(i), row(i + 100)]).unwrap();
    }
    assert!(seg(dir.path(), 2).exists());
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    // Position 3 is the middle of the second frame: only the first is read.
    let b = f.read_upto(1 << 20, 3).unwrap();
    assert_eq!(
        b.records.iter().map(|r| r.position).collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(
        b.sealed.is_empty(),
        "segment 1 read in part: {:?}",
        b.sealed
    );
    let held = f.stop().clone();
    let Stop::Held {
        segment,
        offset,
        position,
        at_unix_ms,
    } = held
    else {
        panic!("{held:?}");
    };
    assert_eq!((segment, offset, position), (1, f.cursor().offset, 3));
    assert!(at_unix_ms > 0);
    assert_eq!(f.cursor().position, 2);
    // Nothing more while the bound stays.
    let c = f.cursor().clone();
    assert!(f.read_upto(1 << 20, 3).unwrap().is_empty());
    assert_eq!(*f.cursor(), c);
    // The rest, once the bound passes it: segment 1 named sealed now.
    let b = f.read_upto(1 << 20, 16).unwrap();
    assert_eq!(b.records.first().map(|r| r.position), Some(3));
    assert_eq!(b.records.last().map(|r| r.position), Some(16));
    assert!(b.sealed.contains(&1), "{:?}", b.sealed);
    assert_eq!(*f.stop(), Stop::CaughtUp);
}

#[test]
fn a_frame_cut_short_in_a_sealed_segment_is_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let w = Wal::open(dir.path(), cfg(200)).unwrap();
    for i in 0..10 {
        w.append(&[row(i)]).unwrap();
    }
    drop(w);
    assert!(seg(dir.path(), 2).exists());
    let path = seg(dir.path(), 1);
    let len = fs::metadata(&path).unwrap().len();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(len - 3)
        .unwrap();
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    let mut seen = 0;
    let err = loop {
        match f.read(1 << 20) {
            Ok(b) => {
                seen += b.records.len();
                assert!(!b.is_empty(), "stopped without an error: {:?}", f.stop());
            }
            Err(e) => break e,
        }
    };
    assert!(matches!(err, FollowError::Corrupt(_)), "{err}");
    assert!(seen < 10);
}

#[test]
fn a_waker_finds_its_directory_within_a_second_of_its_creation() {
    let dir = tempfile::tempdir().unwrap();
    let wal_dir = dir.path().join("wal");
    let mut waker = Waker::new(&wal_dir).unwrap();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        fs::create_dir(&wal_dir).unwrap();
    });
    // A long backstop, but no directory to watch yet: it looks once a second.
    let t = Instant::now();
    let mut wakes = 0;
    while !waker.watching() {
        waker.wait(Duration::from_secs(60)).unwrap();
        wakes += 1;
        assert!(t.elapsed() < Duration::from_secs(10), "never found it");
    }
    h.join().unwrap();
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    assert!(wakes <= 3, "{wakes} wakes");
}

#[test]
fn the_waker_wakes_on_an_append_and_its_timer_is_the_backstop() {
    let dir = tempfile::tempdir().unwrap();
    let wal_dir = dir.path().join("wal");
    // Not there yet: the timer alone, until the directory appears.
    let mut waker = Waker::new(&wal_dir).unwrap();
    assert!(!waker.watching());
    assert_eq!(waker.wait(Duration::from_millis(20)).unwrap(), Wake::Timer);
    let w = std::sync::Arc::new(Wal::open(&wal_dir, cfg(1 << 20)).unwrap());
    assert_eq!(
        waker.wait(Duration::from_millis(20)).unwrap(),
        Wake::Changed
    );
    assert!(waker.watching());
    // Quiet: the backstop, after its time and not before.
    let t = Instant::now();
    assert_eq!(waker.wait(Duration::from_millis(150)).unwrap(), Wake::Timer);
    assert!(
        t.elapsed() >= Duration::from_millis(140),
        "{:?}",
        t.elapsed()
    );

    // An append from another thread wakes it well inside the backstop.
    let writer = w.clone();
    let t = Instant::now();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        writer.append(&[row(1)]).unwrap();
    });
    assert_eq!(waker.wait(Duration::from_secs(30)).unwrap(), Wake::Changed);
    let waited = t.elapsed();
    h.join().unwrap();
    assert!(waited < Duration::from_secs(5), "woke after {waited:?}");

    // An append made before the wait is not missed: the event is queued.
    w.append(&[row(2)]).unwrap();
    assert_eq!(waker.wait(Duration::from_secs(30)).unwrap(), Wake::Changed);
    // And the queue was drained: quiet again.
    assert_eq!(waker.wait(Duration::from_millis(20)).unwrap(), Wake::Timer);

    // Another thread's kick wakes it too, once per kick.
    let kicker = waker.kicker();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        kicker.kick();
    });
    assert_eq!(waker.wait(Duration::from_secs(30)).unwrap(), Wake::Kicked);
    h.join().unwrap();
    assert_eq!(waker.wait(Duration::from_millis(20)).unwrap(), Wake::Timer);
}

/// Frame `i` of appender `a`: one to three records, each naming itself.
fn frame_of(a: u32, i: u32) -> Vec<NewRecord> {
    (0..=i % 3)
        .map(|j| NewRecord::bytes(kinds::LEDGER, None, format!("a{a} f{i} r{j}").into_bytes()))
        .collect()
}

/// What a follower read beside appenders: the records, the segments it
/// sealed, its spans, and how many records it read while they still ran.
#[derive(Default)]
struct Followed {
    records: Vec<Record>,
    sealed: Vec<u32>,
    spans: Vec<(u32, u64, u64)>,
    while_writing: usize,
}

/// Read beside `appenders` until they are done and the follower has caught
/// up with `last`, failing at the first error it reads.
fn follow_beside(
    f: &mut WalFollower,
    appenders: &[std::thread::JoinHandle<()>],
    last: impl Fn() -> u64,
) -> Followed {
    let mut out = Followed::default();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let writing = appenders.iter().any(|h| !h.is_finished());
        let b = f
            .read(4096)
            .unwrap_or_else(|e| panic!("the follower, beside the writer: {e}"));
        if writing {
            out.while_writing += b.records.len();
        }
        let empty = b.is_empty();
        out.records.extend(b.records);
        out.sealed.extend(b.sealed);
        out.spans.extend(b.spans);
        if !writing && *f.stop() == Stop::CaughtUp && f.cursor().position == last() {
            return out;
        }
        assert!(
            Instant::now() < deadline,
            "the follower did not catch up: {:?}",
            f.stop()
        );
        if empty {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// The spans tile each of segments 1 to `last` once, in order: a shipper
/// copies each byte once.
fn assert_tiled(dir: &Path, spans: &[(u32, u64, u64)], last: u32) {
    for s in 1..=last {
        let mut at = 0;
        for (_, from, to) in spans.iter().filter(|(seg, ..)| *seg == s) {
            assert_eq!(*from, at, "segment {s}: a gap or an overlap at {at}");
            at = *to;
        }
        assert_eq!(at, fs::metadata(seg(dir, s)).unwrap().len(), "segment {s}");
    }
}

/// The store's writer (theseus-vni9) writes a batch's frames back to back and
/// syncs once for all of them, where each append used to write and sync its
/// own. A follower reads the page cache, never the syncs, so beside the writer
/// it must see what it always saw: whole frames in position order, each record
/// once, each appender's frames in the order it appended them, a segment
/// sealed only after its last frame, and no frame it calls corrupt, with the
/// writer rolling segments inside its batches.
#[test]
fn a_follower_beside_the_stores_batching_writer_reads_each_record_once_in_order() {
    use std::sync::Arc;
    use theseus_store::{Store as _, WalStore};

    const APPENDERS: u32 = 4;
    const FRAMES: u32 = 100;
    let dir = tempfile::tempdir().unwrap();
    let wal_dir = dir.path().join("wal");
    // Small segments, so the writer rolls inside its batches; fsync on, so
    // appends queue while a sync runs, and the batches form.
    let cfg = WalConfig {
        segment_bytes: 2048,
        ..WalConfig::default()
    };
    let store = Arc::new(WalStore::open(dir.path(), cfg).unwrap());
    let mut f = WalFollower::open(&wal_dir, Cursor::start()).unwrap();
    let appenders: Vec<_> = (0..APPENDERS)
        .map(|a| {
            let store = store.clone();
            std::thread::spawn(move || {
                for i in 0..FRAMES {
                    store.append(&frame_of(a, i)).unwrap();
                }
            })
        })
        .collect();
    let read = follow_beside(&mut f, &appenders, || store.stats().unwrap().last_position);
    for h in appenders {
        h.join().unwrap();
    }

    let stats = store.stats().unwrap();
    // What the test is about: the writer batched, and rolled, while the
    // follower read.
    assert!(
        stats.syncs < stats.frames_appended,
        "{} syncs for {} frames: no batch formed",
        stats.syncs,
        stats.frames_appended
    );
    assert!(stats.wal_segments >= 4, "{} segments", stats.wal_segments);
    assert!(
        read.while_writing > 0,
        "the follower read nothing while the writer wrote"
    );
    // Each record once, in position order.
    let total = u64::from(APPENDERS) * (0..FRAMES).map(|i| u64::from(i % 3 + 1)).sum::<u64>();
    assert_eq!(
        read.records.iter().map(|r| r.position).collect::<Vec<_>>(),
        (1..=total).collect::<Vec<_>>()
    );
    // Each appender's frames whole and in its own order.
    for a in 0..APPENDERS {
        let tag = format!("a{a} ");
        let mine: Vec<&[u8]> = read
            .records
            .iter()
            .map(|r| r.payload.as_slice())
            .filter(|p| p.starts_with(tag.as_bytes()))
            .collect();
        let appended: Vec<Vec<u8>> = (0..FRAMES)
            .flat_map(|i| frame_of(a, i))
            .map(|r| r.payload)
            .collect();
        assert_eq!(mine, appended, "appender {a}");
    }
    // Every segment but the last sealed, once each and in order.
    assert_eq!(read.sealed, (1..stats.wal_segments).collect::<Vec<_>>());
    assert_tiled(&wal_dir, &read.spans, stats.wal_segments);
}

/// A sync that fails cuts its frames back off, and the next frames take
/// their positions (theseus-ljgm). A follower reads the page cache, so it may
/// have read the cut frames: it must meet a rewind, never read on past them
/// as though the log continued, even when the frames written after the cut
/// are the cut ones' size, and its cursor's offset lands on a frame boundary
/// with the next position it expects.
#[test]
fn a_follower_that_read_frames_since_cut_meets_a_rewind() {
    let dir = tempfile::tempdir().unwrap();
    let path = seg(dir.path(), 1);
    let w = Wal::open(dir.path(), cfg(1 << 20)).unwrap();
    w.append(&[row(1)]).unwrap();
    let kept = fs::metadata(&path).unwrap().len();
    w.append(&[row(2)]).unwrap();
    drop(w);
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    assert_eq!(drain(&mut f, 1 << 20), vec![1, 2]);

    // The cut: frame 2 goes, and two frames of its size follow, so the
    // cursor's offset is the end of the first, which holds position 2.
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(kept)
        .unwrap();
    let w = Wal::open(dir.path(), cfg(1 << 20)).unwrap();
    w.append(&[row(7)]).unwrap();
    w.append(&[row(8)]).unwrap();
    drop(w);
    assert!(
        matches!(f.read(1 << 20), Err(FollowError::Rewound(_))),
        "the frame before the cursor is not the one it read"
    );
    assert_eq!(
        f.cursor().position,
        2,
        "the cursor stays where the read began"
    );

    // Followed again from the start, it reads the log as it is now.
    let mut f = WalFollower::open(dir.path(), Cursor::start()).unwrap();
    let b = f.read(1 << 20).unwrap();
    assert_eq!(
        b.records
            .iter()
            .map(|r| r.payload.clone())
            .collect::<Vec<_>>(),
        vec![b"row 1".to_vec(), b"row 7".to_vec(), b"row 8".to_vec()]
    );
}
