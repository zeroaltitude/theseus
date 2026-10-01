//! A job's granted secrets, withheld from its raw output (theseus-l0d).
//!
//! A program the broker grants a secret (theseus-dcy) gets it in its
//! environment, and may print it: `gh auth token` does. The scrubber keeps it
//! from the model, the store, and every surface, but the spool's raw output
//! file held it, and the floor keeps that file. The wrapper knows the granted
//! values, since they are its own environment. So for a job with a grant, the
//! command writes to a pipe, and the wrapper copies what it reads into the
//! spool file with each value replaced by `[redacted:<secret>]`, the
//! scrubber's mark: no granted value reaches the disk.
//!
//! A value can straddle two reads. The copy holds back the bytes at the end of
//! each read that could begin a value (never more than the longest value less
//! one byte) until the next read, or the pipe's end, decides them. Everything
//! else is written as it is read, so the file still grows as the command
//! prints.
//!
//! Every job's output takes this copy since theseus-102, a grant or none: the
//! copy also caps the file. Past the cap it keeps reading, so the command
//! never blocks or dies for printing, and counts what it drops.
//!
//! The cap keeps both ends (theseus-gsn9): builds, tests, and installers print
//! their verdict last. The file takes the output's head as it is read, up to
//! the cap less [`TAIL_BYTES`] (less with a small cap) and a marker's room.
//! Past the head the copy keeps the latest `TAIL_BYTES` in a ring in the
//! wrapper's memory, and at the pipe's end writes them after a marker that
//! says how many bytes between the two ends were dropped. So the runtime's
//! read of a job's last 4 MiB is the job's real end. The copy's memory is one
//! read's buffer and that ring, whatever the job prints. A wrapper killed
//! before the pipe's end loses the ring, and its file holds the head.

use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// The scrubber's floor: a shorter value is not withheld, so that a common
/// short string is not masked everywhere it appears.
pub const MIN_LEN: usize = 8;

/// The granted values to withhold, and what stands in for each.
pub struct Redactor {
    /// (value, its mark), the longest value first.
    values: Vec<(Vec<u8>, Vec<u8>)>,
    /// Bytes read and not yet decided: a tail that could begin a value.
    held: Vec<u8>,
    /// How many values were withheld.
    withheld: u64,
}

impl std::fmt::Debug for Redactor {
    /// Never a value: how many there are, and their lengths.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lens: Vec<usize> = self.values.iter().map(|(v, _)| v.len()).collect();
        f.debug_struct("Redactor")
            .field("value_lens", &lens)
            .field("withheld", &self.withheld)
            .finish()
    }
}

impl Drop for Redactor {
    fn drop(&mut self) {
        for (v, _) in &mut self.values {
            v.fill(0);
        }
        self.held.fill(0);
    }
}

impl Redactor {
    /// `granted`: (the secret's name, its value). A value is trimmed, as the
    /// scrubber trims it, and one shorter than `MIN_LEN` is left alone.
    pub fn new(granted: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        let mut values: Vec<(Vec<u8>, Vec<u8>)> = granted
            .into_iter()
            .map(|(name, v)| {
                (
                    v.trim_ascii().to_vec(),
                    format!("[redacted:{name}]").into_bytes(),
                )
            })
            .filter(|(v, _)| v.len() >= MIN_LEN)
            .collect();
        values.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
        values.dedup_by(|a, b| a.0 == b.0);
        Self {
            values,
            held: Vec::new(),
            withheld: 0,
        }
    }

    /// Nothing to withhold: the command may write its file itself.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// How many values have been withheld so far.
    pub fn withheld(&self) -> u64 {
        self.withheld
    }

    /// The longest value's length: what a copy may hold back is one less.
    pub fn longest(&self) -> usize {
        self.values.first().map_or(0, |(v, _)| v.len())
    }

    /// Bytes read: what is now decided goes to `out`, each whole value
    /// replaced by its mark, and a tail that could still begin one is kept
    /// back for the next read.
    pub fn feed(&mut self, chunk: &[u8], out: &mut Vec<u8>) {
        let mut buf = std::mem::take(&mut self.held);
        buf.extend_from_slice(chunk);
        let kept = self.decide(&buf, false, out);
        self.held = buf[kept..].to_vec();
        buf.fill(0);
    }

