//! `theseus-index serve --parent <pid>` (roadmap row 51), the real binary on
//! an empty store: a tender exits when the process that started it does, so
//! none outlives its daemon holding the index's lock, and one whose parent is
//! not that process exits at once. (An integration test also makes cargo
//! build the binary, which theseusd's tender tests run beside `theseusd`.)

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_theseus-index");

/// The process's state letter, `None` once it is gone.
fn state(pid: u32) -> Option<char> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s[s.rfind(')')? + 2..].chars().next()
}

fn ended(pid: u32) -> bool {
    matches!(state(pid), None | Some('Z') | Some('X'))
}

/// A parent shell starts a tender with `--parent $$` and waits on its stdin;
/// the tender serves until the shell exits, then exits within a second.
#[test]
fn a_tender_exits_with_the_process_that_started_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    std::fs::create_dir_all(&store).unwrap();
    let index = dir.path().join("index");
    let script = format!(
        "'{BIN}' serve --store '{}' --index '{}' --no-vectors --parent $$ 2>'{}' & echo $!; read x; exit 0",
        store.display(),
        index.display(),
        dir.path().join("tender.log").display()
    );
    let mut sh = Command::new("sh")
        .args(["-c", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(sh.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let tender: u32 = line.trim().parse().unwrap();
    let log = || std::fs::read_to_string(dir.path().join("tender.log")).unwrap_or_default();
    // It serves: its socket appears.
    let t0 = Instant::now();
    while !index.join("sock").exists() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "no socket in 20 s:\n{}",
            log()
        );
        assert!(!ended(tender), "the tender ended early:\n{}", log());
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    assert!(!ended(tender), "the tender serves while its parent runs");
    // The parent exits.
    drop(sh.stdin.take());
    assert!(sh.wait().unwrap().success());
    let gone = Instant::now();
    while !ended(tender) {
        if gone.elapsed() > Duration::from_secs(5) {
            // SAFETY: plain integers; the pid is the tender this test started.
            unsafe { libc::kill(tender as libc::pid_t, libc::SIGKILL) };
            panic!("the tender outlived its parent by 5 s; killed:\n{}", log());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        log().contains("the daemon that started this tender exited"),
        "{}",
        log()
    );
}

/// `--parent` naming a process that is not its parent: it exits at once,
/// with 0, before it takes the index.
#[test]
fn a_tender_whose_parent_is_another_process_exits_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    std::fs::create_dir_all(&store).unwrap();
    let index = dir.path().join("index");
    let out = Command::new(BIN)
        .args(["serve", "--no-vectors", "--parent", "1", "--store"])
        .arg(&store)
        .arg("--index")
        .arg(&index)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("the daemon that started this tender is gone"),
        "{err}"
    );
    assert!(!index.join("LOCK").exists(), "it never took the index");
}
