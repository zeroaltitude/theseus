//! Every conversion runs in a child process (theseus-c9l6): `theseusd` in its
//! convert role ([`ROLE`]), with a wall-clock limit and a cap on its address
//! space ([`Limits`]) and its CPU time, no file it may write, and an empty
//! environment. A hostile file is real (a decompression
//! bomb, a deeply nested document): it costs the child, never the daemon. The
//! child reads its request and the file's bytes on stdin and answers on
//! stdout.
//!
//! A daemon names the child once, before serving ([`use_child`]), with the
//! spawn it registers its children through. Until then, and in tests, a
//! conversion runs in the calling thread, without the caps. Either way the
//! caller blocks until it ends: call it from the CPU pool or a blocking
//! section, never from a runtime worker.

use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::pdf;

/// The argument that makes `theseusd` the converter.
pub const ROLE: &str = "files-convert";

/// A conversion's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The longest a conversion may take before its child is killed.
    pub timeout: Duration,
    /// The child's address space.
    pub memory_bytes: u64,
}

impl Limits {
    /// A 32 MiB file, its parse, and what is made of it fit well inside a
    /// gibibyte and half a minute; a bomb does not.
    pub const DEFAULT: Limits = Limits {
        timeout: Duration::from_secs(30),
        memory_bytes: 1024 * 1024 * 1024,
    };
}

/// The most the child may answer: a part of a file, base64, and its text.
const MAX_ANSWER_BYTES: u64 = 3 * crate::MAX_FILE_BYTES + 2 * pdf::MAX_TEXT_BYTES as u64;

/// How the daemon starts a child: through its registry of children, so that
/// its reaper never takes this one's status from the waiter.
pub type Spawn = fn(&mut Command) -> std::io::Result<Child>;

struct ChildRunner {
    exe: PathBuf,
    spawn: Spawn,
    limits: Limits,
}

static CHILD: OnceLock<ChildRunner> = OnceLock::new();

/// Run every conversion from now on in a child: `exe` in [`ROLE`], started
/// by `spawn`, under `limits`. A daemon calls it once, before serving, with
/// [`Limits::DEFAULT`]; a second call changes nothing.
pub fn use_child(exe: PathBuf, spawn: Spawn, limits: Limits) {
    let _ = CHILD.set(ChildRunner { exe, spawn, limits });
}

/// Whether conversions run in a child (health, tests).
pub fn in_child() -> bool {
    CHILD.get().is_some()
}

/// One conversion's request: what to do with the bytes that follow it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Pdf { ask: pdf::Ask },
}

/// A conversion's answer: what it made, or why it made nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Pdf(pdf::Read),
    Refused(String),
}

/// How one conversion went, for its row and its metric.
#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    pub ms: u64,
    /// In a child, with the caps.
    pub capped: bool,
}

/// Read a PDF as `ask` says, under the caps. `Err` says why in words.
pub fn pdf(bytes: &[u8], ask: &pdf::Ask) -> (Result<pdf::Read, String>, Ran) {
    let t0 = Instant::now();
    let req = Request::Pdf { ask: ask.clone() };
    let (answer, capped) = match CHILD.get() {
        Some(c) => (in_a_child(c, &req, bytes), true),
        None => (Ok(answer(&req, bytes)), false),
    };
    let ran = Ran {
        ms: t0.elapsed().as_millis() as u64,
        capped,
    };
    let out = match answer {
        Ok(Answer::Pdf(r)) => Ok(r),
        Ok(Answer::Refused(why)) | Err(why) => Err(why),
    };
    (out, ran)
}

/// The conversion itself, wherever it runs.
fn answer(req: &Request, bytes: &[u8]) -> Answer {
    match req {
        Request::Pdf { ask } => match pdf::read(bytes, ask) {
            Ok(r) => Answer::Pdf(r),
            Err(why) => Answer::Refused(why),
        },
    }
}