    /// No more bytes come: everything held back is decided now.
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        let mut buf = std::mem::take(&mut self.held);
        self.decide(&buf, true, out);
        buf.fill(0);
    }

    /// Write what `buf` decides to `out`, each whole value as its mark, the
    /// leftmost first and the longest of those that start there, as one pass
    /// over the whole output would. Returns where the undecided tail begins:
    /// the longest tail that a later read could still make into a value, or
    /// into a longer one than it holds (none at the `end`).
    fn decide(&mut self, buf: &[u8], end: bool, out: &mut Vec<u8>) -> usize {
        let mut at = 0;
        loop {
            let undecided = if end {
                buf.len()
            } else {
                let rest = &buf[at..];
                buf.len()
                    - self
                        .values
                        .iter()
                        .map(|(v, _)| begins(rest, v))
                        .max()
                        .unwrap_or(0)
            };
            // A value found at or past the undecided tail may still be the
            // start of a longer one, or lie inside one that begins earlier.
            let next = self
                .values
                .iter()
                .enumerate()
                .filter_map(|(i, (v, _))| find(&buf[at..], v).map(|p| (at + p, i)))
                .filter(|&(p, _)| p < undecided)
                .min_by_key(|&(p, i)| (p, i));
            let Some((start, i)) = next else {
                out.extend_from_slice(&buf[at..undecided]);
                return undecided;
            };
            out.extend_from_slice(&buf[at..start]);
            out.extend_from_slice(&self.values[i].1);
            self.withheld += 1;
            at = start + self.values[i].0.len();
        }
    }
}

/// Where `needle` first occurs in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let first = needle[0];
    let last = hay.len() - needle.len();
    let mut i = 0;
    while i <= last {
        i += hay[i..=last].iter().position(|&b| b == first)?;
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The longest tail of `hay` that is a proper beginning of `value`: bytes a
/// later read may complete into it.
fn begins(hay: &[u8], value: &[u8]) -> usize {
    let most = hay.len().min(value.len().saturating_sub(1));
    (1..=most)
        .rev()
        .find(|&k| hay[hay.len() - k..] == value[..k])
        .unwrap_or(0)
}

/// How long the wrapper waits, after its command exits, for the copy to reach
/// the pipe's end before it reports: a descendant that keeps the command's
/// output open would hold the report otherwise. The copy goes on after it.
pub const DRAIN: Duration = Duration::from_millis(200);

/// The most of an output's end the copy keeps past its head (theseus-gsn9):
/// the runtime's read of a job's output is its last 4 MiB.
pub const TAIL_BYTES: u64 = 4 * 1024 * 1024;

/// The room the marker between the two ends takes from the head, so the file
/// never passes the cap: [`marker`] with two 20-digit numbers fits it.
pub const MARKER_ROOM: u64 = 128;

/// How the copy splits a cap of `max_bytes` (theseus-gsn9): the head the file
/// takes as the output is read, and the end the ring keeps past it, half the
/// cap at most. A cap too small to keep both keeps the head alone.
pub fn split(max_bytes: u64) -> (u64, u64) {
    let tail = (max_bytes / 2).min(TAIL_BYTES);
    if tail < MARKER_ROOM * 2 {
        return (max_bytes, 0);
    }
    (max_bytes - tail - MARKER_ROOM, tail)
}

/// The line between the head and the end, when bytes between them were
/// dropped: at most [`MARKER_ROOM`] bytes.
pub fn marker(dropped: u64, tail: u64) -> String {
    format!(
        "\n[theseus: {dropped} bytes dropped here, past the output cap; the last {tail} bytes \
         follow]\n"
    )
}

/// What a finished copy did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Copied {
    /// Granted values withheld (theseus-l0d).
    pub withheld: u64,
    /// Bytes the job printed past the cap, read and dropped (theseus-102):
    /// since theseus-gsn9, those between the head and the end the ring kept.
    pub dropped: u64,
    /// The head's bytes, and the end's that follow the marker (theseus-gsn9),
    /// when bytes between them were dropped; 0 and 0 otherwise.
    pub head: u64,
    pub tail: u64,
    /// Why the file stopped taking the output before the cap: a write that
    /// failed, as on a full disk. The copy read on and counted the rest as
    /// dropped, so the command still never blocked.
    pub write_error: Option<String>,
}

/// The end of the output past the head: its last `cap` bytes, oldest first,
/// in a buffer the wrapper allocates when the head fills (theseus-gsn9).
struct Ring {
    buf: Vec<u8>,
    cap: usize,
    start: usize,
    len: usize,
}

impl Ring {
    fn new(cap: u64) -> Self {
        Ring {
            buf: Vec::new(),
            cap: cap as usize,
            start: 0,
            len: 0,
        }
    }

