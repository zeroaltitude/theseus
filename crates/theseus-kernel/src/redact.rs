//! A job's output on its way from the command's pipe to the spool file: the
//! cap, and nothing else.
//!
//! The command writes to a pipe, and the wrapper copies what it reads into the
//! spool file. The file is the record of what the job printed (the owner: "the
//! session must be the true record of what actually happened"), so a value the
//! broker granted the program, which `gh auth token` prints, is in it as the
//! job wrote it. The file is 0600 under the 0700 spool; the exits that leave
//! the machine substitute the vault's values, not this copy.
//!
//! Every job's output takes this copy (theseus-102): it caps the file. Past the
//! cap it keeps reading, so the command never blocks or dies for printing, and
//! counts what it drops.
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
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// How long the wrapper waits, after its command exits, for the copy to reach
/// the pipe's end before it reports: a descendant that keeps the command's
/// output open would hold the report otherwise. The copy goes on after it.
pub const DRAIN: Duration = Duration::from_millis(200);

/// The most a wrapper keeps waiting past [`DRAIN`] for a copy that is behind: bytes the command wrote still wait in
/// the pipe, because the copy's thread is stalled (a write that waits on a busy disk, a wait for a core), which a
/// descendant that holds the pipe open silently never shows (theseus-20).
pub const BEHIND: Duration = Duration::from_secs(5);

/// The bytes written to `pipe` that nobody has read yet.
fn waiting(pipe: &OwnedFd) -> u64 {
    let mut n: libc::c_int = 0;
    // SAFETY: FIONREAD writes one c_int through the pointer it is given, on a pipe this process holds open.
    let rc = unsafe { libc::ioctl(pipe.as_raw_fd(), libc::FIONREAD, &mut n) };
    if rc == 0 {
        n.max(0) as u64
    } else {
        0
    }
}

/// Whether every writer has closed `pipe`: the copy's next read ends it, and what is left is the copy's own work
/// (the write in its hands, the end it keeps in the ring), however long its thread waits for a core or the disk.
fn closed(pipe: &OwnedFd) -> bool {
    let mut p = libc::pollfd {
        fd: pipe.as_raw_fd(),
        events: 0,
        revents: 0,
    };
    // SAFETY: one pollfd on a pipe this process holds open; a timeout of 0 never blocks.
    let rc = unsafe { libc::poll(&mut p, 1, 0) };
    rc > 0 && p.revents & libc::POLLHUP != 0
}

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
    /// Every byte read from the pipe so far, kept or dropped: the job's sign
    /// of life (theseus-x1zz8).
    read: Arc<AtomicU64>,
    /// Every byte the copy has finished with (written, held or dropped): `read` ahead of it is a copy in the middle
    /// of a write, which a pipe that is already empty does not show.
    handled: Arc<AtomicU64>,
    /// The pipe's read end, held to count what waits in it ([`waiting`]).
    peek: Arc<OwnedFd>,
}

