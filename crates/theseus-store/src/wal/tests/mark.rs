//! The synced mark (theseus-7nfj): what the writer stamps, how an open reads
//! it, and the two frame layouts it reads.

use super::*;

fn cfg(synced_to: u64) -> WalConfig {
    WalConfig {
        synced_to,
        ..WalConfig::default()
    }
}

/// Each frame of a segment's bytes, as an open reads it: its mark.
fn marks(bytes: &[u8]) -> Vec<Option<u64>> {
    let mut out = Vec::new();
    let (mut off, mut next) = (0usize, 1u64);
    while off < bytes.len() {
        match read_frame(bytes, off, 1, 0, next) {
            FrameRead::Whole {
                end, mark, records, ..
            } => {
                next = records.last().map_or(next, |(r, _)| r.position + 1);
                out.push(mark);
                off = end;
            }
            other => panic!("offset {off}: {other:?}"),
        }
    }
    out
}

/// Rot in frame `n` of a `five_frames_in` log: one bit of its record's
/// payload, so its header stays whole and its crc no longer checks.
fn rot(path: &Path, bytes: &mut [u8], n: usize) {
    let at = frame_offsets(bytes)[n - 1] + FRAME_HEADER + MARK + 4 + RECORD_HEADER + 5;
    bytes[at] ^= 0x10;
    fs::write(path, &*bytes).unwrap();
}

/// What an open refused with, the log left as it was: nothing cut.
fn refused<T>(opened: Result<T, WalError>, path: &Path, bytes: &[u8]) -> WalError {
    let Err(e) = opened else {
        panic!("the open went on: the bad frame was cut, not refused")
    };
    assert_eq!(fs::read(path).unwrap(), bytes, "nothing was cut");
    e
}

/// The writer stamps each frame with the last position whose sync had
/// returned when it wrote the frame: the end of the batch before. A batch's
/// frames all carry the same mark.
#[test]
fn each_frame_carries_the_end_of_the_batch_synced_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_, bytes) = five_frames_in(dir.path(), &[1, 2, 1, 1]);
    assert_eq!(marks(&bytes), [Some(0), Some(1), Some(1), Some(3), Some(4)]);
}

/// A reopened log's first frames carry what its open knew: the newest mark
/// it read, or the index's checkpoint when that is larger, never past the
/// log's end. Here the last batch, frames 3 to 5, is known synced by
/// nothing but a checkpoint. The log's own first sync then covers every
/// frame the open found too: an fdatasync takes the file's every dirty page.
#[test]
fn a_reopened_log_marks_what_its_open_knew_then_what_its_sync_covered() {
    for (synced_to, first_mark) in [(0, 2), (4, 4), (9, 5)] {
        let dir = tempfile::tempdir().unwrap();
        five_frames_in(dir.path(), &[1, 1, 3]);
        let wal = Wal::open(dir.path(), cfg(synced_to)).unwrap();
        assert_eq!(wal.synced(), first_mark, "synced_to {synced_to}");
        wal.write(&[rec(kinds::LEDGER, None, b"six")]).unwrap();
        wal.sync().unwrap();
        wal.append(&[rec(kinds::LEDGER, None, b"seven")]).unwrap();
        assert_eq!(wal.synced(), 7);
        drop(wal);
        let bytes = fs::read(segment_path(dir.path(), 1)).unwrap();
        assert_eq!(
            marks(&bytes)[5..],
            [Some(first_mark), Some(6)],
            "synced_to {synced_to}"
        );
    }
}

/// Rot past the index's checkpoint (position 2): frame 3 went bad after its
/// sync returned, and the frames written after that sync say so. The
/// tail-only open refuses, names the frame and the mark, and cuts nothing,
/// where gt12's open cut frames 3 to 5.
#[test]
fn rot_past_the_checkpoint_is_refused_by_a_later_frames_mark() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames(dir.path());
    let at = Some(record_at(&bytes, 2));
    rot(&path, &mut bytes, 3);
    let offs = frame_offsets(&bytes);
    let err = refused(Wal::open_from(dir.path(), cfg(2), 2, at), &path, &bytes);
    let msg = err.to_string();
    assert!(
        matches!(err, WalError::Corrupt { segment: 1, offset, .. } if offset == offs[2] as u64),
        "the bad frame is named: {msg}"
    );
    for says in [
        "at position 3, which was synced".to_string(),
        format!(
            "the whole frame at offset {} (position 5) says so: it was written once every \
             position to 4 was synced (its mark)",
            offs[4]
        ),
        "nothing was cut".to_string(),
        "theseusd restore --repair".to_string(),
    ] {
        assert!(msg.contains(&says), "{says}: {msg}");
    }
    assert!(!msg.contains("checkpoint says"), "{msg}");
}

