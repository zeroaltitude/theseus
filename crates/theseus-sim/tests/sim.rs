//! The simulator in the gate (theseus-hco): the M1 crash test and the M2
//! kernel simulation, each on small fixed seeds so the two take seconds. Long
//! runs stay manual: `theseus-sim crash-test --iterations 20`, `theseus-sim
//! kernel-sim --seeds 40`.

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

/// The TOTAL line's count before `what`.
fn count(out: &str, what: &str) -> u64 {
    let total = out.lines().find(|l| l.starts_with("TOTAL")).unwrap_or("");
    total
        .split(what)
        .next()
        .and_then(|s| s.rsplit(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// The kernel under seeded crashes and lost, duplicate, and late completions,
/// opening executions the way the product does (`open_execution`): every
/// invariant holds after every step. Some calls wait for the operator, and
/// cancels end some of them unsent (theseus-w98).
///
/// The coverage counts are read from a run with no second thread
/// (`--p-race 0`), which reproduces from its seeds throughout, so each count
/// is the same on every run (theseus-81ig): a raced run reproduces only up to
/// its first race, and whether it takes a repeating wake, say, varies.
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
        "--p-race",
        "0",
    ]);
    assert!(out.contains("all invariants held"), "{out}");
    let total = out.lines().find(|l| l.starts_with("TOTAL")).unwrap_or("");
    assert_eq!(count(&out, " raced turns"), 0, "{total}");
    assert!(
        count(&out, " calls asked the operator") > 0,
        "no call asked: {total}"
    );
    assert!(
        count(&out, " unsent actions a cancel ended") > 0,
        "no cancel ended an unsent action: {total}"
    );
    // Wakes, one-shot and repeating (37a): series put back at their next
    // occurrence, and occurrences a crash passed over.
    assert!(count(&out, " repeating;") > 0, "no series was set: {total}");
    assert!(
        count(&out, " series put back") > 0,
        "no series was put back: {total}"
    );
    // sim2 (theseus-celu.35): `/stop`, between turns and in one, and of one
    // call alone; tasks under a parent and their reports, a report's wake
    // among them; a wake that fell due in a turn, queued by its end; and the
    // outbox's posts, staged in a turn's end and planned outside one, sent
    // again under their key after a crash, and settled twice.
    for what in [
        " stops:",
        " while a turn ran,",
        " next inputs ran a turn;",
        " calls stopped alone",
        " tasks opened:",
        " refused at depth one;",
        " reports read:",
        " woke their parent,",
        " wakes due in a turn queued by its end",
        " posts staged in a turn's end",
        " planned outside one;",
        " sent again,",
        " second settles,",
        " crashes around a post",
    ] {
        assert!(count(&out, what) > 0, "none{what}: {total}");
    }
}

/// The same seeds with a share of the turns raced by a second OS thread on
/// the same execution (theseus-id9): every invariant holds after every step,
/// however the threads interleave. Only what the seeds fix up to the first
/// race is counted: that turns were raced, and that the racing thread ran a
/// kernel transaction (theseus-0owd), which a run's first race always does.
#[test]
fn the_kernel_holds_its_invariants_with_raced_turns() {
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
    let total = out.lines().find(|l| l.starts_with("TOTAL")).unwrap_or("");
    assert!(
        count(&out, " raced turns") > 0,
        "no turn was raced: {total}"
    );
    assert!(
        count(&out, " of them transactions") > 0,
        "no racing thread ran a transaction: {total}"
    );
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

/// The store the restore row copies, with the cancel row selected or not
/// (theseus-ma8r): the cancel row's runs each start a job through a turn, so
/// it writes into the rig, and it runs after the restore row. Run first, as it
/// did from d279767f to this commit, it grew the restore's store from a few
/// KB of WAL to over 100 KB, and its row moved for a reason that is not the
/// restore's code. Each run is a real `theseusd`, the one beside `theseus-sim`.
#[test]
fn the_restore_rows_store_is_the_same_with_the_cancel_row_selected() {
    let sim_bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_theseus-sim"));
    let theseusd = sim_bin.with_file_name("theseusd");
    assert!(
        theseusd.exists(),
        "{} is not built: build the workspace first",
        theseusd.display()
    );
    let restore_row = |phases: &str| {
        let tmp = tempfile::tempdir().unwrap();
        let json = tmp.path().join("report.json");
        sim(&[
            "bench",
            "lifecycle",
            "--phases",
            phases,
            "--runs",
            "1",
            "--json",
            &json.to_string_lossy(),
        ]);
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
        let row = report["restore"].clone();
        (
            row["segments"].as_u64().unwrap(),
            row["wal_bytes"].as_u64().unwrap(),
            row["sessions_restored"].as_u64().unwrap(),
        )
    };
    let alone = restore_row("restore");
    let with_cancel = restore_row("restore,cancel");
    assert_eq!(alone.0, with_cancel.0, "segments");
    // The bench's binding binds its DM on the fake Discord once the fake op
    // answers, so a run's store may hold that DM's session, or an orphan one
    // a bind left when its start was stopped, by the machine's load: a
    // session and a few KB either way. The old order put the cancel row's
    // sessions in, twenty times the WAL (3 sessions and 135,855 B against 0
    // and 6,243).
    assert!(
        alone.2.abs_diff(with_cancel.2) <= 1,
        "the restore row restores {} sessions alone and {} with the cancel row selected",
        alone.2,
        with_cancel.2
    );
    let (a, w) = (alone.1, with_cancel.1);
    assert!(
        w < 2 * a && a < 2 * w,
        "the restore row's WAL is {a} bytes alone and {w} with the cancel row selected"
    );
}
