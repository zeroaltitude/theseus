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
//! The loopback web UI's `/ws` is judged the same way (theseus-iie8e). Its
//! port is reachable by every process of the owner's own user, and `Host`
//! and `Origin` are headers any of them can send, so an agent's raw
//! WebSocket is not told from the cockpit's page by the request alone. The
//! peer is the process holding the client's end of the connection
//! (`peer::client_pid`), found at the first act that asks, and judged by the
//! agent list alone (`Rule::Agents`): a browser is a windowed process with
//! no controlling terminal of its own, so a terminal says nothing there. A
//! holder that cannot be found is an agent (`untraced`), never the owner.
//!
//! A peer whose chain passes through the daemon's own process is a job's
//! (a command, a script or a tool a turn ran), on the CLI socket and the web
//! port alike: an agent, never the owner, whatever terminal it holds or
//! program it runs (issue 22).
//!
//! A light guard, as the CLI's refusal inside a job is (theseus-zmgb): an
//! agent that opens a pty and strips its ancestry reads as the owner. The
//! boundary is the DM's card and L1.

use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

/// The name an agent gets when nothing on its chain is on the list and it
/// has no controlling terminal.
pub const NO_TERMINAL: &str = "no terminal";

/// The name of an agent whose process could not be read (it exited before
/// the walk).
pub const UNTRACED: &str = "untraced";

/// The name an agent gets when its chain passes through the daemon's own
/// process: a job's.
pub const JOB: &str = "a job";

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

/// How a peer's chain is judged once it holds no agent host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The CLI's (theseus-5rvss): a peer with no controlling terminal is an
    /// agent too (a service, a cron, a gateway's exec).
    Terminal,
    /// The web's (theseus-iie8e): the agent list alone. The browser holding
    /// the cockpit's page has no controlling terminal of its own, so a
    /// terminal says nothing about a `/ws` peer.
    Agents,
}

/// Classify `pid` over the processes `read` gives, against the program
/// names in `agents`. The nearest agent on the chain, the peer itself
/// first, names it; else, under `Rule::Terminal`, a peer with no terminal is
/// `no terminal`; a peer that cannot be read is `untraced`.
pub fn behind_in(
    read: impl Fn(u32) -> Option<Proc>,
    pid: u32,
    agents: &[String],
    rule: Rule,
) -> Behind {
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
    match (rule, peer.tty) {
        (Rule::Terminal, 0) => Behind::Agent(NO_TERMINAL.into()),
        _ => Behind::Owner,
    }
}

/// Whether `pid`, or a process above it, is `daemon`: what a job's process
/// is, since a job runs under the daemon that started it. A daemon that is
/// pid 1 holds every process there is, so it says nothing.
fn under_in(read: impl Fn(u32) -> Option<Proc>, pid: u32, daemon: u32) -> bool {
    if daemon <= 1 {
        return false;
    }
    let mut at = pid;
    for _ in 0..DEPTH {
        if at == daemon {
            return true;
        }
        match read(at) {
            Some(p) if p.ppid > 1 => at = p.ppid,
            _ => return false,
        }
    }
    // The steps ran out before the chain ended: a job nested this deep must not
    // read as the owner's own terminal, so this is the strict side, a job's.
    true
}

/// `behind_in`, but a peer under `daemon` is a job's before anything else is
/// asked of it.
pub fn behind_of(
    read: impl Fn(u32) -> Option<Proc>,
    pid: u32,
    agents: &[String],
    rule: Rule,
    daemon: u32,
) -> Behind {
    if under_in(&read, pid, daemon) {
        return Behind::Agent(JOB.into());
    }
    behind_in(read, pid, agents, rule)
}

/// `behind_of` over this machine's `/proc`, under this process.
pub fn behind(pid: u32, agents: &[String], rule: Rule) -> Behind {
    behind_of(read_proc, pid, agents, rule, std::process::id())
}

/// Where a connection's peer process comes from, and so how it is judged.
#[derive(Debug, Clone, Default)]
pub enum Source {
    /// No process to read: the owner's own (a test, an in-process binding).
    #[default]
    Owner,
    /// The socket peer's pid, as `SO_PEERCRED` gives it, or `--stdio`'s
    /// parent: the CLI's rule.
    Pid(u32),
    /// A loopback web client's two addresses, as its listener accepted them
    /// (theseus-iie8e): the holder of the client's end, by the web's rule.
    /// A connection whose own server address could not be read names no end
    /// to look for, and so is an agent's.
    Web {
        server: Option<SocketAddr>,
        client: SocketAddr,
    },
}

