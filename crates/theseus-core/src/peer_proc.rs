//! Who is behind a CLI answer (theseus-5rvss). The Unix socket is mode 0600,
//! so every process that reaches it is the owner's own user: an agent the
//! owner runs on this machine included. The daemon cannot tell that agent's
//! `theseus answer` from the owner's own terminal by uid, so it reads the
//! process: one walk of the socket peer's parent chain through `/proc`.
//!
//! - **The owner's own**: the peer has a controlling terminal, and no
//!   process on its chain is on the agent list.
//! - **An agent's**: a process on its chain is an agent host (`claude`,
//!   `codex`: `[policy] agent_processes`), named by it; or the peer has no
//!   controlling terminal (a service, a cron, a gateway's exec), named
//!   `no terminal`.
//!
//! A light guard, as the CLI's refusal inside a job is (theseus-zmgb): an
//! agent that opens a pty and strips its ancestry reads as the owner. The
//! boundary is the DM's card and L1.

use std::sync::{Arc, OnceLock};

/// The name an agent gets when nothing on its chain is on the list and it
/// has no controlling terminal.
pub const NO_TERMINAL: &str = "no terminal";

/// The name of an agent whose process could not be read (it exited before
/// the walk).
pub const UNTRACED: &str = "untraced";

/// What is behind a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Behind {
    /// The owner's own terminal.
    Owner,
    /// An agent, named by its program (`claude`) or `no terminal`.
    Agent(String),
}

/// One process, as `/proc/<pid>` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub ppid: u32,
    /// The controlling terminal's device number; 0 for none.
    pub tty: u64,
    pub comm: String,
    /// The first words of its command line.
    pub argv: Vec<String>,
}

/// A process as `/proc/<pid>/stat` (`pid (comm) S ppid pgrp session tty_nr
/// ...`) and `cmdline` (NUL-separated) give it. The command name may hold
/// spaces and parentheses, so the fields are counted from its last `)`.
pub fn parse_proc(stat: &str, cmdline: &[u8]) -> Option<Proc> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_string();
    let mut rest = stat.get(close + 1..)?.split_whitespace();
    let ppid = rest.nth(1)?.parse().ok()?;
    let tty = rest.nth(2)?.parse().ok()?;
    let argv = cmdline
        .split(|b| *b == 0)
        .filter(|w| !w.is_empty())
        .take(2)
        .map(|w| String::from_utf8_lossy(w).into_owned())
        .collect();
    Some(Proc {
        ppid,
        tty,
        comm,
        argv,
    })
}

fn read_proc(pid: u32) -> Option<Proc> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    parse_proc(&stat, &cmdline)
}

/// Whether `p` runs the program `name`: its command name, or the file name
/// of its command or of the script it runs (`node /usr/bin/claude`), in any
/// case.
fn runs(p: &Proc, name: &str) -> bool {
    let base = |s: &str| s.rsplit('/').next().unwrap_or(s).to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    base(&p.comm) == name || p.argv.iter().any(|a| base(a) == name)
}

/// The chain is walked at most this far: a loop or a deep tree ends it.
const DEPTH: usize = 64;

/// Classify `pid` over the processes `read` gives, against the program
/// names in `agents`. The nearest agent on the chain, the peer itself
/// first, names it; else a peer with no terminal is `no terminal`; a peer
/// that cannot be read is `untraced`.
pub fn behind_in(read: impl Fn(u32) -> Option<Proc>, pid: u32, agents: &[String]) -> Behind {
    let Some(peer) = read(pid) else {
        return Behind::Agent(UNTRACED.into());
    };
    let (mut at, mut up) = (peer.clone(), 0);
    loop {
        if let Some(name) = agents.iter().find(|a| runs(&at, a)) {
            return Behind::Agent(name.clone());
        }
        up += 1;
        let next = match at.ppid {
            0 | 1 => None,
            ppid if up < DEPTH => read(ppid),
            _ => None,
        };
        match next {
            Some(p) => at = p,
            None => break,
        }
    }
    match peer.tty {
        0 => Behind::Agent(NO_TERMINAL.into()),
        _ => Behind::Owner,
    }
}

/// `behind_in` over this machine's `/proc`.
pub fn behind(pid: u32, agents: &[String]) -> Behind {
    behind_in(read_proc, pid, agents)
}

/// The process behind one connection, found when an act first asks and
/// kept: the walk is paid by an answer, never by a connection.
#[derive(Debug, Default)]
pub struct Peer {
    pid: Option<u32>,
    agents: Arc<[String]>,
    found: OnceLock<Behind>,
}