/// The caps, set in the child between its fork and its exec: only calls
/// that are safe there (`setrlimit`, `prctl`).
fn caps(limits: Limits) -> std::io::Result<()> {
    let set = |what, n: u64| {
        let lim = libc::rlimit {
            rlim_cur: n,
            rlim_max: n,
        };
        // SAFETY: setrlimit reads the struct it is given and nothing else.
        if unsafe { libc::setrlimit(what, &lim) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    };
    set(libc::RLIMIT_AS, limits.memory_bytes)?;
    set(libc::RLIMIT_CPU, limits.timeout.as_secs() + 5)?;
    set(libc::RLIMIT_FSIZE, 0)?;
    set(libc::RLIMIT_CORE, 0)?;
    set(libc::RLIMIT_NOFILE, 16)?;
    // SAFETY: prctl with these arguments only sets this process's own
    // death signal: it dies when the daemon does.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0);
    }
    Ok(())
}

fn in_a_child(c: &ChildRunner, req: &Request, bytes: &[u8]) -> Result<Answer, String> {
    let mut cmd = Command::new(&c.exe);
    cmd.arg(ROLE)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if cfg!(debug_assertions) {
        if let Ok(p) = std::env::var(PROBE_ENV) {
            cmd.env(PROBE_ENV, p);
        }
    }
    let limits = c.limits;
    // SAFETY: `caps` makes only calls that are safe between fork and exec.
    unsafe {
        cmd.pre_exec(move || caps(limits));
    }
    let mut child =
        (c.spawn)(&mut cmd).map_err(|e| format!("the converter could not start ({e})"))?;
    let mut stdin = child.stdin.take().expect("piped");
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let mut head = serde_json::to_vec(req).expect("a request serializes");
    head.push(b'\n');
    let body = bytes.to_vec();
    // The writer and the readers each on a thread of their own: a child
    // that answers before it has read everything never deadlocks the pipe.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&head).and_then(|()| stdin.write_all(&body));
    });
    let errs = std::thread::spawn(move || {
        let mut e = Vec::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_end(&mut e);
        String::from_utf8_lossy(&e).into_owned()
    });
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let r = (&mut stdout).take(MAX_ANSWER_BYTES).read_to_end(&mut out);
        let _ = tx.send(r.map(|_| out));
    });
    let out = match rx.recv_timeout(limits.timeout) {
        Ok(r) => r,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            return Err(format!(
                "its conversion took longer than {} and was stopped",
                secs(limits.timeout)
            ));
        }
    };
    let status = child.wait().map_err(|e| e.to_string());
    let _ = writer.join();
    let errs = errs.join().unwrap_or_default();
    let status = status?;
    if status.success() {
        let out = out.map_err(|e| format!("the converter's answer could not be read ({e})"))?;
        return serde_json::from_slice(&out)
            .map_err(|e| format!("the converter's answer could not be read ({e})"));
    }
    Err(died(status, &errs, limits))
}

/// `30 s`, `1.5 s`.
fn secs(d: Duration) -> String {
    format!("{} s", d.as_secs_f64())
}

/// Why a child that did not answer died, in words.
fn died(status: std::process::ExitStatus, errs: &str, limits: Limits) -> String {
    use std::os::unix::process::ExitStatusExt as _;
    let mem = limits.memory_bytes / (1024 * 1024);
    if errs.contains("memory allocation") || errs.contains("capacity overflow") {
        return format!("its conversion ran out of its {mem} MiB of memory and was stopped");
    }
    match status.signal() {
        Some(libc::SIGXCPU) | Some(libc::SIGKILL) => format!(
            "its conversion ran out of its time ({}) and was stopped",
            secs(limits.timeout)
        ),
        Some(s) => format!("its conversion died (signal {s})"),
        None => {
            let line = errs.lines().last().unwrap_or("").trim();
            format!(
                "its conversion failed (exit {}{})",
                status.code().unwrap_or(-1),
                if line.is_empty() {
                    String::new()
                } else {
                    format!(": {}", line.chars().take(200).collect::<String>())
                }
            )
        }
    }
}

