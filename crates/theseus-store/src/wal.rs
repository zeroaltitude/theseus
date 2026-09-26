//! The write-ahead log: segment files of atomic frames.
//!
//! ```text
//! segment file: frame frame frame ...
//! frame:  MAGIC u32 | body_len u32 | crc32(body) u32 | body
//! body:   count u32 | record*
//! record: position u64 | kind u16 | schema u16 | at_unix_ms u64 | key_len u16 | payload_len u32 | key | payload
//! ```
//!
//! A frame is written with one `write_all` and one `fdatasync`. On recovery,
//! every frame is verified; the first frame that fails (short, bad magic, bad
//! crc) ends the log, and if it is in the last segment it is truncated as a
//! torn write. A bad frame followed by good bytes in an earlier segment is
//! corruption, not a torn tail, and recovery refuses to guess.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::record::{now_unix_ms, NewRecord, Record};

pub const MAGIC: u32 = 0x5448_574C; // "THWL"
const FRAME_HEADER: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum WalError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("wal is full: {used} of {max} bytes used")]
    Full { used: u64, max: u64 },
    #[error("corrupt frame in segment {segment} at offset {offset}: {reason} (not the last segment; refusing to truncate)")]
    Corrupt {
        segment: u32,
        offset: u64,
        reason: String,
    },
    #[error("position sequence broken: expected {expected}, found {found} in segment {segment}")]
    Sequence {
        expected: u64,
        found: u64,
        segment: u32,
    },
    #[error("record too large: {0} bytes")]
    TooLarge(usize),
}

#[derive(Debug, Clone)]
pub struct WalConfig {
    /// Roll to a new segment after this many bytes.
    pub segment_bytes: u64,
    /// Refuse appends past this total (disk-full simulation and a safety cap).
    pub max_total_bytes: Option<u64>,
    /// fdatasync every frame. Off only for benchmarks that measure the cost.
    pub fsync: bool,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            segment_bytes: 64 * 1024 * 1024,
            max_total_bytes: None,
            fsync: true,
        }
    }
}

/// Where a record's encoding lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecordLocation {
    pub segment: u32,
    pub offset: u64,
    pub len: u32,
}

/// Outcome of recovery: what was found and what was cut.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recovery {
    pub last_position: u64,
    pub frames: u64,
    pub records: u64,
    pub truncated_bytes: u64,
    pub segments: u32,
}

struct Writer {
    dir: PathBuf,
    cfg: WalConfig,
    segment: u32,
    file: File,
    segment_len: u64,
    total_len: u64,
    next_position: u64,
}

pub struct Wal {
    w: Mutex<Writer>,
    dir: PathBuf,
    recovery: Recovery,
}

fn segment_path(dir: &Path, n: u32) -> PathBuf {
    dir.join(format!("{n:09}.seg"))
}

fn list_segments(dir: &Path) -> io::Result<Vec<u32>> {
    let mut v = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".seg") {
            if let Ok(n) = stem.parse::<u32>() {
                v.push(n);
            }
        }
    }
    v.sort_unstable();
    Ok(v)
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[i..i + 8]);
    u64::from_le_bytes(a)
}