/// Rot with no index, as a full replay with the index gone or `theseusd
/// restore` opens a log: nothing outside the log says what was synced, and
/// the marks do.
#[test]
fn rot_with_no_index_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames(dir.path());
    rot(&path, &mut bytes, 3);
    let msg = refused(Wal::open(dir.path(), WalConfig::default()), &path, &bytes).to_string();
    assert!(msg.contains("at position 3, which was synced"), "{msg}");
    assert!(msg.contains("(its mark)"), "{msg}");
}

/// Rot in the frame before the last: the last frame's mark is exactly the
/// bad frame's first position, which is enough.
#[test]
fn rot_in_the_frame_before_the_last_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames(dir.path());
    rot(&path, &mut bytes, 4);
    let offs = frame_offsets(&bytes);
    let msg = refused(Wal::open(dir.path(), WalConfig::default()), &path, &bytes).to_string();
    assert!(msg.contains("at position 4, which was synced"), "{msg}");
    assert!(
        msg.contains(&format!(
            "the whole frame at offset {} (position 5) says so: it was written once every \
             position to 4 was synced",
            offs[4]
        )),
        "{msg}"
    );
}

/// Two bad frames, then a whole one: its mark still proves the first, which
/// the search for whole frames reaches past the second.
#[test]
fn a_mark_past_a_second_bad_frame_still_proves_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames(dir.path());
    rot(&path, &mut bytes, 3);
    rot(&path, &mut bytes, 4);
    let msg = refused(Wal::open(dir.path(), WalConfig::default()), &path, &bytes).to_string();
    assert!(msg.contains("at position 3, which was synced"), "{msg}");
    assert!(msg.contains("(position 5) says so"), "{msg}");
}

/// Rot in the log's last batch stays undecidable: no frame was written after
/// its sync. Frames 3 to 5 are one batch, and frame 4 went bad: frame 5's
/// mark says only position 2, so frames 4 and 5 are cut, as a torn batch is.
/// The checkpoint covers such a frame once it reaches it.
#[test]
fn rot_in_the_last_batch_is_cut_as_a_torn_batch_is() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames_in(dir.path(), &[1, 1, 3]);
    rot(&path, &mut bytes, 4);
    let offs = frame_offsets(&bytes);
    let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
    assert_eq!(wal.last_position(), 3);
    assert_eq!(
        wal.recovery().cut,
        Some(Cut {
            segment: 1,
            offset: offs[3] as u64,
            bytes: (bytes.len() - offs[3]) as u64,
            position: 4,
            whole_after: Some(WholeAfter {
                offset: offs[4] as u64,
                first: 5
            }),
            synced_to: 2,
        })
    );
    // With the checkpoint at the batch's end, the same frame is refused.
    drop(wal);
    fs::write(&path, &bytes).unwrap();
    let msg = refused(Wal::open(dir.path(), cfg(5)), &path, &bytes).to_string();
    assert!(
        msg.contains("position 5 of the index's checkpoint says so"),
        "{msg}"
    );
}

/// A crc-valid frame whose mark claims its own positions synced is wrong,
/// not torn: the writer stamps only what a sync before the frame covered.
#[test]
fn a_mark_that_claims_its_own_frame_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut bytes) = five_frames(dir.path());
    let last = frame_offsets(&bytes)[4];
    let body = last + FRAME_HEADER;
    bytes[body..body + MARK].copy_from_slice(&5u64.to_le_bytes());
    let crc = Layout::Marked.crc(&bytes[body..]);
    bytes[last + 8..last + 12].copy_from_slice(&crc.to_le_bytes());
    fs::write(&path, &bytes).unwrap();
    let msg = refused(Wal::open(dir.path(), WalConfig::default()), &path, &bytes).to_string();
    assert!(
        msg.contains("whose mark claims its own positions synced"),
        "{msg}"
    );
}

