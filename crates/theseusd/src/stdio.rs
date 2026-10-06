//! A `--stdio` daemon's pipes, relayed by two threads of their own
//! (theseus-xbtr).
//!
//! tokio's stdin reads on its blocking pool, and only the client's end of the
//! pipe ends that read, so a stop by a signal while the client held its end
//! could not drop the runtime, which waits for every blocking task: it shut
//! the runtime down with a 500 ms bound instead, and left behind whatever
//! still ran on the pool. A pass there that held the core past the bound (a
//! warm build, on a loaded machine) kept the store open as the process ended:
//! redb never closed, the stop's last checkpoint (made durable only by that
//! close) was lost, and the next start replayed the whole run.
//!
//! Here the pipes are copied by two plain threads to and from one end of a
//! socket pair, and the core serves the other end, an ordinary async stream.
//! The runtime then ends as the socket daemon's does, waiting for its tasks,
//! and the store closes before the process ends. The thread on stdin is left
//! blocked in its read, holding nothing of the core; the one on stdout copies
//! what the core wrote, the stop's answer included, before the process ends
//! (`flush`).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The stdout thread's end: it sends once it has written everything.
static WRITTEN: OnceLock<Mutex<mpsc::Receiver<()>>> = OnceLock::new();

/// The core's end of the pipes, as an async stream on this runtime.
pub fn pipes() -> std::io::Result<tokio::net::UnixStream> {
    let (ours, theirs) = UnixStream::pair()?;
    let to_core = theirs.try_clone()?;
    std::thread::Builder::new()
        .name("stdio-in".into())
        .spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let _ = std::io::copy(&mut stdin, &mut &to_core);
            // The client's end closed: the core reads the end of its input.
            let _ = to_core.shutdown(std::net::Shutdown::Write);
        })?;
    let (done, written) = mpsc::channel();
    std::thread::Builder::new()
        .name("stdio-out".into())
        .spawn(move || {
            let mut stdout = std::io::stdout();
            let mut buf = vec![0u8; 64 * 1024];
            // Until the core's end closes: each read written whole, at once.
            while let Ok(n) = (&theirs).read(&mut buf) {
                if n == 0 || stdout.write_all(&buf[..n]).is_err() || stdout.flush().is_err() {
                    break;
                }
            }
            // A client gone from stdout: the core's next write fails, as a
            // write to the closed pipe itself did.
            let _ = theirs.shutdown(std::net::Shutdown::Read);
            let _ = done.send(());
        })?;
    let _ = WRITTEN.set(Mutex::new(written));
    ours.set_nonblocking(true)?;
    tokio::net::UnixStream::from_std(ours)
}

/// Waits, at most `bound`, until the stdout thread has written everything
/// the core wrote: called once the runtime has dropped, and with it the
/// core's end of the pair.
pub fn flush(bound: Duration) {
    if let Some(w) = WRITTEN.get() {
        let w = w.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = w.recv_timeout(bound);
    }
}

/// A test's plant (a debug build's `THESEUS_TEST_HOLD_CORE_MS`): a task on
/// the runtime's blocking pool that holds `core` until the stop has begun,
/// then that many ms more, as a slow warm build can on a loaded machine. The
/// stop must wait for it, and the store close, before the process ends. A
/// release build has no such plant.
pub fn planted_hold<T: Send + Sync + 'static>(core: &std::sync::Arc<T>) {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("THESEUS_TEST_HOLD_CORE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        let core = core.clone();
        tokio::task::spawn_blocking(move || {
            while !theseus_core::startup::stop_has_begun() {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(ms));
            drop(core);
        });
    }
    #[cfg(not(debug_assertions))]
    let _ = core;
}
