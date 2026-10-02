//! The daemon's children (theseus-z4b): what it spawned, what it adopted,
//! and who reaps each.
//!
//! - A **job wrapper** (`job::spawn_detached`) is this process's to reap. Its
//!   pid is registered with its job at the spawn, and `sweep` reaps it by that
//!   pid once it has exited. Before this, nothing did, so every job left a
//!   zombie of `theseusd` until the daemon exited.
//! - An **owned** child, whose spawner waits for it, is never reaped here.
//!   These are the `op` processes: tokio's process driver reaps each of its
//!   children by its pid, and a reap here would take the status its `wait()`
//!   needs, so the `op` call would fail with `ECHILD`.
//! - A **tender** (roadmap row 51) is a long-lived child its supervisor
//!   restarts: the index tender, `theseus-index serve`. `sweep` reaps it by
//!   its pid, as it does a wrapper, and reports how it ended, so the
//!   supervisor starts the next one. After an exec, `relearn` knows a tender
//!   by its command line, so the new image takes over the running one.
//! - Any other child is an **orphan**: a job's descendant whose wrapper died,
//!   which the kernel reparented here because the socket daemon is a child
//!   subreaper (`adopt`). `sweep` reaps it once it has exited. Nothing that
//!   answers an approval descends from the daemon, so the core refuses an
//!   answer from any of its descendants (`daemon`).
//!
//! Nothing here waits for a child it does not name by pid: there is no
//! `waitpid(-1)` and no `WNOWAIT`. `spawn` holds the registry's lock across
//! the spawn and the registration, and `sweep` holds it across its scan and
//! its reaps. So a zombie the sweep finds unregistered was not spawned by an
//! owner: had it been, it would have been registered before the sweep could
//! look. Every child the daemon starts goes through `spawn`. One that did not,
//! and was waited for by its spawner, could be reaped here as an orphan.

