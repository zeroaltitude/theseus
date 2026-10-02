//! The frames a daemon writes, counted from outside it (theseus-goa8).
//!
//! A frame is the WAL's atomic unit and costs one `fdatasync`, so frames per
//! turn is the disk's share of a turn counted in a way that does not depend on
//! the disk: §9's per-turn overhead restated (review 2, consideration 8). The
//! daemon does not report them, and the bench must not change the core for
//! its own needs, so a [`Tail`] reads the WAL's segments read-only, the way the
//! WAL follower does, and checks each frame as recovery checks it
//! (`theseus_store::wal::read_frame`). It takes no lock: the daemon holds the
//! store's, and a reader never needs it.
//!
//! A frame still being written is not whole, and is left for the next read.

use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use theseus_store::wal::{list_segments, read_frame, segment_path, FrameRead};
use theseus_store::{kinds, Record};

/// One whole frame, as the tail read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The first record's position.
    pub first: u64,
    /// What each record is, in order: its kind's name, and for a ledger row
    /// its own kind (`ledger:turn.started`), so a failure can say which
    /// frame was the extra one.
    pub records: Vec<String>,
}

impl Frame {
    /// The frame in one line: `[ledger:turn.started, node]`.
    pub fn label(&self) -> String {
        format!("[{}]", self.records.join(", "))
    }
}

/// A read-only follower of one WAL directory (`<state>/store/wal`).
pub struct Tail {
    dir: PathBuf,
    /// The segment of the last frame read (0 before the first).
    segment: u32,
    /// The end of that frame in its segment.
    offset: u64,
    /// The last position read (0 before the first).
    position: u64,
}

impl Tail {
    /// A tail before the log's first frame: the next read returns every
    /// frame.
    pub fn at_start(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            segment: 0,
            offset: 0,
            position: 0,
        }
    }

    /// A tail at the log's end now: what is already written is not counted.
    pub fn at_end(dir: &Path) -> Result<Self> {
        let mut t = Self::at_start(dir);
        t.read()?;
        Ok(t)
    }

    /// The last position read.
    #[cfg(test)]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// The whole frames written since the last read, and the cursor moved
    /// past them. A log with no segment yet reads as no frames.
    pub fn read(&mut self) -> Result<Vec<Frame>> {
        let segments = match list_segments(&self.dir) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("listing {}", self.dir.display())),
        };
        let mut frames = Vec::new();
        let from = self.segment.max(1);
        for &seg in segments.iter().filter(|s| **s >= from) {
            let off = if seg == self.segment { self.offset } else { 0 };
            let file = std::fs::File::open(segment_path(&self.dir, seg))
                .with_context(|| format!("opening segment {seg}"))?;
            let len = file.metadata()?.len();
            if len < off {
                bail!(
                    "segment {seg} is {len} bytes, and the tail is at {off}: the log was rewritten"
                );
            }
            // Only the bytes after the cursor: a poll of a long segment
            // reads what is new, not what it read before.
            let mut bytes = vec![0u8; (len - off) as usize];
            file.read_exact_at(&mut bytes, off)
                .with_context(|| format!("reading segment {seg}"))?;
            let mut i = 0usize;
            while i < bytes.len() {
                match read_frame(&bytes, i, seg, off, self.position + 1) {
                    FrameRead::Whole { end, records, .. } => {
                        if let Some((last, _)) = records.last() {
                            self.position = last.position;
                        }
                        frames.push(Frame {
                            first: records.first().map_or(0, |(r, _)| r.position),
                            records: records.iter().map(|(r, _)| label(r)).collect(),
                        });
                        self.segment = seg;
                        self.offset = off + end as u64;
                        i = end;
                    }
                    // Being written, or a torn tail: the next read sees it
                    // whole, or the core's open cuts it. Not this segment's
                    // end, unless a later one exists.
                    FrameRead::Partial { .. } => {
                        if segments.last() == Some(&seg) {
                            return Ok(frames);
                        }
                        bail!("segment {seg} holds a frame that does not check, and it has a successor");
                    }
                    FrameRead::Wrong(e) => bail!("segment {seg}, offset {i}: {e}"),
                }
            }
            // The end of this segment's bytes: the next read starts here, or
            // at the next segment's start.
            self.segment = seg;
            self.offset = off + bytes.len() as u64;
        }
        Ok(frames)
    }
}

