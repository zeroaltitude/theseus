//! A job's process tree, found and stopped (M4 18a; design §2.3).
//!
//! A job's processes are its wrapper's descendants. The wrapper is a child
//! subreaper (theseus-6qy), so a descendant that a double fork or a `setsid`
//! orphans is reparented to it and stays in its tree, whatever process group
//! or session it moved to. The tree is found through each task's `children`
//! file, and `stop` ends it in three phases:
//!
//! 1. SIGTERM to every process of the tree, as a cancel's SIGTERM to the
//!    process group did before 18a, so a program that cleans up at SIGTERM
//!    (git's lock files) still does; then up to the grace for it to empty.
//! 2. The freeze: SIGSTOP to each process found, and to each new one a rescan
//!    finds, until a scan finds nothing new and every process found reads as
//!    stopped. A stopped process cannot fork, and a fork in flight when its
//!    SIGSTOP came has finished once it reads as stopped, so the set is then
//!    the whole tree, and stays so.
//! 3. SIGKILL to that set, and the reap, up to the kill's wait.
//!
//! Each process is signalled through a pidfd, opened for it and checked
//! against the start time the scan read, so a pid the kernel has since given
//! another process is never signalled. The verdict counts what the stop
//! ended, and names what it left alive: a process in uninterruptible sleep
//! (`D`) can outlast the kill's wait.
//!
//! What the tree cannot show is a process outside it acting for the job: a
//! user systemd unit, a tmux server already running, cron. At L0 a verdict's
//! scope is the wrapper's descendants (`scope: descendants`); L1 closes that
//! path, since its view has no session bus and no tmux socket.

use std::collections::{BTreeMap, BTreeSet};
use std::os::fd::{FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

/// How long the kill's phase waits for the frozen set to be gone.
pub const KILL_WAIT: Duration = Duration::from_millis(500);

/// The longest the freeze rescans before it kills what it has: a tree that
/// forks faster than it can be stopped is still killed, and what escaped
/// the freeze reads as a survivor.
const FREEZE_LIMIT: Duration = Duration::from_millis(500);

/// One process: its pid, and its start time (clock ticks after boot), which
/// tells it from a later process given the same pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Proc {
    pub pid: u32,
    pub start: u64,
}

/// What `/proc/<pid>/stat` says of a process: its state letter, its parent,
/// and its start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub state: char,
    pub ppid: u32,
    pub start: u64,
}

/// `/proc/<pid>/stat`, read. The comm may hold spaces and parentheses, so the
/// fields start after its last `)`.
pub fn stat(pid: u32) -> Option<Stat> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let f: Vec<&str> = s.get(s.rfind(')')? + 1..)?.split_whitespace().collect();
    Some(Stat {
        state: f.first()?.chars().next()?,
        ppid: f.get(1)?.parse().ok()?,
        start: f.get(19)?.parse().ok()?,
    })
}

/// A zombie, or a process being reaped: it runs no more.
fn dead(state: char) -> bool {
    matches!(state, 'Z' | 'X')
}

/// The children of `pid`, from each of its tasks' `children` files: a child
/// is listed under the thread that forked it, or the one the kernel chose
/// when it was reparented there. `None` when no file could be read.
fn children_of(pid: u32) -> Option<Vec<u32>> {
    let tasks = std::fs::read_dir(format!("/proc/{pid}/task")).ok()?;
    let mut out = Vec::new();
    let mut read = false;
    for t in tasks.flatten() {
        if let Ok(s) = std::fs::read_to_string(t.path().join("children")) {
            read = true;
            out.extend(s.split_whitespace().filter_map(|p| p.parse::<u32>().ok()));
        }
    }
    read.then_some(out)
}

/// Every process's parent, from a scan of `/proc`: for a kernel without the
/// `children` files.
fn parents() -> BTreeMap<u32, Vec<u32>> {
    let mut by_parent: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            if let Some(s) = stat(pid) {
                by_parent.entry(s.ppid).or_default().push(pid);
            }
        }
    }
    by_parent
}

/// The live descendants of `root`, not counting it: each process below it,
/// with its start time. A zombie is not live. One that exits while the tree
/// is read may be missed, and so may a child forked meanwhile: the stop
/// rescans until it finds nothing new.
pub fn descendants(root: u32) -> Vec<Proc> {
    let mut scan: Option<BTreeMap<u32, Vec<u32>>> = None;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut queue = vec![root];
    while let Some(p) = queue.pop() {
        let kids = match children_of(p) {
            Some(k) => k,
            None => scan
                .get_or_insert_with(parents)
                .get(&p)
                .cloned()
                .unwrap_or_default(),
        };
        for k in kids {
            if !seen.insert(k) {
                continue;
            }
            if let Some(s) = stat(k) {
                queue.push(k);
                if !dead(s.state) {
                    out.push(Proc {
                        pid: k,
                        start: s.start,
                    });
                }
            }
        }
    }
    out.sort_unstable();
    out
}

/// Whether `p` is still the same live process: its pid holds a process of
/// its start time that is not a zombie.
pub fn alive(p: Proc) -> bool {
    stat(p.pid).is_some_and(|s| s.start == p.start && !dead(s.state))
}

