//! A cancel verified per backend, at L0 (M4 18a; theseus-hcc), in real
//! processes. A job's wrapper, run from this binary as the daemon runs
//! `theseusd job-wrapper`, stops its whole tree at a cancel and at its
//! deadline, a `setsid` descendant included, and says how it knows. A wrapper
//! from before 18a is stopped by its process group, as before, and one that
//! never answers is killed with its group, the cancel uncertain. Each case
//! ends with a scan of `/proc` for its own marker, so a process the stop
//! missed fails it.
//!
//! A test binary of its own (`harness = false`): it re-execs itself as the
//! wrapper, and as the stand-ins for an older wrapper and a deaf one.

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::{Duration, Instant};

use theseus_kernel::job::{self, WrapperArgs};
use theseus_kernel::{children, Outcome, Spool, VerifiedBy};

/// The role a re-exec of this binary as a wrapper plays: absent, the real
/// wrapper; `old`, one from before 18a, which dies at the first SIGTERM;
/// `deaf`, one that catches SIGTERM and never answers.
const ROLE: &str = "THESEUS_TREE_TEST_ROLE";

struct Case {
    name: &'static str,
    run: fn() -> Result<(), String>,
}

const CASES: &[Case] = &[
    Case {
        name: "a_cancel_stops_every_process_of_the_tree_a_setsid_one_too",
        run: cancel_stops_the_tree,
    },
    Case {
        name: "the_deadline_stops_the_whole_tree_too",
        run: deadline_stops_the_tree,
    },
    Case {
        name: "an_older_wrapper_is_stopped_by_its_process_group_as_before",
        run: older_wrapper_by_group,
    },
    Case {
        name: "a_wrapper_that_never_answers_is_killed_with_its_group_and_uncertain",
        run: deaf_wrapper_uncertain,
    },
    Case {
        name: "a_plain_sigterm_stops_the_tree_and_the_wrapper_still_ends_by_it",
        run: plain_sigterm,
    },
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(job::WRAPPER_MODE) {
        let wa =
            job::parse_wrapper_args(args[1..].iter().cloned()).expect("the wrapper's arguments");
        match std::env::var(ROLE).as_deref() {
            Ok("old") => stand_in(&wa, false),
            Ok("deaf") => stand_in(&wa, true),
            _ => {
                job::run_wrapper_process(&wa).expect("the wrapper");
                std::process::exit(0)
            }
        }
    }
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let filters: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with('-') && *a != "terse")
        .map(String::as_str)
        .collect();
    let chosen: Vec<&Case> = CASES
        .iter()
        .filter(|c| {
            filters.is_empty()
                || filters.iter().any(|f| {
                    if has("--exact") {
                        c.name == *f
                    } else {
                        c.name.contains(f)
                    }
                })
        })
        .collect();
    if has("--list") {
        if !has("--ignored") {
            for c in &chosen {
                println!("{}: test", c.name);
            }
        }
        std::process::exit(0);
    }
    let mut failed = 0;
    for c in &chosen {
        let t = Instant::now();
        match (c.run)() {
            Ok(()) => println!("test {} ... ok ({} ms)", c.name, t.elapsed().as_millis()),
            Err(e) => {
                failed += 1;
                println!("test {} ... FAILED\n{e}", c.name);
            }
        }
    }
    println!(
        "test result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        chosen.len() - failed
    );
    std::process::exit(if failed == 0 { 0 } else { 101 })
}

/// A wrapper from before 18a, or a deaf one: a subreaper that runs the
/// command in its own process group and waits for every child. The older
/// one dies at the first SIGTERM; the deaf one catches it and does nothing.
fn stand_in(wa: &WrapperArgs, deaf: bool) -> ! {
    extern "C" fn ignore(_: libc::c_int) {}
    // SAFETY: a subreaper, and for the deaf one a handler that does nothing.
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        if deaf {
            libc::signal(libc::SIGTERM, ignore as *const () as libc::sighandler_t);
        }
    }
    let mut cmd = std::process::Command::new(&wa.argv[0]);
    cmd.args(&wa.argv[1..]);
    if let Some(c) = &wa.cwd {
        cmd.current_dir(c);
    }
    // Reaped below, with every orphan, by `waitpid(-1)`, as a wrapper reaps.
    #[expect(clippy::zombie_processes, reason = "the loop below reaps it")]
    let _child = cmd.spawn().expect("the command");
    loop {
        let mut status = 0;
        if unsafe { libc::waitpid(-1, &mut status, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR)
        {
            std::process::exit(0);
        }
    }
}

