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
//! never blocks or dies for printing, and counts what it drops. Its memory is
//! one read's buffer, whatever the job prints.

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

/// What a finished copy did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Copied {
    /// Granted values withheld (theseus-l0d).
    pub withheld: u64,
    /// Bytes the job printed past the cap, read and dropped (theseus-102).
    pub dropped: u64,
    /// Why the file stopped taking the output before the cap: a write that
    /// failed, as on a full disk. The copy read on and counted the rest as
    /// dropped, so the command still never blocked.
    pub write_error: Option<String>,
}

/// The spool file, holding at most `room` more bytes. What does not fit is
/// counted, never kept.
struct Capped {
    file: File,
    room: u64,
    dropped: Arc<AtomicU64>,
    error: Option<String>,
}

impl Capped {
    /// Keep what fits and count the rest. A failed write keeps nothing more.
    fn put(&mut self, bytes: &[u8]) {
        let fits = (bytes.len() as u64).min(self.room) as usize;
        if fits > 0 {
            if let Err(e) = self.file.write_all(&bytes[..fits]) {
                self.error = Some(e.to_string());
                self.room = 0;
                self.drop_bytes(fits as u64);
            } else {
                self.room -= fits as u64;
            }
        }
        self.drop_bytes((bytes.len() - fits) as u64);
    }

    fn drop_bytes(&self, n: u64) {
        if n > 0 {
            self.dropped.fetch_add(n, Ordering::Relaxed);
        }
    }

    fn full(&self) -> bool {
        self.room == 0
    }
}

/// A job's output on its way from the command's pipe to the spool file.
pub struct Copier {
    done: mpsc::Receiver<Result<Copied, String>>,
    ended: Option<Result<Copied, String>>,
    /// What has been dropped so far, while the copy goes on.
    dropped: Arc<AtomicU64>,
}

impl Copier {
    /// Copy `from` into `to` on a thread of its own, withholding `redactor`'s
    /// values, until every writer has closed the pipe. The file takes at most
    /// `max_bytes`; the copy reads on past them and counts them dropped
    /// (theseus-102). Past the cap the bytes are counted as the job printed
    /// them, unredacted, since none of them is kept.
    pub fn spawn(
        mut from: std::io::PipeReader,
        to: File,
        mut redactor: Redactor,
        max_bytes: u64,
    ) -> std::io::Result<Self> {
        let (tx, done) = mpsc::channel();
        let dropped = Arc::new(AtomicU64::new(0));
        let mut sink = Capped {
            file: to,
            room: max_bytes,
            dropped: dropped.clone(),
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
                            Err(e) => return Err(e),
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
                    Ok(Copied {
                        withheld: redactor.withheld(),
                        dropped: sink.dropped.load(Ordering::Relaxed),
                        write_error: sink.error.take(),
                    })
                };
                let _ = tx.send(copy().map_err(|e| e.to_string()));
            })?;
        Ok(Self {
            done,
            ended: None,
            dropped,
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

    /// The cap (theseus-102): the file keeps exactly its first `max` bytes,
    /// and the rest is read and counted, so the writer finishes.
    #[test]
    fn the_copy_keeps_the_cap_and_counts_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("out");
        let input: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let copied = copy_through(File::create(&path).unwrap(), 100_000, input.clone());
        assert_eq!(std::fs::read(&path).unwrap(), &input[..100_000]);
        assert_eq!(
            copied,
            Copied {
                withheld: 0,
                dropped: 200_000,
                write_error: None
            }
        );
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