/// Signal `p`, and only `p`: through a pidfd opened for its pid and checked
/// against its start time, so a pid reused since the scan is left alone.
/// Whether the signal was sent.
pub fn signal(p: Proc, sig: libc::c_int) -> bool {
    // SAFETY: pidfd_open takes a pid and flags and returns a new descriptor,
    // which the OwnedFd below closes.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, p.pid as libc::pid_t, 0) };
    if fd < 0 {
        return false;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    // Opened before this check, the descriptor names the process the check
    // reads, or one that has since exited, which a signal cannot reach.
    if stat(p.pid).is_none_or(|s| s.start != p.start) {
        return false;
    }
    use std::os::fd::AsRawFd;
    // SAFETY: a signal sent through the descriptor; no memory is shared.
    let sent = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            sig,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    sent == 0
}

/// A stop's outcome: how many processes it ended, what it left alive, and
/// how long it took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stopped {
    /// Every process the stop met: signalled at SIGTERM, or frozen.
    pub killed: u32,
    /// The processes still alive after the kill's wait: pid and state.
    pub survivors: Vec<(u32, char)>,
    pub ms: u64,
}

impl Stopped {
    /// Why it is not verified, when a process is left: "1 process outlived
    /// the kill: pid 4242 (state D)".
    pub fn why(&self) -> Option<String> {
        if self.survivors.is_empty() {
            return None;
        }
        let list: Vec<String> = self
            .survivors
            .iter()
            .take(5)
            .map(|(pid, state)| format!("pid {pid} (state {state})"))
            .collect();
        Some(format!(
            "{} outlived the kill: {}",
            if self.survivors.len() == 1 {
                "1 process".to_string()
            } else {
                format!("{} processes", self.survivors.len())
            },
            list.join(", ")
        ))
    }
}

/// Stop every descendant of `root` (not `root` itself): SIGTERM, up to
/// `grace` for the tree to empty, the freeze, then SIGKILL and up to
/// `KILL_WAIT` for the frozen set to go. `reap` runs between looks: the
/// caller reaps the children it owns (a subreaper's, every one), so a dead
/// process is not counted live while it waits to be reaped.
pub fn stop(root: u32, grace: Duration, reap: &mut dyn FnMut()) -> Stopped {
    let t0 = Instant::now();
    let mut met: BTreeSet<Proc> = BTreeSet::new();
    // 1. SIGTERM, and the grace.
    let until = t0 + grace;
    loop {
        reap();
        let live = descendants(root);
        for p in &live {
            if met.insert(*p) {
                signal(*p, libc::SIGTERM);
            }
        }
        if live.is_empty() {
            return Stopped {
                killed: met.len() as u32,
                survivors: vec![],
                ms: t0.elapsed().as_millis() as u64,
            };
        }
        if Instant::now() >= until {
            break;
        }
        std::thread::sleep(Duration::from_millis(10).min(until - Instant::now()));
    }
    // 2. The freeze.
    let mut frozen: BTreeSet<Proc> = BTreeSet::new();
    let freeze_until = Instant::now() + FREEZE_LIMIT;
    loop {
        let live = descendants(root);
        let mut new = false;
        for p in &live {
            if frozen.insert(*p) {
                new = true;
                met.insert(*p);
                signal(*p, libc::SIGSTOP);
            }
        }
        let still = live
            .iter()
            .all(|p| stat(p.pid).is_none_or(|s| matches!(s.state, 'T' | 't') || dead(s.state)));
        if (!new && still) || Instant::now() >= freeze_until {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // 3. SIGKILL, and the reap.
    for p in &frozen {
        signal(*p, libc::SIGKILL);
    }
    let until = Instant::now() + KILL_WAIT;
    let survivors = loop {
        reap();
        let live = descendants(root);
        // One the freeze missed (forked as it ended) is killed too.
        for p in &live {
            if frozen.insert(*p) {
                met.insert(*p);
                signal(*p, libc::SIGKILL);
            }
        }
        if live.is_empty() {
            break vec![];
        }
        if Instant::now() >= until {
            break live
                .iter()
                .filter_map(|p| Some((p.pid, stat(p.pid)?.state)))
                .collect();
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    Stopped {
        killed: met.len() as u32,
        survivors,
        ms: t0.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This process reads as itself, with a start time, and its stat line's
    /// comm is skipped whatever it holds.
    #[test]
    fn a_process_reads_as_itself_and_a_reused_pid_does_not() {
        let me = std::process::id();
        let s = stat(me).unwrap();
        assert!(s.start > 0 && s.ppid > 0);
        let p = Proc {
            pid: me,
            start: s.start,
        };
        assert!(alive(p));
        assert!(
            !alive(Proc {
                start: s.start + 1,
                ..p
            }),
            "the same pid at another start time is another process"
        );
        assert!(
            !signal(
                Proc {
                    start: s.start + 1,
                    ..p
                },
                0
            ),
            "never signalled"
        );
        assert!(signal(p, 0), "signal 0 reaches it");
    }

    /// A child of this process is its descendant, with its start time, and
    /// is not once it has been reaped.
    #[test]
    fn a_child_is_a_descendant_until_it_is_reaped() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let found = descendants(std::process::id());
        let p = found
            .iter()
            .find(|p| p.pid == pid)
            .copied()
            .expect("the child is found");
        assert_eq!(Some(p.start), stat(pid).map(|s| s.start));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!descendants(std::process::id()).iter().any(|p| p.pid == pid));
        assert!(!alive(p));
    }

    #[test]
    fn a_survivor_names_its_pid_and_state() {
        let s = Stopped {
            killed: 3,
            survivors: vec![(4242, 'D')],
            ms: 600,
        };
        assert_eq!(
            s.why().as_deref(),
            Some("1 process outlived the kill: pid 4242 (state D)")
        );
        assert_eq!(Stopped::default().why(), None);
    }
}