/// The process behind one connection, found when an act first asks and
/// kept: the walk, and the web's socket lookup, are paid by an answer,
/// never by a connection.
#[derive(Debug, Default)]
pub struct Peer {
    source: Source,
    agents: Arc<[String]>,
    found: OnceLock<Behind>,
}

impl Peer {
    /// A connection whose peer `source` names.
    pub fn new(source: Source, agents: Arc<[String]>) -> Self {
        Self {
            source,
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

    /// The pid of the socket's peer, as the daemon logs it for the connection;
    /// none for the other sources.
    pub fn pid(&self) -> Option<u32> {
        match self.source {
            Source::Pid(pid) => Some(pid),
            _ => None,
        }
    }

    pub fn behind(&self) -> &Behind {
        self.found.get_or_init(|| match self.source {
            Source::Owner => Behind::Owner,
            Source::Pid(pid) => behind(pid, &self.agents, Rule::Terminal),
            // No holder is no identity, and no identity is never the owner.
            Source::Web {
                server: Some(server),
                client,
            } => match crate::peer::client_pid(server, client) {
                Some(pid) => behind(pid, &self.agents, Rule::Agents),
                None => Behind::Agent(UNTRACED.into()),
            },
            Source::Web { server: None, .. } => Behind::Agent(UNTRACED.into()),
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
        assert_eq!(behind_in(tree(), 9, &agents, Rule::Terminal), Behind::Owner);
    }

    #[test]
    fn no_terminal_is_an_agent() {
        let agents = names(&["claude", "codex"]);
        assert_eq!(
            behind_in(tree(), 29, &agents, Rule::Terminal),
            Behind::Agent(NO_TERMINAL.into())
        );
    }

    /// A terminal does not make the owner of a process an agent host runs:
    /// the agent list reads the whole chain, a script's name included.
    #[test]
    fn an_agent_above_it_names_it_even_on_a_terminal() {
        let agents = names(&["codex", "claude"]);
        assert_eq!(
            behind_in(tree(), 19, &agents, Rule::Terminal),
            Behind::Agent("claude".into())
        );
        // The list's own name wins nothing from the order of the list.
        assert_eq!(
            behind_in(tree(), 19, &names(&["claude"]), Rule::Terminal),
            Behind::Agent("claude".into())
        );
        // Off the list, the same process is the owner's.
        assert_eq!(
            behind_in(tree(), 19, &names(&["codex"]), Rule::Terminal),
            Behind::Owner
        );
    }

    /// The web's rule (theseus-iie8e): a `/ws` peer with no terminal is not
    /// an agent for that alone — the browser holding the cockpit's page has
    /// none — but an agent host on its chain names it exactly as on the CLI.
    #[test]
    fn a_web_peer_is_judged_by_the_agent_list_and_not_by_its_terminal() {
        let agents = names(&["claude", "codex"]);
        // No terminal: an agent on the CLI, the owner's own on the web.
        assert_eq!(behind_in(tree(), 29, &agents, Rule::Agents), Behind::Owner);
        assert_eq!(behind_in(tree(), 9, &agents, Rule::Agents), Behind::Owner);
        // An agent host above it still names it, terminal or not.
        assert_eq!(
            behind_in(tree(), 19, &agents, Rule::Agents),
            Behind::Agent("claude".into())
        );
        // A peer that cannot be read is untraced under either rule.
        assert_eq!(
            behind_in(tree(), 404, &agents, Rule::Agents),
            Behind::Agent(UNTRACED.into())
        );
    }

    #[test]
    fn a_peer_that_exited_is_untraced() {
        assert_eq!(
            behind_in(tree(), 404, &names(&["claude"]), Rule::Terminal),
            Behind::Agent(UNTRACED.into())
        );
    }

    #[test]
    fn a_loop_in_the_chain_ends_the_walk() {
        let looped = |pid: u32| Some(proc(if pid == 2 { 3 } else { 2 }, 7, "sh", &["sh"]));
        assert_eq!(
            behind_in(looped, 2, &names(&["claude"]), Rule::Terminal),
            Behind::Owner
        );
    }

    /// The daemon (100, under a service manager) and what it ran: a job's
    /// shell (110) and, under it, a `theseus answer` on a terminal (111) and
    /// one under `claude` (121); beside them the owner's own terminal (9).
    fn daemon_tree() -> impl Fn(u32) -> Option<Proc> {
        let base = tree();
        move |pid| match pid {
            100 => Some(proc(1, 0, "theseusd", &["theseusd"])),
            110 => Some(proc(100, 34900, "sh", &["sh", "-c"])),
            111 => Some(proc(110, 34900, "theseus", &["theseus", "answer"])),
            120 => Some(proc(110, 34900, "node", &["node", "/usr/bin/claude"])),
            121 => Some(proc(120, 34900, "theseus", &["theseus", "answer"])),
            _ => base(pid),
        }
    }

    /// A process the daemon ran is a job's, on a terminal or not, on either
    /// rule and with the agent list empty: only the owner's own terminal,
    /// beside the daemon and not under it, is the owner's.
    #[test]
    fn a_peer_under_the_daemon_is_a_jobs_on_either_rule() {
        let none: Vec<String> = Vec::new();
        let job = Behind::Agent(JOB.into());
        for rule in [Rule::Terminal, Rule::Agents] {
            // Without the daemon known, the same terminal is the owner's:
            // this is what the walk by the agent list alone cannot tell.
            assert_eq!(behind_in(daemon_tree(), 111, &none, rule), Behind::Owner);
            assert_eq!(behind_of(daemon_tree(), 111, &none, rule, 100), job);
            assert_eq!(behind_of(daemon_tree(), 110, &none, rule, 100), job);
            // The job's own is named a job before an agent host above it.
            let claude = names(&["claude"]);
            assert_eq!(behind_of(daemon_tree(), 121, &claude, rule, 100), job);
            // The owner's terminal is not under the daemon.
            assert_eq!(behind_of(daemon_tree(), 9, &none, rule, 100), Behind::Owner);
        }
        // A daemon that is pid 1 holds everything and says nothing.
        assert_eq!(
            behind_of(daemon_tree(), 9, &none, Rule::Terminal, 1),
            Behind::Owner
        );
    }

    /// A chain longer than the walk is a job's: the steps run out, and the
    /// answer is the strict side, never the owner's. One that ends at init
    /// inside the steps is not under the daemon.
    #[test]
    fn a_chain_too_deep_to_walk_is_a_jobs() {
        let deep = |pid: u32| {
            (1000..1200)
                .contains(&pid)
                .then(|| proc(pid + 1, 34900, "sh", &["sh"]))
        };
        assert!(under_in(deep, 1000, 100));
        let none: Vec<String> = Vec::new();
        for rule in [Rule::Terminal, Rule::Agents] {
            assert_eq!(
                behind_of(deep, 1000, &none, rule, 100),
                Behind::Agent(JOB.into())
            );
        }
        let ends = |pid: u32| match pid {
            1000 => Some(proc(2000, 34900, "sh", &["sh"])),
            2000 => Some(proc(1, 34900, "bash", &["bash"])),
            _ => None,
        };
        assert!(!under_in(ends, 1000, 100));
    }

    /// On this machine: a process this one started is a job's, and this
    /// process's own pid is under itself.
    #[test]
    fn a_real_child_of_this_process_is_a_jobs() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("sleep starts");
        let job = Behind::Agent(JOB.into());
        let asked = [Rule::Terminal, Rule::Agents].map(|r| behind(child.id(), &[], r));
        child.kill().ok();
        child.wait().ok();
        assert_eq!(asked, [job.clone(), job]);
    }

    /// The web port's way in too: the holder of a loopback client's end is
    /// found as it is on the web, and one under the daemon is a job's. Here
    /// this process holds both ends, so the holder is the daemon itself.
    #[test]
    fn a_web_client_the_daemon_holds_is_a_jobs() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let stream = std::net::TcpStream::connect(server).unwrap();
        let client = stream.local_addr().unwrap();
        let (_accepted, _) = listener.accept().unwrap();
        let peer = Peer::new(
            Source::Web {
                server: Some(server),
                client,
            },
            Arc::from(Vec::<String>::new()),
        );
        assert_eq!(peer.behind(), &Behind::Agent(JOB.into()));
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