/// A job's directory, its spool, and the wrappers it started.
struct Rig {
    dir: tempfile::TempDir,
    spool: Spool,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let spool = Spool::open(&dir.path().join("spool")).expect("the spool");
        Self { dir, spool }
    }

    /// Start `sh -c script` under a wrapper of `role`, as the daemon starts a
    /// job: detached, its own session, its pid in the spool.
    fn start(&self, id: &str, script: &str, deadline_ms: u64, role: Option<&str>) -> u32 {
        let mut env = vec![(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )];
        env.extend(role.map(|r| (ROLE.to_string(), r.to_string())));
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: id.into(),
            deadline_ms,
            notify_socket: None,
            argv: vec!["sh".into(), "-c".into(), script.into()],
            cwd: Some(self.dir.path().to_path_buf()),
            env,
            umask: None,
            redact: vec![],
            output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
            sandbox: None,
        };
        let me = std::env::current_exe().expect("this binary");
        job::spawn_detached(&me, &[job::WRAPPER_MODE], &self.spool, &args).expect("the wrapper")
    }

    /// The pid the job wrote to `file`, once it runs `sleep`.
    fn sleeper(&self, file: &str) -> Result<u32, String> {
        let path = self.dir.path().join(file);
        wait_for(&format!("{file}'s sleep"), || {
            let pid: u32 = std::fs::read_to_string(&path)
                .ok()
                .filter(|s| s.ends_with('\n'))?
                .trim()
                .parse()
                .ok()?;
            let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
            (comm.trim() == "sleep").then_some(pid)
        })
    }
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if t0.elapsed() > Duration::from_secs(20) {
            return Err(format!("timed out waiting for {what}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Every live process whose command line holds `marker` as an argument. A
/// zombie's command line is empty, so it is not among them.
fn with_marker(marker: &str) -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|c| c.split(|&b| b == 0).any(|a| a == marker.as_bytes()))
        })
        .collect()
}

/// The scan: no process of the job is left. What it finds is killed, so a
/// failure leaves nothing behind.
fn none_left(marker: &str) -> Result<(), String> {
    let left = with_marker(marker);
    for pid in &left {
        unsafe { libc::kill(*pid as i32, libc::SIGKILL) };
    }
    if left.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "a /proc scan found {marker} still running: {left:?}"
        ))
    }
}

/// The wrapper's end, once this process (its parent) has reaped it.
fn reaped(wrapper: u32) -> Result<std::process::ExitStatus, String> {
    wait_for("the wrapper's reap", || {
        children::sweep()
            .wrappers
            .into_iter()
            .find(|(pid, _, _)| *pid == wrapper)
            .map(|(_, _, status)| status)
    })
}

fn check(ok: bool, what: impl FnOnce() -> String) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(what())
    }
}

/// theseus-hcc's first gap: a cancel signalled the wrapper's group, and a
/// `setsid` descendant, in another group, ran on while the cancel read as
/// verified. Now the wrapper stops its whole tree: the command and the
/// sleeper in its own session are both gone, the verdict says so (by the
/// tree, two processes, none left), the wrapper exits cleanly, and a
/// cancelled job writes no completion.
fn cancel_stops_the_tree() -> Result<(), String> {
    let (rig, marker) = (Rig::new(), "300.1801");
    let wrapper = rig.start(
        "act_tree",
        &format!(
            "setsid sleep {marker} > /dev/null 2>&1 < /dev/null & echo $! > own-session.pid; \
             echo $$ > main.pid; exec sleep {marker}"
        ),
        60_000,
        None,
    );
    let (own, main) = (rig.sleeper("own-session.pid")?, rig.sleeper("main.pid")?);
    check(job::catches_sigterm(wrapper), || {
        "the wrapper catches SIGTERM".into()
    })?;
    let t0 = Instant::now();
    let v = job::terminate(&rig.spool, wrapper, "act_tree", Duration::from_millis(500));
    let took = t0.elapsed();
    none_left(marker)?;
    check(v.verified(), || format!("verified: {v:?}"))?;
    check(v.verified_by == VerifiedBy::Tree, || {
        format!("by the tree: {v:?}")
    })?;
    check(v.killed == Some(2) && v.survivors == Some(0), || {
        format!("2 killed, none left: {v:?}")
    })?;
    check(v.scope.as_deref() == Some("descendants"), || {
        format!("{v:?}")
    })?;
    check(v.words() == "verified: process tree, 2 processes", || {
        v.words()
    })?;
    check(took < Duration::from_secs(3), || format!("{took:?}"))?;
    let status = reaped(wrapper)?;
    check(status.success(), || {
        format!("the wrapper exited cleanly: {status}")
    })?;
    check(
        rig.spool
            .read_completion("act_tree")
            .ok()
            .flatten()
            .is_none(),
        || "a cancelled job writes no completion".into(),
    )?;
    check(rig.spool.read_stop("act_tree").is_none(), || {
        "the stop read its verdict".into()
    })?;
    check(
        !Path::new(&format!("/proc/{own}")).exists()
            && !Path::new(&format!("/proc/{main}")).exists(),
        || "both sleepers reaped".into(),
    )
}

