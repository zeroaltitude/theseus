//! The write-ahead log: segment files of atomic frames.
//!
//! ```text
//! segment file: frame frame frame ...
//! frame:  magic u32 | body_len u32 | crc u32 | body
//!   marked:   magic "THWM", crc32(magic ‖ body); body: mark u64 | count u32 | record*
//!   unmarked: magic "THWL", crc32(body);         body: count u32 | record*
//! record: position u64 | kind u16 | schema u16 | at_unix_ms u64 | key_len u16 | scope_len u16 | payload_len u32 | key | scope | payload
//! ```
//!
//! **The mark** (theseus-7nfj, store format 6). The writer stamps each frame
//! with the last position whose sync had returned Ok when it wrote the frame:
//! the end of the batch before. It costs 8 bytes a frame, and no syscall. A
//! log writes marked frames only; frames from before the mark are unmarked,
//! and a log holds them until its segments rotate, so the reader tells the
//! layouts apart frame by frame, by the magic it reads first anyway
//! ([`Layout`]). A marked frame's crc covers its magic too: a magic that rots
//! into the other layout's fails the crc, whatever the body (a crc over one
//! prefix never equals the crc over another for the same bytes after, since
//! each byte's step is a bijection of the crc's state).
//!
//! A frame is written with one `write_all` (`write`), and made durable by an
//! `fdatasync` that may cover several frames (`sync`): the store's writer
//! thread writes every frame queued, back to back, then syncs once for all of
//! them (**group commit**, theseus-vni9). `append` is one frame and its sync,
//! for a caller that writes alone (a restore). A segment's name is as
//! durable as its frames (theseus-xprd): the `sync` that makes a new
//! segment's first frame durable also syncs the log's directory before it
//! returns, and a new log's first sync also syncs the directory holding it.
//! An open that appends to a segment it found syncs the log's directory with
//! its first frame too (theseus-c67g): the segment's creator may have died
//! before it did.
//!
//! On recovery, the first frame that fails (short, bad magic, bad crc) ends
//! the log. A bad frame followed by good bytes in an earlier segment is
//! corruption, not a torn tail, and recovery refuses to guess. In the last
//! segment it is cut, with all after it, as a torn write, unless it is known
//! to have been synced (theseus-gt12, 7nfj): a frame at or before a position
//! known synced went bad after it was written, and cutting it would lose
//! acknowledged frames, so the open refuses it. Two facts say a position was
//! synced: the index's checkpoint (`WalConfig::synced_to`; a store's open
//! passes its own), and the mark of any whole frame after the bad one. The
//! bytes alone cannot tell rot from a torn batch: the writer writes a batch's
//! frames back to back and syncs once, and a power loss before that sync can
//! leave a later frame of the batch whole and an earlier one torn. A later
//! batch's frame can tell, by its mark: so only rot in the log's last batch,
//! past the checkpoint, is still cut. `Recovery::cut` says what was cut,
//! whether a whole frame followed it, and how far the log was known synced.
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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::record::{now_unix_ms, NewRecord, Record, FROZEN_SCHEMA};

/// An unmarked frame's magic: every frame written before theseus-7nfj. Read,
/// never written.
pub const MAGIC_UNMARKED: u32 = 0x5448_574C; // "THWL"
/// A marked frame's magic (theseus-7nfj): every frame written since.
pub const MAGIC_MARKED: u32 = 0x5448_574D; // "THWM"
/// The magic, the body's length, and its crc.
pub const FRAME_HEADER: usize = 12;
/// A marked frame's mark: the first bytes of its body.
const MARK: usize = 8;

/// A frame's layout, told by its magic, frame by frame (theseus-7nfj): a log
/// holds unmarked frames until its segments rotate, and a store an older
/// build wrote goes on with marked frames after them. Nothing else differs:
/// the header is the same 12 bytes, so a frame ends at its header plus its
/// body's length in either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Before theseus-7nfj: no mark, so it says nothing of what was synced.
    Unmarked,
    /// The body begins with the frame's mark.
    Marked,
}

impl Layout {
    /// The layout whose magic is `magic`, or `None`.
    pub fn of(magic: u32) -> Option<Self> {
        match magic {
            MAGIC_MARKED => Some(Self::Marked),
            MAGIC_UNMARKED => Some(Self::Unmarked),
            _ => None,
        }
    }

