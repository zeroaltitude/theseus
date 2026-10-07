//! A client of a `--stdio` daemon: its pipes, one request at a time, each
//! answer awaited with a bound, since a request the daemon took as it
//! stopped is never answered. A thread reads the daemon's stdout into a
//! channel, line by line; `hold` keeps it from reading, so the daemon's
//! answers back up in the pipe, until `release`.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::Daemon;

/// How long `call` waits for its answer.
const ANSWER: Duration = Duration::from_secs(60);

pub struct StdioClient {
    stdin: std::process::ChildStdin,
    lines: mpsc::Receiver<std::io::Result<String>>,
    /// Whether the reader may read: `hold` and `release`.
    gate: Arc<(Mutex<bool>, Condvar)>,
    /// The daemon's stdout, the pipe's read end, for `pending`.
    stdout: std::os::fd::RawFd,
    next: u64,
}

impl StdioClient {
    /// The client of `d`, spawned with stdin and stdout piped.
    pub fn new(d: &mut Daemon) -> Self {
        let (stdin, stdout) = d.stdio();
        let fd = std::os::fd::AsRawFd::as_raw_fd(&stdout);
        let (tx, lines) = mpsc::channel();
        let gate = Arc::new((Mutex::new(true), Condvar::new()));
        let open = gate.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                {
                    let (may, cv) = &*open;
                    let mut may = may.lock().unwrap();
                    while !*may {
                        may = cv.wait(may).unwrap();
                    }
                }
                let mut line = String::new();
                match r.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        Self {
            stdin,
            lines,
            gate,
            stdout: fd,
            next: 0,
        }
    }

    /// Send a request without waiting for its answer: its id.
    pub fn send(&mut self, method: &str, params: Value) -> u64 {
        self.next += 1;
        let req = json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params});
        writeln!(self.stdin, "{req}").expect("writing to the daemon's stdin");
        self.next
    }

    /// The next line the daemon wrote, within `bound`: `None` when none
    /// came, an error when its stdout ended. The line as written, its
    /// newline included: one cut short has none.
    pub fn line(&self, bound: Duration) -> Option<Result<String, String>> {
        match self.lines.recv_timeout(bound) {
            Ok(Ok(l)) if l.is_empty() => Some(Err("the daemon's stdout closed".into())),
            Ok(Ok(l)) => Some(Ok(l)),
            Ok(Err(e)) => Some(Err(e.to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Some(Err("the daemon's stdout closed".into()))
            }
        }
    }

    /// The answer to request `id`, within `bound`; every line before it
    /// must parse.
    pub fn answer(&self, id: u64, bound: Duration) -> Result<Value, Value> {
        let t0 = Instant::now();
        loop {
            let left = bound.saturating_sub(t0.elapsed());
            let l = match self.line(left) {
                None => return Err(json!(format!("no answer to {id} in {bound:?}"))),
                Some(l) => l.map_err(|e| json!(e))?,
            };
            let v: Value = serde_json::from_str(&l).map_err(|e| json!(format!("{e}: {l:?}")))?;
            if v["id"] == id {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.clone()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
    }

    /// One request and its answer.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Value> {
        let id = self.send(method, params);
        self.answer(id, ANSWER)
    }

    /// Stop reading the daemon's stdout (after the line being read, if
    /// any): its answers back up in the pipe.
    pub fn hold(&self) {
        *self.gate.0.lock().unwrap() = false;
    }

    /// The bytes waiting in the daemon's stdout pipe, unread (`FIONREAD`).
    pub fn pending(&self) -> usize {
        let mut n: libc::c_int = 0;
        // SAFETY: the reader thread holds the pipe open, and `n` is an int.
        let r = unsafe { libc::ioctl(self.stdout, libc::FIONREAD, &mut n) };
        if r == 0 {
            n as usize
        } else {
            0
        }
    }

    /// Read again.
    pub fn release(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}