/// A record's label: its kind, and a ledger row's own kind.
fn label(r: &Record) -> String {
    let name = kinds::name(r.kind);
    if r.kind == kinds::LEDGER {
        if let Some(k) = serde_json::from_slice::<serde_json::Value>(&r.payload)
            .ok()
            .and_then(|v| v["kind"].as_str().map(str::to_string))
        {
            return format!("{name}:{k}");
        }
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_store::{NewRecord, Wal, WalConfig};

    fn ledger(kind: &str) -> NewRecord {
        NewRecord::bytes(
            kinds::LEDGER,
            None,
            serde_json::to_vec(&serde_json::json!({"kind": kind})).unwrap(),
        )
    }

    /// A WAL in a temporary directory, and a tail on it.
    fn wal() -> (tempfile::TempDir, Wal) {
        let dir = tempfile::tempdir().unwrap();
        let wal = Wal::open(dir.path(), WalConfig::default()).unwrap();
        (dir, wal)
    }

    #[test]
    fn a_tail_counts_the_frames_written_since_its_last_read() {
        let (dir, wal) = wal();
        wal.append(&[ledger("a")]).unwrap();
        let mut tail = Tail::at_end(dir.path()).unwrap();
        assert_eq!(tail.position(), 1, "what was there is not counted");
        assert!(tail.read().unwrap().is_empty());
        wal.append(&[
            ledger("turn.started"),
            NewRecord::bytes(kinds::NODE, None, vec![1]),
        ])
        .unwrap();
        wal.append(&[ledger("turn.ended")]).unwrap();
        let frames = tail.read().unwrap();
        assert_eq!(
            frames.len(),
            2,
            "two appends are two frames, not three records"
        );
        assert_eq!(frames[0].first, 2);
        assert_eq!(frames[0].label(), "[ledger:turn.started, node]");
        assert_eq!(frames[1].first, 4);
        assert_eq!(tail.position(), 4);
        assert!(tail.read().unwrap().is_empty(), "a read moves the cursor");
    }

    #[test]
    fn a_tail_from_the_start_reads_every_frame_and_none_twice() {
        let (dir, wal) = wal();
        for k in ["a", "b", "c"] {
            wal.append(&[ledger(k)]).unwrap();
        }
        let mut tail = Tail::at_start(dir.path());
        assert_eq!(tail.read().unwrap().len(), 3);
        wal.append(&[ledger("d")]).unwrap();
        assert_eq!(tail.read().unwrap().len(), 1);
    }

    #[test]
    fn a_tail_follows_the_log_across_segments() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = WalConfig {
            segment_bytes: 256,
            ..WalConfig::default()
        };
        let wal = Wal::open(dir.path(), cfg).unwrap();
        let mut tail = Tail::at_start(dir.path());
        let mut seen = 0;
        for i in 0..40 {
            wal.append(&[NewRecord::bytes(kinds::META, None, vec![i as u8; 64])])
                .unwrap();
            seen += tail.read().unwrap().len();
        }
        assert_eq!(seen, 40);
        assert!(list_segments(dir.path()).unwrap().len() > 3, "it rolled");
        let again = Tail::at_start(dir.path()).read().unwrap();
        assert_eq!(again.len(), 40, "a fresh tail reads all of them");
    }

    #[test]
    fn a_frame_being_written_waits_for_the_next_read() {
        let (dir, wal) = wal();
        wal.append(&[ledger("a")]).unwrap();
        let seg = segment_path(dir.path(), 1);
        let whole = std::fs::read(&seg).unwrap();
        // The next frame's first bytes only.
        wal.append(&[ledger("b")]).unwrap();
        let both = std::fs::read(&seg).unwrap();
        std::fs::write(&seg, &both[..whole.len() + 5]).unwrap();
        let mut tail = Tail::at_start(dir.path());
        assert_eq!(tail.read().unwrap().len(), 1, "the half frame is left");
        std::fs::write(&seg, &both).unwrap();
        assert_eq!(tail.read().unwrap().len(), 1, "and read once it is whole");
    }

    #[test]
    fn a_log_with_no_directory_yet_reads_as_no_frames() {
        let dir = tempfile::tempdir().unwrap();
        let mut tail = Tail::at_start(&dir.path().join("wal"));
        assert!(tail.read().unwrap().is_empty());
    }
}