    /// Keep `bytes` after what the ring holds; returns how many of the
    /// oldest bytes, its own or theirs, no longer fit.
    fn push(&mut self, bytes: &[u8]) -> u64 {
        if self.cap == 0 {
            return bytes.len() as u64;
        }
        if self.buf.is_empty() {
            self.buf = vec![0u8; self.cap];
        }
        let mut gone = 0u64;
        let bytes = match bytes.len().checked_sub(self.cap) {
            Some(over) if over > 0 => {
                gone += over as u64;
                &bytes[over..]
            }
            _ => bytes,
        };
        let over = (self.len + bytes.len()).saturating_sub(self.cap);
        if over > 0 {
            self.start = (self.start + over) % self.cap;
            self.len -= over;
            gone += over as u64;
        }
        let at = (self.start + self.len) % self.cap;
        let first = (self.cap - at).min(bytes.len());
        self.buf[at..at + first].copy_from_slice(&bytes[..first]);
        self.buf[..bytes.len() - first].copy_from_slice(&bytes[first..]);
        self.len += bytes.len();
        gone
    }

    /// What it holds, oldest first, in at most two pieces.
    fn pieces(&self) -> (&[u8], &[u8]) {
        let end = self.start + self.len;
        if end <= self.cap {
            (&self.buf[self.start..end], &[])
        } else {
            (&self.buf[self.start..], &self.buf[..end - self.cap])
        }
    }

    /// Hold nothing and take nothing more; returns what it held.
    fn close(&mut self) -> u64 {
        let held = self.len as u64;
        self.buf.fill(0);
        self.buf = Vec::new();
        (self.cap, self.start, self.len) = (0, 0, 0);
        held
    }
}

/// The spool file, holding at most `room` more bytes of the output's head,
/// then the ring of its end (theseus-gsn9). What does not fit is counted,
/// never kept.
struct Capped {
    file: File,
    room: u64,
    ring: Ring,
    /// The head's bytes written.
    head: u64,
    dropped: Arc<AtomicU64>,
    /// What the ring holds now, which the report names while the copy goes on.
    held: Arc<AtomicU64>,
    error: Option<String>,
}

impl Capped {
    /// Keep what fits, the head and then the end, and count the rest. A
    /// failed write keeps nothing more.
    fn put(&mut self, bytes: &[u8]) {
        let fits = (bytes.len() as u64).min(self.room) as usize;
        if fits > 0 {
            if let Err(e) = self.file.write_all(&bytes[..fits]) {
                self.fail(e);
                self.drop_bytes(fits as u64);
            } else {
                self.room -= fits as u64;
                self.head += fits as u64;
            }
        }
        let rest = &bytes[fits..];
        if !rest.is_empty() {
            let gone = self.ring.push(rest);
            self.drop_bytes(gone);
            self.held.store(self.ring.len as u64, Ordering::Relaxed);
        }
    }

    /// A write failed: the file takes nothing more, and the ring's bytes are
    /// dropped with the rest.
    fn fail(&mut self, e: std::io::Error) {
        self.error = Some(e.to_string());
        self.room = 0;
        let held = self.ring.close();
        self.drop_bytes(held);
        self.held.store(0, Ordering::Relaxed);
    }

