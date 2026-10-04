//! The WAL follower (spec M6 §2.2, step 29b; theseus-zaz.12): a read-only
//! reader of a store's write-ahead log from outside the process that writes
//! it. The index tender follows the log to index nodes; the durability
//! tender (AWS step 15) follows it to ship segments. Neither needs the other's
//! engine, so this crate holds nothing but the following.
//!
//! - **From a cursor**: the end of the last whole frame read (segment,
//!   offset), the last position in it, and a mark of that frame (where it
//!   starts, its first position, its crc). The mark lets a cursor tell, when
//!   it is opened again, that the log is still the one it was taken on: a
//!   restore, or a tail a machine crash lost before its sync, rewrites what
//!   lies before the cursor ([`FollowError::Rewound`]).
//! - **Whole frames only**, checked as recovery checks them
//!   (`theseus_store::wal::read_frame`): magic, length, crc, and positions in
//!   sequence. A frame that is not whole at the log's end is one being
//!   written or a torn tail, and the follower stops before it and waits: the
//!   core's open repairs a torn tail, never a reader.
//! - **Across rotations**: the writer creates segment n+1 only after its last
//!   write to segment n, so a follower that saw n+1 before it read n has read
//!   all of n. A frame that is not whole in a segment that has a successor is
//!   corruption, never a write in progress.
//! - **Woken by inotify** on the WAL's directory ([`Waker`]), with a timer as
//!   the backstop: no busy loop (QUIET BY CONSTRUCTION).

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use theseus_store::wal::{self, FrameRead};
use theseus_store::Record;

mod wake;
pub use wake::{Kicker, Wake, Waker};

/// Where a follower is: just past the last whole frame it read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// The segment of the last frame read (0 before the first).
    pub segment: u32,
    /// The end of that frame in its segment.
    pub offset: u64,
    /// The last position in that frame (0 before the first).
    pub position: u64,
    /// That frame, to check the log again when the cursor is reopened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<FrameMark>,
}

/// The last frame a cursor read: where it starts in its segment, its first
/// position, and the crc of its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameMark {
    pub start: u64,
    pub first: u64,
    pub crc: u32,
}

impl Cursor {
    /// Before the log's first frame.
    pub fn start() -> Self {
        Self {
            segment: 0,
            offset: 0,
            position: 0,
            last: None,
        }
    }

    pub fn at_start(&self) -> bool {
        self.last.is_none()
    }
}

/// Why the last read stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum Stop {
    /// Nothing read since the follower opened.
    Opened,
    /// At the end of the log's whole frames.
    CaughtUp,
    /// Before a frame that is not whole at the log's end: one being written,
    /// or a torn tail that the core's next open cuts. It waits.
    Partial {
        segment: u32,
        offset: u64,
        reason: String,
    },
    /// At the read's byte budget, with more to read.
    Budget,
}

#[derive(Debug, thiserror::Error)]
pub enum FollowError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// The log no longer holds what the cursor was taken on: start again
    /// from the beginning (an index rebuilds).
    #[error("the WAL was rewound or replaced: {0}")]
    Rewound(String),
    /// A frame that checks but holds the wrong records, or one that does not
    /// check in a segment that has a successor. The store's own open refuses
    /// the same log; the follower stops and reports it.
    #[error("corrupt WAL: {0}")]
    Corrupt(String),
}

/// What one read returned.
#[derive(Debug, Default)]
pub struct Batch {
    /// Every record of the whole frames read, in position order.
    pub records: Vec<Record>,
    pub frames: u64,
    pub bytes: u64,
    /// The bytes read, as (segment, from, to): whole frames only, so a
    /// follower that ships segments ships these ranges.
    pub spans: Vec<(u32, u64, u64)>,
    /// Each span's last position, in step with `spans`: which segment holds
    /// a record (the durability tender's index rows).
    pub ends: Vec<u64>,
    /// Segments this read finished: a later segment exists, so nothing more
    /// is ever written to them.
    pub sealed: Vec<u32>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.frames == 0
    }
}