/// Encode one record; returns bytes.
fn encode_record(position: u64, at: u64, r: &NewRecord) -> Result<Vec<u8>, WalError> {
    let key = r.key.as_deref().unwrap_or("");
    if key.len() > u16::MAX as usize {
        return Err(WalError::TooLarge(key.len()));
    }
    if r.payload.len() > (u32::MAX - 64) as usize {
        return Err(WalError::TooLarge(r.payload.len()));
    }
    let mut out = Vec::with_capacity(26 + key.len() + r.payload.len());
    out.extend_from_slice(&position.to_le_bytes());
    out.extend_from_slice(&r.kind.to_le_bytes());
    out.extend_from_slice(&r.schema.to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&(key.len() as u16).to_le_bytes());
    out.extend_from_slice(&(r.payload.len() as u32).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(&r.payload);
    Ok(out)
}

/// Decode one record from `b` starting at `i`; returns (record, consumed).
pub fn decode_record(b: &[u8], i: usize) -> Option<(Record, usize)> {
    if b.len() < i + 26 {
        return None;
    }
    let position = u64_at(b, i);
    let kind = u16_at(b, i + 8);
    let schema = u16_at(b, i + 10);
    let at_unix_ms = u64_at(b, i + 12);
    let key_len = u16_at(b, i + 20) as usize;
    let payload_len = u32_at(b, i + 22) as usize;
    let start = i + 26;
    let end = start.checked_add(key_len)?.checked_add(payload_len)?;
    if b.len() < end {
        return None;
    }
    let key = if key_len == 0 {
        None
    } else {
        Some(String::from_utf8_lossy(&b[start..start + key_len]).into_owned())
    };
    let payload = b[start + key_len..end].to_vec();
    Some((
        Record {
            position,
            kind,
            schema,
            key,
            at_unix_ms,
            payload,
        },
        end - i,
    ))
}

impl Wal {
    /// Open or create the log in `dir`, recovering to the last good frame.
    /// Returns the WAL and, for each record found, its position and location
    /// (so an index can be rebuilt from `replay_from`).
    pub fn open(dir: &Path, cfg: WalConfig) -> Result<Self, WalError> {
        fs::create_dir_all(dir)?;
        let segments = list_segments(dir)?;
        let mut recovery = Recovery::default();
        let mut expected_pos: u64 = 1;
        let mut total_len: u64 = 0;
        let last_seg = segments.last().copied();

        for &seg in &segments {
            let path = segment_path(dir, seg);
            let bytes = fs::read(&path)?;
            let is_last = Some(seg) == last_seg;
            let (good_len, frames, records, next_pos) =
                verify_segment(&bytes, seg, expected_pos, is_last)?;
            if good_len < bytes.len() as u64 {
                // Torn tail in the last segment: cut it.
                let f = OpenOptions::new().write(true).open(&path)?;
                f.set_len(good_len)?;
                f.sync_all()?;
                recovery.truncated_bytes += bytes.len() as u64 - good_len;
            }
            recovery.frames += frames;
            recovery.records += records;
            expected_pos = next_pos;
            total_len += good_len;
        }
        recovery.segments = segments.len() as u32;
        recovery.last_position = expected_pos - 1;

        // Open (or create) the segment to append to.
        let (segment, file, segment_len) = match last_seg {
            Some(seg) => {
                let path = segment_path(dir, seg);
                let f = OpenOptions::new().append(true).read(true).open(&path)?;
                let len = f.metadata()?.len();
                (seg, f, len)
            }
            None => {
                let path = segment_path(dir, 1);
                let f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .read(true)
                    .open(&path)?;
                recovery.segments = 1;
                (1, f, 0)
            }
        };
        Ok(Self {
            w: Mutex::new(Writer {
                dir: dir.to_path_buf(),
                cfg,
                segment,
                file,
                segment_len,
                total_len,
                next_position: expected_pos,
            }),
            dir: dir.to_path_buf(),
            recovery,
        })
    }

    pub fn recovery(&self) -> &Recovery {
        &self.recovery
    }

    pub fn last_position(&self) -> u64 {
        self.w.lock().unwrap().next_position - 1
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Append records as one atomic frame. Returns (position, location) per
    /// record, in order. Durable when this returns (if `fsync` is on).
    pub fn append(&self, batch: &[NewRecord]) -> Result<Vec<(u64, RecordLocation)>, WalError> {
        if batch.is_empty() {
            return Ok(Vec::new());
        }
        let mut w = self.w.lock().unwrap();
        let at = now_unix_ms();
        let first = w.next_position;

        // Build body.
        let mut body = Vec::new();
        body.extend_from_slice(&(batch.len() as u32).to_le_bytes());
        let mut rel: Vec<(u64, usize, usize)> = Vec::with_capacity(batch.len());
        for (i, r) in batch.iter().enumerate() {
            let pos = first + i as u64;
            let enc = encode_record(pos, at, r)?;
            rel.push((pos, body.len(), enc.len()));
            body.extend_from_slice(&enc);
        }
        let mut frame = Vec::with_capacity(FRAME_HEADER + body.len());
        frame.extend_from_slice(&MAGIC.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
        frame.extend_from_slice(&body);

        if let Some(max) = w.cfg.max_total_bytes {
            if w.total_len + frame.len() as u64 > max {
                return Err(WalError::Full {
                    used: w.total_len,
                    max,
                });
            }
        }

        // Roll segment if needed (never split a frame).
        if w.segment_len > 0 && w.segment_len + frame.len() as u64 > w.cfg.segment_bytes {
            w.file.sync_all()?;
            let next = w.segment + 1;
            let path = segment_path(&w.dir, next);
            w.file = OpenOptions::new()
                .create_new(true)
                .append(true)
                .read(true)
                .open(&path)?;
            w.segment = next;
            w.segment_len = 0;
        }

        let frame_offset = w.segment_len;
        w.file.write_all(&frame)?;
        if w.cfg.fsync {
            w.file.sync_data()?;
        }
        w.segment_len += frame.len() as u64;
        w.total_len += frame.len() as u64;
        w.next_position = first + batch.len() as u64;

        let seg = w.segment;
        Ok(rel
            .into_iter()
            .map(|(pos, body_off, len)| {
                (
                    pos,
                    RecordLocation {
                        segment: seg,
                        offset: frame_offset + FRAME_HEADER as u64 + body_off as u64,
                        len: len as u32,
                    },
                )
            })
            .collect())
    }

    /// Read one record at a known location.
    pub fn read_at(&self, loc: RecordLocation) -> Result<Record, WalError> {
        let path = segment_path(&self.dir, loc.segment);
        let mut f = File::open(&path)?;
        f.seek(SeekFrom::Start(loc.offset))?;
        let mut buf = vec![0u8; loc.len as usize];
        f.read_exact(&mut buf)?;
        decode_record(&buf, 0)
            .map(|(r, _)| r)
            .ok_or_else(|| WalError::Corrupt {
                segment: loc.segment,
                offset: loc.offset,
                reason: "record did not decode at indexed location".into(),
            })
    }

    /// Walk every record with position > `after`, in order, yielding
    /// (record, location). Used to rebuild the index after a checkpoint.
    pub fn replay_from(&self, after: u64) -> Result<Vec<(Record, RecordLocation)>, WalError> {
        let mut out = Vec::new();
        for seg in list_segments(&self.dir)? {
            let bytes = fs::read(segment_path(&self.dir, seg))?;
            let mut off = 0usize;
            while off + FRAME_HEADER <= bytes.len() {
                let body_len = u32_at(&bytes, off + 4) as usize;
                let body_start = off + FRAME_HEADER;
                let body_end = body_start + body_len;
                if body_end > bytes.len() {
                    break;
                }
                let body = &bytes[body_start..body_end];
                let count = u32_at(body, 0) as usize;
                let mut i = 4usize;
                for _ in 0..count {
                    let Some((rec, used)) = decode_record(body, i) else {
                        break;
                    };
                    if rec.position > after {
                        out.push((
                            rec,
                            RecordLocation {
                                segment: seg,
                                offset: (body_start + i) as u64,
                                len: used as u32,
                            },
                        ));
                    }
                    i += used;
                }
                off = body_end;
            }
        }
        Ok(out)
    }

    /// Total bytes of all segments (as recovered plus appended).
    pub fn total_bytes(&self) -> u64 {
        self.w.lock().unwrap().total_len
    }

    pub fn segment_count(&self) -> u32 {
        self.w.lock().unwrap().segment
    }
}

/// Verify one segment. Returns (good_len, frames, records, next_expected_pos).
/// A bad frame in the last segment ends the good region; anywhere else it is
/// corruption.
fn verify_segment(
    bytes: &[u8],
    seg: u32,
    mut expected_pos: u64,
    is_last: bool,
) -> Result<(u64, u64, u64, u64), WalError> {
    let mut off = 0usize;
    let mut frames = 0u64;
    let mut records = 0u64;
    let fail = |off: usize, reason: &str| -> Result<(u64, u64, u64, u64), WalError> {
        if is_last {
            Ok((off as u64, 0, 0, 0)) // caller uses frames/records from outer scope below
        } else {
            Err(WalError::Corrupt {
                segment: seg,
                offset: off as u64,
                reason: reason.into(),
            })
        }
    };
    loop {
        if off == bytes.len() {
            break;
        }
        if off + FRAME_HEADER > bytes.len() {
            return fail(off, "short frame header")
                .map(|(g, _, _, _)| (g, frames, records, expected_pos));
        }
        if u32_at(bytes, off) != MAGIC {
            return fail(off, "bad magic").map(|(g, _, _, _)| (g, frames, records, expected_pos));
        }
        let body_len = u32_at(bytes, off + 4) as usize;
        let crc = u32_at(bytes, off + 8);
        let body_start = off + FRAME_HEADER;
        let Some(body_end) = body_start.checked_add(body_len) else {
            return fail(off, "absurd body length")
                .map(|(g, _, _, _)| (g, frames, records, expected_pos));
        };
        if body_end > bytes.len() {
            return fail(off, "short frame body")
                .map(|(g, _, _, _)| (g, frames, records, expected_pos));
        }
        let body = &bytes[body_start..body_end];
        if crc32fast::hash(body) != crc {
            return fail(off, "crc mismatch")
                .map(|(g, _, _, _)| (g, frames, records, expected_pos));
        }
        // Frame is intact: positions inside must be the next in sequence.
        let count = u32_at(body, 0) as usize;
        let mut i = 4usize;
        for _ in 0..count {
            let Some((rec, used)) = decode_record(body, i) else {
                return Err(WalError::Corrupt {
                    segment: seg,
                    offset: (body_start + i) as u64,
                    reason: "record inside a crc-valid frame did not decode".into(),
                });
            };
            if rec.position != expected_pos {
                return Err(WalError::Sequence {
                    expected: expected_pos,
                    found: rec.position,
                    segment: seg,
                });
            }
            expected_pos += 1;
            records += 1;
            i += used;
        }
        frames += 1;
        off = body_end;
    }
    Ok((bytes.len() as u64, frames, records, expected_pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::kinds;

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
            half.extend_from_slice(&MAGIC.to_le_bytes());
            half.extend_from_slice(&(500u32).to_le_bytes());
            half.extend_from_slice(&(0u32).to_le_bytes());
            half.extend_from_slice(&[7u8; 40]);
            f.write_all(&half).unwrap();
        }
        let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
        assert_eq!(wal.recovery().records, 10);
        assert_eq!(wal.recovery().truncated_bytes, 52);
        assert_eq!(fs::metadata(&path).unwrap().len(), good_len);
        assert_eq!(wal.last_position(), 10);
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
}
