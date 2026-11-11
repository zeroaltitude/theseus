//! A program on a pty (theseus-n88g.4): opened with libc's `posix_openpt`,
//! its child started through `children::spawn` as every child the daemon
//! starts is, in a session and process group of its own (`setsid`), with
//! the pty as its controlling terminal, so a close reaches the program and
//! everything it started.
//!
//! A thread per terminal reads the pty's master and feeds the screen. It
//! blocks in `read` (never on a runtime worker) and ends when the pty's
//! last slave closes, which is when the program and every child that held
//! the terminal have gone.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_kernel::children;
use theseus_kernel::tree;

use super::vt::Screen;

/// What the reader thread and the tools share.
pub struct Shared {
    pub screen: Mutex<Screen>,
    /// Bumped at every chunk of output: a read that waits watches it.
    pub version: tokio::sync::watch::Sender<u64>,
    /// The pty's last slave has closed: nothing more will be written.
    pub hung_up: AtomicBool,
    /// When output last came, in ms since the epoch.
    pub last_output_ms: AtomicU64,
    /// Bytes the program wrote, and bytes sent to it.
    pub bytes_out: AtomicU64,
    pub bytes_in: AtomicU64,
    /// The reader has a chunk it took off the master and has not yet put on
    /// the screen: output the program wrote that no look can see yet.
    pub feeding: AtomicBool,
}

/// A running program and its pty's master.
pub struct Pty {
    pub shared: Arc<Shared>,
    master: Mutex<File>,
    child: Mutex<Child>,
    pid: u32,
    /// The pty's slave (`/dev/pts/N`): a process that still holds it open
    /// at a close is the terminal's, whatever tree it left.
    slave_path: std::path::PathBuf,
}

/// What a pty's program is started with.
pub struct Spawn<'a> {
    pub argv: &'a [String],
    pub cwd: &'a Path,
    pub env: &'a [(String, String)],
    pub rows: u16,
    pub cols: u16,
    pub umask: Option<u32>,
}

fn last_error(what: &str) -> std::io::Error {
    let e = std::io::Error::last_os_error();
    std::io::Error::new(e.kind(), format!("{what}: {e}"))
}

/// A new pty pair: the master, and the slave's open file.
fn open_pair(rows: u16, cols: u16) -> std::io::Result<(OwnedFd, File, std::path::PathBuf)> {
    // SAFETY: plain libc calls on a descriptor this function owns; each
    // result is checked before it is used.
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        if m < 0 {
            return Err(last_error("posix_openpt"));
        }
        let master = OwnedFd::from_raw_fd(m);
        if libc::grantpt(m) != 0 {
            return Err(last_error("grantpt"));
        }
        if libc::unlockpt(m) != 0 {
            return Err(last_error("unlockpt"));
        }
        let mut name = [0 as libc::c_char; 128];
        if libc::ptsname_r(m, name.as_mut_ptr(), name.len()) != 0 {
            return Err(last_error("ptsname_r"));
        }
        let s = libc::open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        );
        if s < 0 {
            return Err(last_error("opening the pty's slave"));
        }
        let slave = File::from_raw_fd(s);
        let path = std::ffi::CStr::from_ptr(name.as_ptr())
            .to_string_lossy()
            .into_owned();
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if libc::ioctl(m, libc::TIOCSWINSZ, &size) != 0 {
            return Err(last_error("setting the pty's size"));
        }
        Ok((master, slave, path.into()))
    }
}