    fn drop_bytes(&self, n: u64) {
        if n > 0 {
            self.dropped.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Nothing more can be kept: past the head with no ring (a cap too small
    /// for one, or a failed write).
    fn full(&self) -> bool {
        self.room == 0 && self.ring.cap == 0
    }

    /// The pipe's end: the ring's bytes follow the head, after the marker
    /// when bytes between them were dropped. Returns the head's bytes and the
    /// end's when there was a marker, (0, 0) when there was none.
    fn finish(&mut self) -> (u64, u64) {
        if self.ring.len == 0 {
            return (0, 0);
        }
        let dropped = self.dropped.load(Ordering::Relaxed);
        let tail = self.ring.len as u64;
        let written = {
            let (a, b) = self.ring.pieces();
            let line = (dropped > 0).then(|| marker(dropped, tail));
            line.map_or(Ok(()), |l| self.file.write_all(l.as_bytes()))
                .and_then(|()| self.file.write_all(a))
                .and_then(|()| self.file.write_all(b))
        };
        self.ring.close();
        self.held.store(0, Ordering::Relaxed);
        match written {
            Ok(()) if dropped > 0 => (self.head, tail),
            Ok(()) => (0, 0),
            Err(e) => {
                self.error = Some(e.to_string());
                self.drop_bytes(tail);
                (0, 0)
            }
        }
    }
}

/// A job's output on its way from the command's pipe to the spool file.
pub struct Copier {
    done: mpsc::Receiver<Result<Copied, String>>,
    ended: Option<Result<Copied, String>>,
    /// What has been dropped so far, while the copy goes on.
    dropped: Arc<AtomicU64>,
    /// What the ring holds so far, not yet in the file (theseus-gsn9).
    held: Arc<AtomicU64>,
}

impl Copier {
    /// Copy `from` into `to` on a thread of its own, withholding `redactor`'s
    /// values, until every writer has closed the pipe. The file takes at most
    /// `max_bytes`: the head as it is read, and at the pipe's end the last
    /// bytes, which wait in a ring meanwhile, after a marker ([`split`],
    /// theseus-gsn9). The copy reads on past them and counts the bytes
    /// between dropped (theseus-102). Every byte that may be kept goes
    /// through the redactor; once nothing more can be kept (a write failed),
    /// the bytes are counted as the job printed them, unredacted.
    pub fn spawn(
        mut from: std::io::PipeReader,
        to: File,
        mut redactor: Redactor,
        max_bytes: u64,
    ) -> std::io::Result<Self> {
        let (tx, done) = mpsc::channel();
        let dropped = Arc::new(AtomicU64::new(0));
        let held = Arc::new(AtomicU64::new(0));
        let (head, tail) = split(max_bytes);
        let mut sink = Capped {
            file: to,
            room: head,
            ring: Ring::new(tail),
            head: 0,
            dropped: dropped.clone(),
            held: held.clone(),
            error: None,
        };
        std::thread::Builder::new()
            .name("job-output".into())
            .spawn(move || {
                let mut copy = || -> std::io::Result<Copied> {
                    let mut buf = vec![0u8; 64 * 1024];
                    let mut out = Vec::with_capacity(buf.len() + 256);
                    loop {
                        let n = match from.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(e) => {
                                // What the ring kept still follows the head.
                                sink.finish();
                                return Err(e);
                            }
                        };
                        if sink.full() {
                            sink.drop_bytes(n as u64);
                        } else if redactor.is_empty() {
                            sink.put(&buf[..n]);
                        } else {
                            out.clear();
                            redactor.feed(&buf[..n], &mut out);
                            sink.put(&out);
                        }
                    }
                    out.clear();
                    redactor.finish(&mut out);
                    sink.put(&out);
                    buf.fill(0);
                    out.fill(0);
                    let (head, tail) = sink.finish();
                    Ok(Copied {
                        withheld: redactor.withheld(),
                        dropped: sink.dropped.load(Ordering::Relaxed),
                        head,
                        tail,
                        write_error: sink.error.take(),
                    })
                };
                let _ = tx.send(copy().map_err(|e| e.to_string()));
            })?;
        Ok(Self {
            done,
            ended: None,
            dropped,
            held,
        })
    }

    /// Wait at most `bound` for the copy to end; its result once it has: what
    /// it withheld and dropped, or why it stopped reading.
    pub fn wait(&mut self, bound: Duration) -> Option<Result<Copied, String>> {
        if self.ended.is_none() {
            self.ended = self.done.recv_timeout(bound).ok();
        }
        self.ended.clone()
    }

    /// Bytes dropped past the cap so far: while a descendant keeps the output
    /// open, the copy has not ended, and this is what the report can say.
    pub fn dropped_so_far(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The end's bytes the ring holds so far, not yet in the file
    /// (theseus-gsn9): the copy writes them at the pipe's end.
    pub fn held_so_far(&self) -> u64 {
        self.held.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALUE: &[u8] = b"tv-invented_token-7f3a9c0d";

    fn redactor(values: &[(&str, &[u8])]) -> Redactor {
        Redactor::new(values.iter().map(|(n, v)| (n.to_string(), v.to_vec())))
    }

    /// Every chunking of `input`, fed in two reads split at each byte.
    fn split_everywhere(values: &[(&str, &[u8])], input: &[u8]) -> Vec<Vec<u8>> {
        (0..=input.len())
            .map(|k| {
                let mut r = redactor(values);
                let mut out = Vec::new();
                r.feed(&input[..k], &mut out);
                r.feed(&input[k..], &mut out);
                r.finish(&mut out);
                out
            })
            .collect()
    }

    /// A value straddling two reads is withheld wherever the split falls, and
    /// the rest of the output is intact, byte for byte.
    #[test]
    fn a_value_split_across_two_reads_is_withheld_wherever_the_split_falls() {
        let input = [b"before ".as_slice(), VALUE, b" after\n"].concat();
        let want = b"before [redacted:invented_token] after\n".to_vec();
        for (k, got) in split_everywhere(&[("invented_token", VALUE)], &input)
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                String::from_utf8_lossy(&got),
                String::from_utf8_lossy(&want),
                "split at {k}"
            );
        }
    }

    /// Only a tail that could begin a value is held back, and it is let go as
    /// soon as the next read shows it does not.
    #[test]
    fn only_a_tail_that_could_begin_a_value_waits_for_the_next_read() {
        let mut r = redactor(&[("invented_token", VALUE)]);
        let mut out = Vec::new();
        r.feed(b"plain text, no secret\n", &mut out);
        assert_eq!(out, b"plain text, no secret\n", "nothing held");
        out.clear();
        r.feed(b"almost tv-inv", &mut out);
        assert_eq!(out, b"almost ", "`tv-inv` could begin the value");
        out.clear();
        r.feed(b"oice\n", &mut out);
        assert_eq!(out, b"tv-invoice\n");
        out.clear();
        r.feed(b"t", &mut out);
        r.finish(&mut out);
        assert_eq!(out, b"t", "the end lets the tail go");
        assert_eq!(r.withheld(), 0);
    }

    /// The same output however it is read: a long run with values of two
    /// lengths, one the start of the other, and near misses, cut into reads
    /// of every size from 1 to 97 bytes, matches one replace over the whole.
    #[test]
    fn any_chunking_matches_a_replace_over_the_whole_output() {
        let long: &[u8] = b"tv-invented_token-7f3a9c0d-and-more";
        let short: &[u8] = b"tv-invented_token";
        let values = [("long", long), ("short", short)];
        let mut input = Vec::new();
        let mut seed = 7u64;
        for i in 0..400 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            match (seed >> 33) % 6 {
                0 => input.extend_from_slice(long),
                1 => input.extend_from_slice(short),
                2 => input.extend_from_slice(&short[..short.len() - 1]),
                3 => input.extend_from_slice(b"tv-tv-invented_tok"),
                _ => input.extend_from_slice(format!(" line {i}\n").as_bytes()),
            }
        }
        // The whole output at once: leftmost, then longest.
        let mut want = Vec::new();
        let mut at = 0;
        while at < input.len() {
            if input[at..].starts_with(long) {
                want.extend_from_slice(b"[redacted:long]");
                at += long.len();
            } else if input[at..].starts_with(short) {
                want.extend_from_slice(b"[redacted:short]");
                at += short.len();
            } else {
                want.push(input[at]);
                at += 1;
            }
        }
        for size in 1..=97 {
            let mut r = redactor(&values);
            let mut out = Vec::new();
            for chunk in input.chunks(size) {
                r.feed(chunk, &mut out);
            }
            r.finish(&mut out);
            assert!(out == want, "reads of {size} bytes differ");
            assert!(find(&out, short).is_none(), "reads of {size} bytes");
        }
    }

    /// Copy `input`, written by a thread of its own, into `to` with no grant
    /// and the cap `max`: what the copy did, once the writer has finished.
    fn copy_through(to: File, max: u64, input: Vec<u8>) -> Copied {
        let (read, mut write) = std::io::pipe().unwrap();
        let mut c = Copier::spawn(read, to, Redactor::new([]), max).unwrap();
        let writer = std::thread::spawn(move || {
            write.write_all(&input).unwrap();
        });
        writer.join().unwrap();
        c.wait(Duration::from_secs(10)).unwrap().unwrap()
    }

    /// `input`, as the copy keeps it under a cap of `max`: the whole, or
    /// the head, the marker, and the exact last bytes.
    fn kept(input: &[u8], max: u64) -> Vec<u8> {
        let (head, tail) = split(max);
        if input.len() as u64 <= head + tail {
            return input.to_vec();
        }
        let dropped = input.len() as u64 - head - tail;
        [
            &input[..head as usize],
            marker(dropped, tail).as_bytes(),
            &input[input.len() - tail as usize..],
        ]
        .concat()
    }

    /// The cap (theseus-102) keeps both ends (theseus-gsn9): the file holds
    /// the head, a marker that counts the bytes dropped between, and the
    /// output's exact last bytes, never more than the cap; the rest is read
    /// and counted, so the writer finishes.
    #[test]
    fn the_copy_keeps_the_head_and_the_exact_last_bytes_and_counts_the_middle() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("out");
        let input: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let copied = copy_through(File::create(&path).unwrap(), 100_000, input.clone());
        let file = std::fs::read(&path).unwrap();
        assert_eq!(split(100_000), (49_872, 50_000));
        assert_eq!(file, kept(&input, 100_000));
        assert!(file.len() <= 100_000, "{}", file.len());
        assert_eq!(&file[file.len() - 50_000..], &input[250_000..]);
        assert_eq!(
            copied,
            Copied {
                withheld: 0,
                dropped: 200_128,
                head: 49_872,
                tail: 50_000,
                write_error: None
            }
        );
    }

