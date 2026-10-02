//! The write-ahead log: segment files of atomic frames.
//!
//! ```text
//! segment file: frame frame frame ...
//! frame:  MAGIC u32 | body_len u32 | crc32(body) u32 | body
//! body:   count u32 | record*
//! record: position u64 | kind u16 | schema u16 | at_unix_ms u64 | key_len u16 | scope_len u16 | payload_len u32 | key | scope | payload
//! ```
//!
//! A frame is written with one `write_all`; durability is one `fdatasync`
//! that may cover several frames (**group commit**): concurrent appenders
//! write their frames back to back under a short lock, then one of them syncs
//! the file once for everyone whose bytes are already written. A single
//! writer sees exactly the old behaviour, one sync per frame. On recovery,
//! the first frame that fails (short, bad magic, bad crc) ends the log, and if
//! it is in the last segment it is truncated as a torn write. A bad frame
//! followed by good bytes in an earlier segment is corruption, not a torn
//! tail, and recovery refuses to guess.
//!
//! **What open checks** (theseus-8ni). Given where a known-good record lies
//! (the index's checkpoint), `open_from` checks only the frames after it:
//! the next position and a torn frame are both at the tail, and the
//! checkpoint's own frame was synced before the index claimed it. The
//! history before it was checked when it was written; `verify_history`
//! checks it again after serving, and refuses reads from a corrupt frame.
//! Without that record, or when the log there is not what the index says,
//! open checks every segment, as it always did.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::record::{now_unix_ms, NewRecord, Record};

pub const MAGIC: u32 = 0x5448_574C; // "THWL"
/// MAGIC, the body's length, and its crc.
pub const FRAME_HEADER: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum WalError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("wal is full: {used} of {max} bytes used")]
    Full { used: u64, max: u64 },
    #[error("corrupt frame in segment {segment} at offset {offset}: {reason}")]
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
    /// Let one fdatasync cover every frame written since the last one
    /// (concurrent appenders share the sync). Off: every append syncs itself.
    pub group_commit: bool,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            segment_bytes: 64 * 1024 * 1024,
            max_total_bytes: None,
            fsync: true,
            group_commit: true,
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
    /// Frames and records checked at open: the tail after the checkpoint,
    /// or every one when open checked the whole log.
    pub frames: u64,
    pub records: u64,
    pub truncated_bytes: u64,
    pub segments: u32,
    /// Where open began checking (segment, offset): the frame after the
    /// checkpoint's record. `None` when it checked every segment.
    pub checked_from: Option<(u32, u64)>,
    /// The bytes before `checked_from`, left to `verify_history`.
    pub history_bytes: u64,
}

/// What `verify_history` checked.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct History {
    pub segments: u32,
    pub frames: u64,
    pub records: u64,
    pub bytes: u64,
    /// Time spent checking, pauses left out.
    pub busy_ms: f64,
    /// Open checked every segment, so there was no history to check.
    pub checked_at_open: bool,
    /// Where the check began, when it began at an earlier check's mark
    /// (theseus-0dq): the last position that check proved. `None`: from the
    /// log's start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_position: Option<u64>,
    /// The mark the next check may start from.
    #[serde(skip)]
    pub verified: Option<Verified>,
}

/// How far a history check proved the log (theseus-0dq): the last frame it
/// checked, which the next check starts from and checks again, so a check
/// after a restart reads only what was written since. When that frame no
/// longer checks, or does not hold the positions named here, the next check
/// reads the whole log instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Verified {
    /// The last frame checked: its segment, its offset, and the position of
    /// its first record.
    pub segment: u32,
    pub offset: u64,
    pub first: u64,
    /// The last position checked: that frame's last record.
    pub position: u64,
    /// When the last check from the log's start ended, in unix ms: a check
    /// that starts at a mark keeps it, so the caller can ask for a whole
    /// check again on its own schedule, for what rots where nothing writes.
    pub full_at_unix_ms: u64,
}

/// How much `verify_history` checks between two calls of its `pace`.
const PACE_BYTES: usize = 4 << 20;