/// theseus-hcc's second gap: the deadline killed the wrapper's direct child
/// alone, and the wrapper lingered on the rest. Now the deadline uses the
/// cancel's stop: the whole tree goes, and the completion says how.
fn deadline_stops_the_tree() -> Result<(), String> {
    let (rig, marker) = (Rig::new(), "300.1802");
    let wrapper = rig.start(
        "act_deadline",
        &format!(
            "setsid sleep {marker} > /dev/null 2>&1 < /dev/null & echo $! > own-session.pid; \
             echo $$ > main.pid; exec sleep {marker}"
        ),
        1_000,
        None,
    );
    rig.sleeper("own-session.pid")?;
    let c = wait_for("the completion", || {
        rig.spool.read_completion("act_deadline").ok().flatten()
    })?;
    // The scan first: a sleeper left would hold the wrapper in its linger.
    none_left(marker)?;
    let status = reaped(wrapper)?;
    check(c.outcome == Outcome::Failed, || format!("{c:?}"))?;
    let d = c.detail.unwrap_or_default();
    check(d["timed_out"] == true, || format!("timed out: {d}"))?;
    let stop = &d["stop"];
    check(
        stop["verified_by"] == "tree" && stop["killed"] == 2 && stop["survivors"] == 0,
        || format!("the stop's verdict: {d}"),
    )?;
    check(status.success(), || {
        format!("the wrapper exited cleanly: {status}")
    })
}

/// An older wrapper (one started before an install) dies at the first
/// SIGTERM: it is stopped by its process group as every job was before, and
/// the verdict says `group`, counting the wrapper and its command's two.
fn older_wrapper_by_group() -> Result<(), String> {
    let (rig, marker) = (Rig::new(), "300.1803");
    let wrapper = rig.start(
        "act_old",
        &format!("sleep {marker} & echo $! > child.pid; echo $$ > main.pid; exec sleep {marker}"),
        60_000,
        Some("old"),
    );
    rig.sleeper("child.pid")?;
    rig.sleeper("main.pid")?;
    check(!job::catches_sigterm(wrapper), || {
        "an older wrapper does not catch SIGTERM".into()
    })?;
    let v = job::terminate(&rig.spool, wrapper, "act_old", Duration::from_millis(500));
    none_left(marker)?;
    check(v.verified() && v.verified_by == VerifiedBy::Group, || {
        format!("{v:?}")
    })?;
    check(v.killed == Some(3) && v.survivors == Some(0), || {
        format!("{v:?}")
    })?;
    let status = reaped(wrapper)?;
    check(status.signal() == Some(libc::SIGTERM), || {
        format!("it died at SIGTERM: {status}")
    })
}

/// A wrapper that catches SIGTERM and never answers: past the grace and the
/// answer's wait, its group is killed, and the cancel is uncertain, with why.
fn deaf_wrapper_uncertain() -> Result<(), String> {
    let (rig, marker) = (Rig::new(), "300.1804");
    let wrapper = rig.start(
        "act_deaf",
        &format!("echo $$ > main.pid; exec sleep {marker}"),
        60_000,
        Some("deaf"),
    );
    rig.sleeper("main.pid")?;
    wait_for("the handler", || {
        job::catches_sigterm(wrapper).then_some(())
    })?;
    let grace = Duration::from_millis(200);
    let t0 = Instant::now();
    let v = job::terminate(&rig.spool, wrapper, "act_deaf", grace);
    let took = t0.elapsed();
    none_left(marker)?;
    check(!v.verified(), || format!("uncertain: {v:?}"))?;
    check(
        v.why
            .as_deref()
            .is_some_and(|w| w.contains("did not answer")),
        || format!("why: {v:?}"),
    )?;
    check(took >= grace + job::ANSWER_WAIT, || format!("{took:?}"))?;
    let status = reaped(wrapper)?;
    check(status.signal() == Some(libc::SIGKILL), || {
        format!("killed with its group: {status}")
    })
}

/// A SIGTERM that is not a cancel (the job's own `kill $PPID`, a hand
/// `kill`): the wrapper stops its tree, the `setsid` sleeper too, and then
/// ends by the signal, so the daemon still reads a wrapper killed before it
/// reported (theseus-6uo).
fn plain_sigterm() -> Result<(), String> {
    let (rig, marker) = (Rig::new(), "300.1805");
    let wrapper = rig.start(
        "act_plain",
        &format!(
            "setsid sleep {marker} > /dev/null 2>&1 < /dev/null & echo $! > own-session.pid; \
             echo $$ > main.pid; exec sleep {marker}"
        ),
        60_000,
        None,
    );
    rig.sleeper("own-session.pid")?;
    rig.sleeper("main.pid")?;
    wait_for("the handler", || {
        job::catches_sigterm(wrapper).then_some(())
    })?;
    unsafe { libc::kill(wrapper as i32, libc::SIGTERM) };
    let status = reaped(wrapper)?;
    none_left(marker)?;
    check(status.signal() == Some(libc::SIGTERM), || {
        format!("it ended by SIGTERM: {status}")
    })?;
    check(rig.spool.read_stop("act_plain").is_none(), || {
        "no verdict: no cancel asked".into()
    })?;
    check(
        rig.spool
            .read_completion("act_plain")
            .ok()
            .flatten()
            .is_none(),
        || "no completion".into(),
    )
}
