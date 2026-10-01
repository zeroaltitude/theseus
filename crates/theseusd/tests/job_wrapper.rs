//! The job wrapper as its own process (theseus-6qy), run as the kernel runs
//! it: `theseusd job-wrapper`, detached. It is a child subreaper, so a
//! double-forked descendant is reparented to it instead of to init; it reaps
//! the orphans that exit while the command runs; after the command exits it
//! reports as before, then lingers until the last descendant has exited; and
//! a cancel kills what it killed before, the wrapper's process group.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use theseus_kernel::job::{self, WrapperArgs};
use theseus_kernel::Spool;

struct Rig {
    dir: tempfile::TempDir,
    spool: Spool,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let spool = Spool::open(&dir.path().join("spool")).unwrap();
        Self { dir, spool }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    /// Start `sh -c script` under a detached wrapper, in the rig's directory,
    /// which is `$1`. Returns the wrapper's pid.
    fn start(&self, id: &str, script: &str) -> u32 {
        self.start_with(id, script, None)
    }

    /// `start`, with the umask the daemon passes for the command.
    fn start_with(&self, id: &str, script: &str, umask: Option<u32>) -> u32 {
        self.start_capped(id, script, umask, job::DEFAULT_OUTPUT_MAX_BYTES)
    }

    /// `start_with`, with the cap on the job's output file the daemon passes
    /// (theseus-102).
    fn start_capped(
        &self,
        id: &str,
        script: &str,
        umask: Option<u32>,
        output_max_bytes: u64,
    ) -> u32 {
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: id.into(),
            deadline_ms: 60_000,
            notify_socket: None,
            // `$1` is the rig's directory: every loop also ends when it is
            // gone, so a test that fails leaves nothing running.
            argv: vec![
                "sh".into(),
                "-c".into(),
                script.into(),
                "sh".into(),
                self.dir.path().display().to_string(),
            ],
            cwd: Some(self.dir.path().to_path_buf()),
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
            umask,
            redact: vec![],
            output_max_bytes,
        };
        job::spawn_detached(
            Path::new(env!("CARGO_BIN_EXE_theseusd")),
            &[job::WRAPPER_MODE],
            &self.spool,
            &args,
        )
        .unwrap()
    }

    /// A pid the job wrote to `file`.
    fn pid(&self, file: &str) -> u32 {
        wait_for(&format!("{file} to be written"), || {
            std::fs::read_to_string(self.path(file))
                .ok()
                .filter(|s| s.ends_with('\n'))
                .and_then(|s| s.trim().parse().ok())
        })
    }
}

/// `/proc/<pid>/stat`: the state letter and the parent's pid.
fn stat(pid: u32) -> Option<(char, u32)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut f = s[s.rfind(')')? + 2..].split(' ');
    let state = f.next()?.chars().next()?;
    Some((state, f.next()?.parse().ok()?))
}

/// Running or sleeping: neither gone nor a zombie.
fn alive(pid: u32) -> bool {
    stat(pid).is_some_and(|(s, _)| s != 'Z' && s != 'X')
}

/// The children of `pid` that are zombies.
fn zombie_children(pid: u32) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&p| stat(p).is_some_and(|(s, pp)| s == 'Z' && pp == pid))
        .collect()
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn kill(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status();
}

/// Review 2's H3 (theseus-wz2): a job's raw output, which the scrubber has
/// not seen, is the operator's alone (0600) whatever the umask, and the
/// job's command runs under the umask the daemon passed it, the operator's,
/// so what it makes in the workspace is made as their shell would make it.
#[test]
fn a_jobs_raw_output_is_private_and_its_command_has_the_operators_umask() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let rig = Rig::new();
    let script = "umask > umask.tmp; mv umask.tmp umask.txt; : > made; echo out";
    rig.start_with("act_umask", script, Some(0o027));
    wait_for("the completion", || {
        rig.spool.read_completion("act_umask").unwrap()
    });
    let out = rig.spool.result_path("act_umask");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "out\n");
    assert_eq!(mode(&out), 0o600, "the raw output");
    assert_eq!(
        std::fs::read_to_string(rig.path("umask.txt"))
            .unwrap()
            .trim(),
        "0027"
    );
    assert_eq!(mode(&rig.path("made")), 0o640, "a file the job made");
    // Without one, the command keeps the wrapper's (this test's) umask; the
    // output is 0600 all the same.
    rig.start_with("act_none", "umask > none.tmp; mv none.tmp none.txt", None);
    wait_for("the completion", || {
        rig.spool.read_completion("act_none").unwrap()
    });
    assert_eq!(mode(&rig.spool.result_path("act_none")), 0o600);
}

