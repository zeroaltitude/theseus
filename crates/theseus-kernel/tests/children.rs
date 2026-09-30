//! The registry of the daemon's children and its sweep (theseus-z4b), in a
//! test binary of its own, with one test: a sweep reaps any child this
//! process may reap, and `adopt` makes the process a subreaper, so no other
//! test's children may be about.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use theseus_kernel::children::{self, Kind, Relearned};
use theseus_kernel::job;

/// `/proc/<pid>/stat`: the state letter and the parent's pid.
fn stat(pid: u32) -> Option<(char, u32)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let mut f = s[s.rfind(')')? + 2..].split(' ');
    Some((f.next()?.chars().next()?, f.next()?.parse().ok()?))
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn kill(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

/// The children of `pid`, from its main thread's `children` file.
fn children_of(pid: u32) -> Vec<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// A wrapper is reaped by its pid, and an orphan that this subreaper adopted
/// once it exits. An owned child is left as a zombie until its owner waits,
/// and the owner's wait still gets its status. After an exec, a live child
/// whose command line is a wrapper's is learned as that job's wrapper, and
/// any other as an orphan.
#[test]
fn a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child() {
    let me = std::process::id();
    assert_eq!(children::daemon(), None, "nothing is adopted yet");
    children::adopt().unwrap();
    assert!(children::is_subreaper());
    assert_eq!(children::daemon(), Some(me));

    let w = children::spawn(
        Kind::Wrapper("act_w"),
        || Command::new("true").spawn(),
        |c| Some(c.id()),
    )
    .unwrap();
    let wrapper = w.id();
    std::mem::forget(w);
    let mut owned_child = children::spawn(
        Kind::Owned,
        || Command::new("true").spawn(),
        |c| Some(c.id()),
    )
    .unwrap();
    let owned = owned_child.id();
    // An owned `sh` leaves a `sleep` behind, which comes here.
    let mut sh = children::spawn(
        Kind::Owned,
        || {
            Command::new("sh")
                .args(["-c", "sleep 0.2 & echo $!"])
                .stdout(Stdio::piped())
                .spawn()
        },
        |c| Some(c.id()),
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(sh.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(sh.wait().unwrap().success());
    let orphan: u32 = line.trim().parse().unwrap();
    for (what, pid) in [
        ("the wrapper", wrapper),
        ("the owned child", owned),
        ("the orphan", orphan),
    ] {
        wait_for(&format!("{what} to exit"), || {
            (stat(pid)? == ('Z', me)).then_some(())
        });
    }
    let before = children::census();
    assert_eq!(before.zombies, 3, "{before:?}");

    let swept = children::sweep();
    let reaped: Vec<(u32, &str, bool)> = swept
        .wrappers
        .iter()
        .map(|(p, j, s)| (*p, j.as_str(), s.success()))
        .collect();
    assert_eq!(reaped, [(wrapper, "act_w", true)]);
    let orphans: Vec<u32> = swept.orphans.iter().map(|(p, _)| *p).collect();
    assert_eq!(orphans, [orphan]);
    assert_eq!((stat(wrapper), stat(orphan)), (None, None));
    assert_eq!(
        stat(owned),
        Some(('Z', me)),
        "an owned child waits for its owner"
    );
    let after = children::census();
    assert_eq!(
        (after.zombies, after.reaped_wrappers, after.reaped_orphans),
        (1, 1, 1),
        "{after:?}"
    );
    assert!(owned_child.wait().unwrap().success(), "its owner's wait");
    children::sweep();
    assert_eq!(children::census().zombies, 0);

    // After an exec. A stand-in wrapper: `flock`, with the lock file named
    // for the mode word, has a wrapper's command line. And a plain `sleep`.
    // Neither is registered, as nothing is in a new image.
    let dir = tempfile::tempdir().unwrap();
    let old_wrapper = Command::new("flock")
        .current_dir(dir.path())
        .args([
            job::WRAPPER_MODE,
            "sh",
            "-c",
            "exec sleep 30",
            "--correlation-id",
            "act_old",
        ])
        .spawn()
        .unwrap()
        .id();
    let old_orphan = Command::new("sleep").arg("30").spawn().unwrap().id();
    let held = wait_for("flock to start its command", || {
        children_of(old_wrapper).first().copied()
    });
    assert_eq!(
        children::relearn(),
        Relearned {
            wrappers: 1,
            orphans: 1,
            zombies: 0
        }
    );
    let c = children::census();
    assert_eq!(
        (c.wrappers, c.orphans),
        (vec![(old_wrapper, "act_old".to_string())], 1)
    );
    // The job kills its wrapper: what the wrapper held comes here.
    kill(old_wrapper);
    wait_for("the held process to come here", || {
        (stat(held)?.1 == me).then_some(())
    });
    assert_eq!(children::census().orphans, 2);
    kill(held);
    kill(old_orphan);
    let (mut wrappers, mut orphans) = (vec![], vec![]);
    wait_for("the sweeps to reap all three", || {
        let s = children::sweep();
        wrappers.extend(s.wrappers.iter().map(|(p, j, st)| {
            use std::os::unix::process::ExitStatusExt;
            (*p, j.clone(), st.signal())
        }));
        orphans.extend(s.orphans.iter().map(|(p, _)| *p));
        (wrappers.len() + orphans.len() == 3).then_some(())
    });
    assert_eq!(
        wrappers,
        [(old_wrapper, "act_old".to_string(), Some(libc::SIGKILL))]
    );
    orphans.sort_unstable();
    let mut want = vec![held, old_orphan];
    want.sort_unstable();
    assert_eq!(orphans, want);
    let end = children::census();
    assert_eq!(
        (end.zombies, end.orphans, end.wrappers.len(), end.owned),
        (0, 0, 0, 0),
        "{end:?}"
    );
    assert_eq!((end.reaped_wrappers, end.reaped_orphans), (2, 3));
}
