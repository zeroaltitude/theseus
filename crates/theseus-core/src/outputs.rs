//! A capped result's whole output, kept (theseus-v73m). When the result cap
//! cuts a job's output (`proc.run`, a batch's step) or a terminal's screen
//! (`term.read`), the runtime keeps a scrubbed copy of the whole of it, the
//! text the model would have been shown uncut, at
//! `<state>/outputs/<session>/<call>.out`, and the cut line names it with the
//! lines it left out, which `fs_read` then reads (the session's own outputs
//! read as a file inside the roots does: `own_read`, in the gate's order).
//!
//! Scrubbed, never raw: the job's raw output file in the spool, which holds
//! what it printed before the scrubber saw it, is still deleted once its
//! result is written (theseus-wz2). The copy is scrubbed in a streaming pass
//! of whole lines, a chunk at a time, so a job's 64 MiB is never held whole;
//! a private-key block open at a chunk's end runs on to its END line first,
//! so the scrubber sees it whole. It is written 0600 under a 0700 directory,
//! to a temporary name and renamed, with no sync: losing a copy to a crash
//! costs a rerun, as before.
//!
//! The sweep (`Outputs::sweep`, from `Core::sweep_outputs`, after serving
//! with the spool's hourly sweep, never on a turn's path) deletes a session's
//! copies once it retires, any copy older than `[tools] outputs_keep_days`,
//! and the oldest first while all of them pass `[tools] outputs_max_bytes`.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use theseus_protocol::SessionState;

/// The raw output read into one chunk before it is scrubbed, cut at a line's
/// end; a line longer than `LONG_LINE` is cut there.
const CHUNK: usize = 1 << 20;
const LONG_LINE: u64 = 4 << 20;

/// The most a chunk runs on past `CHUNK` to reach an open private-key
/// block's END line.
const BLOCK_MORE: usize = 1 << 20;

/// Where the kept outputs live, and the sweep's bounds.
#[derive(Debug, Clone)]
pub struct Outputs {
    dir: Option<PathBuf>,
    keep: Duration,
    max_total: u64,
}

/// What one sweep did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Swept {
    pub removed: u64,
    pub removed_bytes: u64,
    pub kept: u64,
    pub kept_bytes: u64,
}

/// A path's part from an id: its letters, digits, `-` and `_`, every other
/// character `_`, so no id climbs out of its directory.
fn part(id: &str) -> String {
    let s: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "_".into()
    } else {
        s
    }
}

impl Outputs {
    /// Kept under `dir`, swept past `keep_days` and `max_total` bytes.
    pub fn new(dir: PathBuf, keep_days: u64, max_total: u64) -> Self {
        Self {
            dir: Some(dir),
            keep: Duration::from_secs(keep_days.saturating_mul(24 * 60 * 60)),
            max_total,
        }
    }

    /// Nothing kept: a runtime with no state dir.
    pub fn off() -> Self {
        Self {
            dir: None,
            keep: Duration::ZERO,
            max_total: 0,
        }
    }