struct Writer {
    dir: PathBuf,
    cfg: WalConfig,
    segment: u32,
    file: File,
    segment_len: u64,
    total_len: u64,
    next_position: u64,
}

/// Group-commit state: how many bytes (across all segments) have been written
/// and how many are known durable; whether a sync is in flight.
#[derive(Default)]
struct SyncState {
    written: u64,
    synced: u64,
    syncing: bool,
}

pub struct Wal {
    w: Mutex<Writer>,
    sync: Mutex<SyncState>,
    sync_cv: Condvar,
    dir: PathBuf,
    recovery: Recovery,
    /// Counters for visibility: frames appended, fdatasync calls made.
    frames: std::sync::atomic::AtomicU64,
    syncs: std::sync::atomic::AtomicU64,
    /// One read handle per segment, opened on its first read: a record read
    /// is then one `pread`, where it was an open, a seek, a read, and a close
    /// (theseus-qa0: 10,000 executions read at startup cost 10,000 opens).
    readers: Mutex<HashMap<u32, Arc<File>>>,
    /// Where the history open left unchecked ends: the checkpoint's position,
    /// and the segment and offset of the frame after it.
    history_end: Option<(u64, u32, u64)>,
    /// A corrupt frame `verify_history` found (segment, from, to): reads of
    /// its records are refused. `to` is the segment's end when the frame's
    /// header cannot say where it ends.
    bad: Arc<OnceLock<(u32, u64, u64)>>,
}

/// Segment `n`'s file in the log's directory.
pub fn segment_path(dir: &Path, n: u32) -> PathBuf {
    dir.join(format!("{n:09}.seg"))
}