use std::collections::BTreeMap;
use std::io;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Who reaps a child that `spawn` starts.
#[derive(Debug, Clone, Copy)]
pub enum Kind<'a> {
    /// A job wrapper, for this job: `sweep` reaps it.
    Wrapper(&'a str),
    /// Its spawner waits for it (tokio, for `op`): never reaped here.
    Owned,
    /// A long-lived child, by what it tends (`index`): `sweep` reaps it and
    /// reports its exit to its supervisor, which starts the next.
    Tender(&'a str),
}

/// The tenders `relearn` knows by their command lines: the binary's file
/// name, its first argument, and what it tends.
const TENDERS: &[(&str, &str, &str)] = &[("theseus-index", "serve", "index")];

struct Registry {
    /// Wrappers not yet reaped: pid, then job.
    wrappers: BTreeMap<u32, String>,
    /// Owned children: pid, then start time (clock ticks after boot) when it
    /// could be read, so a pid reused after the owner reaped it is not taken
    /// for its child. With none, any process with that pid is taken for it.
    owned: BTreeMap<u32, Option<u64>>,
    /// Tenders not yet reaped: pid, then what it tends.
    tenders: BTreeMap<u32, String>,
    reaped_wrappers: u64,
    reaped_orphans: u64,
}

impl Registry {
    fn owns(&self, pid: u32, start: u64) -> bool {
        self.owned
            .get(&pid)
            .is_some_and(|t| t.is_none_or(|t| t == start))
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    wrappers: BTreeMap::new(),
    owned: BTreeMap::new(),
    tenders: BTreeMap::new(),
    reaped_wrappers: 0,
    reaped_orphans: 0,
});

/// The daemon none of whose descendants may answer an approval; 0 before
/// `adopt`.
static DAEMON: AtomicU32 = AtomicU32::new(0);

fn registry() -> MutexGuard<'static, Registry> {
    // Each change under the lock is one insert or one remove, so a panic
    // while it was held leaves nothing half done.
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Start a child and register it as `kind`, under the registry's lock, so no
/// sweep looks between the spawn and the registration. `pid` names the child
/// that `spawn` returned.
pub fn spawn<C>(
    kind: Kind<'_>,
    spawn: impl FnOnce() -> io::Result<C>,
    pid: impl FnOnce(&C) -> Option<u32>,
) -> io::Result<C> {
    let mut reg = registry();
    let child = spawn()?;
    if let Some(p) = pid(&child) {
        match kind {
            Kind::Wrapper(job) => {
                reg.wrappers.insert(p, job.to_string());
            }
            // Its owner has not waited for it yet, since it waits only once
            // this returns, so its stat is there to read.
            Kind::Owned => {
                reg.owned.insert(p, stat(p).map(|s| s.start));
            }
            Kind::Tender(name) => {
                reg.tenders.insert(p, name.to_string());
            }
        }
    }
    Ok(child)
}

/// What one sweep reaped.
#[derive(Debug, Default)]
pub struct Swept {
    /// Job wrappers: pid, job, and how each ended.
    pub wrappers: Vec<(u32, String, ExitStatus)>,
    /// Orphans: pid, and how each ended.
    pub orphans: Vec<(u32, ExitStatus)>,
    /// Tenders: pid, what it tended, and how it ended (`None`: it was no
    /// longer this process's child, so its status was never this process's
    /// to read).
    pub tenders: Vec<(u32, String, Option<ExitStatus>)>,
}

/// Reap every child that has exited and is this process's to reap: each
/// registered wrapper and tender, by its pid, then each zombie child that is
/// none of those and not owned. An owned child is left to its owner. Its entry
/// goes once its pid is no longer a child with its start time, which means the
/// owner has reaped it.
pub fn sweep() -> Swept {
    let mut reg = registry();
    let mut out = Swept::default();
    let wrappers: Vec<u32> = reg.wrappers.keys().copied().collect();
    for pid in wrappers {
        match reap(pid) {
            Wait::Running => {}
            Wait::Exited(status) => {
                let job = reg.wrappers.remove(&pid).unwrap_or_default();
                reg.reaped_wrappers += 1;
                out.wrappers.push((pid, job, status));
            }
            // Not a child of this process: nothing to reap, or to count.
            Wait::NotAChild => {
                reg.wrappers.remove(&pid);
            }
        }
    }
    let tenders: Vec<u32> = reg.tenders.keys().copied().collect();
    for pid in tenders {
        let status = match reap(pid) {
            Wait::Running => continue,
            Wait::Exited(status) => Some(status),
            // Gone, and not as this process's child: still the end of it,
            // which its supervisor must hear.
            Wait::NotAChild => None,
        };
        let name = reg.tenders.remove(&pid).unwrap_or_default();
        out.tenders.push((pid, name, status));
    }
    let me = std::process::id();
    for pid in children() {
        if reg.wrappers.contains_key(&pid) || reg.tenders.contains_key(&pid) {
            continue;
        }
        let Some(s) = stat(pid) else { continue };
        if s.ppid != me || s.state != 'Z' || reg.owns(pid, s.start) {
            continue;
        }
        if let Wait::Exited(status) = reap(pid) {
            reg.reaped_orphans += 1;
            out.orphans.push((pid, status));
        }
    }
    reg.owned.retain(|&pid, start| {
        stat(pid).is_some_and(|s| s.ppid == me && start.is_none_or(|t| t == s.start))
    });
    out
}

/// What this process holds, for health.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    /// This process is a child subreaper.
    pub subreaper: bool,
    /// The job wrappers among its live children: pid, then job.
    pub wrappers: Vec<(u32, String)>,
    /// Live children that are neither wrappers nor owned: the orphans it
    /// adopted.
    pub orphans: u64,
    /// Children that have exited and wait to be reaped. A sweep leaves none
    /// but an owned child whose owner has not yet waited, so this is 0 in
    /// steady state, and a count that grows is a leak.
    pub zombies: u64,
    /// Live children their spawner waits for.
    pub owned: u64,
    /// The live tenders among its children: pid, then what it tends.
    pub tenders: Vec<(u32, String)>,
    /// Reaped since this image started.
    pub reaped_wrappers: u64,
    pub reaped_orphans: u64,
}

/// Count this process's children by kind. It reaps nothing.
pub fn census() -> Census {
    let reg = registry();
    let me = std::process::id();
    let mut c = Census {
        subreaper: is_subreaper(),
        reaped_wrappers: reg.reaped_wrappers,
        reaped_orphans: reg.reaped_orphans,
        ..Census::default()
    };
    for pid in children() {
        let Some(s) = stat(pid) else { continue };
        if s.ppid != me {
            continue;
        }
        if matches!(s.state, 'Z' | 'X') {
            c.zombies += 1;
        } else if let Some(job) = reg.wrappers.get(&pid) {
            c.wrappers.push((pid, job.clone()));
        } else if let Some(name) = reg.tenders.get(&pid) {
            c.tenders.push((pid, name.clone()));
        } else if reg.owns(pid, s.start) {
            c.owned += 1;
        } else {
            c.orphans += 1;
        }
    }
    c
}