    /// The layout of the frame whose header is at `off` of `bytes`, when
    /// the bytes there are a magic.
    pub fn at(bytes: &[u8], off: usize) -> Option<Self> {
        Self::of(u32_at(bytes.get(off..off.checked_add(4)?)?, 0))
    }

    /// The crc the writer stored for `body`: a marked frame's covers its
    /// magic too, so a magic that rots into the other layout's never checks.
    pub fn crc(self, body: &[u8]) -> u32 {
        match self {
            Self::Unmarked => crc32fast::hash(body),
            Self::Marked => {
                let mut h = crc32fast::Hasher::new();
                h.update(&MAGIC_MARKED.to_le_bytes());
                h.update(body);
                h.finalize()
            }
        }
    }

    /// Where the body's record count lies; its records follow it.
    pub fn count_at(self) -> usize {
        match self {
            Self::Unmarked => 0,
            Self::Marked => MARK,
        }
    }

    /// The frame's mark, from its body: the last position whose sync had
    /// returned when the frame was written. `None` when unmarked.
    pub fn mark(self, body: &[u8]) -> Option<u64> {
        match self {
            Self::Unmarked => None,
            Self::Marked => body.get(..MARK).map(|b| u64_at(b, 0)),
        }
    }
}

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
    /// A position known synced, from outside the log's bytes: the checkpoint
    /// of an index of this log (a store's open passes its own; a repair the
    /// checkpoint of the store it repairs). A frame of the last segment that
    /// does not check at or before it was synced and then went bad, so the
    /// open refuses it rather than cut it as a torn tail (theseus-gt12).
    /// 0: nothing is known, and such a frame is cut.
    pub synced_to: u64,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            segment_bytes: 64 * 1024 * 1024,
            max_total_bytes: None,
            fsync: true,
            synced_to: 0,
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
    /// The torn tail open cut, when it cut one (theseus-gt12).
    pub cut: Option<Cut>,
}

/// A torn tail an open cut from the last segment (theseus-gt12): the frame
/// that did not check, and all after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cut {
    pub segment: u32,
    pub offset: u64,
    pub bytes: u64,
    /// The position the frame that did not check would have begun with.
    pub position: u64,
    /// The first whole frame found after it, which continues the positions
    /// past it. Whole frames after a bad one are a torn batch's when no sync
    /// covered the bad one, as nothing known synced did here; they are cut
    /// with it, and the open says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whole_after: Option<WholeAfter>,
    /// The last position known synced when the open cut (theseus-7nfj): the
    /// larger of the index's checkpoint and the newest mark of a whole frame
    /// after the cut. Always before `position`, or the open refuses instead.
    #[serde(default)]
    pub synced_to: u64,
}

/// A whole frame found after one that does not check (theseus-gt12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WholeAfter {
    /// Its offset in the segment.
    pub offset: u64,
    /// Its first record's position.
    pub first: u64,
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
    /// Why the log takes no more frames: a write cut short that could not be
    /// cut back off, so the next frame would land after part of one.
    broken: Option<String>,
    /// A test's way to cut the next write short after this many bytes, as a
    /// full disk would.
    #[cfg(test)]
    short_write: Option<usize>,
}

pub struct Wal {
    w: Mutex<Writer>,
    dir: PathBuf,
    recovery: Recovery,
    /// Counters for visibility: frames appended, fdatasync calls made.
    frames: std::sync::atomic::AtomicU64,
    syncs: std::sync::atomic::AtomicU64,
    /// The last position known synced, which each frame written carries as
    /// its mark (theseus-7nfj): what the open knew (the index's checkpoint,
    /// the newest mark it read), then the last position each `sync` that
    /// returned Ok covered.
    synced: std::sync::atomic::AtomicU64,
    /// Directories that hold a name no sync has made durable yet: the log's
    /// own, once a segment is created in it or an open finds the segment it
    /// appends to (theseus-c67g), and the one that holds the log, once open
    /// created the log's directory. The next `sync` syncs each
    /// after its fdatasync and before it returns, so no frame is reported
    /// durable while a power loss could still lose its segment's name
    /// (theseus-xprd).
    unsynced_dirs: Mutex<Vec<PathBuf>>,
    /// Directory syncs since open.
    dir_syncs: std::sync::atomic::AtomicU64,
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

impl Writer {
    /// One frame's bytes, at the end of the segment.
    fn write_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        if let Some(n) = self.short_write.take() {
            self.file.write_all(&frame[..n.min(frame.len())])?;
            return Err(io::Error::other("a write cut short (a test's)"));
        }
        self.file.write_all(frame)
    }
}