/// A grandchild orphaned by a double fork is the wrapper's child at once,
/// while the job's command still runs, and not init's. The command then
/// exits and is reported, and the wrapper lingers, marked in the spool,
/// until the grandchild ends; then it exits and the mark is gone.
#[test]
fn a_double_fork_stays_under_its_wrapper_which_lingers_until_it_ends() {
    let rig = Rig::new();
    let wrapper = rig.start(
        "act_linger",
        "echo $PPID > wrapper.pid; \
         ( setsid sh -c 'echo $$ > grandchild.pid; \
             while [ ! -e release ] && [ -d \"$1\" ]; do sleep 0.02; done' sh \"$1\" \
           > /dev/null 2>&1 < /dev/null & ); \
         while [ ! -e main.exit ] && [ -d \"$1\" ]; do sleep 0.02; done; echo done",
    );
    assert_eq!(rig.pid("wrapper.pid"), wrapper, "the command's parent");
    let grandchild = rig.pid("grandchild.pid");
    // Its parent, the subshell, exited at once: it was reparented to the
    // nearest subreaper, which is the wrapper, while the command runs.
    wait_for("the grandchild's reparenting", || {
        (stat(grandchild)?.1 == wrapper).then_some(())
    });
    assert!(rig.spool.lingering().is_empty(), "the command still runs");
    // The command exits: the result is reported as before.
    std::fs::write(rig.path("main.exit"), "").unwrap();
    let c = wait_for("the completion", || {
        rig.spool.read_completion("act_linger").unwrap()
    });
    assert_eq!(c.outcome, theseus_kernel::Outcome::Succeeded);
    assert_eq!(c.producer, format!("wrapper:{wrapper}"));
    // The wrapper lingers for the grandchild, and says so, once its report
    // is done.
    let lingering = wait_for("the lingering mark", || {
        Some(rig.spool.lingering()).filter(|l| !l.is_empty())
    });
    assert_eq!(lingering, vec![("act_linger".to_string(), wrapper)]);
    assert!(rig.spool.read_pid("act_linger").is_none());
    assert!(alive(wrapper) && alive(grandchild));
    assert_eq!(stat(grandchild).unwrap().1, wrapper);
    assert_eq!(job::wrapper_job(wrapper).as_deref(), Some("act_linger"));
    // The grandchild ends; the wrapper reaps it and exits.
    std::fs::write(rig.path("release"), "").unwrap();
    wait_for("the wrapper to exit", || (!alive(wrapper)).then_some(()));
    assert!(!alive(grandchild));
    assert!(rig.spool.lingering().is_empty());
    assert!(!rig.path("spool/lingering/act_linger").exists());
    assert_eq!(job::wrapper_job(wrapper), None, "an exited wrapper is none");
}

/// Orphans that exit while the command runs are reaped at once: none waits
/// as a zombie under the wrapper.
#[test]
fn orphans_that_exit_while_the_command_runs_are_reaped() {
    let rig = Rig::new();
    let wrapper = rig.start(
        "act_reap",
        "for i in 1 2 3 4 5 6 7 8; do ( sleep 0.05 & ); done; touch forked; \
         while [ ! -e main.exit ] && [ -d \"$1\" ]; do sleep 0.02; done",
    );
    wait_for("the forks", || rig.path("forked").exists().then_some(()));
    // Each orphan was reparented to the wrapper and has exited by now.
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(wrapper));
    assert_eq!(zombie_children(wrapper), Vec::<u32>::new());
    std::fs::write(rig.path("main.exit"), "").unwrap();
    wait_for("the wrapper to exit", || (!alive(wrapper)).then_some(()));
    assert!(
        rig.spool.lingering().is_empty(),
        "nothing was left to wait for"
    );
}

