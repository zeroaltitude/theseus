//! The door's fetch is bounded (theseus-6vlr4): a remote that never answers
//! fails the fetch at its bound with git's whole process group killed and
//! reaped, and holds up no close of another tree. A stand-in `git` ahead of
//! the real one hangs on `fetch`, so none of it needs a network.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::join::git as jgit;
use super::tests_tree::fixture;
use super::tree::{self, Place};

const NOW: u64 = 1_792_000_000_000;

/// A `git` that hangs on `fetch` (and records its pid and its `sleep`'s) and
/// is the real one for everything else.
pub(crate) struct Hang {
    /// The place's environment with the stand-in first on `PATH` and the
    /// door's bound set to `limit`.
    pub(crate) env: Vec<(OsString, OsString)>,
    pids: PathBuf,
    _dir: tempfile::TempDir,
}

pub(crate) fn hanging_git(base: Vec<(OsString, OsString)>, limit: Duration) -> Hang {
    let dir = crate::scratch::tempdir();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let pids = dir.path().join("pids");
    let real = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real = String::from_utf8_lossy(&real.stdout).trim().to_string();
    let script = format!(
        "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = fetch ]; then\n    echo $$ >> {p}\n    sleep 600 &\n    echo $! >> {p}\n    wait\n    exit 0\n  fi\ndone\nexec {real} \"$@\"\n",
        p = pids.display()
    );
    let path = bin.join("git");
    std::fs::write(&path, script).unwrap();
    let mut perm = std::fs::metadata(&path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    std::fs::set_permissions(&path, perm).unwrap();
    let mut env = base;
    env.push((
        "PATH".into(),
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()).into(),
    ));
    env.push((
        "THESEUS_SELF_FETCH_LIMIT_MS".into(),
        limit.as_millis().to_string().into(),
    ));
    Hang {
        env,
        pids,
        _dir: dir,
    }
}

impl Hang {
    /// The pids the stand-in recorded: its shell, then its `sleep`.
    pub(crate) fn pids(&self) -> Vec<u32> {
        std::fs::read_to_string(&self.pids)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect()
    }

    /// Both are recorded: the fetch is in flight.
    pub(crate) fn hanging(&self) -> bool {
        self.pids().len() == 2
    }

    /// Wait for the fetch to be in flight.
    pub(crate) fn wait_hanging(&self) {
        let by = Instant::now() + Duration::from_secs(30);
        while !self.hanging() {
            assert!(Instant::now() < by, "the fetch never started");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// No process of the stand-in's is left running (a zombie its reaper has
    /// not yet collected is not running). Waits up to 5 s for the kernel.
    pub(crate) fn nothing_left(&self) -> Result<(), String> {
        let by = Instant::now() + Duration::from_secs(5);
        loop {
            let alive: Vec<u32> = self
                .pids()
                .into_iter()
                .filter(|p| {
                    std::fs::read_to_string(format!("/proc/{p}/stat"))
                        .ok()
                        .and_then(|s| {
                            s.rsplit_once(')')
                                .and_then(|(_, r)| r.trim().chars().next())
                        })
                        .is_some_and(|state| state != 'Z')
                })
                .collect();
            if alive.is_empty() {
                return Ok(());
            }
            if Instant::now() >= by {
                return Err(format!("still running after the bound: {alive:?}"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn hanging_place(base: &Place, limit: Duration) -> (Place, Hang) {
    let h = hanging_git(base.env.clone(), limit);
    let mut p = base.clone();
    p.env = h.env.clone();
    (p, h)
}

/// KEY: the bound fires, the fetch fails saying so, and the stand-in's shell
/// and the `sleep` it started are both gone: the group was killed and reaped.
/// The fetch runs on a worker thread, so a bound that is gone fails here on a
/// message, not on the test runner's kill; and the repository is fetchable
/// after (no lock file of the killed fetch is left behind).
#[test]
fn a_hung_fetch_fails_at_its_bound_and_leaves_no_git_behind() {
    let f = fixture();
    let (p, h) = hanging_place(&f.place, Duration::from_secs(60));
    let bound = Duration::from_millis(400);
    let t = Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(jgit::fetch_within(&p, "origin", bound));
    });
    let e = rx
        .recv_timeout(bound * 10 + Duration::from_secs(1))
        .unwrap_or_else(|_| {
            panic!(
                "the fetch was still running {:?} after its {bound:?} bound",
                t.elapsed()
            )
        })
        .unwrap_err();
    let took = t.elapsed();
    assert!(
        e.to_string().contains("did not finish") && e.to_string().contains("killed"),
        "{e:#}"
    );
    assert!(took >= bound, "{took:?}");
    assert_eq!(h.pids().len(), 2, "the fetch ran: {:?}", h.pids());
    h.nothing_left().unwrap();
    // The place without the stand-in: a real fetch goes through.
    jgit::fetch_within(&f.place, "origin", Duration::from_secs(60))
        .expect("the repository is fetchable after a killed fetch");
}

/// A fetch that answers inside its bound is the fetch it was.
#[test]
fn a_fetch_that_answers_inside_its_bound_succeeds() {
    let f = fixture();
    jgit::fetch_within(&f.place, "origin", Duration::from_secs(60)).unwrap();
    assert!(jgit::fetch_within(&f.place, "no-such-remote", Duration::from_secs(60)).is_err());
}

/// KEY: while an open waits on a hung fetch, a close of another tree is not
/// held behind it (the fetch is outside the tree lock), and the open then
/// goes on, from the base it has.
#[test]
fn a_hung_fetch_holds_no_close() {
    let f = fixture();
    let other = jgit::detached(&f.place, "probe", "self/probe", "origin/main").unwrap();
    let (p, h) = hanging_place(&f.place, Duration::from_secs(4));
    let opener = std::thread::spawn(move || tree::open(&p, "stuck", NOW));
    h.wait_hanging();
    let t = Instant::now();
    tree::close(&f.place, &other).unwrap();
    assert!(
        !opener.is_finished(),
        "the open was still waiting on its fetch when the close ended ({:?})",
        t.elapsed()
    );
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    let opened = opener.join().unwrap().expect("the door goes on");
    assert!(opened.dir.is_dir());
    h.nothing_left().unwrap();
}
