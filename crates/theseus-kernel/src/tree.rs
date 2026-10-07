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
//!    (git's lock files) still does; then up to the grace for it to empty,
//!    woken by each signalled process's exit (its pidfd).
//! 2. The freeze: SIGSTOP to each process found, and to each new one a rescan
//!    finds, until a scan finds nothing new and every process found reads as
//!    stopped. A stopped process cannot fork, and a fork in flight when its
//!    SIGSTOP came has finished once it reads as stopped, so the set is then
//!    the whole tree, and stays so.
//! 3. SIGKILL to that set, and the reap, until each process killed has
//!    exited (its pidfd polled), up to the kill's wait.
//!
//! No phase ends while a process it signalled still runs, or while the
//! caller's reap says a child is left (theseus-g11i). A `children` file is
//! reliable only while the children are stopped (proc(5)): a scan taken as
//! one exits and its children move to the subreaper can miss a live one, so
//! a scan that finds nothing ends a phase only when the reap agrees. In a
//! wrapper, a subreaper, `waitpid` answering ECHILD after the reap means no
//! descendant is left at all ([`Left::None`]). And a process with SIGKILL
//! pending still runs until the scheduler lets it die: on a loaded machine a
//! starved one outlived the old 500 ms wait with its command line whole, and
//! the completion went out beside it.
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

/// How long the kill's phase waits for the frozen set to be gone: far past
/// the moment a SIGKILLed process dies on an idle machine, since a starved
/// one dies only once it runs (theseus-g11i; it was 500 ms).
pub const KILL_WAIT: Duration = Duration::from_secs(2);

/// The longest the freeze rescans before it kills what it has: a tree that
/// forks faster than it can be stopped is still killed, and what escaped
/// the freeze reads as a survivor.
const FREEZE_LIMIT: Duration = Duration::from_millis(500);

/// The longest a wait on the tree's pidfds goes before the next scan: the
/// bound for a child no scan has found yet, whose exit no pidfd can tell.
const LOOK: Duration = Duration::from_millis(10);

/// The most pidfds the SIGTERM phase keeps to wake its wait at an exit. A
/// process past them is signalled as before, its pidfd closed at once, and
/// its end seen at the next look, so a large tree never takes the
/// descriptors the freeze and the kill need: a wrapper's open-file limit is
/// the daemon's (1,024 by default).
const WATCHED: usize = 64;

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

/// What the caller knows of the stop's root's children once it has reaped
/// the ones that exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// None at all, live or dead: a subreaper's `waitpid` answered ECHILD.
    None,
    /// At least one: live, or exited since the reap.
    Some,
    /// It cannot tell: the scan alone decides.
    Unknown,
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
    pidfd(p).is_some_and(|fd| send(&fd, sig))
}

/// A pidfd for `p`, checked against its start time: `None` when its pid
/// holds no process, or another one.
fn pidfd(p: Proc) -> Option<OwnedFd> {
    let fd = open_pidfd(p.pid)?;
    // Opened before this check, the descriptor names the process the check
    // reads, or one that has since exited, which a signal cannot reach.
    stat(p.pid)
        .is_some_and(|s| s.start == p.start)
        .then_some(fd)
}