/// The segments in the log's directory, in order.
pub fn list_segments(dir: &Path) -> io::Result<Vec<u32>> {
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
const RECORD_HEADER: usize = 28;

fn encode_record(position: u64, at: u64, r: &NewRecord) -> Result<Vec<u8>, WalError> {
    let key = r.key.as_deref().unwrap_or("");
    let scope = r.scope.as_deref().unwrap_or("");
    if key.len() > u16::MAX as usize {
        return Err(WalError::TooLarge(key.len()));
    }
    if scope.len() > u16::MAX as usize {
        return Err(WalError::TooLarge(scope.len()));
    }
    if r.payload.len() > (u32::MAX - 64) as usize {
        return Err(WalError::TooLarge(r.payload.len()));
    }
    let mut out = Vec::with_capacity(RECORD_HEADER + key.len() + scope.len() + r.payload.len());
    out.extend_from_slice(&position.to_le_bytes());
    out.extend_from_slice(&r.kind.to_le_bytes());
    out.extend_from_slice(&r.schema.to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&(key.len() as u16).to_le_bytes());
    out.extend_from_slice(&(scope.len() as u16).to_le_bytes());
    out.extend_from_slice(&(r.payload.len() as u32).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(scope.as_bytes());
    out.extend_from_slice(&r.payload);
    Ok(out)
}

/// Decode one record from `b` starting at `i`; returns (record, consumed).
pub fn decode_record(b: &[u8], i: usize) -> Option<(Record, usize)> {
    if b.len() < i + RECORD_HEADER {
        return None;
    }
    let position = u64_at(b, i);
    let kind = u16_at(b, i + 8);
    let schema = u16_at(b, i + 10);
    let at_unix_ms = u64_at(b, i + 12);
    let key_len = u16_at(b, i + 20) as usize;
    let scope_len = u16_at(b, i + 22) as usize;
    let payload_len = u32_at(b, i + 24) as usize;
    let start = i + RECORD_HEADER;
    let end = start
        .checked_add(key_len)?
        .checked_add(scope_len)?
        .checked_add(payload_len)?;
    if b.len() < end {
        return None;
    }
    let key = if key_len == 0 {
        None
    } else {
        Some(String::from_utf8_lossy(&b[start..start + key_len]).into_owned())
    };
    let scope_start = start + key_len;
    let scope = if scope_len == 0 {
        None
    } else {
        Some(String::from_utf8_lossy(&b[scope_start..scope_start + scope_len]).into_owned())
    };
    let payload = b[scope_start + scope_len..end].to_vec();
    Some((
        Record {
            position,
            kind,
            schema,
            key,
            scope,
            at_unix_ms,
            payload,
        },
        end - i,
    ))
}

impl Wal {
    /// Open or create the log in `dir`, checking every segment and
    /// recovering to the last good frame.
    pub fn open(dir: &Path, cfg: WalConfig) -> Result<Self, WalError> {
        Ok(Self::open_from(dir, cfg, u64::MAX, None)?.0)
    }

    /// Open or create the log in `dir`, recovering to the last good frame,
    /// and return, with their locations, the records after position `after`
    /// (what an index whose checkpoint is `after` lacks).
    ///
    /// `at` is where the index says record `after` lies. Given it, open
    /// checks only the frames that follow that record (theseus-8ni). When the
    /// log there is not what the index says (the record is missing or
    /// another, or a frame after it does not check), it checks every segment
    /// instead, which alone tells a torn tail from corruption.
    pub fn open_from(
        dir: &Path,
        cfg: WalConfig,
        after: u64,
        at: Option<RecordLocation>,
    ) -> Result<(Self, Vec<(Record, RecordLocation)>), WalError> {
        fs::create_dir_all(dir)?;
        let segments = list_segments(dir)?;
        let last_seg = segments.last().copied();
        let tail = match at {
            Some(loc) if after > 0 => {
                let t = tail_after(dir, &segments, after, loc)?;
                if t.is_none() {
                    tracing::warn!(
                        checkpoint = after,
                        segment = loc.segment,
                        offset = loc.offset,
                        "wal: the log after the index's checkpoint did not check; checking every segment"
                    );
                }
                t
            }
            _ => None,
        };
        let (walk, total_len, mut recovery, history_end) = match tail {
            Some(t) => t,
            None => {
                let mut walk = Walk::new(1, after);
                let mut recovery = Recovery::default();
                let mut total_len: u64 = 0;
                for &seg in &segments {
                    let path = segment_path(dir, seg);
                    let bytes = fs::read(&path)?;
                    let (good_len, bad) = walk.segment(&bytes, seg, 0);
                    match bad {
                        None => {}
                        Some(Bad::Torn { .. }) if Some(seg) == last_seg => {
                            // Torn tail in the last segment: cut it.
                            let f = OpenOptions::new().write(true).open(&path)?;
                            f.set_len(good_len)?;
                            f.sync_all()?;
                            recovery.truncated_bytes += bytes.len() as u64 - good_len;
                        }
                        Some(Bad::Torn { offset, reason }) => {
                            return Err(WalError::Corrupt {
                                segment: seg,
                                offset,
                                reason: format!(
                                    "{reason} (not the last segment; refusing to truncate)"
                                ),
                            })
                        }
                        Some(Bad::Wrong(e)) => return Err(e),
                    }
                    total_len += good_len;
                }
                (walk, total_len, recovery, None)
            }
        };
        recovery.frames = walk.frames;
        recovery.records = walk.records;
        recovery.segments = segments.len() as u32;
        recovery.last_position = walk.expected - 1;
        let expected_pos = walk.expected;

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
        let wal = Self {
            w: Mutex::new(Writer {
                dir: dir.to_path_buf(),
                cfg,
                segment,
                file,
                segment_len,
                total_len,
                next_position: expected_pos,
            }),
            sync: Mutex::new(SyncState {
                written: total_len,
                synced: total_len,
                syncing: false,
            }),
            sync_cv: Condvar::new(),
            dir: dir.to_path_buf(),
            recovery,
            frames: std::sync::atomic::AtomicU64::new(0),
            syncs: std::sync::atomic::AtomicU64::new(0),
            readers: Mutex::default(),
            history_end,
            bad: Arc::default(),
        };
        Ok((wal, walk.out))
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
            {
                // Everything in the old segment is now durable.
                let mut st = self.sync.lock().unwrap();
                st.synced = st.synced.max(w.total_len);
                st.written = st.written.max(w.total_len);
            }
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
        w.segment_len += frame.len() as u64;
        w.total_len += frame.len() as u64;
        w.next_position = first + batch.len() as u64;
        let seg = w.segment;
        let written_upto = w.total_len;
        let fsync = w.cfg.fsync;
        let group = w.cfg.group_commit;
        self.frames
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if fsync && !group {
            w.file.sync_data()?;
            self.syncs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        // Clone the handle so the sync can run without the writer lock; other
        // appenders keep writing behind us while we (or a leader) sync.
        let file = if fsync && group {
            Some(w.file.try_clone()?)
        } else {
            None
        };
        drop(w);
        if let Some(file) = file {
            self.group_sync(&file, written_upto)?;
        }
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

    /// Group commit. `upto` is the total byte count this appender needs
    /// durable. If a sync that covers it already finished, return. If one is
    /// in flight, wait for it and re-check (it may not have covered us). Else
    /// become the leader: sync once for every byte written so far.
    fn group_sync(&self, file: &File, upto: u64) -> Result<(), WalError> {
        let mut st = self.sync.lock().unwrap();
        st.written = st.written.max(upto);
        loop {
            if st.synced >= upto {
                return Ok(());
            }
            if st.syncing {
                st = self.sync_cv.wait(st).unwrap();
                continue;
            }
            st.syncing = true;
            let target = st.written;
            drop(st);
            let r = file.sync_data();
            self.syncs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            st = self.sync.lock().unwrap();
            st.syncing = false;
            match r {
                Ok(()) => {
                    st.synced = st.synced.max(target);
                    self.sync_cv.notify_all();
                    if st.synced >= upto {
                        return Ok(());
                    }
                }
                Err(e) => {
                    self.sync_cv.notify_all();
                    return Err(e.into());
                }
            }
        }
    }

    /// Frames appended since open.
    pub fn frames_appended(&self) -> u64 {
        self.frames.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// fdatasync calls since open (< frames when group commit batched).
    pub fn syncs(&self) -> u64 {
        self.syncs.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Read one record at a known location. A record in a corrupt frame
    /// that `verify_history` found is refused: record reads check no crc of
    /// their own.
    pub fn read_at(&self, loc: RecordLocation) -> Result<Record, WalError> {
        if let Some(&(segment, from, to)) = self.bad.get() {
            if loc.segment == segment && (from..to).contains(&loc.offset) {
                return Err(WalError::Corrupt {
                    segment,
                    offset: from,
                    reason: format!(
                        "the record at offset {} is in a frame the history check found corrupt; \
                         its reads are refused",
                        loc.offset
                    ),
                });
            }
        }
        let f = self.reader(loc.segment)?;
        let mut buf = vec![0u8; loc.len as usize];
        f.read_exact_at(&mut buf, loc.offset)?;
        decode_record(&buf, 0)
            .map(|(r, _)| r)
            .ok_or_else(|| WalError::Corrupt {
                segment: loc.segment,
                offset: loc.offset,
                reason: "record did not decode at indexed location".into(),
            })
    }

    /// The segment's read handle. Segments only grow and are never removed
    /// while the store is open, so a handle stays good for the store's life.
    fn reader(&self, segment: u32) -> io::Result<Arc<File>> {
        let mut readers = self.readers.lock().unwrap();
        if let Some(f) = readers.get(&segment) {
            return Ok(f.clone());
        }
        let f = Arc::new(File::open(segment_path(&self.dir, segment))?);
        readers.insert(segment, f.clone());
        Ok(f)
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

    /// Check what open did not (theseus-8ni); see `HistoryCheck::run`.
    pub fn verify_history(&self, pace: impl FnMut(Duration)) -> Result<History, WalError> {
        self.history_check().run(pace)
    }

    /// What checking the history needs, apart from the WAL: a thread that
    /// holds it keeps no file of the store open but the segments it reads.
    pub fn history_check(&self) -> HistoryCheck {
        HistoryCheck {
            dir: self.dir.clone(),
            end: self.history_end,
            bad: self.bad.clone(),
            from: None,
        }
    }

    /// Total bytes of all segments (as recovered plus appended).
    pub fn total_bytes(&self) -> u64 {
        self.w.lock().unwrap().total_len
    }

    pub fn segment_count(&self) -> u32 {
        self.w.lock().unwrap().segment
    }
}

/// The history check of one open WAL (theseus-8ni), on its own: the WAL's
/// directory, where the history open left unchecked ends, and the WAL's
/// refusal of a corrupt frame's reads, which it sets.
pub struct HistoryCheck {
    dir: PathBuf,
    end: Option<(u64, u32, u64)>,
    bad: Arc<OnceLock<(u32, u64, u64)>>,
    /// An earlier check's mark, to start from (theseus-0dq).
    from: Option<Verified>,
}

/// Where a walk of the history begins: a segment, an offset in it, and the
/// position expected there.
#[derive(Clone, Copy)]
struct Start {
    segment: u32,
    offset: u64,
    first: u64,
}

impl HistoryCheck {
    /// Start at `mark`, an earlier check's (theseus-0dq): the next run checks
    /// that check's last frame again, then only what follows it.
    pub fn from_mark(mut self, mark: Option<Verified>) -> Self {
        self.from = mark;
        self
    }

    /// Every frame of the history to the frame after the index's checkpoint,
    /// with its crc, its records, and the position sequence, which must end
    /// at the checkpoint. From the log's start, or, given a mark
    /// (`from_mark`), from the last frame the earlier check proved: that
    /// frame must still check and hold the positions the mark names, or the
    /// whole log is checked instead. Read-only. `pace` gets the time each
    /// stretch of `PACE_BYTES` took, so a caller can keep to its share of a
    /// core. On a corrupt frame, reads of its records are refused from then
    /// on (`Wal::read_at`), and the error names it. `History::verified` is
    /// the mark the next check may start from.
    pub fn run(&self, mut pace: impl FnMut(Duration)) -> Result<History, WalError> {
        let Some((last, end_seg, end_off)) = self.end else {
            return Ok(History {
                checked_at_open: true,
                ..History::default()
            });
        };
        let full = Start {
            segment: 0,
            offset: 0,
            first: 1,
        };
        let mark = self.from.filter(|v| {
            v.position <= last
                && v.first <= v.position
                && (v.segment, v.offset) < (end_seg, end_off)
        });
        if let Some(v) = mark {
            let at = Start {
                segment: v.segment,
                offset: v.offset,
                first: v.first,
            };
            match self.walk(at, (last, end_seg, end_off), Some(v), &mut pace) {
                Ok(Some(mut h)) => {
                    h.from_position = Some(v.position);
                    if let Some(next) = h.verified.as_mut() {
                        next.full_at_unix_ms = v.full_at_unix_ms;
                    }
                    return Ok(h);
                }
                // The mark's frame is not what it says: check everything.
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }
        let mut h = self
            .walk(full, (last, end_seg, end_off), None, &mut pace)?
            .expect("a walk from the log's start has no mark to miss");
        if let Some(next) = h.verified.as_mut() {
            next.full_at_unix_ms = now_unix_ms();
        }
        Ok(h)
    }

    /// Walk the frames from `at` to the history's end. With a `mark`, the
    /// first frame is the mark's: `Ok(None)` when it does not check or does
    /// not end at the mark's position, and nothing is refused for it, since
    /// the whole check that follows decides.
    fn walk(
        &self,
        at: Start,
        (last, end_seg, end_off): (u64, u32, u64),
        mark: Option<Verified>,
        pace: &mut impl FnMut(Duration),
    ) -> Result<Option<History>, WalError> {
        let t0 = Instant::now();
        let mut paused = Duration::ZERO;
        let mut h = History::default();
        let mut walk = Walk::new(at.first, u64::MAX);
        let mut stretch = Instant::now();
        let mut since = 0usize;
        // The last frame checked: segment, offset, first position.
        let mut newest: Option<(u32, u64, u64)> = None;
        for seg in list_segments(&self.dir)?
            .into_iter()
            .filter(|s| *s >= at.segment && *s <= end_seg)
        {
            let path = segment_path(&self.dir, seg);
            let from = if seg == at.segment { at.offset } else { 0 };
            let to = if seg == end_seg {
                end_off
            } else {
                fs::metadata(&path)?.len()
            };
            if mark.is_some() && newest.is_none() && from >= to {
                return Ok(None);
            }
            let bytes = read_range(&path, from, to)?;
            let mut off = 0usize;
            while off < bytes.len() {
                let before = walk.expected;
                match check_frame(&bytes, off, seg, from, &mut walk) {
                    Ok(end) => {
                        since += end - off;
                        if let (Some(v), None) = (mark, newest) {
                            if walk.expected != v.position + 1 {
                                return Ok(None);
                            }
                        }
                        newest = Some((seg, from + off as u64, before));
                        off = end;
                        walk.frames += 1;
                        walk.records += walk.expected - before;
                    }
                    Err(_) if mark.is_some() && newest.is_none() => return Ok(None),
                    // In the history a frame that does not check is corrupt:
                    // a later frame was written after it.
                    Err(bad) => {
                        let _ = self.bad.set((
                            seg,
                            from + off as u64,
                            frame_end(&bytes, off).saturating_add(from),
                        ));
                        return Err(match bad {
                            Bad::Torn { offset, reason } => WalError::Corrupt {
                                segment: seg,
                                offset,
                                reason: reason.into(),
                            },
                            Bad::Wrong(e) => e,
                        });
                    }
                }
                if since >= PACE_BYTES {
                    let p = Instant::now();
                    pace(stretch.elapsed());
                    paused += p.elapsed();
                    stretch = Instant::now();
                    since = 0;
                }
            }
            h.segments += 1;
            h.bytes += bytes.len() as u64;
        }
        if mark.is_some() && newest.is_none() {
            return Ok(None);
        }
        if walk.expected != last + 1 {
            // No frame to blame: reads are checked against their position.
            return Err(WalError::Corrupt {
                segment: end_seg,
                offset: end_off,
                reason: format!(
                    "the history ends at position {}, but the index's checkpoint is {last}",
                    walk.expected - 1
                ),
            });
        }
        h.frames = walk.frames;
        h.records = walk.records;
        h.busy_ms = t0.elapsed().saturating_sub(paused).as_secs_f64() * 1000.0;
        h.verified = newest.map(|(segment, offset, first)| Verified {
            segment,
            offset,
            first,
            position: last,
            full_at_unix_ms: 0,
        });
        Ok(Some(h))
    }
}

/// Why a frame does not check.
enum Bad {
    /// Short, bad magic, an absurd length, or a crc mismatch: a torn write
    /// when it is the log's last frame, corruption anywhere else.
    Torn { offset: u64, reason: &'static str },
    /// A crc-valid frame that is still wrong (a record that does not decode,
    /// a position out of sequence): never a torn write.
    Wrong(WalError),
}

/// A walk over frames in position order: the next position expected, what
/// it has counted, and the records after `keep` with their locations.
struct Walk {
    expected: u64,
    keep: u64,
    frames: u64,
    records: u64,
    out: Vec<(Record, RecordLocation)>,
}

impl Walk {
    fn new(expected: u64, keep: u64) -> Self {
        Self {
            expected,
            keep,
            frames: 0,
            records: 0,
            out: Vec::new(),
        }
    }

    /// Every frame in `bytes`, which begin at offset `base` of segment
    /// `seg`: how many bytes are whole frames, and the first frame that is
    /// not, if any.
    fn segment(&mut self, bytes: &[u8], seg: u32, base: u64) -> (u64, Option<Bad>) {
        let mut off = 0usize;
        while off < bytes.len() {
            let before = self.expected;
            match check_frame(bytes, off, seg, base, self) {
                Ok(end) => {
                    off = end;
                    self.frames += 1;
                    self.records += self.expected - before;
                }
                Err(bad) => return (off as u64, Some(bad)),
            }
        }
        (off as u64, None)
    }
}

/// Check the frame at `off` in `bytes` (which begin at offset `base` of
/// segment `seg`): its magic, length, crc, and records, whose positions must
/// be the walk's next. Returns the frame's end; on success the walk moves
/// past its records and keeps those after `keep`.
fn check_frame(
    bytes: &[u8],
    off: usize,
    seg: u32,
    base: u64,
    walk: &mut Walk,
) -> Result<usize, Bad> {
    let torn = |reason: &'static str| {
        Err(Bad::Torn {
            offset: base + off as u64,
            reason,
        })
    };
    if off + FRAME_HEADER > bytes.len() {
        return torn("short frame header");
    }
    if u32_at(bytes, off) != MAGIC {
        return torn("bad magic");
    }
    let body_len = u32_at(bytes, off + 4) as usize;
    let crc = u32_at(bytes, off + 8);
    let body_start = off + FRAME_HEADER;
    let Some(body_end) = body_start.checked_add(body_len) else {
        return torn("absurd body length");
    };
    if body_end > bytes.len() {
        return torn("short frame body");
    }
    let body = &bytes[body_start..body_end];
    if crc32fast::hash(body) != crc {
        return torn("crc mismatch");
    }
    let wrong = |at: usize, reason: &str| {
        Err(Bad::Wrong(WalError::Corrupt {
            segment: seg,
            offset: base + at as u64,
            reason: reason.into(),
        }))
    };
    if body.len() < 4 {
        return wrong(
            body_start,
            "a crc-valid frame too short for its record count",
        );
    }
    // Frame is intact: positions inside must be the next in sequence.
    let count = u32_at(body, 0) as usize;
    let mut i = 4usize;
    let mut next = walk.expected;
    let mut kept = Vec::new();
    for _ in 0..count {
        let Some((rec, used)) = decode_record(body, i) else {
            return wrong(
                body_start + i,
                "record inside a crc-valid frame did not decode",
            );
        };
        if rec.position != next {
            return Err(Bad::Wrong(WalError::Sequence {
                expected: next,
                found: rec.position,
                segment: seg,
            }));
        }
        if rec.position > walk.keep {
            kept.push((
                rec,
                RecordLocation {
                    segment: seg,
                    offset: base + (body_start + i) as u64,
                    len: used as u32,
                },
            ));
        }
        next += 1;
        i += used;
    }
    walk.expected = next;
    walk.out.extend(kept);
    Ok(body_end)
}

/// One frame read on its own, by a reader outside the store (the WAL
/// follower, `theseus-follow`): what [`read_frame`] found.
#[derive(Debug)]
pub enum FrameRead {
    /// A whole frame: where it ends in the bytes given, the crc of its body,
    /// and its records with their locations.
    Whole {
        end: usize,
        crc: u32,
        records: Vec<(Record, RecordLocation)>,
    },
    /// Short, bad magic, an absurd length, or a crc mismatch: at the log's
    /// end, a frame still being written or a torn tail; anywhere else,
    /// corruption.
    Partial { reason: &'static str },
    /// A crc-valid frame that is still wrong (a record that does not decode,
    /// a position out of sequence): never a write in progress.
    Wrong(WalError),
}

/// Check the frame at `off` in `bytes` (which begin at offset `base` of
/// segment `seg`) as recovery checks it, its records' positions starting
/// at `first`. Read-only: nothing is cut.
pub fn read_frame(bytes: &[u8], off: usize, seg: u32, base: u64, first: u64) -> FrameRead {
    let mut walk = Walk::new(first, 0);
    match check_frame(bytes, off, seg, base, &mut walk) {
        Ok(end) => FrameRead::Whole {
            end,
            crc: u32_at(bytes, off + 8),
            records: walk.out,
        },
        Err(Bad::Torn { reason, .. }) => FrameRead::Partial { reason },
        Err(Bad::Wrong(e)) => FrameRead::Wrong(e),
    }
}

/// Where a frame that did not check ends, as far as its header can say:
/// its stated length when the header is whole and fits, else nowhere short
/// of its segment's end.
fn frame_end(bytes: &[u8], off: usize) -> u64 {
    if off + FRAME_HEADER <= bytes.len() && u32_at(bytes, off) == MAGIC {
        let end = off + FRAME_HEADER + u32_at(bytes, off + 4) as usize;
        if end <= bytes.len() {
            return end as u64;
        }
    }
    u64::MAX
}

/// Bytes `from..to` of a file, in one read.
fn read_range(path: &Path, from: u64, to: u64) -> io::Result<Vec<u8>> {
    let f = File::open(path)?;
    let mut buf = vec![0u8; to.saturating_sub(from) as usize];
    f.read_exact_at(&mut buf, from)?;
    Ok(buf)
}

/// What a tail-only open found: the walk, the log's length, the recovery,
/// and where the unchecked history ends.
type Tail = (Walk, u64, Recovery, Option<(u64, u32, u64)>);

/// The log after record `after`, which the index says lies at `loc`:
/// that record must be there, whole, with that position, and the frames
/// after it must check to the log's end, but for a torn last frame, which is
/// cut. Reads nothing before the record.
///
/// The record ends its frame: a checkpoint is taken with no append between
/// its WAL write and its index write, so the position it claims is the last
/// of a synced frame, and the index holds the location the WAL wrote it at.
/// `None` when the log is not what the index says there (an index from
/// another WAL), or when a frame before the last segment's end does not
/// check: the caller then checks every segment, and says which it is.
fn tail_after(
    dir: &Path,
    segments: &[u32],
    after: u64,
    loc: RecordLocation,
) -> Result<Option<Tail>, WalError> {
    if !segments.contains(&loc.segment) {
        return Ok(None);
    }
    let path = segment_path(dir, loc.segment);
    let len = fs::metadata(&path)?.len();
    let end = loc.offset + u64::from(loc.len);
    if end > len {
        return Ok(None);
    }
    match decode_record(&read_range(&path, loc.offset, end)?, 0) {
        Some((r, used)) if r.position == after && used as u64 == u64::from(loc.len) => {}
        _ => return Ok(None),
    }
    let last_seg = segments.last().copied();
    let mut walk = Walk::new(after + 1, after);
    let mut recovery = Recovery::default();
    let mut total_len = 0u64;
    for &seg in segments {
        let path = segment_path(dir, seg);
        if seg < loc.segment {
            let n = fs::metadata(&path)?.len();
            total_len += n;
            recovery.history_bytes += n;
            continue;
        }
        let from = if seg == loc.segment { end } else { 0 };
        let to = if seg == loc.segment {
            len
        } else {
            fs::metadata(&path)?.len()
        };
        let (good, bad) = walk.segment(&read_range(&path, from, to)?, seg, from);
        match bad {
            None => {}
            Some(Bad::Torn { .. }) if Some(seg) == last_seg => {
                // Torn tail in the last segment: cut it.
                let f = OpenOptions::new().write(true).open(&path)?;
                f.set_len(from + good)?;
                f.sync_all()?;
                recovery.truncated_bytes += to - (from + good);
            }
            Some(_) => return Ok(None),
        }
        total_len += from + good;
    }
    recovery.history_bytes += end;
    recovery.checked_from = Some((loc.segment, end));
    Ok(Some((
        walk,
        total_len,
        recovery,
        Some((after, loc.segment, end)),
    )))
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
    fn scope_roundtrips_and_group_commit_syncs_less_than_it_writes() {
        let dir = tempfile::tempdir().unwrap();
        let wal = std::sync::Arc::new(Wal::open(dir.path(), WalConfig::default()).unwrap());
        let locs = wal
            .append(&[rec(kinds::LEDGER, None, b"x").scoped("ses_1")])
            .unwrap();
        assert_eq!(
            wal.read_at(locs[0].1).unwrap().scope.as_deref(),
            Some("ses_1")
        );
        // 8 threads x 50 appends: every append durable when it returns, and
        // the number of fdatasync calls is at most the number of frames.
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
}