/// A command that leaves nothing running: the wrapper exits with it, and
/// never marks itself lingering.
#[test]
fn a_wrapper_with_nothing_left_exits_with_its_command() {
    let rig = Rig::new();
    let wrapper = rig.start("act_plain", "echo hi");
    let c = wait_for("the completion", || {
        rig.spool.read_completion("act_plain").unwrap()
    });
    assert_eq!(c.outcome, theseus_kernel::Outcome::Succeeded);
    wait_for("the wrapper to exit", || (!alive(wrapper)).then_some(()));
    assert!(!rig.path("spool/lingering/act_plain").exists());
    let out = std::fs::read_to_string(c.result_ref.unwrap()).unwrap();
    assert_eq!(out, "hi\n");
}

/// Review 2's R3 (theseus-102): a job that prints far past the cap leaves its
/// file within the cap, runs to its own end (the wrapper reads on and counts,
/// it never stops the command for printing), and its completion says what
/// was dropped and what the cap was. The cap reaches the wrapper on its
/// command line, as the daemon passes it. Since theseus-gsn9 the file keeps
/// both ends: the head, a marker, and the exact last bytes, its verdict
/// among them, which the wrapper's ring held until the pipe's end.
#[test]
fn a_job_that_prints_past_the_cap_keeps_both_ends_and_runs_to_its_end() {
    let rig = Rig::new();
    let cap = 64 * 1024;
    let printed = 5 * 1024 * 1024 + "the end\n".len() as u64;
    rig.start_capped(
        "act_chatty",
        "head -c 5242880 /dev/zero; echo 'the end'; : > after; exit 3",
        None,
        cap,
    );
    let c = wait_for("the completion", || {
        rig.spool.read_completion("act_chatty").unwrap()
    });
    assert!(rig.path("after").exists(), "the job ran to its end");
    let (head, tail) = theseus_kernel::redact::split(cap);
    assert_eq!((head, tail), (32_640, 32_768));
    let dropped = printed - head - tail;
    let marker = theseus_kernel::redact::marker(dropped, tail);
    let detail = c.detail.unwrap();
    assert_eq!(detail["exit_code"], 3, "{detail}");
    assert_eq!(
        detail["bytes"],
        head + marker.len() as u64 + tail,
        "{detail}"
    );
    assert_eq!(detail["truncated"], true, "{detail}");
    assert_eq!(detail["dropped"], dropped, "{detail}");
    assert_eq!(
        (detail["head"].as_u64(), detail["tail"].as_u64()),
        (Some(head), Some(tail)),
        "{detail}"
    );
    assert_eq!(detail["output_max_bytes"], cap, "{detail}");
    let out = std::fs::read(c.result_ref.unwrap()).unwrap();
    assert!(out.len() as u64 <= cap, "{}", out.len());
    let (first, rest) = out.split_at(head as usize);
    assert!(first.iter().all(|&b| b == 0), "the first bytes it printed");
    let (between, end) = rest.split_at(marker.len());
    assert_eq!(between, marker.as_bytes());
    assert_eq!(end.len() as u64, tail);
    assert!(end.ends_with(b"\0\0the end\n"), "the last bytes it printed");
    assert!(end[..end.len() - 8].iter().all(|&b| b == 0));
}

/// A command that ignores SIGTERM, as `trap '' TERM` makes it (its `sleep`s
/// inherit the ignore), writing `main<i>.pid`, until the rig is gone.
fn stubborn(i: usize) -> String {
    format!("trap '' TERM; echo $$ > main{i}.pid; while [ -d \"$1\" ]; do sleep 0.05; done")
}