/// A pidfd for whatever process `pid` holds now, unchecked: its caller
/// checks, once it is open, that the process is the one it means.
pub(crate) fn open_pidfd(pid: u32) -> Option<OwnedFd> {
    // SAFETY: pidfd_open takes a pid and flags and returns a new descriptor,
    // which the OwnedFd below closes.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    if fd < 0 {
        return None;
    }
    Some(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

/// `sig` through a pidfd: whether it was sent.
fn send(fd: &OwnedFd, sig: libc::c_int) -> bool {
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

/// SIGTERM to `p`, through a pidfd checked against its start time, kept in
/// `termed` to wake the grace's wait at its exit while fewer than `WATCHED`
/// are; past them, closed at once, and its end seen at the next look.
fn term(p: Proc, termed: &mut Vec<(Proc, OwnedFd)>) {
    if let Some(fd) = pidfd(p) {
        send(&fd, libc::SIGTERM);
        if termed.len() < WATCHED {
            termed.push((p, fd));
        }
    }
}

/// A stop's outcome: how many processes it ended, what it left alive, and
/// how long it took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stopped {
    /// Every process the stop met: signalled at SIGTERM, or frozen.
    pub killed: u32,
    /// The processes still alive after the kill's wait: pid and state.
    pub survivors: Vec<(u32, char)>,
    /// The reap still found a child at the kill's wait's end that no scan
    /// found: a process left whose pid the stop does not know.
    pub unseen: bool,
    pub ms: u64,
}

impl Stopped {
    /// How many it left: each survivor, and one more for an unseen child.
    pub fn left(&self) -> u32 {
        self.survivors.len() as u32 + u32::from(self.unseen)
    }

    /// Why it is not verified, when a process is left: "1 process outlived
    /// the kill: pid 4242 (state D)".
    pub fn why(&self) -> Option<String> {
        if self.survivors.is_empty() && !self.unseen {
            return None;
        }
        let mut list: Vec<String> = self
            .survivors
            .iter()
            .take(5)
            .map(|(pid, state)| format!("pid {pid} (state {state})"))
            .collect();
        if self.unseen {
            list.push("a child no scan found".into());
        }
        Some(format!(
            "{} outlived the kill: {}",
            if self.left() == 1 {
                "1 process".to_string()
            } else {
                format!("{} processes", self.left())
            },
            list.join(", ")
        ))
    }
}

/// Stop every descendant of `root` (not `root` itself): SIGTERM, up to
/// `grace` for the tree to empty, the freeze, then SIGKILL and up to
/// `KILL_WAIT` for each killed process to exit. `reap` runs between looks:
/// the caller reaps the children it owns (a subreaper's, every one), so a
/// dead process is not counted live while it waits to be reaped, and says
/// what it knows is left ([`Left`]): no phase ends on an empty scan while it
/// says a child is.
pub fn stop(root: u32, grace: Duration, reap: &mut dyn FnMut() -> Left) -> Stopped {
    stop_with(root, grace, reap, &mut descendants)
}

/// `stop`, with the scan that finds the tree: `descendants`, or a test's.
fn stop_with(
    root: u32,
    grace: Duration,
    reap: &mut dyn FnMut() -> Left,
    scan: &mut dyn FnMut(u32) -> Vec<Proc>,
) -> Stopped {
    let t0 = Instant::now();
    let mut met: BTreeSet<Proc> = BTreeSet::new();
    // 1. SIGTERM, and the grace: asleep on the signalled processes' pidfds
    // (theseus-dwoj), woken by the first of them to exit, as the kill's
    // wait is, to see the tree empty at once. A process a scan finds new is
    // signalled only at a look, every `LOOK`, as before: a scan an exit woke
    // comes just as a process that cleans up at SIGTERM forks its cleanup
    // (a shell's trap runs once the child it waited on is gone), and a
    // SIGTERM to that would cut the cleanup short. A child no scan has seen
    // yet is found at the next look, at most `LOOK` later.
    let until = t0 + grace;
    let mut termed: Vec<(Proc, OwnedFd)> = Vec::new();
    let mut look = t0;
    loop {
        let left = reap();
        let live = scan(root);
        let now = Instant::now();
        if now >= look {
            look = now + LOOK;
            for p in &live {
                if met.insert(*p) {
                    term(*p, &mut termed);
                }
            }
        }
        if live.is_empty() && left != Left::Some {
            return Stopped {
                killed: met.len() as u32,
                ms: t0.elapsed().as_millis() as u64,
                ..Stopped::default()
            };
        }
        if now >= until {
            break;
        }
        termed.retain(|(_, fd)| !exited(fd));
        wait_exit(&termed, (look - now).min(until - now));
    }
    // 2. The freeze. The grace's pidfds are closed first: the freeze and
    // the kill open their own, one for each process they signal.
    drop(termed);
    let mut frozen: BTreeSet<Proc> = BTreeSet::new();
    let freeze_until = Instant::now() + FREEZE_LIMIT;
    loop {
        let left = reap();
        let live = scan(root);
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
        let whole = !(live.is_empty() && left == Left::Some);
        if (!new && still && whole) || Instant::now() >= freeze_until {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // 3. SIGKILL, and the wait for each killed process to exit.
    fn kill(p: Proc, killed: &mut Vec<(Proc, OwnedFd)>) {
        if let Some(fd) = pidfd(p) {
            send(&fd, libc::SIGKILL);
            killed.push((p, fd));
        }
    }
    let mut killed: Vec<(Proc, OwnedFd)> = Vec::new();
    for p in &frozen {
        kill(*p, &mut killed);
    }
    let until = Instant::now() + KILL_WAIT;
    let (survivors, unseen) = loop {
        let left = reap();
        let live = scan(root);
        // One the freeze missed (forked as it ended) is killed too.
        for p in &live {
            if frozen.insert(*p) {
                met.insert(*p);
                kill(*p, &mut killed);
            }
        }
        killed.retain(|(_, fd)| !exited(fd));
        if live.is_empty() && killed.is_empty() && left != Left::Some {
            break (vec![], false);
        }
        let now = Instant::now();
        if now >= until {
            let mut left_alive: BTreeSet<Proc> = live.into_iter().collect();
            left_alive.extend(killed.iter().map(|(p, _)| *p));
            let survivors: Vec<(u32, char)> = left_alive
                .iter()
                .filter_map(|p| Some((p.pid, stat(p.pid)?.state)))
                .collect();
            let unseen = survivors.is_empty() && left == Left::Some;
            break (survivors, unseen);
        }
        // Woken by the first of them to exit; a reap's zombie or a new
        // child is seen at the next look.
        wait_exit(&killed, (until - now).min(LOOK));
    };
    Stopped {
        killed: met.len() as u32,
        survivors,
        unseen,
        ms: t0.elapsed().as_millis() as u64,
    }
}

/// Whether the process behind a pidfd has exited: its pidfd polls readable.
pub(crate) fn exited(fd: &OwnedFd) -> bool {
    use std::os::fd::AsRawFd;
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd, for the call's length; no wait.
    unsafe { libc::poll(&mut p, 1, 0) > 0 }
}

/// Up to `limit` for any of `killed` to exit, asleep on their pidfds.
fn wait_exit(killed: &[(Proc, OwnedFd)], limit: Duration) {
    wait_any(killed.iter().map(|(_, fd)| fd), limit);
}

/// Up to `limit` (a millisecond at least, a second at most) for any of
/// these pidfds' processes to exit; a sleep of `limit` when there is none.
pub(crate) fn wait_any<'a>(pidfds: impl Iterator<Item = &'a OwnedFd>, limit: Duration) {
    use std::os::fd::AsRawFd;
    let mut fds: Vec<libc::pollfd> = pidfds
        .map(|fd| libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let ms = limit.as_millis().clamp(1, 1_000) as i32;
    if fds.is_empty() {
        std::thread::sleep(Duration::from_millis(ms as u64));
        return;
    }
    // SAFETY: the pollfds live for the call's length.
    unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
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

    /// theseus-g11i, reading (b): a scan can miss a live child (a `children`
    /// file read as a parent exits and its children move). The scan here
    /// misses this process's `sleep` child on its first three reads, as one
    /// moving to the subreaper would be missed; the reap, a subreaper's
    /// `waitpid` here asked of that child alone, says it is left. The stop
    /// does not end on the empty scan: it finds the child, ends it, and only
    /// then says none is left. The old stop said so at once, the child alive.
    #[test]
    fn a_scan_that_misses_a_live_child_does_not_end_the_stop() {
        #[expect(clippy::zombie_processes, reason = "the reap below reaps it")]
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let p = Proc {
            pid,
            start: stat(pid).unwrap().start,
        };
        let mut misses = 3;
        let mut scan = |root: u32| -> Vec<Proc> {
            let mine: Vec<Proc> = descendants(root)
                .into_iter()
                .filter(|q| q.pid == pid)
                .collect();
            if misses > 0 {
                misses -= 1;
                return vec![];
            }
            mine
        };
        let mut reap = || {
            let mut status = 0;
            match unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) } {
                0 => Left::Some,
                _ => Left::None,
            }
        };
        let s = stop_with(
            std::process::id(),
            Duration::from_secs(5),
            &mut reap,
            &mut scan,
        );
        let gone = !alive(p);
        if !gone {
            signal(p, libc::SIGKILL);
        }
        assert!(gone, "the stop ended while its child ran: {s:?}");
        assert_eq!((s.killed, s.left()), (1, 0), "{s:?}");
        assert!(
            s.ms < 3_000,
            "ended by the child's exit, not the grace: {s:?}"
        );
    }

    /// A child the reap still counts, which no scan ever finds, is left
    /// unverified at the kill's wait's end, never called gone.
    #[test]
    fn a_child_no_scan_finds_is_a_survivor_not_a_verified_stop() {
        let mut reap = || Left::Some;
        let s = stop_with(u32::MAX, Duration::ZERO, &mut reap, &mut |_| vec![]);
        assert!(s.unseen && s.survivors.is_empty(), "{s:?}");
        assert_eq!(
            s.why().as_deref(),
            Some("1 process outlived the kill: a child no scan found")
        );
    }

    #[test]
    fn a_survivor_names_its_pid_and_state() {
        let s = Stopped {
            killed: 3,
            survivors: vec![(4242, 'D')],
            unseen: false,
            ms: 600,
        };
        assert_eq!(
            s.why().as_deref(),
            Some("1 process outlived the kill: pid 4242 (state D)")
        );
        assert_eq!(Stopped::default().why(), None);
    }
}