impl Copier {
    /// Copy `from` into `to` on a thread of its own, until every writer has
    /// closed the pipe. The file takes at most
    /// `max_bytes`: the head as it is read, and at the pipe's end the last
    /// bytes, which wait in a ring meanwhile, after a marker ([`split`],
    /// theseus-gsn9). The copy reads on past them and counts the bytes
    /// between dropped (theseus-102); once nothing more can be kept (a write
    /// failed), the bytes are counted.
    pub fn spawn(mut from: std::io::PipeReader, to: File, max_bytes: u64) -> std::io::Result<Self> {
        let (tx, done) = mpsc::channel();
        let peek = Arc::new(OwnedFd::from(from.try_clone()?));
        let dropped = Arc::new(AtomicU64::new(0));
        let held = Arc::new(AtomicU64::new(0));
        let read = Arc::new(AtomicU64::new(0));
        let counted = read.clone();
        let handled = Arc::new(AtomicU64::new(0));
        let finished = handled.clone();
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
                        counted.fetch_add(n as u64, Ordering::Relaxed);
                        if sink.full() {
                            sink.drop_bytes(n as u64);
                        } else {
                            sink.put(&buf[..n]);
                        }
                        finished.fetch_add(n as u64, Ordering::Relaxed);
                    }
                    let (head, tail) = sink.finish();
                    Ok(Copied {
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
            read,
            handled,
            peek,
        })
    }

    /// The job's sign of life: the bytes it has written to the pipe, read by the copy or waiting for it. A copy
    /// stalled in a write that waits on a busy disk reads nothing for seconds while the job goes on printing, and
    /// the bytes that wait in the pipe say so; it only grows while the job prints.
    pub fn life(&self) -> impl Fn() -> u64 + Send + 'static {
        let (read, peek) = (self.read.clone(), self.peek.clone());
        move || read.load(Ordering::Relaxed) + waiting(&peek)
    }

    /// Wait at most `bound` for the copy to end; its result once it has: what
    /// it withheld and dropped, or why it stopped reading.
    pub fn wait(&mut self, bound: Duration) -> Option<Result<Copied, String>> {
        if self.ended.is_none() {
            self.ended = self.done.recv_timeout(bound).ok();
        }
        self.ended.clone()
    }

    /// Whether the copy is behind: bytes wait in the pipe, or it has read some it has not finished writing, or
    /// every writer has closed the pipe and the copy has not said it ended. The last is a copy between its last
    /// write and the pipe's end, or writing the end it kept in the ring (`finish`): nothing waits and nothing is
    /// in its hands, and it is about to end by itself. A descendant that holds the pipe open is none of the three.
    fn behind(&self) -> bool {
        waiting(&self.peek) > 0
            || self.read.load(Ordering::Relaxed) > self.handled.load(Ordering::Relaxed)
            || closed(&self.peek)
    }

    /// [`Copier::wait`] for a report that is about to be written: a copy that has not ended and is behind (bytes still
    /// waiting in the pipe, a write in the middle, or a pipe every writer has closed), not held open by a descendant that is silent, so it is waited for, up to
    /// [`BEHIND`] more (in all, at most `bound` + [`BEHIND`] + `bound`). A stop's verdict does not take this wait:
    /// the daemon kills a wrapper that has not answered within a few seconds of the grace, and a stop's verdict
    /// carries no output.
    pub fn wait_behind(&mut self, bound: Duration) -> Option<Result<Copied, String>> {
        if self.ended.is_none() {
            self.ended = self.done.recv_timeout(bound).ok();
            let until = Instant::now() + BEHIND;
            while self.ended.is_none() && self.behind() && Instant::now() < until {
                self.ended = self.done.recv_timeout(bound).ok();
            }
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

    /// Copy `input`, written by a thread of its own, into `to` with the cap
    /// `max`: what the copy did, once the writer has finished.
    fn copy_through(to: File, max: u64, input: Vec<u8>) -> Copied {
        let (read, mut write) = std::io::pipe().unwrap();
        let mut c = Copier::spawn(read, to, max).unwrap();
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

    /// What a job prints is what the file holds, a value the broker granted
    /// it among it, split across two writes: nothing is masked.
    #[test]
    fn a_value_the_job_prints_is_in_the_file_as_printed() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("out");
        let value = b"tv-invented_token-7f3a9c0d";
        let input = [b"before ".as_slice(), value, b" after\n"].concat();
        let copied = copy_through(File::create(&path).unwrap(), 1 << 20, input.clone());
        assert_eq!(std::fs::read(&path).unwrap(), input);
        assert_eq!(copied, Copied::default());
    }

    /// A sink that stalls the copy: a FIFO nobody reads yet, so the copy's thread waits in its write with bytes left
    /// in the pipe, as a write that waits on a busy disk does (theseus-20). The reader of it comes when `gate` says.
    fn stalled_copy(
        dir: &std::path::Path,
        gate: mpsc::Receiver<()>,
        total: usize,
    ) -> (
        Copier,
        std::thread::JoinHandle<()>,
        std::thread::JoinHandle<()>,
    ) {
        let (copier, _, writer, reader) = stalled_copy_capped(dir, gate, total, 10 << 20);
        (copier, writer, reader)
    }

    /// [`stalled_copy`] under a cap of `max`, with the sink's own handle, which says how much the sink holds
    /// before a write to it waits ([`room`]).
    fn stalled_copy_capped(
        dir: &std::path::Path,
        gate: mpsc::Receiver<()>,
        total: usize,
        max: u64,
    ) -> (
        Copier,
        OwnedFd,
        std::thread::JoinHandle<()>,
        std::thread::JoinHandle<()>,
    ) {
        use std::os::unix::ffi::OsStrExt;
        let fifo = dir.join("sink");
        let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: a NUL-terminated path and a mode.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let to = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        let sink = OwnedFd::from(to.try_clone().unwrap());
        let (read, mut write) = std::io::pipe().unwrap();
        let copier = Copier::spawn(read, to, max).unwrap();
        // More than the FIFO (64 KB) and the copy's buffer (64 KB) hold: bytes stay in the pipe.
        let writer = std::thread::spawn(move || write.write_all(&vec![b'x'; total]).unwrap());
        // The sink's reader, from the moment the gate opens: it reads what the copy keeps of `total` under the
        // cap, all of it or both ends and the marker.
        let expect = kept(&vec![b'x'; total], max).len();
        let reader = std::thread::spawn(move || {
            let _ = gate.recv();
            let mut from = std::fs::File::open(&fifo).unwrap();
            let mut got = 0;
            let mut buf = vec![0u8; 64 * 1024];
            while got < expect {
                match from.read(&mut buf).unwrap() {
                    0 => break,
                    n => got += n,
                }
            }
            assert_eq!(got, expect, "the sink ended early");
        });
        (copier, sink, writer, reader)
    }

    /// The most bytes the sink holds before a write to it waits: its size as the kernel made it (64 KB, or less
    /// for a user who holds many pipes).
    fn room(sink: &OwnedFd) -> u64 {
        // SAFETY: F_GETPIPE_SZ takes no argument and reads the size of a pipe this process holds open.
        let n = unsafe { libc::fcntl(sink.as_raw_fd(), libc::F_GETPIPE_SZ) };
        assert!(
            n > 0,
            "the sink's size: {}",
            std::io::Error::last_os_error()
        );
        n as u64
    }

    /// What a pipe made now holds: the size the sink will have.
    fn pipe_room() -> u64 {
        let (read, _write) = std::io::pipe().unwrap();
        room(&OwnedFd::from(read))
    }

    /// Wait until the copy is stalled for good: it has read more than the sink can hold, so the write in its
    /// hands cannot end, and it reads no more, until the gate opens. No clock runs against this: the gate is
    /// opened only after it.
    fn until_stalled(copier: &Copier, sink: &OwnedFd) {
        let (room, t0) = (room(sink), Instant::now());
        while copier.read.load(Ordering::Relaxed) <= room {
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "the copy has read {} and the sink holds {room}",
                copier.read.load(Ordering::Relaxed)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Open the gate `after` from now, on a thread of its own. Started only once the stall is seen, so the time
    /// the machine took to reach the stall is no part of the test.
    fn open_after(release: mpsc::Sender<()>, after: Duration) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let _ = release.send(());
        })
    }

    /// The bytes that wait in the pipe because the copy is stalled still count as the job printing, and a wait for
    /// the copy outlasts its bound while they are there: the copy is behind, and is not held open by a descendant.
    #[test]
    fn a_stalled_copy_is_waited_for_and_the_job_is_seen_printing() {
        let dir = tempfile::tempdir().unwrap();
        let (release, gate) = mpsc::channel();
        let (mut copier, sink, writer, reader) =
            stalled_copy_capped(dir.path(), gate, 300_000, 10 << 20);
        let life = copier.life();
        // The copy's write waits on the sink and it reads no more; the writer fills the pipe behind it, in its
        // own time: bytes wait there, and the sign of life counts the copy's reads and those together.
        until_stalled(&copier, &sink);
        let t0 = Instant::now();
        while waiting(&copier.peek) == 0 {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "nothing waits in the pipe"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let read = copier.read.load(Ordering::Relaxed);
        assert!(read < 300_000, "the copy is behind: it has read {read}");
        assert!(
            life() > read,
            "the bytes that wait count: {} of {read} read",
            life()
        );
        assert!(copier.behind());
        // The gate opens three bounds from now: a wait that gave up at its bound would be over long before.
        let opener = open_after(release, Duration::from_millis(300));
        let ended = copier.wait_behind(Duration::from_millis(100));
        assert!(matches!(ended, Some(Ok(_))), "{ended:?}");
        opener.join().unwrap();
        writer.join().unwrap();
        reader.join().unwrap();
    }

    /// A descendant that holds the pipe open and prints nothing is not waited for past the bound, as before.
    #[test]
    fn a_silent_open_pipe_is_given_up_on_at_the_bound() {
        let (read, write) = std::io::pipe().unwrap();
        let to = tempfile::tempfile().unwrap();
        let mut copier = Copier::spawn(read, to, 1 << 20).unwrap();
        let t0 = Instant::now();
        assert!(copier.wait_behind(Duration::from_millis(100)).is_none());
        assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
        drop(write);
    }

    /// A stop's wait is the plain one: a copy that is behind does not hold it past its bound.
    #[test]
    fn the_plain_wait_gives_up_on_a_stalled_copy_at_its_bound() {
        let dir = tempfile::tempdir().unwrap();
        let (release, gate) = mpsc::channel();
        let (mut copier, sink, writer, reader) =
            stalled_copy_capped(dir.path(), gate, 300_000, 10 << 20);
        until_stalled(&copier, &sink);
        assert!(copier.behind());
        // Nothing opens the gate until the wait is over: a wait that went on behind the copy would take all of
        // `BEHIND`, and the plain one is over at its bound.
        let t0 = Instant::now();
        assert!(copier.wait(Duration::from_millis(100)).is_none());
        assert!(
            t0.elapsed() < BEHIND - Duration::from_secs(2),
            "{:?}",
            t0.elapsed()
        );
        release.send(()).unwrap();
        writer.join().unwrap();
        reader.join().unwrap();
    }

    /// A copy stalled in its last write with the pipe already empty (the sink took what it holds, and the rest
    /// is in the copy's hands) is behind as well: the bytes it holds are the job's output.
    #[test]
    fn a_copy_stalled_in_a_write_with_an_empty_pipe_is_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let (release, gate) = mpsc::channel();
        // A sink's worth and a little more: the writer finishes, and the little more stays in the copy's hands.
        let total = pipe_room() as usize + 4_464;
        let (mut copier, sink, writer, reader) =
            stalled_copy_capped(dir.path(), gate, total, 10 << 20);
        assert!(total as u64 > room(&sink));
        writer.join().unwrap();
        let t0 = Instant::now();
        while copier.read.load(Ordering::Relaxed) < total as u64 || waiting(&copier.peek) > 0 {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "the copy has not taken it all"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(copier.handled.load(Ordering::Relaxed) < total as u64);
        let opener = open_after(release, Duration::from_millis(300));
        let ended = copier.wait_behind(Duration::from_millis(100));
        assert!(matches!(ended, Some(Ok(_))), "{ended:?}");
        opener.join().unwrap();
        reader.join().unwrap();
    }

    /// A copy that has finished with every byte it read and is writing the end it kept (the pipe is empty and
    /// closed, nothing is in its hands) is behind too: without that end the file stops at the head, and the
    /// report would say the output is still open. Seen as a job past its cap reported with its end missing.
    #[test]
    fn a_copy_stalled_writing_the_end_it_kept_is_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let (release, gate) = mpsc::channel();
        // The sink is made first, to cut the cap to its size: the head goes to the sink as it is read and fits;
        // the marker and the end are written at the pipe's end, and do not fit until the sink is read.
        let probe = pipe_room();
        let (max, total) = (probe + probe / 2, 300_000usize);
        let (head, tail) = split(max);
        let (mut copier, sink, writer, reader) = stalled_copy_capped(dir.path(), gate, total, max);
        let room = room(&sink);
        assert!(
            head < room && head + tail > room && (total as u64) > max,
            "{head} {tail} {room}"
        );
        writer.join().unwrap();
        let t0 = Instant::now();
        while copier.handled.load(Ordering::Relaxed) < total as u64 {
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "the copy has not done with it all: {}",
                copier.handled.load(Ordering::Relaxed)
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        // Every byte is read and done with, and nothing waits: only the closed pipe says the copy is not over.
        assert_eq!(copier.read.load(Ordering::Relaxed), total as u64);
        assert_eq!(waiting(&copier.peek), 0);
        assert!(closed(&copier.peek));
        assert!(copier.behind());
        let opener = open_after(release, Duration::from_millis(300));
        let ended = copier.wait_behind(Duration::from_millis(100));
        let Some(Ok(copied)) = ended else {
            panic!("{ended:?}")
        };
        assert_eq!(copied.dropped, total as u64 - head - tail);
        assert_eq!((copied.head, copied.tail), (head, tail));
        opener.join().unwrap();
        reader.join().unwrap();
    }

    /// A pipe a descendant still holds open is not a closed one, with bytes read or without.
    #[test]
    fn a_pipe_is_closed_only_when_every_writer_has_gone() {
        let (read, mut write) = std::io::pipe().unwrap();
        let other = write.try_clone().unwrap();
        let peek = OwnedFd::from(read.try_clone().unwrap());
        assert!(!closed(&peek));
        write.write_all(b"x").unwrap();
        drop(write);
        assert!(!closed(&peek), "a second writer holds it");
        drop(other);
        assert!(
            closed(&peek),
            "a byte still waits, and every writer has gone"
        );
        let mut buf = [0u8; 8];
        assert_eq!((&read).read(&mut buf).unwrap(), 1);
        assert!(closed(&peek));
    }
}