/// What review 2's S2 hid (theseus-bzq): the wrapper dies at SIGTERM, so a
/// cancel that waited on the wrapper alone said the job was gone at once,
/// and a command that ignores SIGTERM ran on, orphaned. The cancel now waits
/// on the job's process group: the grace passes, SIGKILL ends the command,
/// and only then is the job gone.
#[test]
fn a_cancel_waits_out_a_command_that_ignores_sigterm_then_kills_it() {
    let rig = Rig::new();
    let wrapper = rig.start("act_stubborn", &stubborn(0));
    let main = rig.pid("main0.pid");
    let grace = Duration::from_millis(400);
    let t0 = Instant::now();
    assert!(job::terminate(wrapper, "act_stubborn", grace));
    let took = t0.elapsed();
    assert!(!alive(main), "the command is gone");
    assert!(took >= grace, "it waited out the grace: {took:?}");
    assert!(took < grace + Duration::from_secs(1), "{took:?}");
}

/// Review 2's S2 (theseus-bzq): three jobs that ignore SIGTERM, stopped
/// together, cost one grace, not three: each group gets SIGTERM at once, the
/// grace is shared, and the stragglers get SIGKILL together. Every command
/// is gone after it.
#[test]
fn three_jobs_that_ignore_sigterm_are_stopped_in_one_grace() {
    let rig = Rig::new();
    let (mut jobs, mut mains) = (vec![], vec![]);
    for i in 0..3 {
        let id = format!("act_stubborn_{i}");
        jobs.push((rig.start(&id, &stubborn(i)), id));
        mains.push(rig.pid(&format!("main{i}.pid")));
    }
    let grace = Duration::from_millis(600);
    let t0 = Instant::now();
    let mut stop = job::Stopping::start(jobs, grace);
    while let Some(wait) = stop.poll() {
        std::thread::sleep(wait);
    }
    let took = t0.elapsed();
    assert!(stop.all_gone());
    for main in mains {
        assert!(!alive(main), "command {main} is gone");
    }
    assert!(took >= grace, "{took:?}");
    assert!(took < grace * 2, "one grace, not three: {took:?}");
}

/// A cancel kills what it killed before: the wrapper's process group, which
/// is the wrapper, the command, and the descendants that stayed in it. A
/// descendant that left the group with `setsid` was never in reach of a
/// cancel, and still is not.
#[test]
fn a_cancel_kills_the_wrappers_process_group_as_before() {
    let rig = Rig::new();
    let wrapper = rig.start(
        "act_cancel",
        "sleep 30 & echo $! > in-group.pid; \
         setsid sleep 30 > /dev/null 2>&1 < /dev/null & echo $! > own-session.pid; \
         echo $$ > main.pid; wait",
    );
    let main = rig.pid("main.pid");
    let in_group = rig.pid("in-group.pid");
    let own_session = rig.pid("own-session.pid");
    wait_for("setsid to exec sleep", || {
        std::fs::read_to_string(format!("/proc/{own_session}/comm"))
            .ok()
            .filter(|c| c.trim() == "sleep")
    });
    assert!(job::terminate(
        wrapper,
        "act_cancel",
        Duration::from_secs(2)
    ));
    for (what, pid) in [
        ("the wrapper", wrapper),
        ("the command", main),
        ("its child", in_group),
    ] {
        wait_for(&format!("{what} to be killed"), || {
            (!alive(pid)).then_some(())
        });
    }
    assert!(
        alive(own_session),
        "a descendant in its own session is out of a cancel's reach, as before"
    );
    kill(own_session);
    assert!(
        rig.spool.read_completion("act_cancel").unwrap().is_none(),
        "a killed wrapper reports nothing; the reconciler finds it gone"
    );
}

/// Once the daemon has reaped a wrapper, its pid may become another
/// process's (theseus-z4b). A cancel then finds the wrapper gone and signals
/// nothing, since that process's command line does not name the job: here, a
/// `sleep` that leads its own process group, as a new wrapper would.
#[test]
fn a_cancel_leaves_a_process_that_took_the_wrappers_pid_alone() {
    use std::os::unix::process::CommandExt;
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = other.id();
    assert!(!job::wrapper_alive(pid, "act_gone"));
    assert!(job::terminate(pid, "act_gone", Duration::from_secs(2)));
    assert!(alive(pid), "not the job's wrapper, so not signalled");
    let _ = other.kill();
    let _ = other.wait();
}

