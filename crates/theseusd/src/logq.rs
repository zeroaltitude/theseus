//! The daemon's log never blocks the daemon.
//!
//! The log is diagnostic; the ledger and the store are the record. A log line written straight to standard error
//! waits for whatever standard error is (a file on a disk whose journal is full, a service manager's journal that has
//! stalled), and the thread that logged waits with it: found holding the daemon's one runtime worker, and `health`
//! with it, for seconds. So a line is formatted by the thread that logs it, handed to a bounded queue, and written by
//! one thread of its own, in the order it was handed over, unchanged.
//!
//! - **Lossy when full.** At [`LINES`] lines or [`BYTES`] bytes waiting, a new line is dropped and counted, and once
//!   the writer has caught up it says how many in one line. A line is whole or absent.
//! - **Flushed** at a clean stop, before a restart's exec, before an error that ends the start is printed, and from
//!   the panic hook ([`flush`]), each for at most a stated time: the writer may be stuck in the very write that
//!   stalled. The common case loses no line. What can still be lost: lines queued while the destination is stalled
//!   at the moment of a crash (the flush gives up after its bound), and everything on a kill the process cannot hook
//!   (`SIGKILL`, the kernel's out-of-memory kill, an abort that is not a panic).
//! - **Nothing that is the record goes through it**: ledger rows and the store never touch this queue.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use tracing_subscriber::fmt::MakeWriter;

/// The most lines waiting for the writer.
pub const LINES: usize = 8192;
/// The most bytes waiting for the writer.
pub const BYTES: usize = 8 << 20;
/// How long a clean stop waits for the queue to be written.
pub const STOP_WAIT: Duration = Duration::from_secs(2);
/// How long the panic hook waits: the process is ending, and a write that has stalled may not come back.
pub const PANIC_WAIT: Duration = Duration::from_secs(1);

/// The queue's limits: a test sets small ones.
#[derive(Debug, Clone, Copy)]
pub struct Caps {
    pub lines: usize,
    pub bytes: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            lines: LINES,
            bytes: BYTES,
        }
    }
}

#[derive(Default)]
struct State {
    queue: VecDeque<Vec<u8>>,
    bytes: usize,
    /// Lines dropped since the writer last said so.
    dropped: u64,
    /// The writer holds a line or a notice it has taken and not finished writing.
    busy: bool,
    /// Callers of `flush` that are waiting (a test opens its gate on it, not on a clock).
    flushers: usize,
}

struct Shared {
    caps: Caps,
    state: Mutex<State>,
    /// The writer waits here for a line.
    work: Condvar,
    /// A flush waits here for the queue to be written.
    done: Condvar,
}

fn locked(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The queue's handle: what the subscriber writes through, and what a stop or a panic flushes.
#[derive(Clone)]
pub struct Log(Arc<Shared>);

impl Log {
    /// A queue written to `sink` by a thread of its own.
    pub fn start<W: Write + Send + 'static>(sink: W, caps: Caps) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            caps,
            state: Mutex::default(),
            work: Condvar::new(),
            done: Condvar::new(),
        });
        let writer = shared.clone();
        std::thread::Builder::new()
            .name("log-writer".into())
            .spawn(move || write_all(&writer, sink))?;
        Ok(Self(shared))
    }

    /// Hand over one whole line (or event): queued, or dropped and counted when the queue is full.
    fn push(&self, line: Vec<u8>) {
        let mut s = locked(&self.0.state);
        if s.queue.len() >= self.0.caps.lines || s.bytes + line.len() > self.0.caps.bytes {
            s.dropped += 1;
            drop(s);
            self.0.work.notify_one();
            return;
        }
        s.bytes += line.len();
        s.queue.push_back(line);
        drop(s);
        self.0.work.notify_one();
    }

    /// Wait up to `wait` for everything handed over so far, and the notice of what was dropped, to be written.
    /// Whether it was: false when the writer is stuck in a write that has not come back.
    pub fn flush(&self, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        let mut s = locked(&self.0.state);
        s.flushers += 1;
        let clear = loop {
            if s.queue.is_empty() && s.dropped == 0 && !s.busy {
                break true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break false;
            }
            s = self
                .0
                .done
                .wait_timeout(s, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        };
        s.flushers -= 1;
        clear
    }

    /// How many calls of `flush` are waiting now.
    #[cfg(test)]
    fn flushers(&self) -> usize {
        locked(&self.0.state).flushers
    }
}

/// The writer thread: lines in the order they came, then the notice of what was dropped once none is waiting.
fn write_all<W: Write>(shared: &Shared, mut sink: W) {
    loop {
        let job = {
            let mut s = locked(&shared.state);
            loop {
                if let Some(line) = s.queue.pop_front() {
                    s.bytes -= line.len();
                    s.busy = true;
                    break line;
                }
                if s.dropped > 0 {
                    let n = std::mem::take(&mut s.dropped);
                    s.busy = true;
                    break notice(n);
                }
                s = shared.work.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
        };
        // A sink that fails loses the line: the log is not the record.
        let _ = sink.write_all(&job).and_then(|()| sink.flush());
        locked(&shared.state).busy = false;
        shared.done.notify_all();
    }
}

fn notice(n: u64) -> Vec<u8> {
    let lines = if n == 1 { "line was" } else { "lines were" };
    format!(
        "Theseus log: {n} {lines} not written, because the place the log goes to was too slow\n"
    )
    .into_bytes()
}

/// One event's text, handed over whole when it is dropped.
pub struct Event {
    log: Log,
    buf: Vec<u8>,
}

impl Write for Event {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            self.log.push(std::mem::take(&mut self.buf));
        }
    }
}

impl<'a> MakeWriter<'a> for Log {
    type Writer = Event;

    fn make_writer(&'a self) -> Event {
        Event {
            log: self.clone(),
            buf: Vec::new(),
        }
    }
}

static GLOBAL: OnceLock<Log> = OnceLock::new();

/// Start the daemon's queue on standard error, and keep its handle for `flush`.
pub fn start_on_stderr() -> std::io::Result<Log> {
    #[cfg(debug_assertions)]
    let sink = Held(std::io::stderr());
    #[cfg(not(debug_assertions))]
    let sink = std::io::stderr();
    let log = Log::start(sink, Caps::default())?;
    let _ = GLOBAL.set(log.clone());
    Ok(log)
}

/// The daemon's queue, if it has one.
pub fn handle() -> Option<Log> {
    GLOBAL.get().cloned()
}

/// Flush the daemon's queue, if it has one: false when it was not all written within `wait`.
pub fn flush(wait: Duration) -> bool {
    GLOBAL.get().is_none_or(|l| l.flush(wait))
}

/// The panic hook: the queue first, then the hook it wraps (the crash file, the panic's own message).
pub fn flushing(
    before: Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static>,
    log: Log,
) -> Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static> {
    Box::new(move |info| {
        log.flush(PANIC_WAIT);
        before(info);
    })
}

/// A debug build's plant: `THESEUS_TEST_LOG_HOLD` names a FIFO a test makes when it wants the log's write held
/// (`theseus_store::hold_by_fifo`), as a stalled destination holds it.
#[cfg(debug_assertions)]
struct Held<W>(W);

#[cfg(debug_assertions)]
impl<W: Write> Write for Held<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        theseus_store::hold_by_fifo("THESEUS_TEST_LOG_HOLD");
        self.0.write(b)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests;
