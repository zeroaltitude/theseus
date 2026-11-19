//! The queue's promises, counted and ordered, not timed: a gate stands in for a destination that has stopped (the
//! writer waits at it, no sleep), and a flush opens it by waiting, so a line reaches the sink only if the flush
//! was made.

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::mpsc;

use super::*;

/// Closed until opened; the writer waits at it, and `entered` says it has.
#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl Gate {
    fn open(&self) {
        self.state.lock().unwrap().0 = true;
        self.changed.notify_all();
    }

    fn pass(&self) {
        let mut s = self.state.lock().unwrap();
        s.1 = true;
        self.changed.notify_all();
        while !s.0 {
            s = self.changed.wait(s).unwrap();
        }
    }

    /// Wait until the writer has taken a line and is held at the gate.
    fn entered(&self) {
        let mut s = self.state.lock().unwrap();
        while !s.1 {
            s = self.changed.wait(s).unwrap();
        }
    }
}

#[derive(Clone)]
struct Sink {
    out: Arc<Mutex<Vec<u8>>>,
    gate: Arc<Gate>,
}

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.gate.pass();
        self.out.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn rig(caps: Caps) -> (Log, Sink) {
    let sink = Sink {
        out: Arc::default(),
        gate: Arc::default(),
    };
    (Log::start(sink.clone(), caps).unwrap(), sink)
}

impl Sink {
    fn text(&self) -> String {
        String::from_utf8(self.out.lock().unwrap().clone()).unwrap()
    }
}

/// The lines `n=<i>` the daemon's own format would write, through the subscriber the daemon installs.
fn say(log: &Log, range: std::ops::Range<u32>) {
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .finish();
    tracing::subscriber::with_default(sub, || {
        for i in range {
            tracing::info!(n = i, "line");
        }
    });
}

/// While the sink holds the writer, the threads that log go on: a hundred lines are handed over and the call
/// that hands them returns, which with a write straight to the sink it never would.
#[test]
fn a_destination_that_has_stopped_does_not_stop_the_thread_that_logs() {
    let (log, sink) = rig(Caps::default());
    say(&log, 0..1);
    sink.gate.entered();
    let (said, heard) = mpsc::channel();
    {
        let log = log.clone();
        std::thread::spawn(move || {
            say(&log, 1..101);
            let _ = said.send(());
        });
    }
    let returned = heard.recv_timeout(Duration::from_secs(20));
    sink.gate.open();
    assert!(returned.is_ok(), "the thread that logs waited for the sink");
    assert!(log.flush(Duration::from_secs(20)));
    assert_eq!(sink.text().lines().count(), 101);
}

/// Full, the queue drops a new line, and once the writer has caught up it says how many in one line, after the
/// lines that were kept.
#[test]
fn a_full_queue_drops_and_then_says_how_many() {
    let (log, sink) = rig(Caps {
        lines: 3,
        ..Caps::default()
    });
    say(&log, 0..1);
    sink.gate.entered();
    // Three wait behind the line the writer holds; five more find the queue full.
    say(&log, 1..9);
    sink.gate.open();
    assert!(log.flush(Duration::from_secs(20)));
    let text = sink.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5, "{text}");
    for (i, l) in lines[..4].iter().enumerate() {
        assert!(l.ends_with(&format!("line n={i}")), "{text}");
    }
    assert_eq!(
        lines[4],
        "Theseus log: 5 lines were not written, because the place the log goes to was too slow"
    );
}

/// The bytes waiting are bounded too: one line over the cap is dropped whole, never cut.
#[test]
fn a_line_that_would_pass_the_byte_cap_is_dropped_whole() {
    let (log, sink) = rig(Caps {
        bytes: 40,
        ..Caps::default()
    });
    say(&log, 0..1);
    sink.gate.entered();
    say(&log, 1..4);
    sink.gate.open();
    assert!(log.flush(Duration::from_secs(20)));
    let text = sink.text();
    assert!(text.lines().next().unwrap().ends_with("line n=0"), "{text}");
    // 15 bytes a line: two wait inside the 40 and the third would pass it.
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert_eq!(
        lines[3],
        "Theseus log: 1 line was not written, because the place the log goes to was too slow"
    );
}

