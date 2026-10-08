//! A host that refuses the newer system calls, as a container's seccomp
//! profile does (theseus-f7tz): Docker's answers `clone3` with ENOSYS, and an
//! older profile `pidfd_open` with EPERM. Every `proc.run` in a container
//! failed to start, "Function not implemented (os error 38)", before. Each
//! case runs on a thread of its own, the only one the filter holds.

use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::Path;

use theseus_sandbox::seccomp::refuse_here;

/// `f` on a new thread that the host refuses `calls` on.
fn refused<T: Send + 'static>(
    calls: &'static [(libc::c_long, i32)],
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    std::thread::spawn(move || {
        refuse_here(calls).expect("a seccomp filter on this thread");
        f()
    })
    .join()
    .expect("the refused thread")
}

/// `argv` started by the wrapper's spawn; its output and exit code.
fn run(argv: &[&str]) -> std::io::Result<(String, i32)> {
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let e = crate::spawn::Exec::new(&argv, Some(Path::new("/")))?;
    let (mut r, w) = std::io::pipe()?;
    let null = std::fs::File::open("/dev/null")?;
    let stdio = [null.as_raw_fd(), w.as_raw_fd(), w.as_raw_fd()];
    let pid = crate::spawn::spawn(&e, stdio, Some(0o027), None)?;
    drop(w);
    let mut out = String::new();
    r.read_to_string(&mut out)?;
    let mut status = 0;
    // SAFETY: this test's own child.
    unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) };
    Ok((out, libc::WEXITSTATUS(status)))
}

/// Where `clone3` answers ENOSYS, the command is made by `clone`: it runs,
/// with its umask, cwd, and output, and the process says it fell back. An
/// exec that fails is still the spawn's error.
#[test]
fn a_command_runs_by_clone_where_clone3_is_refused() {
    let (ran, missing, fell_back) = refused(&[(libc::SYS_clone3, libc::ENOSYS)], || {
        (
            run(&["sh", "-c", "umask; pwd; echo err >&2; exit 3"]),
            run(&["theseus-no-such-program-7f3a"]),
            crate::spawn::by_clone(),
        )
    });
    let (out, code) = ran.expect("the command starts where clone3 is refused");
    assert_eq!(out, "0027\n/\nerr\n");
    assert_eq!(code, 3);
    let e = missing.unwrap_err();
    assert_eq!(e.raw_os_error(), Some(libc::ENOENT), "{e}");
    assert!(fell_back, "the fallback is said");
}

/// Where `pidfd_open` is refused, a process is signalled by its pid, still
/// checked against its start time; a stop of a tree still ends it.
#[test]
fn a_tree_is_signalled_and_stopped_by_pid_where_pidfds_are_refused() {
    for errno in [libc::ENOSYS, libc::EPERM] {
        let calls: &'static [(libc::c_long, i32)] = match errno {
            libc::ENOSYS => &[(libc::SYS_pidfd_open, libc::ENOSYS)],
            _ => &[(libc::SYS_pidfd_open, libc::EPERM)],
        };
        let (signalled, reused, stopped, status) = refused(calls, || {
            let mut sleeper = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            let pid = sleeper.id();
            let start = crate::tree::stat(pid).unwrap().start;
            let reused = crate::tree::signal(
                crate::tree::Proc {
                    pid,
                    start: start + 1,
                },
                libc::SIGKILL,
            );
            let signalled = crate::tree::signal(crate::tree::Proc { pid, start }, libc::SIGKILL);
            let status = sleeper.wait().unwrap();
            // A tree below a shell: its children, stopped by pid.
            let mut sh = std::process::Command::new("sh")
                .args(["-c", "sleep 30 & sleep 30 & wait"])
                .spawn()
                .unwrap();
            let t0 = std::time::Instant::now();
            while crate::tree::descendants(sh.id()).len() < 2
                && t0.elapsed() < std::time::Duration::from_secs(5)
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let stopped =
                crate::tree::stop(sh.id(), std::time::Duration::from_millis(200), &mut || {
                    crate::tree::Left::Unknown
                });
            let _ = sh.kill();
            let _ = sh.wait();
            (signalled, reused, stopped, status)
        });
        use std::os::unix::process::ExitStatusExt;
        assert!(
            !reused,
            "a pid whose start time differs is left alone ({errno})"
        );
        assert!(signalled, "the signal was sent by pid ({errno})");
        assert_eq!(status.signal(), Some(libc::SIGKILL), "({errno})");
        assert_eq!(stopped.killed, 2, "{stopped:?} ({errno})");
        assert!(stopped.survivors.is_empty(), "{stopped:?} ({errno})");
    }
}

