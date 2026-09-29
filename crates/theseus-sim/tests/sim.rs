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
/// committed is still there, byte for byte. Tearing stays off until its
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