/// Two frames an older build wrote, before the mark: 460a35b's, the fixture
/// `crates/theseus-core/tests/fixtures/store-460a35b`'s segment 1 at offsets
/// 2340 and 2437, positions 8 and 9, the ledger rows `server.stopping` and
/// `driver.started`, at schema 1. Literal bytes: today's encoder writes the
/// marked layout only.
const UNMARKED: &str = concat!(
    "4c574854550000000097159301000000080000000000000002000100d1c8f3f3a0010000",
    "00000000350000007b2261745f756e69785f6d73223a313739303739393233353238312c",
    "226b696e64223a227365727665722e73746f7070696e67227d",
    "4c5748548e0000001492bb7f01000000090000000000000002000100d7c8f3f3a0010000",
    "000000006e0000007b2261745f756e69785f6d73223a313739303739393233353238372c",
    "226b696e64223a226472697665722e73746172746564222c2264617461223a7b2262696e",
    "64696e67735f7265616479223a747275652c227761697465645f666f725f62696e64696e",
    "67735f6d73223a307d7d",
);

fn unmarked() -> Vec<u8> {
    (0..UNMARKED.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&UNMARKED[i..i + 2], 16).unwrap())
        .collect()
}

/// The old layout is read in place, frame by frame: each frame decodes as an
/// open decodes it, says nothing of what was synced, and keeps every byte of
/// its records; the walk over both reads no mark.
#[test]
fn frames_an_older_build_wrote_read_as_before() {
    let b = unmarked();
    assert_eq!(b.len(), 97 + 154);
    assert_eq!(Layout::at(&b, 0), Some(Layout::Unmarked));
    let FrameRead::Whole {
        end, mark, records, ..
    } = read_frame(&b, 0, 1, 2340, 8)
    else {
        panic!("frame 8 does not read")
    };
    assert_eq!((end, mark), (97, None));
    let (r, loc) = &records[0];
    assert_eq!((r.position, r.kind, r.schema), (8, kinds::LEDGER, 1));
    assert_eq!((r.key.as_deref(), r.scope.as_deref()), (None, None));
    assert_eq!(r.at_unix_ms, 1_790_799_235_281);
    assert_eq!(
        r.payload,
        br#"{"at_unix_ms":1790799235281,"kind":"server.stopping"}"#
    );
    assert_eq!(
        *loc,
        RecordLocation {
            segment: 1,
            offset: 2340 + 12 + 4,
            len: 28 + 53
        }
    );
    let FrameRead::Whole { mark, records, .. } = read_frame(&b, 97, 1, 2340, 9) else {
        panic!("frame 9 does not read")
    };
    assert_eq!(mark, None);
    assert_eq!(records[0].0.position, 9);
    assert!(records[0]
        .0
        .payload
        .starts_with(br#"{"at_unix_ms":1790799235287,"kind":"driver.started""#));
    let mut walk = Walk::new(8, 0);
    let (good, bad) = walk.segment(&b, 1, 2340);
    assert!(bad.is_none());
    assert_eq!((good, walk.expected, walk.mark), (b.len() as u64, 10, 0));
}

/// A magic that rots into the other layout's fails the crc, whatever the
/// body: a marked frame's crc covers its magic. One body shows it for every
/// body, since each byte's step is a bijection of the crc's state: the crcs
/// after the two prefixes differ for every body, or for none.
#[test]
fn a_magic_rotted_into_the_other_layout_fails_the_crc() {
    let mut old = unmarked();
    old[..4].copy_from_slice(&MAGIC_MARKED.to_le_bytes());
    assert!(
        matches!(
            read_frame(&old, 0, 1, 0, 8),
            FrameRead::Partial {
                reason: "crc mismatch"
            }
        ),
        "an unmarked frame read as marked"
    );
    let dir = tempfile::tempdir().unwrap();
    let (_, mut new) = five_frames(dir.path());
    new[..4].copy_from_slice(&MAGIC_UNMARKED.to_le_bytes());
    assert!(
        matches!(
            read_frame(&new, 0, 1, 0, 1),
            FrameRead::Partial {
                reason: "crc mismatch"
            }
        ),
        "a marked frame read as unmarked"
    );
    assert_ne!(Layout::Marked.crc(b""), Layout::Unmarked.crc(b""));
}