/// A debug build's probe of the caps: the child first allocates this many
/// bytes (`alloc:<n>`) or sleeps this long (`sleep:<ms>`). Release builds
/// neither pass nor read it.
pub const PROBE_ENV: &str = "THESEUS_FILES_PROBE";

/// The child's main: read the request and the bytes on stdin, answer on
/// stdout, and exit. One thread; it writes no file.
pub fn role_main() -> i32 {
    if cfg!(debug_assertions) {
        probe();
    }
    let mut input = Vec::new();
    let cap = crate::MAX_FILE_BYTES + 64 * 1024;
    if let Err(e) = std::io::stdin().take(cap + 1).read_to_end(&mut input) {
        eprintln!("theseusd {ROLE}: stdin: {e}");
        return 2;
    }
    let Some(nl) = input.iter().position(|b| *b == b'\n') else {
        eprintln!("theseusd {ROLE}: no request");
        return 2;
    };
    let req: Request = match serde_json::from_slice(&input[..nl]) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("theseusd {ROLE}: bad request: {e}");
            return 2;
        }
    };
    let bytes = &input[nl + 1..];
    let a = if bytes.len() as u64 > crate::MAX_FILE_BYTES {
        Answer::Refused(format!(
            "it is over the {} MiB a conversion reads",
            crate::MAX_FILE_BYTES / (1024 * 1024)
        ))
    } else {
        answer(&req, bytes)
    };
    let mut out = std::io::stdout().lock();
    let written = serde_json::to_writer(&mut out, &a)
        .map_err(std::io::Error::from)
        .and_then(|()| out.flush());
    match written {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("theseusd {ROLE}: stdout: {e}");
            3
        }
    }
}

fn probe() {
    let Ok(p) = std::env::var(PROBE_ENV) else {
        return;
    };
    if let Some(n) = p
        .strip_prefix("alloc:")
        .and_then(|n| n.parse::<usize>().ok())
    {
        let mut v: Vec<u8> = Vec::new();
        v.resize(n, 1);
        std::hint::black_box(&v);
    }
    if let Some(ms) = p.strip_prefix("sleep:").and_then(|n| n.parse::<u64>().ok()) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_the_calling_thread_a_conversion_answers_as_the_child_would() {
        let pdf = pdf::sample(&["Moorings are free after six."]);
        let (r, ran) = pdf_in_thread(&pdf);
        assert_eq!(r.unwrap().texts, vec!["Moorings are free after six."]);
        assert!(!ran.capped);
        let (bad, _) = pdf_in_thread(b"not one");
        assert_eq!(bad.unwrap_err(), "it is not a PDF (no %PDF- header)");
    }

    fn pdf_in_thread(bytes: &[u8]) -> (Result<pdf::Read, String>, Ran) {
        assert!(!in_child(), "no test in this crate names a child");
        pdf(
            bytes,
            &pdf::Ask {
                text: true,
                ..Default::default()
            },
        )
    }

    #[test]
    fn a_dead_childs_words_name_the_cap_it_met() {
        use std::os::unix::process::ExitStatusExt as _;
        let d = Limits::DEFAULT;
        let abort = std::process::ExitStatus::from_raw(libc::SIGABRT);
        assert_eq!(
            died(abort, "memory allocation of 2147483648 bytes failed\n", d),
            "its conversion ran out of its 1024 MiB of memory and was stopped"
        );
        let xcpu = std::process::ExitStatus::from_raw(libc::SIGXCPU);
        assert_eq!(
            died(xcpu, "", d),
            "its conversion ran out of its time (30 s) and was stopped"
        );
        let exit = std::process::ExitStatus::from_raw(2 << 8);
        assert_eq!(
            died(exit, "theseusd files-convert: no request\n", d),
            "its conversion failed (exit 2: theseusd files-convert: no request)"
        );
    }
}