/// Segment `n`'s file in the log's directory.
pub fn segment_path(dir: &Path, n: u32) -> PathBuf {
    dir.join(format!("{n:09}.seg"))
}

/// The segment an open appends to: the last, or segment 1, created, in a log
/// with none. With it, the directories whose names the first frame's sync
/// makes durable (theseus-xprd): segment 1's, and the log directory's own
/// when the open created that too. A last segment found is synced into the
/// log's directory too (theseus-c67g): the process that created it may have
/// died before the sync of its first frame synced its name, and this one
/// never creates it, so nothing else would sync that name until a roll.
fn append_segment(
    dir: &Path,
    last: Option<u32>,
    new_dir: bool,
) -> io::Result<(u32, File, u64, Vec<PathBuf>)> {
    if let Some(seg) = last {
        let f = OpenOptions::new()
            .append(true)
            .read(true)
            .open(segment_path(dir, seg))?;
        let len = f.metadata()?.len();
        return Ok((seg, f, len, vec![dir.to_path_buf()]));
    }
    let f = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(segment_path(dir, 1))?;
    let mut unsynced = vec![dir.to_path_buf()];
    if new_dir {
        unsynced.push(match dir.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        });
    }
    Ok((1, f, 0, unsynced))
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
    out.extend_from_slice(&FROZEN_SCHEMA.to_le_bytes());
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
        let new_dir = !dir.exists();
        fs::create_dir_all(dir)?;
        let segments = list_segments(dir)?;
        let last_seg = segments.last().copied();
        let tail = match at {
            Some(loc) if after > 0 => {
                let t = tail_after(dir, &segments, (after, loc), cfg.synced_to)?;
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
                        Some(Bad::Torn { reason, .. }) if Some(seg) == last_seg => {
                            // A torn tail in the last segment, unless it
                            // was synced: cut it, or refuse.
                            let cut = torn_or_rot(
                                &bytes,
                                (good_len, reason),
                                (seg, 0),
                                walk.expected,
                                cfg.synced_to,
                            )?;
                            cut_tail(&path, good_len, cut)?;
                            recovery.truncated_bytes += cut.bytes;
                            recovery.cut = Some(cut);
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
        // A log with none gets segment 1, below.
        recovery.segments = segments.len().max(1) as u32;
        recovery.last_position = walk.expected - 1;
        let expected_pos = walk.expected;
        // The first frames' mark: what this open knows synced, never past
        // what it found (theseus-7nfj). The frames after the newest mark are
        // known synced only once this log's own first sync returns, which
        // covers them too: an fdatasync takes every dirty page of the file.
        let synced = cfg.synced_to.max(walk.mark).min(recovery.last_position);

        let (segment, file, segment_len, unsynced_dirs) = append_segment(dir, last_seg, new_dir)?;
        let wal = Self {
            w: Mutex::new(Writer {
                dir: dir.to_path_buf(),
                cfg,
                segment,
                file,
                segment_len,
                total_len,
                next_position: expected_pos,
                broken: None,
                #[cfg(test)]
                short_write: None,
            }),
            dir: dir.to_path_buf(),
            recovery,
            frames: std::sync::atomic::AtomicU64::new(0),
            syncs: std::sync::atomic::AtomicU64::new(0),
            synced: std::sync::atomic::AtomicU64::new(synced),
            unsynced_dirs: Mutex::new(unsynced_dirs),
            dir_syncs: std::sync::atomic::AtomicU64::new(0),
            readers: Mutex::default(),
            history_end,
            bad: Arc::default(),
        };
        Ok((wal, walk.out))
    }

    pub fn recovery(&self) -> &Recovery {
        &self.recovery
    }

    /// Directories holding names the open that made this log created above
    /// it (a store's own directory, and any the open made on the way to
    /// it): the first frame's sync syncs them too (theseus-gf00).
    pub(crate) fn sync_with_first_frame(&self, dirs: Vec<PathBuf>) {
        let mut unsynced = self.unsynced_dirs.lock().unwrap();
        for d in dirs {
            if !unsynced.contains(&d) {
                unsynced.push(d);
            }
        }
    }

    /// Directory syncs since open.
    #[cfg(test)]
    pub(crate) fn dir_syncs(&self) -> u64 {
        self.dir_syncs.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn last_position(&self) -> u64 {
        self.w.lock().unwrap().next_position - 1
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Append records as one atomic frame (`write`, then `sync`). Returns
    /// (position, location) per record, in order. Durable when this returns
    /// (if `fsync` is on).
    pub fn append(&self, batch: &[NewRecord]) -> Result<Vec<(u64, RecordLocation)>, WalError> {
        let placed = self.write(batch)?;
        if !placed.is_empty() && self.fsync() {
            self.sync()?;
        }
        Ok(placed)
    }

    /// Whether frames are synced (`WalConfig::fsync`).
    pub fn fsync(&self) -> bool {
        self.w.lock().unwrap().cfg.fsync
    }

    /// Write records as one atomic frame, with no sync: it is durable once a
    /// `sync` that follows returns. Returns (position, location) per record,
    /// in order. A frame the log refuses (too large, past its cap, or a write
    /// cut short) leaves the log as it was: a write cut short is cut back
    /// off, so the next frame starts where this one did.
    pub fn write(&self, batch: &[NewRecord]) -> Result<Vec<(u64, RecordLocation)>, WalError> {
        Ok(self.write_timed(batch)?.0)
    }

    /// `write`, with the frame's time, which every record in it carries.
    pub fn write_timed(
        &self,
        batch: &[NewRecord],
    ) -> Result<(Vec<(u64, RecordLocation)>, u64), WalError> {
        if batch.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut w = self.w.lock().unwrap();
        if let Some(why) = &w.broken {
            return Err(WalError::Io(io::Error::other(format!(
                "the log takes no more frames: {why}; a restart's open cuts the torn tail"
            ))));
        }
        #[cfg(not(test))]
        let at = now_unix_ms();
        #[cfg(test)]
        let at = test_clock::now(&w.dir);
        let first = w.next_position;

        // Build body: the mark first (theseus-7nfj). A sync advances it only
        // past frames already written, so it is always before `first`.
        let mut body = Vec::new();
        body.extend_from_slice(&self.synced().to_le_bytes());
        body.extend_from_slice(&(batch.len() as u32).to_le_bytes());
        let mut rel: Vec<(u64, usize, usize)> = Vec::with_capacity(batch.len());
        for (i, r) in batch.iter().enumerate() {
            let pos = first + i as u64;
            let enc = encode_record(pos, at, r)?;
            rel.push((pos, body.len(), enc.len()));
            body.extend_from_slice(&enc);
        }
        let mut frame = Vec::with_capacity(FRAME_HEADER + body.len());
        frame.extend_from_slice(&MAGIC_MARKED.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(&Layout::Marked.crc(&body).to_le_bytes());
        frame.extend_from_slice(&body);

        if let Some(max) = w.cfg.max_total_bytes {
            if w.total_len + frame.len() as u64 > max {
                return Err(WalError::Full {
                    used: w.total_len,
                    max,
                });
            }
        }

        // Roll segment if needed (never split a frame). Everything in the old
        // segment is durable before the first frame of the next is written.
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
            // Its name is synced with its first frame (`sync`, theseus-xprd).
            let mut dirs = self.unsynced_dirs.lock().unwrap();
            if !dirs.contains(&w.dir) {
                dirs.push(w.dir.clone());
            }
        }

        let frame_offset = w.segment_len;
        if let Err(e) = w.write_frame(&frame) {
            // Part of the frame may be on disk, where the next one would go:
            // the log must stay a run of whole frames, or the next open
            // would end it here, and drop every frame written after.
            if let Err(cut) = w.file.set_len(frame_offset) {
                w.broken = Some(format!(
                    "a write cut short at segment {} offset {frame_offset} ({e}) could not be cut \
                     back off ({cut})",
                    w.segment
                ));
            }
            return Err(e.into());
        }
        w.segment_len += frame.len() as u64;
        w.total_len += frame.len() as u64;
        w.next_position = first + batch.len() as u64;
        let seg = w.segment;
        self.frames
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let placed = rel
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
            .collect();
        Ok((placed, at))
    }

    /// Make every frame written so far durable: one fdatasync of the segment
    /// written last (a roll synced the one before it), then each directory
    /// holding a name created since the last sync (a new segment's), so no
    /// frame is reported durable in a segment a power loss could unname
    /// (theseus-xprd). The handle is cloned, so the fdatasync holds no lock a
    /// reader of the last position waits on. Once it returns Ok, every frame
    /// written before it is synced, and the frames written after it say so
    /// (their mark, theseus-7nfj).
    pub fn sync(&self) -> Result<(), WalError> {
        // The last position written before the fdatasync starts: a frame is
        // written whole under this lock, so it is all in the file by then.
        let (file, last) = {
            let w = self.w.lock().unwrap();
            (w.file.try_clone()?, w.next_position - 1)
        };
        file.sync_data()?;
        self.syncs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Held across the directory syncs: a sync that finds none left
        // returns only once another's have landed, and one that fails leaves
        // its directory for the next.
        let mut dirs = self.unsynced_dirs.lock().unwrap();
        while let Some(dir) = dirs.first() {
            File::open(dir)?.sync_all()?;
            dirs.remove(0);
            self.dir_syncs
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.synced
            .fetch_max(last, std::sync::atomic::Ordering::AcqRel);
        Ok(())
    }

    /// The last position known synced: the mark the next frame carries
    /// (theseus-7nfj).
    pub fn synced(&self) -> u64 {
        self.synced.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Cut the next write short after `bytes`, as a full disk would.
    #[cfg(test)]
    pub(crate) fn cut_next_write(&self, bytes: usize) {
        self.w.lock().unwrap().short_write = Some(bytes);
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
                let Some(layout) = Layout::at(&bytes, off) else {
                    break;
                };
                let body_len = u32_at(&bytes, off + 4) as usize;
                let body_start = off + FRAME_HEADER;
                let body_end = body_start + body_len;
                if body_end > bytes.len() || body_len < layout.count_at() + 4 {
                    break;
                }
                let body = &bytes[body_start..body_end];
                let count = u32_at(body, layout.count_at()) as usize;
                let mut i = layout.count_at() + 4;
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
/// it has counted, the newest mark it read, and the records after `keep`
/// with their locations.
struct Walk {
    expected: u64,
    keep: u64,
    frames: u64,
    records: u64,
    /// The newest mark of the frames checked (theseus-7nfj): 0 when none
    /// was marked.
    mark: u64,
    out: Vec<(Record, RecordLocation)>,
}

impl Walk {
    fn new(expected: u64, keep: u64) -> Self {
        Self {
            expected,
            keep,
            frames: 0,
            records: 0,
            mark: 0,
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
/// segment `seg`): its magic, length, crc, mark, and records, whose
/// positions must be the walk's next. Returns the frame's end; on success
/// the walk moves past its records, keeps those after `keep`, and keeps the
/// frame's mark when it is the newest.
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
    let Some(layout) = Layout::at(bytes, off) else {
        return torn("bad magic");
    };
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
    if layout.crc(body) != crc {
        return torn("crc mismatch");
    }
    let wrong = |at: usize, reason: &str| {
        Err(Bad::Wrong(WalError::Corrupt {
            segment: seg,
            offset: base + at as u64,
            reason: reason.into(),
        }))
    };
    let count_at = layout.count_at();
    if body.len() < count_at + 4 {
        return wrong(
            body_start,
            "a crc-valid frame too short for its record count",
        );
    }
    // A mark claims only positions written before its frame (theseus-7nfj).
    let mark = layout.mark(body);
    if mark.is_some_and(|m| m >= walk.expected) {
        return wrong(
            body_start,
            "a crc-valid frame whose mark claims its own positions synced",
        );
    }
    // Frame is intact: positions inside must be the next in sequence.
    let count = u32_at(body, count_at) as usize;
    let mut i = count_at + 4;
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
    walk.mark = walk.mark.max(mark.unwrap_or(0));
    walk.out.extend(kept);
    Ok(body_end)
}

/// One frame read on its own, by a reader outside the store (the WAL
/// follower, `theseus-follow`): what [`read_frame`] found.
#[derive(Debug)]
pub enum FrameRead {
    /// A whole frame: where it ends in the bytes given, the crc of its body,
    /// its mark (theseus-7nfj; `None` when it is unmarked), and its records
    /// with their locations.
    Whole {
        end: usize,
        crc: u32,
        mark: Option<u64>,
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
            mark: Layout::at(bytes, off).and_then(|l| l.mark(&bytes[off + FRAME_HEADER..end])),
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
    if off + FRAME_HEADER <= bytes.len() && Layout::at(bytes, off).is_some() {
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

/// A frame of the last segment that does not check, at `good` of `bytes`
/// (which begin at offset `base` of segment `seg`), where position
/// `expected` was due: a torn tail to cut, or a synced frame gone bad, which
/// is refused (theseus-gt12, theseus-7nfj).
///
/// A frame at or before a position known synced was synced: a sync covers
/// every frame written before it, and frames are written in position order.
/// Cutting it would lose it and every acknowledged frame after it. Two facts
/// say how far the log was synced, and the larger decides: `synced` (the
/// index's checkpoint), and the mark of each whole frame after the bad one,
/// written only once every position to its mark was synced. Anything past
/// both is a torn write the bytes alone cannot tell from rot, even with a
/// whole frame after it (a torn batch, whose pages reached the disk out of
/// order, and whose frames all carry the mark of the batch before), so it is
/// cut, and the cut says whether a whole frame followed, and how far the log
/// was known synced.
fn torn_or_rot(
    bytes: &[u8],
    (good, reason): (u64, &'static str),
    (seg, base): (u32, u64),
    expected: u64,
    synced: u64,
) -> Result<Cut, WalError> {
    let off = good as usize;
    let after = after_bad_frame(bytes, off, seg, base, expected);
    let marked = after.marked.map_or(0, |(mark, _)| mark);
    if expected <= synced.max(marked) {
        let mut says = Vec::new();
        if expected <= synced {
            says.push(format!(
                "position {synced} of the index's checkpoint says so"
            ));
        }
        if let Some((mark, at)) = after.marked.filter(|&(m, _)| expected <= m) {
            says.push(format!(
                "the whole frame at offset {} (position {}) says so: it was written once every \
                 position to {mark} was synced (its mark)",
                at.offset, at.first
            ));
        }
        let follows = after.first.map_or_else(String::new, |w| {
            format!(
                ", and a whole frame follows it at offset {} (position {})",
                w.offset, w.first
            )
        });
        return Err(WalError::Corrupt {
            segment: seg,
            offset: base + good,
            reason: format!(
                "{reason} in the last segment, at position {expected}, which was synced: {}. The \
                 frame went bad after it was written, so it is no torn tail{follows}. Cutting it \
                 would lose acknowledged records; nothing was cut. Stop the daemon and repair it \
                 from a copy that holds the frame whole: `theseusd restore --repair --from <a \
                 copy of the store>`",
                says.join("; and ")
            ),
        });
    }
    let cut = Cut {
        segment: seg,
        offset: base + good,
        bytes: (bytes.len() - off) as u64,
        position: expected,
        whole_after: after.first,
        synced_to: synced.max(marked),
    };
    if let Some(w) = after.first {
        tracing::warn!(
            segment = seg,
            offset = cut.offset,
            bytes = cut.bytes,
            position = expected,
            whole_at = w.offset,
            whole_first = w.first,
            synced_to = cut.synced_to,
            "wal: cut a torn tail with a whole frame after it: a batch torn before its sync, since \
             no mark after it and no checkpoint says it was synced; rot in the log's last batch \
             would look the same, and the frames after it are lost"
        );
    }
    Ok(cut)
}

/// What follows a frame that does not check (theseus-gt12, theseus-7nfj):
/// the first whole frame after it, and the newest mark among the whole
/// frames after it, with the frame that holds it.
#[derive(Default)]
struct AfterBad {
    first: Option<WholeAfter>,
    marked: Option<(u64, WholeAfter)>,
}

/// Every whole frame after the one at `off` of `bytes` that does not check,
/// each found by its magic, checked as a frame, and holding positions past
/// those before it (the first, past `expected`, which the bad frame would
/// have begun with). Read only after a bad frame, so a healthy open never
/// pays for it.
fn after_bad_frame(bytes: &[u8], off: usize, seg: u32, base: u64, expected: u64) -> AfterBad {
    let mut found = AfterBad::default();
    let (mut from, mut past) = (off + 1, expected);
    while let Some(f) = next_whole_frame(bytes, from, seg, base, past) {
        let whole = WholeAfter {
            offset: base + f.at as u64,
            first: f.first,
        };
        found.first.get_or_insert(whole);
        if f.mark > found.marked.map_or(0, |(m, _)| m) {
            found.marked = Some((f.mark, whole));
        }
        from = f.end;
        past = f.next - 1;
    }
    found
}

/// A whole frame `next_whole_frame` found: its offset and end in the bytes
/// searched, its first position, the position after its last, and its mark
/// (0 when unmarked).
struct Found {
    at: usize,
    end: usize,
    first: u64,
    next: u64,
    mark: u64,
}

/// The first whole frame at or after `from` of `bytes`: found by its magic,
/// checked as a frame, with positions past `past`.
fn next_whole_frame(bytes: &[u8], from: usize, seg: u32, base: u64, past: u64) -> Option<Found> {
    let mut i = from;
    while i + FRAME_HEADER <= bytes.len() {
        let at = i + bytes[i..]
            .windows(4)
            .position(|w| Layout::of(u32_at(w, 0)).is_some())?;
        if let Some(first) = first_position(bytes, at).filter(|&p| p > past) {
            let mut walk = Walk::new(first, u64::MAX);
            if let Ok(end) = check_frame(bytes, at, seg, base, &mut walk) {
                return Some(Found {
                    at,
                    end,
                    first,
                    next: walk.expected,
                    mark: walk.mark,
                });
            }
        }
        i = at + 1;
    }
    None
}

/// The position of the first record of the frame whose header is at `at`
/// of `bytes`, in either layout, when its body fits. Nothing is checked: a
/// way to find a frame by its position.
pub fn first_position(bytes: &[u8], at: usize) -> Option<u64> {
    let layout = Layout::at(bytes, at)?;
    let body = at.checked_add(FRAME_HEADER)?;
    let end = body.checked_add(u32_at(bytes.get(at..body)?, 4) as usize)?;
    decode_record(bytes.get(body..end)?, layout.count_at() + 4).map(|(r, _)| r.position)
}

/// Cut the last segment at `at`: what `cut` says goes.
fn cut_tail(path: &Path, at: u64, cut: Cut) -> io::Result<()> {
    let f = OpenOptions::new().write(true).open(path)?;
    f.set_len(at)?;
    f.sync_all()?;
    tracing::info!(
        segment = cut.segment,
        offset = cut.offset,
        bytes = cut.bytes,
        whole_after = cut.whole_after.is_some(),
        synced_to = cut.synced_to,
        "wal: cut a torn tail"
    );
    Ok(())
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
    (after, loc): (u64, RecordLocation),
    synced: u64,
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
        let bytes = read_range(&path, from, to)?;
        let (good, bad) = walk.segment(&bytes, seg, from);
        match bad {
            None => {}
            // A torn tail in the last segment, unless it was synced: cut
            // it, or refuse, as the walk of every segment would.
            Some(Bad::Torn { reason, .. }) if Some(seg) == last_seg => {
                let cut = torn_or_rot(&bytes, (good, reason), (seg, from), walk.expected, synced)?;
                cut_tail(&path, from + good, cut)?;
                recovery.truncated_bytes += cut.bytes;
                recovery.cut = Some(cut);
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

    /// The synced mark (theseus-7nfj).
    mod mark;

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
}

/// A test's clock for a log's frames, by its directory: a window test steps
/// it back as a host's clock may step (theseus-vm3n.5).
#[cfg(test)]
pub(crate) mod test_clock {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static SET: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());

    /// Frames written to the log in `dir` from now on carry `ms`.
    pub fn set(dir: &Path, ms: u64) {
        SET.lock().unwrap().insert(dir.to_path_buf(), ms);
    }

    pub(super) fn now(dir: &Path) -> u64 {
        SET.lock()
            .unwrap()
            .get(dir)
            .copied()
            .unwrap_or_else(super::now_unix_ms)
    }
}