/// An invented granted value: 26 bytes, two halves of 13.
const GRANTED: &str = "tv-invented_grant-5c1e9b7a";

impl Rig {
    /// `start`, with `INVENTED_GRANT` granted: its value in the wrapper's
    /// environment, its name on the wrapper's command line (`--redact`), as
    /// the daemon passes a broker's grant (theseus-l0d).
    fn start_granted(&self, id: &str, script: &str) -> u32 {
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: id.into(),
            deadline_ms: 60_000,
            notify_socket: None,
            argv: vec![
                "sh".into(),
                "-c".into(),
                script.into(),
                "sh".into(),
                self.dir.path().display().to_string(),
            ],
            cwd: Some(self.dir.path().to_path_buf()),
            env: vec![
                ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
                ("INVENTED_GRANT".into(), GRANTED.into()),
            ],
            umask: None,
            redact: vec![("INVENTED_GRANT".into(), "invented_grant".into())],
            output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
        };
        job::spawn_detached(
            Path::new(env!("CARGO_BIN_EXE_theseusd")),
            &[job::WRAPPER_MODE],
            &self.spool,
            &args,
        )
        .unwrap()
    }
}

/// A job that prints its own granted value (theseus-l0d), through the
/// wrapper process as the daemon runs it: split across two writes, among
/// other output on stdout and stderr. Its spool file holds everything else,
/// byte for byte, and the value withheld, and the completion counts it.
#[test]
fn a_granted_value_the_job_prints_never_reaches_its_spool_file() {
    let rig = Rig::new();
    let wrapper = rig.start_granted(
        "act_grant",
        "printf 'one\\n%s' \"${INVENTED_GRANT%?????????????}\"; sleep 0.3; \
         printf '%s two\\n' \"${INVENTED_GRANT#?????????????}\"; echo three >&2",
    );
    let c = wait_for("the completion", || {
        rig.spool.read_completion("act_grant").unwrap()
    });
    let out = std::fs::read(rig.spool.result_path("act_grant")).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out),
        "one\n[redacted:invented_grant] two\nthree\n"
    );
    let detail = c.detail.unwrap();
    assert_eq!(detail["withheld"], 1, "{detail}");
    assert!(detail.get("output_open").is_none(), "{detail}");
    assert_eq!(detail["bytes"], out.len() as u64, "{detail}");
    wait_for("the wrapper's exit", || (!alive(wrapper)).then_some(()));
}

/// A descendant that keeps the job's output open past the command's exit
/// does not hold the report: the completion comes once the command exits,
/// saying the output is still open. What the descendant prints later still
/// goes through the copy, its granted value withheld, until the wrapper,
/// lingering for it, exits.
#[test]
fn a_descendant_holding_the_output_neither_holds_the_report_nor_leaks_the_value() {
    let rig = Rig::new();
    let wrapper = rig.start_granted(
        "act_late",
        "( while [ ! -e release ] && [ -d \"$1\" ]; do sleep 0.02; done; \
           printf 'late %s\\n' \"$INVENTED_GRANT\" ) & echo early",
    );
    let c = wait_for("the completion", || {
        rig.spool.read_completion("act_late").unwrap()
    });
    let detail = c.detail.unwrap();
    assert_eq!(detail["output_open"], true, "{detail}");
    assert_eq!(
        std::fs::read_to_string(rig.spool.result_path("act_late")).unwrap(),
        "early\n"
    );
    std::fs::write(rig.path("release"), "").unwrap();
    wait_for("the wrapper's exit", || (!alive(wrapper)).then_some(()));
    assert_eq!(
        std::fs::read_to_string(rig.spool.result_path("act_late")).unwrap(),
        "early\nlate [redacted:invented_grant]\n"
    );
}