/// What `relearn` found.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Relearned {
    pub wrappers: u64,
    pub tenders: u64,
    pub orphans: u64,
    pub zombies: u64,
}

/// After an exec, which keeps the pid and so every child (theseus-2fo's
/// restart onto a changed note): register each live child whose command line
/// is a job wrapper's, with its job, as 6qy's check reads it, and each whose
/// command line is a tender's (`TENDERS`), so its supervisor takes it over
/// instead of starting a second. Every other child is an orphan, reaped by a
/// sweep once it exits. The old image's `op`, which its runtime killed as it
/// stopped, is among them, since the new image's tokio never knew it.
pub fn relearn() -> Relearned {
    let mut reg = registry();
    let me = std::process::id();
    let mut r = Relearned::default();
    for pid in children() {
        let Some(s) = stat(pid) else { continue };
        if s.ppid != me
            || reg.wrappers.contains_key(&pid)
            || reg.tenders.contains_key(&pid)
            || reg.owns(pid, s.start)
        {
            continue;
        }
        if matches!(s.state, 'Z' | 'X') {
            r.zombies += 1;
        } else if let Some(job) = crate::job::wrapper_job(pid) {
            reg.wrappers.insert(pid, job);
            r.wrappers += 1;
        } else if let Some(name) = tender_of(pid) {
            reg.tenders.insert(pid, name.to_string());
            r.tenders += 1;
        } else {
            r.orphans += 1;
        }
    }
    r
}

/// The live tender of `name` among this process's children: the one it
/// spawned, or the one `relearn` found after an exec.
pub fn tender(name: &str) -> Option<u32> {
    let reg = registry();
    let me = std::process::id();
    reg.tenders
        .iter()
        .filter(|(_, n)| n.as_str() == name)
        .map(|(&pid, _)| pid)
        .find(|&pid| stat(pid).is_some_and(|s| s.ppid == me && !matches!(s.state, 'Z' | 'X')))
}

/// What a process tends, read from its command line (`TENDERS`): a tender's
/// binary, by its file name, with its first argument.
pub fn tender_of(pid: u32) -> Option<&'static str> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    tender_in(&cmdline)
}

fn tender_in(cmdline: &[u8]) -> Option<&'static str> {
    let mut args = cmdline.split(|&b| b == 0);
    let bin = std::path::Path::new(std::str::from_utf8(args.next()?).ok()?)
        .file_name()?
        .to_str()?
        .to_string();
    let first = std::str::from_utf8(args.next()?).ok()?;
    TENDERS
        .iter()
        .find(|(b, a, _)| *b == bin && *a == first)
        .map(|(_, _, name)| *name)
}

/// Make this process a child subreaper (theseus-z4b). A job's descendant
/// whose wrapper died is then reparented here, not to init, and this process
/// is recorded as the daemon none of whose descendants may answer an
/// approval. `execve` keeps the flag; a restart calls this again all the same.
pub fn adopt() -> Result<(), String> {
    DAEMON.store(std::process::id(), Ordering::Relaxed);
    set_subreaper()
}

/// The daemon none of whose descendants may answer an approval: this process,
/// once `adopt` has run.
pub fn daemon() -> Option<u32> {
    match DAEMON.load(Ordering::Relaxed) {
        0 => None,
        pid => Some(pid),
    }
}

