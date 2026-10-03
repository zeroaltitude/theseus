//! The simulator in the gate (theseus-hco): the M1 crash test, the M2 kernel
//! simulation, and the M4 disclosure simulator (19b), each on small fixed
//! seeds so the three take seconds. Long runs stay manual: `theseus-sim
//! crash-test --iterations 20`, `theseus-sim kernel-sim --seeds 40`,
//! `theseus-sim disclosure --seeds 40 --steps 2000`.

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
/// store's very first open (theseus-0b8), and most kills are followed by a
/// tear of the WAL's tail past what was reported durable (theseus-4x6).
#[test]
fn a_killed_store_keeps_every_committed_record() {
    let out = sim(&[
        "crash-test",
        "--iterations",
        "3",
        "--seed",
        "7",
        "--tear",
        "true",
    ]);
    assert!(out.contains("CRASH TEST OK"), "{out}");
    assert!(out.contains("tear true"), "{out}");
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
/// invariant holds after every step. Some calls wait for the operator, and
/// cancels end some of them unsent (theseus-w98).
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
    // The TOTAL line's count before `what`.
    let total = out.lines().find(|l| l.starts_with("TOTAL")).unwrap_or("");
    let count = |what: &str| -> u64 {
        total
            .split(what)
            .next()
            .and_then(|s| s.rsplit(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    assert!(
        count(" calls asked the operator") > 0,
        "no call asked: {total}"
    );
    assert!(
        count(" unsent actions a cancel ended") > 0,
        "no cancel ended an unsent action: {total}"
    );
    // Kernel transactions on the racing thread (theseus-0owd). These two
    // seeds answer no question; longer runs count the answers, each one
    // frame with its wake (theseus-jj9f).
    assert!(
        count(" of them transactions") > 0,
        "no racing thread ran a transaction: {total}"
    );
}

/// The disclosure simulator (M4 19b) on fixed seeds: a synthetic world of
/// people, channels whose viewers change, sessions, tasks, graduations, and
/// held posts, driven through the core, with every disclosure invariant
/// checked at every compile, streamed edit, and post. These four seeds of 40
/// steps take about 2 s, and each of the lane's planted bugs fails in them (a
/// filter that skips attachments, a held post released before the owner
/// answers, a loop's readers taken from the prefix alone). The live check's 40
/// seeds of 2,000 steps stay manual.
#[test]
fn the_disclosure_invariants_hold_on_fixed_seeds() {
    let out = sim(&["disclosure", "--seed", "3", "--seeds", "4", "--steps", "40"]);
    assert!(out.contains("DISCLOSURE OK"), "{out}");
    // The seeds reach what the invariants are about, and what the planted
    // bugs need: something withheld, a withheld message's files, a post held,
    // a node graduated, a loop kept quiet.
    let total = out
        .lines()
        .find(|l| l.starts_with("DISCLOSURE OK"))
        .unwrap_or("");
    for what in [
        " withheld nodes",
        " withheld files",
        " held posts",
        " graduated",
        " quiet loops",
    ] {
        let n: u64 = total
            .split(what)
            .next()
            .and_then(|s| s.rsplit(['(', ' ']).next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        assert!(n > 0, "no{what} in the gate's seeds: {total}");
    }
}

/// `bench history` reads the file `$THESEUS_BENCH_HISTORY` names: none yet is
/// not an error, and a torn last line is skipped, and said so (theseus-1hk).
#[test]
fn bench_history_reads_the_file_the_environment_names() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("bench-history.csv");
    let history = || {
        let out = Command::new(env!("CARGO_BIN_EXE_theseus-sim"))
            .args(["bench", "history", "--last", "3"])
            .env("THESEUS_BENCH_HISTORY", &path)
            .output()
            .expect("running theseus-sim");
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let out = history();
    assert!(out.contains("no history yet"), "{out}");
    std::fs::write(
        &path,
        "time,label,load1,cold_p50,cold_p95,cold_limit,passed\n\
         2026-10-01T10:20:11-07:00,lane/x 1a2b3c4,2.5,23.8,51.3,57,true\n\
         2026-10-01T10:24:00-07:00,lane/x 1a2b3c4-dirty,2.5,23",
    )
    .unwrap();
    let out = history();
    assert!(out.contains("1 run(s), 0 missed"), "{out}");
    assert!(
        out.contains("skipped line 3: the last line is torn"),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.contains("lane/x 1a2b3c4 ") && l.ends_with("5.7  within 10%")),
        "{out}"
    );
}