/// A read-only follower of the WAL in `dir` (a store's `wal/` directory).
pub struct WalFollower {
    dir: PathBuf,
    cursor: Cursor,
    stop: Stop,
    /// The last segment a batch named sealed, so none is named twice.
    sealed_upto: u32,
}

impl WalFollower {
    /// Follow the log in `dir` from `cursor`. A cursor past the start is
    /// checked against the log first: its segment must hold, just before
    /// its offset, the frame it marks.
    pub fn open(dir: &Path, cursor: Cursor) -> Result<Self, FollowError> {
        let f = Self {
            dir: dir.to_path_buf(),
            cursor,
            stop: Stop::Opened,
            sealed_upto: 0,
        };
        f.verify()?;
        Ok(f)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    pub fn stop(&self) -> &Stop {
        &self.stop
    }

    fn verify(&self) -> Result<(), FollowError> {
        let c = &self.cursor;
        let Some(mark) = c.last else {
            if c.position != 0 || c.offset != 0 {
                return Err(FollowError::Rewound(format!(
                    "a cursor at position {} without the mark of its frame",
                    c.position
                )));
            }
            return Ok(());
        };
        let path = wal::segment_path(&self.dir, c.segment);
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(FollowError::Rewound(format!(
                    "segment {} is gone",
                    c.segment
                )))
            }
            Err(e) => return Err(e.into()),
        };
        let len = file.metadata()?.len();
        if len < c.offset || mark.start >= c.offset {
            return Err(FollowError::Rewound(format!(
                "segment {} is {len} bytes, and the cursor is at {}",
                c.segment, c.offset
            )));
        }
        let mut bytes = vec![0u8; (c.offset - mark.start) as usize];
        file.read_exact_at(&mut bytes, mark.start)?;
        match wal::read_frame(&bytes, 0, c.segment, mark.start, mark.first) {
            FrameRead::Whole {
                end, crc, records, ..
            }
                if end == bytes.len()
                    && crc == mark.crc
                    && records.last().map(|(r, _)| r.position) == Some(c.position) =>
            {
                Ok(())
            }
            _ => Err(FollowError::Rewound(format!(
                "the frame before the cursor (segment {}, offset {}, positions {} to {}) is not the \
                 one it read",
                c.segment, mark.start, mark.first, c.position
            ))),
        }
    }

    /// Where the next frame is read: just past the cursor, or the start of
    /// the next segment once the cursor's segment is finished. `None` when
    /// the log has no segment yet.
    fn reading_from(&self) -> io::Result<Option<(u32, u64)>> {
        if self.cursor.segment == 0 {
            return Ok(wal::list_segments(&self.dir)
                .or_else(|e| {
                    if e.kind() == io::ErrorKind::NotFound {
                        Ok(Vec::new())
                    } else {
                        Err(e)
                    }
                })?
                .first()
                .map(|s| (*s, 0)));
        }
        Ok(Some((self.cursor.segment, self.cursor.offset)))
    }

    /// Read the whole frames after the cursor, about `max_bytes` of them (at
    /// least one frame when there is one, however large), and move the
    /// cursor past them. An empty batch means it is caught up, or waiting at
    /// a partial frame ([`WalFollower::stop`] says which). On an error the
    /// cursor stays where the read began.
    pub fn read(&mut self, max_bytes: usize) -> Result<Batch, FollowError> {
        let began = self.cursor.clone();
        let mut batch = Batch::default();
        match self.read_into(max_bytes, &mut batch) {
            Ok(()) => Ok(batch),
            Err(e) => {
                self.cursor = began;
                Err(e)
            }
        }
    }

    fn read_into(&mut self, max_bytes: usize, batch: &mut Batch) -> Result<(), FollowError> {
        let Some((mut seg, mut off)) = self.reading_from()? else {
            self.stop = Stop::CaughtUp;
            return Ok(());
        };
        loop {
            // Checked before this segment is read: once segment n+1 exists,
            // nothing more is written to n, so what is read of n is all of it.
            let next_exists = wal::segment_path(&self.dir, seg + 1).exists();
            let path = wal::segment_path(&self.dir, seg);
            let file = match File::open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Err(FollowError::Rewound(format!("segment {seg} is gone")))
                }
                Err(e) => return Err(e.into()),
            };
            let len = file.metadata()?.len();
            if len < off {
                return Err(FollowError::Rewound(format!(
                    "segment {seg} is {len} bytes, and the cursor is at {off}"
                )));
            }
            let budget = max_bytes.saturating_sub(batch.bytes as usize);
            let (window, capped) = read_window(&file, off, len, budget, batch.is_empty())?;
            let mut i = 0usize;
            let span_from = off;
            let mut partial = None;
            while i < window.len() {
                match wal::read_frame(&window, i, seg, off, self.cursor.position + 1) {
                    FrameRead::Whole {
                        end, crc, records, ..
                    } => {
                        let first = records.first().map_or(0, |(r, _)| r.position);
                        if let Some((r, _)) = records.last() {
                            self.cursor.position = r.position;
                        }
                        self.cursor.last = Some(FrameMark {
                            start: off + i as u64,
                            first,
                            crc,
                        });
                        self.cursor.segment = seg;
                        self.cursor.offset = off + end as u64;
                        batch.records.extend(records.into_iter().map(|(r, _)| r));
                        batch.frames += 1;
                        batch.bytes += (end - i) as u64;
                        i = end;
                    }
                    FrameRead::Partial { reason } => {
                        partial = Some(reason);
                        break;
                    }
                    FrameRead::Wrong(e) => {
                        return Err(FollowError::Corrupt(format!(
                            "segment {seg}, offset {}: {e}",
                            off + i as u64
                        )))
                    }
                }
            }
            if i > 0 {
                batch.spans.push((seg, span_from, off + i as u64));
                batch.ends.push(self.cursor.position);
            }
            off += i as u64;
            if let Some(reason) = partial {
                if capped {
                    // The window cut the frame short, not the log.
                    self.stop = Stop::Budget;
                } else if next_exists {
                    return Err(FollowError::Corrupt(format!(
                        "segment {seg}, offset {off}: {reason}, in a segment that has a successor"
                    )));
                } else {
                    self.stop = Stop::Partial {
                        segment: seg,
                        offset: off,
                        reason: reason.to_string(),
                    };
                }
                return Ok(());
            }
            if capped {
                self.stop = Stop::Budget;
                return Ok(());
            }
            // At the end of this segment's bytes.
            if !next_exists {
                self.stop = Stop::CaughtUp;
                return Ok(());
            }
            if seg > self.sealed_upto {
                batch.sealed.push(seg);
                self.sealed_upto = seg;
            }
            seg += 1;
            off = 0;
            if batch.bytes as usize >= max_bytes {
                self.stop = Stop::Budget;
                return Ok(());
            }
        }
    }
}

/// Bytes `off..len` of a segment, at most `budget` of them, unless the first
/// frame is larger and `whole_first` asks for it whole. Returns the bytes,
/// and whether the budget cut them short of `len`.
fn read_window(
    file: &File,
    off: u64,
    len: u64,
    budget: usize,
    whole_first: bool,
) -> io::Result<(Vec<u8>, bool)> {
    let avail = len - off;
    let mut want = avail.min(budget as u64);
    if want < avail && whole_first {
        // Peek at the first frame's length, so one frame larger than the
        // budget is still read whole.
        let mut header = [0u8; wal::FRAME_HEADER];
        if avail >= header.len() as u64 {
            file.read_exact_at(&mut header, off)?;
            let body = u64::from(u32::from_le_bytes([
                header[4], header[5], header[6], header[7],
            ]));
            let frame = wal::FRAME_HEADER as u64 + body;
            if frame <= avail {
                want = want.max(frame);
            }
        }
    }
    let mut buf = vec![0u8; want as usize];
    file.read_exact_at(&mut buf, off)?;
    Ok((buf, want < avail))
}

#[cfg(test)]
mod tests;