/// No line is reordered or changed, through the subscriber's own format and a thousand lines.
#[test]
fn lines_come_out_in_the_order_they_went_in_and_unchanged() {
    let (log, sink) = rig(Caps::default());
    sink.gate.open();
    say(&log, 0..1000);
    assert!(log.flush(Duration::from_secs(20)));
    let text = sink.text();
    let want: Vec<String> = (0..1000).map(|i| format!(" INFO line n={i}")).collect();
    let got: Vec<&str> = text.lines().collect();
    assert_eq!(got, want);
}

/// An event that is written in pieces is handed over whole: another thread's event never lands between them.
#[test]
fn an_event_written_in_pieces_is_one_line() {
    let (log, sink) = rig(Caps::default());
    sink.gate.open();
    let mut a = log.make_writer();
    let mut b = log.make_writer();
    a.write_all(b"one ").unwrap();
    b.write_all(b"two\n").unwrap();
    a.write_all(b"three\n").unwrap();
    drop(b);
    drop(a);
    assert!(log.flush(Duration::from_secs(20)));
    assert_eq!(sink.text(), "two\none three\n");
}

/// Opens `gate` once a flush is waiting, so a line reaches the sink only through the flush: a stop that did not
/// flush leaves them where they are. `stop` ends it.
fn open_when_flushed(
    log: &Log,
    gate: &Arc<Gate>,
) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let (log, gate) = (log.clone(), gate.clone());
    let stop = Arc::new(AtomicBool::new(false));
    let end = stop.clone();
    let opener = std::thread::spawn(move || {
        while !end.load(SeqCst) {
            if log.flushers() > 0 {
                gate.open();
                return;
            }
            std::thread::yield_now();
        }
    });
    (stop, opener)
}

/// A clean stop flushes: everything handed over is written before it returns.
#[test]
fn a_clean_stop_flushes_the_queue() {
    let (log, sink) = rig(Caps::default());
    say(&log, 0..50);
    let (stop, opener) = open_when_flushed(&log, &sink.gate);
    assert!(log.flush(STOP_WAIT * 10), "the flush gave up");
    stop.store(true, SeqCst);
    opener.join().unwrap();
    assert_eq!(sink.text().lines().count(), 50);
}

/// A panic flushes: the hook writes the queue before the panic goes on.
#[test]
fn a_panic_flushes_the_queue_before_the_hook_it_wraps() {
    let (log, sink) = rig(Caps::default());
    say(&log, 0..50);
    let (stop, opener) = open_when_flushed(&log, &sink.gate);
    let seen = Arc::new(Mutex::new(None));
    let wrapped = {
        let seen = seen.clone();
        Box::new(move |_: &std::panic::PanicHookInfo<'_>| {
            *seen.lock().unwrap() = Some(sink.text().lines().count());
        })
    };
    let before = std::panic::take_hook();
    std::panic::set_hook(flushing(wrapped, log));
    let _ = std::panic::catch_unwind(|| panic!("a test's panic"));
    std::panic::set_hook(before);
    stop.store(true, SeqCst);
    opener.join().unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        Some(50),
        "the hook it wraps ran before the queue was written"
    );
}

/// A writer stuck in its write is waited for only so long: the flush says it gave up.
#[test]
fn a_flush_gives_up_when_the_writer_is_stuck() {
    let (log, sink) = rig(Caps::default());
    say(&log, 0..3);
    sink.gate.entered();
    assert!(!log.flush(Duration::from_millis(50)));
    sink.gate.open();
    assert!(log.flush(Duration::from_secs(20)));
}