/// A child bound for a cgroup is in it before it runs a thing, made by
/// `clone3` (born there) or, where that is refused, by `clone` (moved by its
/// own write to `cgroup.procs`). It needs a cgroup v2 directory this test may
/// make: skipped, and said, where there is none.
#[test]
fn a_command_starts_in_its_cgroup_by_clone3_and_by_clone() {
    let made = ["/sys/fs/cgroup/unified", "/sys/fs/cgroup"]
        .iter()
        .map(|root| Path::new(root).join(format!("theseus-f7tz-{}", std::process::id())))
        .find(|dir| {
            dir.parent()
                .is_some_and(|p| p.join("cgroup.procs").exists())
                && std::fs::create_dir(dir).is_ok()
        });
    let Some(dir) = made else {
        eprintln!("skipped: no cgroup v2 directory this test may make");
        return;
    };
    let in_cgroup = |by_clone: bool| {
        let dir = dir.clone();
        let calls: &'static [(libc::c_long, i32)] = match by_clone {
            true => &[(libc::SYS_clone3, libc::ENOSYS)],
            false => &[],
        };
        refused(calls, move || {
            let cg = std::fs::File::open(&dir).unwrap();
            let argv = ["cat".to_string(), "/proc/self/cgroup".to_string()];
            let e = crate::spawn::Exec::new(&argv, None).unwrap();
            let (mut r, w) = std::io::pipe().unwrap();
            let null = std::fs::File::open("/dev/null").unwrap();
            let stdio = [null.as_raw_fd(), w.as_raw_fd(), w.as_raw_fd()];
            use std::os::fd::AsFd;
            let pid = crate::spawn::spawn(&e, stdio, None, Some(cg.as_fd())).unwrap();
            drop(w);
            let mut out = String::new();
            r.read_to_string(&mut out).unwrap();
            // SAFETY: this test's own child.
            unsafe { libc::waitpid(pid as libc::pid_t, &mut 0, 0) };
            (out, crate::spawn::by_clone())
        })
    };
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    let (born, by_clone3) = in_cgroup(false);
    let (moved, fell_back) = in_cgroup(true);
    let _ = std::fs::remove_dir(&dir);
    assert!(!by_clone3, "clone3 was refused with no filter");
    assert!(fell_back, "clone made the second");
    let v2 = |out: &str| {
        out.lines()
            .find(|l| l.starts_with("0::"))
            .map(str::to_string)
    };
    assert!(v2(&born).is_some_and(|l| l.ends_with(&name)), "{born}");
    assert!(v2(&moved).is_some_and(|l| l.ends_with(&name)), "{moved}");
}

/// The micro-bench of a job's spawn (theseus-f7tz's FAST): `true` started
/// 200 times by `clone3`, then by `clone` where `clone3` is refused, each
/// to its exit; p50 and p95 of each. `cargo nextest run -E
/// 'test(spawn_of_true)' --run-ignored only --no-capture`.
#[test]
#[ignore = "a measure, run by hand"]
fn spawn_of_true_by_clone3_and_by_clone() {
    fn spawns() -> Vec<std::time::Duration> {
        let e = crate::spawn::Exec::new(&["true".to_string()], None).unwrap();
        let null = std::fs::File::open("/dev/null").unwrap();
        let stdio = [null.as_raw_fd(); 3];
        let mut took: Vec<_> = (0..200)
            .map(|_| {
                let t0 = std::time::Instant::now();
                let pid = crate::spawn::spawn(&e, stdio, None, None).unwrap();
                // SAFETY: this test's own child.
                unsafe { libc::waitpid(pid as libc::pid_t, &mut 0, 0) };
                t0.elapsed()
            })
            .collect();
        took.sort();
        took
    }
    let by_clone3 = refused(&[], spawns);
    let by_clone = refused(&[(libc::SYS_clone3, libc::ENOSYS)], spawns);
    for (how, t) in [("clone3", by_clone3), ("clone", by_clone)] {
        println!("spawn of true by {how}: p50 {:?}, p95 {:?}", t[100], t[190]);
    }
}
