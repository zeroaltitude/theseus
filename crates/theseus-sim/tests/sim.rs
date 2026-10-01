//! The simulator in the gate (theseus-hco): the M1 crash test and the M2
//! kernel simulation, each on small fixed seeds so the pair takes seconds.
//! Long runs stay manual: `theseus-sim crash-test --iterations 20`,
//! `theseus-sim kernel-sim --seeds 40`.

use std::process::Command;

fn sim(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_theseus-sim"))
        .args(args)
        .output()
        .expect("running theseus-sim");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "theseus-sim {} failed: {}\n{stdout}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// SIGKILL a writer at random moments and reopen: every record reported
/// committed is still there, byte for byte. The kill may land inside the
/// store's very first open (theseus-0b8). Tearing stays off until its
/// durable bound is finished (see the `--tear` flag).
#[test]
fn a_killed_store_keeps_every_committed_record() {
    let out = sim(&[
        "crash-test",
        "--iterations",
        "3",
        "--seed",
        "7",
        "--tear",
        "false",
    ]);
    assert!(out.contains("CRASH TEST OK"), "{out}");
}

/// A store another process holds is refused with the message it always
/// had, and nothing is moved aside (theseus-0b8): the held index is the
/// holder's, never a file to replace. The holder is a real second process,
/// the crash test's worker, which keeps the store open as it appends.
#[test]
fn a_store_another_process_holds_is_refused_and_nothing_is_moved() {
    use std::io::{BufRead, BufReader};
    use theseus_store::Store as _;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("store");
    let mut worker = Command::new(env!("CARGO_BIN_EXE_theseus-sim"))
        .args(["worker", "--dir", &dir.to_string_lossy(), "--seed", "5"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawning the worker");
    let mut lines = BufReader::new(worker.stdout.take().unwrap()).lines();
    let first = lines.next().expect("the worker's first line").unwrap();
    assert!(first.starts_with("R "), "the store is open: {first}");
    // Its output keeps flowing; drain it so the worker never blocks.
    let drain = std::thread::spawn(move || lines.count());
    let e = theseus_store::WalStore::open_waiting(
        &dir,
        theseus_store::WalConfig::default(),
        std::time::Duration::from_millis(200),
    )
    .err()
    .expect("the worker holds the store");
    let msg = format!("{e:#}");
    let _ = worker.kill();
    let _ = worker.wait();
    drain.join().unwrap();
    assert!(msg.contains("is another theseusd serving it?"), "{msg}");
    let aside: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".bad-"))
        .collect();
    assert!(aside.is_empty(), "{aside:?}");
    // Its holder gone, the store opens, and nothing was moved.
    let s = theseus_store::WalStore::open(&dir, theseus_store::WalConfig::default()).unwrap();
    assert!(s.stats().unwrap().index_moved_aside.is_none());
}

/// The kernel under seeded crashes and lost, duplicate, and late completions,
/// opening executions the way the product does (`open_execution`), with a
/// share of its turns raced by a second OS thread (theseus-id9): every
/// invariant holds after every step.
#[test]
fn the_kernel_holds_its_invariants_under_seeded_faults() {
    let out = sim(&[
        "kernel-sim",
        "--seed",
        "1",
        "--seeds",
        "2",
        "--steps",
        "300",
    ]);
    assert!(out.contains("all invariants held"), "{out}");
    let raced: u64 = out
        .split(" raced turns")
        .next()
        .and_then(|s| s.rsplit(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(raced > 0, "no turn was raced: {out}");
}