impl Pty {
    /// Start `argv` on a new pty, and its reader thread.
    pub fn spawn(s: &Spawn<'_>) -> std::io::Result<Pty> {
        let program = s
            .argv
            .first()
            .ok_or_else(|| std::io::Error::other("no program to run"))?;
        let (master, slave, slave_path) = open_pair(s.rows, s.cols)?;
        let mut cmd = Command::new(program);
        cmd.args(&s.argv[1..])
            .current_dir(s.cwd)
            .env_clear()
            .envs(s.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        let umask = s.umask;
        // SAFETY: setsid, ioctl, and umask are async-signal-safe, and touch
        // only the child.
        unsafe {
            cmd.pre_exec(move || {
                // Its own session and process group, so a close reaches the
                // whole group, and the pty is its controlling terminal, so
                // Ctrl-C and a hang-up reach the program in front.
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some(u) = umask {
                    libc::umask(u);
                }
                Ok(())
            });
        }
        // Registered as an owned child as it is spawned: its spawner (the
        // close) waits for it, and the daemon's sweep leaves it alone.
        let child = children::spawn(children::Kind::Owned, || cmd.spawn(), |c| Some(c.id()))?;
        drop(cmd);
        let pid = child.id();
        let reader = File::from(master.try_clone()?);
        let writer = File::from(master);
        let shared = Arc::new(Shared {
            screen: Mutex::new(Screen::new(s.rows, s.cols)),
            version: tokio::sync::watch::Sender::new(0),
            hung_up: AtomicBool::new(false),
            last_output_ms: AtomicU64::new(theseus_protocol::now_unix_ms()),
            bytes_out: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            feeding: AtomicBool::new(false),
        });
        let replies = writer.try_clone()?;
        let feed = shared.clone();
        std::thread::Builder::new()
            .name(format!("term-{pid}"))
            .spawn(move || read_loop(reader, replies, &feed))?;
        Ok(Pty {
            shared,
            master: Mutex::new(writer),
            child: Mutex::new(child),
            pid,
            slave_path,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Send bytes to the program, as typed keys.
    pub fn send(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut m = self.master.lock().unwrap_or_else(|p| p.into_inner());
        m.write_all(bytes)?;
        self.shared
            .bytes_in
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    /// How the program ended, once it has; it is reaped then.
    pub fn exited(&self) -> Option<std::process::ExitStatus> {
        let mut c = self.child.lock().unwrap_or_else(|p| p.into_inner());
        c.try_wait().ok().flatten()
    }

    /// The pty's foreground process group, as the kernel holds it:
    /// `tcgetpgrp` on the master. None once its session has ended.
    pub fn foreground(&self) -> Option<u32> {
        use std::os::fd::AsRawFd;
        let m = self.master.lock().unwrap_or_else(|p| p.into_inner());
        // SAFETY: an ioctl that reads the slave's process group into an int.
        let g = unsafe { libc::tcgetpgrp(m.as_raw_fd()) };
        (g > 0).then_some(g as u32)
    }

    /// Whether the program is back in front of its pty (theseus-ggqf): a
    /// shell at its prompt, a REPL waiting for its next line. Its process
    /// group is its pid (`setsid`). In front is not enough: a shell between
    /// the commands of a list (`a; b`, `make && make test`) holds the front
    /// for a moment, and a REPL computing never gives it up; so it must also
    /// be waiting for input (`waits_for_input`), where that can be read.
    pub fn idle(&self) -> bool {
        self.foreground() == Some(self.pid)
            && waits_for_input(self.pid).unwrap_or(true)
            && self.drained()
    }

    /// Everything the program wrote is on the screen: nothing unread in the
    /// master, and no chunk the reader holds. A program that prints and goes
    /// back to its read is waiting for input before the reader thread, on a
    /// loaded machine, has seen what it printed; a look then would read
    /// "idle" with the output missing from the screen. Looked at after the
    /// program's own state, so output written before it began to wait is
    /// counted.
    ///
    /// The window is narrower, not closed: between the kernel handing the
    /// reader its bytes and the `feeding` store below, both halves of this
    /// read clear. Closing it needs the reader to publish a monotone
    /// counter, or an idle confirmed on two looks, which costs latency
    /// against the `< 100 ms` bounds in `tests_idle.rs` (theseus-o1jnt,
    /// OWNER CALL).
    fn drained(&self) -> bool {
        use std::os::fd::AsRawFd;
        let m = self.master.lock().unwrap_or_else(|p| p.into_inner());
        let mut unread: libc::c_int = 0;
        // SAFETY: an ioctl that reads the count of unread bytes into an int.
        let r = unsafe { libc::ioctl(m.as_raw_fd(), libc::FIONREAD, &mut unread) };
        (r != 0 || unread == 0) && !self.shared.feeding.load(Ordering::SeqCst)
    }

    /// Stop the program and everything it started: a hang-up and SIGTERM to
    /// its process group and to each of its descendants, up to `grace` for
    /// them to go, then SIGKILL to what is left, and the program reaped. It
    /// blocks for up to the grace and the kill's wait, so it runs off the
    /// runtime's workers. With `keep_background` (a session's end, the
    /// daemon's stop: theseus-ggqf), only the program and the pty's
    /// foreground process group are ended, and what else ran is left
    /// (`Ended::left`; `close_keeping`).
    pub fn close(&self, grace: Duration, keep_background: bool) -> Ended {
        let mut c = self.child.lock().unwrap_or_else(|p| p.into_inner());
        if let Ok(Some(status)) = c.try_wait() {
            // The program is gone; what it left in its group still goes, but
            // only while something in that group holds our pty: once the group
            // empties its id may be another process's (`after_exit` says why
            // it reads nothing of that group).
            //
            // The accepted cost, named so it is not rediscovered as a bug
            // (theseus-3i6d1): when nothing in that group holds the pty, a
            // leftover still in it is spared, where the unconditional kill
            // this guard replaced would have SIGKILLed it. Nothing else
            // reaches it either, since `after_exit` filters the holders it
            // reads to `pgid != self.pid`. That is the specified design and
            // not a regression: the only cheaper predicate, `group_lives`,
            // passes on a recycled group id, so trusting it would sometimes
            // SIGKILL a group that is now another session's -- a stranger's
            // processes for a leftover of ours. A guard that can kill the
            // wrong group is worse than one that can spare the right one.
            //
            // `keep_background` carries that same hazard into the program's
            // own group: a keeping close never SIGKILLs it either, and
            // `after_exit` names what is found there instead of signalling
            // it (B6 L186 part (a), the owner's decision, OWNER-ANSWERS-
            // 20261110-1406.md S2-2, 2026-11-10 22:31:37 MST).
            if self.group_still_ours() && !keep_background {
                signal_group(self.pid, libc::SIGKILL);
            }
            return self.after_exit(status, keep_background);
        }
        // Its descendants as they are now: one that left the process group
        // (a `setsid` of its own) is still signalled by its pid.
        let mut procs = tree::descendants(self.pid);
        procs.extend(holders(&self.slave_path));
        // A process both below the program and holding its pty, once.
        procs.sort_unstable();
        procs.dedup();
        if let Some(st) = tree::stat(self.pid) {
            procs.push(tree::Proc {
                pid: self.pid,
                start: st.start,
            });
        }
        if keep_background {
            let groups = self.front_groups();
            let back: Vec<tree::Proc> = procs
                .iter()
                .copied()
                .filter(|p| pgid(p.pid).is_some_and(|g| !groups.contains(&g)))
                .collect();
            // Nothing in the background: the close is today's, no slower.
            if !back.is_empty() {
                return self.close_keeping(&mut c, grace, &groups, &back);
            }
        }
        // Every group met, the foreground's included, is signalled whole, so
        // a process forked into one after the scan is not spared; one that
        // made a session of its own since is in no group, and is found by the
        // scan after the groups' kill, repeated (theseus-z0kk).
        let groups = self.groups_of(&procs);
        for sig in [libc::SIGHUP, libc::SIGTERM] {
            for &g in &groups {
                signal_group(g, sig);
            }
            for p in &procs {
                tree::signal(*p, sig);
            }
        }
        let t0 = Instant::now();
        while t0.elapsed() < grace {
            if matches!(c.try_wait(), Ok(Some(_))) && procs.iter().all(|p| !tree::alive(*p)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // The groups go first: the foreground's forker dies, so what is
        // scanned next is no longer growing. Scanned before, a child it
        // forked into a session of its own between the scan and the kill was
        // in no group and no list (theseus-z0kk). A round of the scan finds
        // what the last kill's victims forked before they died; it repeats
        // until one finds nothing alive.
        for &g in &groups {
            signal_group(g, libc::SIGKILL);
        }
        let killed = self.kill_rounds(&procs, &groups, Some(self.pid));
        Ended {
            status: c.wait().ok(),
            killed,
            left: Vec::new(),
        }
    }

    /// The kill's rounds: each scans what is alive among `procs`, below
    /// `below` (when given) and holding the pty, SIGKILLs each, signals
    /// `groups` again (a new group may have formed meanwhile), and settles
    /// 5 ms; the first round that finds nothing alive ends it. A round finds
    /// what the last kill's victims forked before they died (theseus-z0kk).
    /// Returns how many distinct processes were signalled (`count_new`).
    fn kill_rounds(&self, procs: &[tree::Proc], groups: &[u32], below: Option<u32>) -> usize {
        let mut seen = std::collections::BTreeSet::new();
        let mut killed = 0;
        for _ in 0..KILL_ROUNDS {
            let late: Vec<tree::Proc> = procs
                .iter()
                .copied()
                .chain(below.into_iter().flat_map(tree::descendants))
                .chain(holders(&self.slave_path))
                .filter(|p| tree::alive(*p))
                .collect();
            if late.is_empty() {
                break;
            }
            killed += count_new(
                &mut seen,
                late.into_iter().filter(|p| tree::signal(*p, libc::SIGKILL)),
            );
            for &g in groups {
                signal_group(g, libc::SIGKILL);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        killed
    }

    /// How many holders of this pty's slave are in the program's own group.
    /// A program that was reaped leads no group unless a leftover of it still
    /// holds the pty, the discriminator `after_exit` filters out.
    pub(crate) fn holders_in_group(&self) -> usize {
        holders(&self.slave_path)
            .iter()
            .filter(|p| pgid(p.pid) == Some(self.pid))
            .count()
    }

    /// The program's group is still this terminal's: a holder of the pty is in
    /// it. Not "any member": a recycled group id passes `group_lives`.
    pub(crate) fn group_still_ours(&self) -> bool {
        self.holders_in_group() > 0
    }

    /// A close of a program that had already exited (theseus-d1zw). Its jobs
    /// are no longer below it (they were reparented), so the processes still
    /// holding its pty are what is found, the way a close finds a process
    /// that left the tree. Kept, they are named as a live shell's are; else
    /// they end, as every close that keeps nothing ends them.
    fn after_exit(&self, status: std::process::ExitStatus, keep_background: bool) -> Ended {
        if keep_background {
            // A keeping close names every holder it finds, the program's own
            // (former) group included: no SIGKILL of a group whose id may
            // already be a stranger's (B6 L186 part (a), the owner's
            // decision above `close`'s guard).
            let left = holders(&self.slave_path)
                .into_iter()
                .map(|p| Kept {
                    proc: p,
                    program: comm(p.pid),
                    why: match pgid(p.pid) == Some(self.pid) {
                        true => WHY_OWN_GROUP,
                        false => match sid_of(p.pid) == Some(self.pid) {
                            true => WHY_BACKGROUND,
                            false => WHY_SESSION,
                        },
                    },
                })
                .collect();
            return Ended {
                status: Some(status),
                killed: 0,
                left,
            };
        }
        // The program was reaped, so its pid may be recycled: its own group
        // and its descendants are not read, only what holds the pty, by
        // processes (`tree::signal` checks the start time) and by the groups
        // those were in, repeated as a live close's kill is (theseus-z0kk).
        // The program's own former group is excluded here: a close that
        // keeps nothing already SIGKILLed it above, while it still owned it
        // (`group_still_ours`), before the pid could be recycled.
        let held: Vec<tree::Proc> = holders(&self.slave_path)
            .into_iter()
            .filter(|p| pgid(p.pid).is_some_and(|g| g != self.pid))
            .collect();
        // SAFETY: reads this process's own group id.
        let mine = unsafe { libc::getpgrp() } as u32;
        let mut groups: Vec<u32> = held.iter().filter_map(|p| pgid(p.pid)).collect();
        groups.sort_unstable();
        groups.dedup();
        groups.retain(|&g| g > 1 && g != mine);
        let killed = self.kill_rounds(&held, &groups, None);
        Ended {
            status: Some(status),
            killed,
            left: Vec::new(),
        }
    }

    /// The process groups a close that ends everything signals whole: the
    /// program's, the pty's foreground group, and each group of `procs`,
    /// never this process's own.
    fn groups_of(&self, procs: &[tree::Proc]) -> Vec<u32> {
        // SAFETY: reads this process's own group id.
        let mine = unsafe { libc::getpgrp() } as u32;
        let mut g = self.front_groups();
        g.extend(procs.iter().filter_map(|p| pgid(p.pid)));
        g.sort_unstable();
        g.dedup();
        g.retain(|&g| g > 1 && g != mine);
        g
    }

    /// The process groups a keeping close ends: the program's own, and the
    /// pty's foreground group, read now.
    fn front_groups(&self) -> Vec<u32> {
        let mut g = vec![self.pid];
        g.extend(self.foreground().filter(|&f| f != self.pid));
        g
    }

    /// A close that leaves the background running (theseus-ggqf): SIGTERM
    /// to each of `groups` (and the hang-up a foreground job would get from
    /// the kernel), never a hang-up to the program itself, since an
    /// interactive shell sends its own to every job it holds. A program
    /// that ignores SIGTERM (an interactive bash) is killed at once, so its
    /// grace is not waited out. After the grace, SIGKILL to each group: a
    /// signal to a group reaches every member, one forked since any scan
    /// included, so no process of the foreground is spared by a scan that
    /// missed it (theseus-z0kk's race). `back` is what it leaves, named.
    fn close_keeping(
        &self,
        c: &mut Child,
        grace: Duration,
        groups: &[u32],
        back: &[tree::Proc],
    ) -> Ended {
        let pid = self.pid;
        for &g in groups.iter().filter(|&&g| g != pid) {
            signal_group(g, libc::SIGHUP);
        }
        for &g in groups {
            signal_group(g, libc::SIGTERM);
        }
        if ignores(pid, libc::SIGTERM) {
            // SAFETY: the program is this pty's own child, not yet reaped
            // (its `Child` is held), so its pid is still its own.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
        // Whether a process of the front groups, the program aside, lives.
        let front = || {
            tree::descendants(pid)
                .iter()
                .any(|p| pgid(p.pid).is_some_and(|g| groups.contains(&g)))
                || groups.iter().filter(|&&g| g != pid).any(|&g| group_runs(g))
        };
        let t0 = Instant::now();
        while t0.elapsed() < grace {
            if matches!(c.try_wait(), Ok(Some(_))) && !front() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut killed = 0;
        for &g in groups {
            if group_lives(g) {
                killed += members(g);
                signal_group(g, libc::SIGKILL);
            }
        }
        // What began while the close ran, into a session of its own, is in
        // no group the close ended and was not in `back`: found again now
        // that the foreground is dead and no longer forking, it is left and
        // named like the rest, never unseen (theseus-z0kk).
        let mut now: Vec<tree::Proc> = tree::descendants(pid);
        now.extend(holders(&self.slave_path));
        now.retain(|p| {
            !back.contains(p)
                && pgid(p.pid).is_some_and(|g| !groups.contains(&g))
                && tree::alive(*p)
        });
        now.sort_unstable();
        now.dedup();
        let status = c.wait().ok();
        // The program leads its own session (`setsid`): its pid is its id.
        let left = back
            .iter()
            .copied()
            .chain(now)
            .filter(|p| tree::alive(*p))
            .map(|p| Kept {
                proc: p,
                program: comm(p.pid),
                why: match sid_of(p.pid) == Some(pid) {
                    true => WHY_BACKGROUND,
                    false => WHY_SESSION,
                },
            })
            .collect();
        Ended {
            status,
            killed,
            left,
        }
    }
}

/// The rounds a close's final scan and kill repeat, at most (theseus-z0kk).
const KILL_ROUNDS: usize = 5;

/// How many of `signalled` were not in `seen`, which then holds them: a
/// process still alive a round after its SIGKILL is signalled again and is
/// not counted again.
pub(super) fn count_new(
    seen: &mut std::collections::BTreeSet<tree::Proc>,
    signalled: impl IntoIterator<Item = tree::Proc>,
) -> usize {
    signalled.into_iter().filter(|p| seen.insert(*p)).count()
}

/// Why a keeping close left a process: a process group of its own in the
/// terminal's session (a shell's `&` job, `nohup … &`), a session of its own
/// (`setsid`, a server's double fork), or, only on the already-exited path,
/// a leftover still in the program's own former group (B6 L186 part (a)).
pub const WHY_BACKGROUND: &str = "a background job";
pub const WHY_SESSION: &str = "its own session (setsid, or a daemon)";
pub const WHY_OWN_GROUP: &str = "the exited program's own group";

/// A process a close left running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub proc: tree::Proc,
    pub program: String,
    pub why: &'static str,
}

/// What a close did: how the program ended, how many processes it had to
/// kill after the grace, and what it left running.
#[derive(Debug, Default)]
pub struct Ended {
    pub status: Option<std::process::ExitStatus>,
    pub killed: usize,
    pub left: Vec<Kept>,
}

/// A process's group, or None once it is gone.
pub(crate) fn pgid(pid: u32) -> Option<u32> {
    // SAFETY: reads another process's group id; no memory is shared.
    let g = unsafe { libc::getpgid(pid as libc::pid_t) };
    (g > 0).then_some(g as u32)
}

fn sid_of(pid: u32) -> Option<u32> {
    // SAFETY: reads another process's session id; no memory is shared.
    let s = unsafe { libc::getsid(pid as libc::pid_t) };
    (s > 0).then_some(s as u32)
}

/// Whether a process group has a member, a zombie not yet reaped included.
fn group_lives(g: u32) -> bool {
    // SAFETY: signal 0 checks for the group's members and sends nothing.
    unsafe { libc::kill(-(g as libc::pid_t), 0) == 0 }
}

/// Whether a process group has a member that has not exited. A zombie
/// does not count: a foreground job the close ended is reparented to the
/// daemon, a child subreaper, and stays a zombie until its sweep, so a wait
/// on `group_lives` alone ran out the whole grace (the join's review).
fn group_runs(g: u32) -> bool {
    group_lives(g) && members(g) > 0
}

/// The live members of group `g`, by a scan of `/proc`.
fn members(g: u32) -> usize {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 0;
    };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&p| pgid(p) == Some(g))
        .filter(|&p| tree::stat(p).is_some_and(|s| !matches!(s.state, 'Z' | 'X')))
        .count()
}

/// Whether `pid` is blocked waiting for input: in a read of its fd 0, or in
/// a select, poll or epoll wait that watches a descriptor
/// (`/proc/<pid>/syscall`, read by `waits_in`). A shell at its prompt and a
/// REPL at its line are (bash and python3's readline: pselect6 on fd 0); one
/// running, or waiting for a child (`wait4`) or a timer, is not. None where
/// it cannot be read: another architecture, or no leave to read it (the
/// program is the daemon's own child, so it has).
fn waits_for_input(pid: u32) -> Option<bool> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/syscall")).ok()?;
    waits_in(&s)
}

/// What a `/proc/<pid>/syscall` line says of the wait it is in. The line is
/// the syscall's number, its six arguments, the stack pointer and the
/// program counter, in hex; "running" instead of a number when the process
/// is not in a syscall.
///
/// A select or poll is a wait for input only when it watches something:
/// CPython before 3.11 sleeps in `time.sleep` with a `pselect6` of zero
/// descriptors, and a program asleep is not waiting for a line. The
/// arguments are read at the kernel's own layout (checked on x86_64 against
/// a process parked in each call; aarch64 prints x0 to x5 the same way):
/// - `select` and `pselect6` take `(nfds, readfds, writefds, exceptfds,
///   timeout, ..)`: arg 0 is the count, args 1 to 3 the three sets, and a
///   wait is a watch only with a count and at least one set (`select(5,
///   NULL, NULL, NULL, t)` is a sleep too);
/// - `poll` and `ppoll` take `(fds, nfds, ..)`: arg 1 is the count, arg 0
///   the array, which is not read (that is `/proc/<pid>/mem`), so an array
///   of `fd < 0` entries still counts;
/// - the epoll waits take `(epfd, ..)`: arg 0 is the epoll descriptor, which
///   is itself a watch set, so the call alone is a wait for input.
///
/// `read` counts only on fd 0 (arg 0).
pub(super) fn waits_in(line: &str) -> Option<bool> {
    #[cfg(target_arch = "x86_64")]
    const READ: i64 = 0;
    #[cfg(target_arch = "x86_64")]
    const SELECTS: &[i64] = &[23, 270];
    #[cfg(target_arch = "x86_64")]
    const POLLS: &[i64] = &[7, 271];
    #[cfg(target_arch = "x86_64")]
    const EPOLLS: &[i64] = &[232, 281, 441];
    #[cfg(target_arch = "aarch64")]
    const READ: i64 = 63;
    #[cfg(target_arch = "aarch64")]
    const SELECTS: &[i64] = &[72];
    #[cfg(target_arch = "aarch64")]
    const POLLS: &[i64] = &[73];
    #[cfg(target_arch = "aarch64")]
    const EPOLLS: &[i64] = &[22, 441];
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = line;
        return None;
    }
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        let mut f = line.split_whitespace();
        let Ok(nr) = f.next()?.parse::<i64>() else {
            // "running"
            return Some(false);
        };
        let args: Vec<u64> = f
            .take(6)
            .map_while(|a| u64::from_str_radix(a.trim_start_matches("0x"), 16).ok())
            .collect();
        let arg = |n: usize| args.get(n).copied();
        Some(if nr == READ {
            arg(0) == Some(0)
        } else if SELECTS.contains(&nr) {
            arg(0).is_some_and(|n| n > 0) && (1..4).any(|i| arg(i).is_some_and(|p| p != 0))
        } else if POLLS.contains(&nr) {
            arg(1).is_some_and(|n| n > 0)
        } else {
            EPOLLS.contains(&nr)
        })
    }
}

/// A process's name (`/proc/<pid>/comm`).
fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|c| c.trim_end().to_string())
        .unwrap_or_default()
}

/// Whether `pid` ignores `sig` (`/proc/<pid>/status`'s `SigIgn`).
fn ignores(pid: u32, sig: libc::c_int) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            let mask = s.lines().find_map(|l| l.strip_prefix("SigIgn:"))?;
            u64::from_str_radix(mask.trim(), 16).ok()
        })
        .is_some_and(|m| m & (1u64 << (sig - 1)) != 0)
}