impl Peer {
    /// A connection whose peer is process `pid`; with none, the owner's
    /// (`--stdio` names its parent, a test names nobody).
    pub fn new(pid: Option<u32>, agents: Arc<[String]>) -> Self {
        Self {
            pid,
            agents,
            found: OnceLock::new(),
        }
    }

    /// A peer a test says is `behind`.
    #[cfg(test)]
    pub fn known(behind: Behind) -> Self {
        Self {
            found: OnceLock::from(behind),
            ..Self::default()
        }
    }

    pub fn behind(&self) -> &Behind {
        self.found.get_or_init(|| match self.pid {
            Some(pid) => behind(pid, &self.agents),
            None => Behind::Owner,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn proc(ppid: u32, tty: u64, comm: &str, argv: &[&str]) -> Proc {
        Proc {
            ppid,
            tty,
            comm: comm.into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn names(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    /// A tree of fixtures: `theseus` (9) under `bash` (5, on a terminal)
    /// under `sshd`; the same CLI (19) under `claude` (15) under a cron-less
    /// shell with no terminal; and one (29) with no terminal at all.
    fn tree() -> impl Fn(u32) -> Option<Proc> {
        let m: HashMap<u32, Proc> = HashMap::from([
            (4, proc(1, 0, "sshd", &["sshd"])),
            (5, proc(4, 34816, "bash", &["-bash"])),
            (9, proc(5, 34816, "theseus", &["theseus", "answer"])),
            (14, proc(1, 0, "tmux", &["tmux"])),
            (15, proc(14, 34817, "node", &["node", "/usr/bin/Claude"])),
            (19, proc(15, 34817, "theseus", &["theseus", "answer"])),
            (24, proc(1, 0, "gateway", &["gateway"])),
            (29, proc(24, 0, "theseus", &["theseus", "answer"])),
        ]);
        move |pid| m.get(&pid).cloned()
    }

    #[test]
    fn a_terminal_with_no_agent_above_it_is_the_owners() {
        let agents = names(&["claude", "codex"]);
        assert_eq!(behind_in(tree(), 9, &agents), Behind::Owner);
    }

    #[test]
    fn no_terminal_is_an_agent() {
        let agents = names(&["claude", "codex"]);
        assert_eq!(
            behind_in(tree(), 29, &agents),
            Behind::Agent(NO_TERMINAL.into())
        );
    }

    /// A terminal does not make the owner of a process an agent host runs:
    /// the agent list reads the whole chain, a script's name included.
    #[test]
    fn an_agent_above_it_names_it_even_on_a_terminal() {
        let agents = names(&["codex", "claude"]);
        assert_eq!(
            behind_in(tree(), 19, &agents),
            Behind::Agent("claude".into())
        );
        // The list's own name wins nothing from the order of the list.
        assert_eq!(
            behind_in(tree(), 19, &names(&["claude"])),
            Behind::Agent("claude".into())
        );
        // Off the list, the same process is the owner's.
        assert_eq!(behind_in(tree(), 19, &names(&["codex"])), Behind::Owner);
    }

    #[test]
    fn a_peer_that_exited_is_untraced() {
        assert_eq!(
            behind_in(tree(), 404, &names(&["claude"])),
            Behind::Agent(UNTRACED.into())
        );
    }

    #[test]
    fn a_loop_in_the_chain_ends_the_walk() {
        let looped = |pid: u32| Some(proc(if pid == 2 { 3 } else { 2 }, 7, "sh", &["sh"]));
        assert_eq!(behind_in(looped, 2, &names(&["claude"])), Behind::Owner);
    }

    #[test]
    fn a_proc_is_read_from_its_stat_and_command_line() {
        let stat = "4242 (my (odd) cmd) S 4100 4242 4100 34816 4242 4194304 1 0 0 0";
        let p = parse_proc(stat, b"theseus\0answer\0abc\0").unwrap();
        assert_eq!(p, proc(4100, 34816, "my (odd) cmd", &["theseus", "answer"]));
        assert_eq!(parse_proc("garbage", b""), None);
    }

    /// This test process is a real one: its own `/proc` entry parses, and
    /// its parent is found.
    #[test]
    fn the_real_proc_reads() {
        let me = read_proc(std::process::id()).expect("our own /proc entry reads");
        assert!(me.ppid > 0);
        assert!(read_proc(me.ppid).is_some());
    }
}