    /// Every output size around the cap, written in reads of odd sizes: what
    /// fits is kept whole with no marker; past it, the head and the exact
    /// end, the marker between, and the file never past the cap. A cap too
    /// small for a ring keeps the head alone, as before.
    #[test]
    fn every_size_around_the_cap_keeps_what_fits_or_both_ends() {
        let d = tempfile::tempdir().unwrap();
        let max = 10_000u64;
        let (head, tail) = split(max);
        for len in [
            0,
            1,
            head - 1,
            head,
            head + 1,
            head + tail,
            head + tail + 1,
            max,
            3 * max,
        ] {
            let input: Vec<u8> = (0..len as u32).map(|i| (i * 7 % 253) as u8).collect();
            let path = d.path().join(format!("out-{len}"));
            let copied = copy_through(File::create(&path).unwrap(), max, input.clone());
            let file = std::fs::read(&path).unwrap();
            assert_eq!(file, kept(&input, max), "{len} bytes");
            assert!(file.len() as u64 <= max, "{len} bytes: {}", file.len());
            let dropped = len.saturating_sub(head + tail);
            assert_eq!(copied.dropped, dropped, "{len} bytes");
            let ends = if dropped > 0 { (head, tail) } else { (0, 0) };
            assert_eq!((copied.head, copied.tail), ends, "{len} bytes");
        }
        assert_eq!(split(300), (300, 0), "too small for both ends");
        let input = vec![9u8; 1_000];
        let path = d.path().join("small");
        let copied = copy_through(File::create(&path).unwrap(), 300, input);
        assert_eq!(std::fs::read(&path).unwrap(), vec![9u8; 300]);
        assert_eq!((copied.dropped, copied.tail), (700, 0));
    }