    /// The directory every session's copies are under.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// A session's directory.
    pub fn session_dir(&self, session: &str) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(part(session)))
    }

    /// Where a call's whole output is kept.
    pub fn path_for(&self, session: &str, call: &str) -> Option<PathBuf> {
        Some(
            self.session_dir(session)?
                .join(format!("{}.out", part(call))),
        )
    }

    /// Whether `tool` reads only the session's own kept outputs: an
    /// `fs.read` whose every path (resolved, as the gate judges it) is under
    /// the session's directory. Nothing else of the state dir.
    pub fn own_read(&self, session: &str, tool: &str, plan: &theseus_tools::Plan) -> bool {
        let Some(dir) = self.session_dir(session) else {
            return false;
        };
        let dir = theseus_tools::paths::canonical_best_effort(&dir);
        tool == "fs.read"
            && !plan.resources.is_empty()
            && plan
                .resources
                .iter()
                .all(|r| theseus_tools::paths::within(&r.path, &dir))
    }

    /// The scrubbed whole of `raw` (at most `max` bytes of it) kept at `to`.
    /// Returns the lines of the raw output before byte `at`: the number of
    /// the line the result's text begins on, less one.
    pub fn keep_file(
        &self,
        raw: &Path,
        to: &Path,
        scrub: &dyn Fn(&str) -> String,
        at: u64,
        max: u64,
    ) -> io::Result<u64> {
        let f = fs::File::open(raw)?;
        // Bounded by the length seen: a file that grows meanwhile is not followed.
        let len = f.metadata()?.len().min(max);
        let mut r = BufReader::with_capacity(64 * 1024, f.take(len));
        write_new(to, |w| {
            let (mut pos, mut before) = (0u64, 0u64);
            let mut buf: Vec<u8> = Vec::with_capacity(CHUNK + 4096);
            loop {
                let carried = buf.len();
                while buf.len() < CHUNK {
                    if (&mut r).take(LONG_LINE).read_until(b'\n', &mut buf)? == 0 {
                        break;
                    }
                }
                let mut more = 0;
                while more < BLOCK_MORE && open_key_block(&buf) {
                    let n = (&mut r).take(LONG_LINE).read_until(b'\n', &mut buf)?;
                    if n == 0 {
                        break;
                    }
                    more += n;
                }
                if buf.len() == carried {
                    if !buf.is_empty() {
                        w.write_all(scrub(&String::from_utf8_lossy(&buf)).as_bytes())?;
                    }
                    break;
                }
                // A chunk a long line cut ends on a character's edge: the
                // rest of a character waits for the next.
                let end = utf8_end(&buf);
                if pos < at {
                    let upto = usize::try_from(at - pos).unwrap_or(usize::MAX).min(end);
                    before += buf[..upto].iter().filter(|&&b| b == b'\n').count() as u64;
                }
                pos += end as u64;
                w.write_all(scrub(&String::from_utf8_lossy(&buf[..end])).as_bytes())?;
                buf.drain(..end);
            }
            Ok(before)
        })
    }

    /// `text`, scrubbed already, kept whole at `to`.
    pub fn keep_text(&self, text: &str, to: &Path) -> io::Result<()> {
        write_new(to, |w| w.write_all(text.as_bytes()))
    }

    /// Delete what has outlived its keep: every copy of a session `retired`
    /// says has retired, each copy older than the keep, then the oldest while
    /// all of them pass the total. Empty directories go too.
    pub fn sweep(&self, retired: &dyn Fn(&str) -> bool, now: SystemTime) -> Swept {
        let mut s = Swept::default();
        let Some(dir) = &self.dir else {
            return s;
        };
        let Ok(sessions) = fs::read_dir(dir) else {
            return s;
        };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        for d in sessions.flatten() {
            let path = d.path();
            if !path.is_dir() {
                continue;
            }
            let mine = listed(&path);
            let gone = retired(&d.file_name().to_string_lossy());
            for (at, len, f) in mine {
                let old = now.duration_since(at).unwrap_or_default() > self.keep;
                if gone || old {
                    s.remove(&f, len);
                } else {
                    files.push((at, len, f));
                }
            }
        }
        // The oldest first, past the total.
        files.sort();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        for (_, len, f) in files {
            if total > self.max_total {
                total -= len;
                s.remove(&f, len);
            } else {
                s.kept += 1;
                s.kept_bytes += len;
            }
        }
        for d in fs::read_dir(dir).into_iter().flatten().flatten() {
            // Only an empty one goes.
            let _ = fs::remove_dir(d.path());
        }
        s
    }
}

impl Swept {
    fn remove(&mut self, f: &Path, len: u64) {
        match fs::remove_file(f) {
            Ok(()) => {
                self.removed += 1;
                self.removed_bytes += len;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(path = %f.display(), error = %e, "a kept output was not removed")
            }
        }
    }
}

/// A directory's files: when each was written, and its length.
fn listed(dir: &Path) -> Vec<(SystemTime, u64, PathBuf)> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok().filter(fs::Metadata::is_file)?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect()
}

/// Write a file through `fill`: 0600, under a 0700 directory, to a
/// temporary name renamed over `to`. No sync (see the module's doc).
fn write_new<R>(to: &Path, fill: impl FnOnce(&mut dyn Write) -> io::Result<R>) -> io::Result<R> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let parent = to
        .parent()
        .ok_or_else(|| io::Error::other("a kept output's path has no directory"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let tmp = to.with_extension("out.tmp");
    let f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    let mut w = io::BufWriter::new(f);
    let r = fill(&mut w).and_then(|r| w.flush().map(|()| r));
    match r {
        Ok(r) => {
            fs::rename(&tmp, to)?;
            Ok(r)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Whether the chunk ends inside a private-key block: its last `-----BEGIN
/// … PRIVATE KEY` has no `-----END ` after it.
fn open_key_block(buf: &[u8]) -> bool {
    const BEGIN: &[u8] = b"-----BEGIN ";
    let Some(at) = buf.windows(BEGIN.len()).rposition(|w| w == BEGIN) else {
        return false;
    };
    let after = &buf[at..];
    let line = after.split(|&b| b == b'\n').next().unwrap_or_default();
    line.windows(11).any(|w| w == b"PRIVATE KEY") && !after.windows(9).any(|w| w == b"-----END ")
}

/// Where the chunk's last whole character ends: its length, unless it ends
/// part-way through a character's bytes.
fn utf8_end(buf: &[u8]) -> usize {
    match std::str::from_utf8(buf) {
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        _ => buf.len(),
    }
}

impl crate::rpc::Core {
    /// Sweep the kept outputs (theseus-v73m): after serving, beside the
    /// spool's sweep, on its blocking thread.
    pub fn sweep_outputs(&self) -> Swept {
        let rule = self.cfg.sessions.rule();
        let now_ms = theseus_protocol::now_unix_ms();
        let retired = |sid: &str| {
            self.store
                .get_session::<crate::session::SessionRecord>(sid)
                .ok()
                .flatten()
                .is_some_and(|r| r.state_at(rule, now_ms, false).0 == SessionState::Retired)
        };
        let s = self.tools.outputs.sweep(&retired, SystemTime::now());
        if s.removed > 0 {
            tracing::info!(
                removed = s.removed,
                bytes = s.removed_bytes,
                kept = s.kept,
                kept_bytes = s.kept_bytes,
                "outputs: kept outputs past their keep were swept"
            );
        }
        s
    }
}