/// Become a child subreaper: an orphaned descendant is reparented here instead
/// of to init. The error, when there is one, says why not.
pub(crate) fn set_subreaper() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == -1 {
            return Err(format!(
                "PR_SET_CHILD_SUBREAPER: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("a child subreaper needs Linux".into())
    }
}

/// Whether this process is a child subreaper now.
pub fn is_subreaper() -> bool {
    #[cfg(target_os = "linux")]
    {
        let mut on: libc::c_int = 0;
        let got = unsafe {
            libc::prctl(
                libc::PR_GET_CHILD_SUBREAPER,
                &mut on as *mut libc::c_int,
                0,
                0,
                0,
            )
        };
        got == 0 && on != 0
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

enum Wait {
    Running,
    Exited(ExitStatus),
    NotAChild,
}

/// `waitpid(pid, WNOHANG)`: one child, named.
fn reap(pid: u32) -> Wait {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        loop {
            let mut status = 0;
            match unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) } {
                0 => return Wait::Running,
                -1 if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) => {}
                -1 => return Wait::NotAChild,
                _ => return Wait::Exited(ExitStatus::from_raw(status)),
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Wait::NotAChild
    }
}

/// `/proc/<pid>/stat`: the fields the sweep reads. The comm may hold spaces
/// and parentheses, so the fields start after its last `)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stat {
    state: char,
    ppid: u32,
    /// Clock ticks after boot.
    start: u64,
}

fn stat(pid: u32) -> Option<Stat> {
    parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn parse_stat(text: &str) -> Option<Stat> {
    let f: Vec<&str> = text
        .get(text.rfind(')')? + 1..)?
        .split_whitespace()
        .collect();
    Some(Stat {
        state: f.first()?.chars().next()?,
        ppid: f.get(1)?.parse().ok()?,
        start: f.get(19)?.parse().ok()?,
    })
}

/// This process's children, from every thread's `children` file: a child is
/// listed under the thread that forked it, or under the one the kernel chose
/// when it was reparented here. A file can miss a child that exits while it is
/// read, and the next sweep finds it. A kernel without those files gets a scan
/// of `/proc` for the processes whose parent is this one.
fn children() -> Vec<u32> {
    let mut out = Vec::new();
    let mut read_any = false;
    if let Ok(tasks) = std::fs::read_dir("/proc/self/task") {
        for t in tasks.flatten() {
            if let Ok(s) = std::fs::read_to_string(t.path().join("children")) {
                read_any = true;
                out.extend(s.split_whitespace().filter_map(|p| p.parse::<u32>().ok()));
            }
        }
    }
    if !read_any {
        let me = std::process::id();
        if let Ok(procs) = std::fs::read_dir("/proc") {
            out.extend(
                procs
                    .flatten()
                    .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
                    .filter(|&p| stat(p).is_some_and(|s| s.ppid == me)),
            );
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stat line as the kernel writes it, a comm with spaces and
    /// parentheses among them, and a zombie's.
    #[test]
    fn a_stat_line_gives_state_parent_and_start() {
        let line = "4242 (a (b) c) Z 17 4242 17 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 \
                    0 0 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        assert_eq!(
            parse_stat(line),
            Some(Stat {
                state: 'Z',
                ppid: 17,
                start: 987654,
            })
        );
        assert_eq!(parse_stat("12 (x) S 1"), None);
        let me = stat(std::process::id()).unwrap();
        assert!(me.start > 0 && me.ppid > 0);
    }

    /// An owned child is known by its pid and start time; a reused pid is
    /// not it, and one whose start could not be read matches any.
    #[test]
    fn an_owned_child_is_its_pid_and_start_time() {
        let mut reg = Registry {
            wrappers: BTreeMap::new(),
            owned: BTreeMap::new(),
            tenders: BTreeMap::new(),
            reaped_wrappers: 0,
            reaped_orphans: 0,
        };
        reg.owned.insert(10, Some(500));
        reg.owned.insert(11, None);
        assert!(reg.owns(10, 500));
        assert!(!reg.owns(10, 501), "a pid reused later is another process");
        assert!(reg.owns(11, 1) && reg.owns(11, 2));
        assert!(!reg.owns(12, 500));
    }

    /// A tender is known by its binary's file name and its first argument
    /// (row 51): the same binary as a client, or another binary, is not one.
    #[test]
    fn a_tender_is_known_by_its_command_line() {
        let serve = b"/home/op/.local/bin/theseus-index\0serve\0--store\0/s\0--parent\x0012\0";
        assert_eq!(tender_in(serve), Some("index"));
        assert_eq!(tender_in(b"theseus-index\0serve\0"), Some("index"));
        assert_eq!(tender_in(b"/x/theseus-index\0query\0--socket\0/s\0"), None);
        assert_eq!(tender_in(b"/x/theseusd\0serve\0"), None);
        assert_eq!(tender_in(b"/x/theseus-index\0"), None);
        assert_eq!(tender_in(b""), None);
    }
}
