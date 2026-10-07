//! A tree stop under a tight open-file limit (theseus-dwoj's review): the
//! SIGTERM phase keeps at most `tree::WATCHED` pidfds and closes them before
//! the freeze, so a large tree is still signalled, frozen and killed whole,
//! and the scan never fails for want of a descriptor. A test binary of its
//! own: it lowers this process's open-file limit. Each tree is a `sh` this
//! test starts, found by its pid, so nothing else on the machine is touched.

use std::time::{Duration, Instant};

use theseus_kernel::tree::{self, Left};

fn limit() -> libc::rlimit {
    let mut l = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit writes the struct it is given.
    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) };
    l
}

fn set_limit(l: &libc::rlimit) {
    // SAFETY: setrlimit reads the struct it is given.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, l) }, 0);
}

fn open_now() -> u64 {
    std::fs::read_dir("/proc/self/fd").unwrap().count() as u64
}

/// A `sh` running `script`, and its pid, once `n` of its descendants run.
fn tree_of(script: &str, n: usize) -> (std::process::Child, u32) {
    let sh = std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .spawn()
        .unwrap();
    let root = sh.id();
    let t0 = Instant::now();
    while tree::descendants(root).len() < n {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "the tree never grew to {n}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    (sh, root)
}

/// Stop `root`'s tree, `grace` its SIGTERM's, with this process's open-file
/// limit `room` above what is open now; the limit is put back before
/// anything else is read.
fn stop_with_room(root: u32, grace: Duration, room: u64) -> tree::Stopped {
    let before = limit();
    set_limit(&libc::rlimit {
        rlim_cur: open_now() + room,
        rlim_max: before.rlim_max,
    });
    let mut reap = || Left::Unknown;
    let s = tree::stop(root, grace, &mut reap);
    set_limit(&before);
    s
}

/// What is left of `root`'s tree, killed so a failure leaves nothing behind.
fn left_of(root: u32, mut sh: std::process::Child) -> usize {
    std::thread::sleep(Duration::from_millis(200));
    let left = tree::descendants(root);
    for p in &left {
        tree::signal(*p, libc::SIGKILL);
    }
    let _ = sh.kill();
    let _ = sh.wait();
    left.len()
}

/// 40 processes that ignore SIGTERM, and room for the kill's 40 pidfds, not
/// for the grace's 40 on top: every one is killed, and the verdict says so.
#[test]
fn a_stop_under_a_tight_open_file_limit_kills_a_tree_that_outlives_sigterm() {
    let (sh, root) = tree_of(
        "trap '' TERM; i=0; while [ $i -lt 40 ]; do sleep 120 & i=$((i+1)); done; wait",
        40,
    );
    let s = stop_with_room(root, Duration::from_millis(300), 60);
    let left = left_of(root, sh);
    assert_eq!(left, 0, "{left} of 40 outlived the stop: {s:?}");
    assert!(s.survivors.is_empty() && !s.unseen, "{s:?}");
}

/// 50 shells that trap SIGTERM, each with a `sleep`: 100 processes, and room
/// for 70 descriptors. Every shell has its SIGTERM, so every trap runs.
#[test]
fn a_stop_under_a_tight_open_file_limit_gives_every_process_its_sigterm() {
    let dir = tempfile::tempdir().unwrap();
    let marks = dir.path().join("marks");
    let (sh, root) = tree_of(
        &format!(
            "i=0; while [ $i -lt 50 ]; do sh -c 'trap \"echo x >> {}; exit 0\" TERM; sleep 120 & wait' & \
             i=$((i+1)); done; wait",
            marks.display()
        ),
        100,
    );
    // The stop ends once the traps have run: the grace is only their bound.
    let s = stop_with_room(root, Duration::from_secs(2), 70);
    let left = left_of(root, sh);
    let ran = std::fs::read_to_string(&marks)
        .unwrap_or_default()
        .lines()
        .count();
    assert_eq!(
        ran, 50,
        "{ran} of 50 traps ran (a SIGTERM never sent): {s:?}"
    );
    assert_eq!(left, 0, "{left} outlived the stop: {s:?}");
}