    /// The ring keeps its last bytes whatever the reads: wrapping, a read
    /// longer than it, and an empty one.
    #[test]
    fn the_ring_keeps_its_last_bytes_across_wraps() {
        let mut r = Ring::new(5);
        assert_eq!(r.push(b"abc"), 0);
        assert_eq!(r.push(b"de"), 0);
        assert_eq!(r.push(b"fg"), 2);
        assert_eq!([r.pieces().0, r.pieces().1].concat(), b"cdefg");
        assert_eq!(r.push(b""), 0);
        assert_eq!(r.push(b"0123456789"), 10);
        assert_eq!([r.pieces().0, r.pieces().1].concat(), b"56789");
        assert_eq!(r.close(), 5);
        assert_eq!(r.push(b"x"), 1, "a closed ring keeps nothing");
    }

    /// The marker fits its room with the largest numbers.
    #[test]
    fn the_marker_fits_its_room() {
        assert!(marker(u64::MAX, u64::MAX).len() as u64 <= MARKER_ROOM);
    }

    /// A full disk (`/dev/full`) fails every write: the copy keeps reading,
    /// so the writer is never blocked or killed by a closed pipe, and it
    /// says why the file took nothing.
    #[test]
    fn a_full_disk_never_blocks_the_writer() {
        let full = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .unwrap();
        let copied = copy_through(full, u64::MAX, vec![7u8; 1_000_000]);
        assert_eq!(copied.dropped, 1_000_000);
        assert!(copied.write_error.is_some(), "{copied:?}");
    }

    /// The scrubber's rule: a value is trimmed, and one under 8 bytes is not
    /// withheld. Nothing prints a value.
    #[test]
    fn short_values_are_left_alone_and_nothing_prints_a_value() {
        assert!(redactor(&[("pin", b"1234")]).is_empty());
        let r = redactor(&[("invented_token", b"  tv-invented_token-7f3a9c0d\n")]);
        assert_eq!(r.longest(), VALUE.len());
        let shown = format!("{r:?}");
        assert!(!shown.contains("tv-"), "{shown}");
    }
}