impl Drop for Pty {
    fn drop(&mut self) {
        // A terminal dropped without its close (a test's panic): nothing it
        // ran outlives it.
        if self.exited().is_none() {
            self.close(Duration::ZERO, false);
        }
    }
}

/// Every process that holds the pty's slave open: what the terminal ran,
/// including a process that left its tree (`setsid` and a double fork, or a
/// parent that died). Read from `/proc/*/fd`, only at a close.
fn holders(slave: &Path) -> Vec<tree::Proc> {
    let me = std::process::id();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    procs
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| pid != me)
        .filter(|&pid| {
            std::fs::read_dir(format!("/proc/{pid}/fd")).is_ok_and(|fds| {
                fds.flatten()
                    .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l == slave))
            })
        })
        .filter_map(|pid| {
            Some(tree::Proc {
                pid,
                start: tree::stat(pid)?.start,
            })
        })
        .collect()
}

fn signal_group(pgid: u32, sig: libc::c_int) {
    // SAFETY: a signal to a process group id; callers signal only a group this
    // terminal's program leads or that a holder of its pty is still in
    // (`group_still_ours`), never a bare id of a reaped program.
    unsafe {
        libc::kill(-(pgid as libc::pid_t), sig);
    }
}

/// The reader thread: the pty's output into the screen, the screen's
/// answers back to the program, until the last slave closes (EIO).
fn read_loop(mut master: File, mut replies: File, shared: &Shared) {
    let mut buf = [0u8; 8192];
    loop {
        match master.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                shared.feeding.store(true, Ordering::SeqCst);
                let answer = {
                    let mut s = shared.screen.lock().unwrap_or_else(|p| p.into_inner());
                    s.feed(&buf[..n]);
                    s.take_replies()
                };
                if !answer.is_empty() {
                    let _ = replies.write_all(&answer);
                }
                shared.bytes_out.fetch_add(n as u64, Ordering::Relaxed);
                shared
                    .last_output_ms
                    .store(theseus_protocol::now_unix_ms(), Ordering::Relaxed);
                shared.version.send_modify(|v| *v += 1);
                shared.feeding.store(false, Ordering::SeqCst);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    shared.hung_up.store(true, Ordering::Relaxed);
    shared.version.send_modify(|v| *v += 1);
}
