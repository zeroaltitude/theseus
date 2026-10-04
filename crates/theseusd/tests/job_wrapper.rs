//! The job wrapper as its own process (theseus-6qy), run as the kernel runs
//! it: `theseusd job-wrapper`, detached. It is a child subreaper, so a
//! double-forked descendant is reparented to it instead of to init; it reaps
//! the orphans that exit while the command runs; after the command exits it
//! reports as before, then lingers until the last descendant has exited; and
//! a cancel stops its whole tree (M4 18a): the wrapper is asked alone, and
//! stops every descendant, one in its own session too.

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
            sandbox: None,
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

/// Tier 7.1 (review F2): a running wrapper sleeps until its command ends,
/// where it looked every 20 ms. Over a second of a command that does nothing,
/// its main thread is switched out a handful of times, not fifty; then the
/// command's end wakes it, and it reports.
#[test]
fn a_running_wrapper_sleeps_until_its_command_ends() {
    let rig = Rig::new();
    let wrapper = rig.start("act_sleep", "echo $$ > main.pid; exec sleep 1.6");
    rig.pid("main.pid");
    let switches = || -> u64 {
        std::fs::read_to_string(format!("/proc/{wrapper}/status"))
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    };
    std::thread::sleep(Duration::from_millis(100));
    let before = switches();
    std::thread::sleep(Duration::from_secs(1));
    let woke = switches() - before;
    assert!(woke <= 5, "the wrapper woke {woke} times in a second");
    let c = wait_for("the completion", || {
        rig.spool.read_completion("act_sleep").unwrap()
    });
    assert_eq!(c.outcome, theseus_kernel::Outcome::Succeeded);
    wait_for("the wrapper to exit", || (!alive(wrapper)).then_some(()));
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

/// What review 2's S2 hid (theseus-bzq): the wrapper died at SIGTERM, so a
/// cancel that waited on the wrapper alone said the job was gone at once,
/// and a command that ignores SIGTERM ran on, orphaned. Since 18a the
/// wrapper stops its tree: the grace passes, the freeze and SIGKILL end the
/// command, and only then is the job gone, verified.
#[test]
fn a_cancel_waits_out_a_command_that_ignores_sigterm_then_kills_it() {
    let rig = Rig::new();
    let wrapper = rig.start("act_stubborn", &stubborn(0));
    let main = rig.pid("main0.pid");
    let grace = Duration::from_millis(400);
    let t0 = Instant::now();
    let v = job::terminate(&rig.spool, wrapper, "act_stubborn", grace);
    assert!(v.verified(), "{v:?}");
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
    let mut stop = job::Stopping::start(&rig.spool, jobs, grace);
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

/// A cancel ends the whole job (M4 18a; theseus-hcc): the command, the
/// descendants that stayed in its group, and one that left it with `setsid`,
/// which a cancel by the group never reached. The wrapper, `theseusd
/// job-wrapper` itself, stops its tree and exits; the verdict says how.
#[test]
fn a_cancel_kills_the_whole_tree_a_setsid_descendant_too() {
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
    let v = job::terminate(&rig.spool, wrapper, "act_cancel", Duration::from_secs(2));
    if alive(own_session) {
        kill(own_session);
        panic!("the descendant in its own session survived the cancel: {v:?}");
    }
    for (what, pid) in [
        ("the wrapper", wrapper),
        ("the command", main),
        ("its child", in_group),
    ] {
        wait_for(&format!("{what} to be gone"), || {
            (!alive(pid)).then_some(())
        });
    }
    assert!(v.verified(), "{v:?}");
    assert_eq!(v.verified_by, theseus_kernel::VerifiedBy::Tree);
    assert_eq!((v.killed, v.survivors), (Some(3), Some(0)), "{v:?}");
    assert!(
        rig.spool.read_completion("act_cancel").unwrap().is_none(),
        "a cancelled job writes no completion: its verdict is the cancel's"
    );
}

/// Once the daemon has reaped a wrapper, its pid may become another
/// process's (theseus-z4b). A cancel then finds the wrapper gone and signals
/// nothing, since that process's command line does not name the job: here, a
/// `sleep` that leads its own process group, as a new wrapper would. With no
/// verdict and no report from the job, its cancel is uncertain (18a).
#[test]
fn a_cancel_leaves_a_process_that_took_the_wrappers_pid_alone() {
    use std::os::unix::process::CommandExt;
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = other.id();
    // Until its exec is done, nobody can tell whose it is (theseus-mi6a).
    wait_for("sleep's exec", || {
        std::fs::read(format!("/proc/{pid}/cmdline"))
            .ok()
            .filter(|c| c.starts_with(b"sleep\0"))
    });
    assert!(!job::wrapper_alive(pid, "act_gone"));
    let rig = Rig::new();
    let v = job::terminate(&rig.spool, pid, "act_gone", Duration::from_secs(2));
    assert!(!v.verified(), "nothing says the job ended: {v:?}");
    assert!(alive(pid), "not the job's wrapper, so not signalled");
    let _ = other.kill();
    let _ = other.wait();
}

/// A child that reads as a process in its exec until it is released, then
/// execs `argv` for real (theseus-mi6a). A real exec reads so for about 0.1
/// ms after its spawn returns, from its switch to the new image until it sets
/// the image's arguments; this one holds that state open. Forked from a
/// thread of its own, it leads its own session and process group, as
/// `spawn_detached` makes a wrapper, unmaps the pages that hold the
/// arguments it inherited, so that its command line reads empty, and waits
/// on a pipe. Everything it uses after the fork is made before it.
///
/// It unmaps from its arguments' page to the end of the main stack, the
/// environment's pages included. A hole below a part of the stack left
/// mapped is not empty to `/proc`: the read of a command line grows the stack
/// down over it and reads zeros.
struct InExec {
    pid: u32,
    release: std::os::fd::OwnedFd,
}

impl InExec {
    fn start(cwd: &Path, argv: &[String]) -> Self {
        use std::ffi::{c_void, CString};
        use std::os::fd::FromRawFd;
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(argv[0].as_str()).unwrap();
        let args: Vec<CString> = argv
            .iter()
            .map(|a| CString::new(a.as_str()).unwrap())
            .collect();
        let env = [CString::new(format!(
            "PATH={}",
            std::env::var("PATH").unwrap_or_default()
        ))
        .unwrap()];
        let cwd = CString::new(cwd.as_os_str().as_bytes()).unwrap();
        let null = CString::new("/dev/null").unwrap();
        // This process's arguments start at `/proc/self/stat`'s field 48
        // (arg_start); the mapping that holds them ends the stack.
        let s = std::fs::read_to_string("/proc/self/stat").unwrap();
        let arg_start: usize = s[s.rfind(')').unwrap() + 2..]
            .split_whitespace()
            .nth(45)
            .and_then(|x| x.parse().ok())
            .unwrap();
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let lo = arg_start & !(page - 1);
        let hi = std::fs::read_to_string("/proc/self/maps")
            .unwrap()
            .lines()
            .find_map(|l| {
                let (a, b) = l.split_whitespace().next()?.split_once('-')?;
                let (a, b) = (
                    usize::from_str_radix(a, 16).ok()?,
                    usize::from_str_radix(b, 16).ok()?,
                );
                (a <= arg_start && arg_start < b).then_some(b)
            })
            .expect("the mapping that holds the arguments");
        assert!(lo > 0 && hi > lo, "no argument area in /proc/self/stat");
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        let [wait_fd, release_fd] = fds;
        // The main thread's stack holds the arguments; this thread's does not.
        let pid = std::thread::spawn(move || {
            let mut argp: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
            argp.push(std::ptr::null());
            let envp = [env[0].as_ptr(), std::ptr::null()];
            // SAFETY: after the fork the child makes system calls only, on
            // memory made before it, and ends in `execve` or `_exit`.
            unsafe {
                let pid = libc::fork();
                if pid == 0 {
                    libc::close(release_fd);
                    libc::setsid();
                    libc::munmap(lo as *mut c_void, hi - lo);
                    let mut b = 0u8;
                    if libc::read(wait_fd, (&mut b as *mut u8).cast(), 1) == 1 {
                        // Its own stdio, as a wrapper's is: never the test's.
                        let n = libc::open(null.as_ptr(), libc::O_RDWR);
                        for fd in 0..3 {
                            libc::dup2(n, fd);
                        }
                        libc::chdir(cwd.as_ptr());
                        libc::execve(path.as_ptr(), argp.as_ptr(), envp.as_ptr());
                    }
                    libc::_exit(127);
                }
                pid
            }
        })
        .join()
        .unwrap();
        assert!(pid > 0, "fork failed");
        unsafe { libc::close(wait_fd) };
        Self {
            pid: pid as u32,
            // SAFETY: the pipe's write end, owned here alone.
            release: unsafe { std::os::fd::OwnedFd::from_raw_fd(release_fd) },
        }
    }

    /// Wait until `holder` reads it as a process in its exec; on a timeout,
    /// say what `/proc` showed instead.
    fn wait_starting(&self, job: &str) {
        let t0 = Instant::now();
        while job::holder(self.pid, job) != job::Holder::Starting {
            if t0.elapsed() > Duration::from_secs(10) {
                let cmdline = std::fs::read(format!("/proc/{}/cmdline", self.pid));
                let stat = std::fs::read_to_string(format!("/proc/{}/stat", self.pid));
                panic!(
                    "the child never read as in its exec: {:?}; cmdline {cmdline:?}; stat {stat:?}",
                    job::holder(self.pid, job)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Let it exec.
    fn release(&self) {
        use std::io::Write;
        let mut f = std::fs::File::from(self.release.try_clone().unwrap());
        f.write_all(b"x").unwrap();
    }

    /// Reap it, killing its group first if it still runs; how it ended.
    fn reap(&self) -> libc::c_int {
        unsafe { libc::kill(-(self.pid as i32), libc::SIGKILL) };
        let mut status = 0;
        unsafe { libc::waitpid(self.pid as i32, &mut status, 0) };
        status
    }
}

/// A stop that comes while a job's wrapper is still in its exec
/// (theseus-mi6a). Its command line reads empty, as a zombie's does, but it
/// is live: the stop must not take it for another process holding the pid,
/// skip its signal, and say the job is gone while it runs on, as it did. It
/// signals nothing until the pid reads as the job's wrapper, then stops the
/// wrapper and its command as it stops any job.
#[test]
fn a_stop_waits_for_a_wrapper_still_in_its_exec_then_stops_it() {
    let rig = Rig::new();
    let id = "act_starting";
    let dir = rig.dir.path().display().to_string();
    let argv: Vec<String> = [
        env!("CARGO_BIN_EXE_theseusd"),
        job::WRAPPER_MODE,
        "--spool",
        &rig.spool.dir().display().to_string(),
        "--correlation-id",
        id,
        "--deadline-ms",
        "60000",
        "--",
        "sh",
        "-c",
        "echo $$ > main.pid; while [ -d \"$1\" ]; do sleep 0.05; done",
        "sh",
        &dir,
    ]
    .map(String::from)
    .into();
    let w = InExec::start(rig.dir.path(), &argv);
    w.wait_starting(id);
    assert!(
        job::wrapper_alive(w.pid, id),
        "in its exec, it counts as alive"
    );
    let mut stop = job::Stopping::start(&rig.spool, [(w.pid, id.to_string())], job::STOP_GRACE);
    assert!(!stop.all_gone(), "not taken for another process");
    assert!(stop.poll().is_some(), "still waiting for it");
    assert!(alive(w.pid), "nothing signalled while it cannot be told");
    // The exec completes, and the wrapper starts the job's command.
    w.release();
    let main = rig.pid("main.pid");
    assert_eq!(job::holder(w.pid, id), job::Holder::Wrapper);
    while let Some(wait) = stop.poll() {
        std::thread::sleep(wait);
    }
    assert!(stop.all_gone(), "the stop saw it end");
    assert!(!alive(main), "the command is gone");
    let v = stop.verdicts().next().unwrap().1;
    assert!(v.verified(), "{v:?}");
    let status = w.reap();
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "the wrapper stopped its tree and exited (18a): status {status:#x}"
    );
}

/// The other side of the same window (theseus-mi6a): a process in its exec
/// may be another program that took a reaped wrapper's pid. The stop waits
/// to read it, finds another program, and leaves it alone.
#[test]
fn a_stop_leaves_alone_a_process_in_its_exec_that_becomes_another_program() {
    let rig = Rig::new();
    let id = "act_reused";
    let sleep = ["/usr/bin/sleep", "/bin/sleep"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap();
    let w = InExec::start(rig.dir.path(), &[sleep.to_string(), "30".into()]);
    w.wait_starting(id);
    let mut stop = job::Stopping::start(&rig.spool, [(w.pid, id.to_string())], job::STOP_GRACE);
    assert!(!stop.all_gone());
    w.release();
    wait_for("sleep's exec", || {
        (job::holder(w.pid, id) == job::Holder::Other).then_some(())
    });
    assert_eq!(stop.poll(), None, "another program: the job is long gone");
    assert!(stop.all_gone());
    assert!(alive(w.pid), "not the job's wrapper, so not signalled");
    let status = w.reap();
    assert!(libc::WIFSIGNALED(status) && libc::WTERMSIG(status) == libc::SIGKILL);
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
            sandbox: None,
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
