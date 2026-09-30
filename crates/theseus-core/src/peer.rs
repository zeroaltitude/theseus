//! Who is asking (theseus-6qy): the process on the other end of a protocol
//! connection, and whether it descends from a Theseus job.
//!
//! At L0 a `proc.run` job runs as the operator's own OS user, so it reaches
//! the CLI socket and the loopback web UI as the operator does. So an answer
//! to a waiting call, the spend reset, and the undo of a tightening are
//! refused from a job's process (`Core::judge_act`): the asking process, or
//! any ancestor of it up to pid 1, is a live job wrapper. The wrapper is a
//! child subreaper, so a double fork stays under it. A process that cannot
//! be traced counts as a job's.
//!
//! A job can kill its own wrapper. Its orphans then go to the daemon, which
//! is a child subreaper too (theseus-z4b), and nothing that answers an
//! approval descends from the daemon: so an asker with the daemon above it is
//! refused as well, as a job's orphan.
//!
//! This is a speed bump before M4's sandbox (L1: no route to localhost
//! services), not a boundary. A job can still drive a process that is not
//! its descendant: a user systemd unit, a tmux server already running, `at`
//! or cron, or anything started outside the job that reads what it writes.

use std::net::SocketAddr;
use std::time::Instant;

use serde::Serialize;

/// The process on the other end of a connection, as its listener knows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Peer {
    /// No process to ask about: the in-process Discord binding, or a
    /// connection no listener named (a test's).
    #[default]
    None,
    /// A process, read when the connection was accepted: the Unix socket's
    /// peer (`SO_PEERCRED`), or the parent of `--stdio`. `start` is its start
    /// time (`/proc/<pid>/stat`, in clock ticks since boot), so a pid reused
    /// later is not taken for it.
    Process { pid: u32, start: u64 },
    /// The process could not be read when the connection was accepted: why.
    Unknown(String),
    /// The web UI's TCP connection, by its two ends. The process that holds
    /// the client's end is looked up only when a judged act arrives.
    Loopback {
        server: SocketAddr,
        client: SocketAddr,
    },
}

/// The process that asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Asker {
    pub pid: u32,
    /// Its program: the file name of its `argv[0]`, else its `comm`.
    pub argv0: String,
    /// How long the trace took, in µs.
    pub trace_us: u64,
}

/// Who asked, as the process tree says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Traced {
    /// There is no process to trace: Discord, or a test's connection.
    NoProcess,
    /// A process outside every job.
    Outside(Asker),
    /// A process under a live job wrapper: the job's id, and the wrapper's pid.
    Job {
        asker: Asker,
        job: String,
        wrapper: u32,
    },
    /// A process under the daemon itself, with no live wrapper between: a
    /// job's orphan, whose wrapper died (theseus-z4b). The daemon's pid.
    Orphan { asker: Asker, daemon: u32 },
    /// It could not be traced, which counts as a job's: why, and how long
    /// finding that out took, in µs.
    Untraceable { why: String, trace_us: u64 },
}

impl Traced {
    /// The reason a refusal gives, when this is a job's process or one that
    /// counts as one.
    pub fn refusal(&self) -> Option<String> {
        match self {
            Self::NoProcess | Self::Outside(_) => None,
            Self::Job { asker, job, .. } => Some(format!(
                "from a Theseus job's process (job {job}, pid {}, {})",
                asker.pid, asker.argv0
            )),
            Self::Orphan { asker, .. } => Some(format!(
                "from a process under theseusd itself (pid {}, {}), which is a job's orphan",
                asker.pid, asker.argv0
            )),
            Self::Untraceable { why, .. } => Some(format!(
                "from a process that could not be traced ({why}), which counts as a Theseus \
                 job's"
            )),
        }
    }

    /// The asker, for a ledger row.
    pub fn json(&self) -> serde_json::Value {
        match self {
            Self::NoProcess => serde_json::Value::Null,
            Self::Outside(a) => {
                serde_json::json!({"pid": a.pid, "argv0": a.argv0, "trace_us": a.trace_us})
            }
            Self::Job {
                asker,
                job,
                wrapper,
            } => serde_json::json!({
                "pid": asker.pid, "argv0": asker.argv0, "trace_us": asker.trace_us,
                "job": job, "wrapper_pid": wrapper,
            }),
            Self::Orphan { asker, daemon } => serde_json::json!({
                "pid": asker.pid, "argv0": asker.argv0, "trace_us": asker.trace_us,
                "under_daemon": daemon,
            }),
            Self::Untraceable { why, trace_us } => {
                serde_json::json!({"untraceable": why, "trace_us": trace_us})
            }
        }
    }
}

impl Peer {
    /// The Unix socket's peer, from `SO_PEERCRED`, as the connection is
    /// accepted.
    pub fn of_unix(stream: &tokio::net::UnixStream) -> Self {
        match stream.peer_cred() {
            Ok(c) => match c.pid() {
                Some(pid) if pid > 0 => Self::process(pid as u32),
                _ => Self::Unknown("the socket named no peer pid".into()),
            },
            Err(e) => Self::Unknown(format!("the socket's peer could not be read: {e}")),
        }
    }

    /// A process by pid, with its start time.
    pub fn process(pid: u32) -> Self {
        match stat(pid) {
            Ok(s) if !s.exited() => Self::Process {
                pid,
                start: s.start,
            },
            Ok(_) => Self::Unknown(format!("pid {pid} has exited")),
            Err(why) => Self::Unknown(format!("pid {pid}: {why}")),
        }
    }

    /// Trace the process that asked: the one on the other end, then each
    /// parent up to pid 1, looking for a live job wrapper, and for the daemon
    /// once it is a child subreaper (`children::adopt`).
    pub fn trace(&self) -> Traced {
        self.trace_under(theseus_kernel::children::daemon())
    }

    /// `trace`, where an asker with `daemon` above it is a job's orphan.
    fn trace_under(&self, daemon: Option<u32>) -> Traced {
        let t0 = Instant::now();
        let us = || t0.elapsed().as_micros() as u64;
        let found = match self {
            Self::None => return Traced::NoProcess,
            Self::Unknown(why) => Err(why.clone()),
            Self::Process { pid, start } => walk(*pid, Some(*start), daemon),
            Self::Loopback { server, client } => {
                loopback_owners(*server, *client).and_then(|pids| owners_verdict(&pids, daemon))
            }
        };
        match found {
            Ok(Found::Outside(mut a)) => {
                a.trace_us = us();
                Traced::Outside(a)
            }
            Ok(Found::Job {
                mut asker,
                job,
                wrapper,
            }) => {
                asker.trace_us = us();
                Traced::Job {
                    asker,
                    job,
                    wrapper,
                }
            }
            Ok(Found::Orphan { mut asker, daemon }) => {
                asker.trace_us = us();
                Traced::Orphan { asker, daemon }
            }
            Err(why) => Traced::Untraceable {
                why,
                trace_us: us(),
            },
        }
    }
}

/// What a walk found, before it is timed.
enum Found {
    Outside(Asker),
    Job {
        asker: Asker,
        job: String,
        wrapper: u32,
    },
    Orphan {
        asker: Asker,
        daemon: u32,
    },
}

/// The deepest process tree a walk follows.
const MAX_DEPTH: usize = 4096;

/// From `pid` up the parent chain to pid 1: the first live job wrapper met,
/// the asker itself included, or else `daemon` met above the asker. `start`
/// is the asker's start time when the connection was accepted. An ancestor
/// that exits mid-walk has had its children reparented, so the walk starts
/// again, at most three times.
fn walk(pid: u32, start: Option<u64>, daemon: Option<u32>) -> Result<Found, String> {
    'again: for _ in 0..3 {
        let first = stat(pid).map_err(|why| format!("pid {pid}: {why}"))?;
        if first.exited() {
            return Err(format!("pid {pid} has exited"));
        }
        if start.is_some_and(|s| s != first.start) {
            return Err(format!(
                "pid {pid} is not the process that connected: that one exited, and its pid was \
                 reused"
            ));
        }
        let asker = Asker {
            pid,
            argv0: argv0(pid).unwrap_or_else(|| first.comm.clone()),
            trace_us: 0,
        };
        let (mut p, mut s) = (pid, first);
        for _ in 0..MAX_DEPTH {
            if let Some(job) = theseus_kernel::job::wrapper_job(p) {
                return Ok(Found::Job {
                    asker,
                    job,
                    wrapper: p,
                });
            }
            if p != pid && daemon == Some(p) {
                return Ok(Found::Orphan { asker, daemon: p });
            }
            if s.ppid == 0 {
                return Ok(Found::Outside(asker));
            }
            p = s.ppid;
            s = match stat(p) {
                Ok(s) => s,
                Err(_) => continue 'again,
            };
        }
        return Err(format!(
            "pid {pid}: its ancestry is deeper than {MAX_DEPTH}"
        ));
    }
    Err(format!(
        "pid {pid}: its ancestry kept changing while it was read"
    ))
}

/// The verdict over every process that holds a socket: a job's (or a job's
/// orphan's) if any is, else the operator's if any could be traced.
fn owners_verdict(pids: &[u32], daemon: Option<u32>) -> Result<Found, String> {
    let mut outside = None;
    let mut why = None;
    for &pid in pids {
        match walk(pid, None, daemon) {
            Ok(j @ (Found::Job { .. } | Found::Orphan { .. })) => return Ok(j),
            Ok(Found::Outside(a)) => {
                outside.get_or_insert(a);
            }
            Err(w) => {
                why.get_or_insert(w);
            }
        }
    }
    match (outside, why) {
        (Some(a), _) => Ok(Found::Outside(a)),
        (None, Some(w)) => Err(w),
        (None, None) => Err("no process holds its socket".into()),
    }
}

/// `/proc/<pid>/stat`, the fields a walk needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stat {
    comm: String,
    state: char,
    ppid: u32,
    /// Clock ticks after boot.
    start: u64,
}

impl Stat {
    /// A zombie, or a process being reaped.
    fn exited(&self) -> bool {
        matches!(self.state, 'Z' | 'X')
    }
}

fn stat(pid: u32) -> Result<Stat, String> {
    let text =
        std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => "no such process".to_string(),
            _ => format!("/proc/{pid}/stat: {e}"),
        })?;
    parse_stat(&text).ok_or_else(|| format!("/proc/{pid}/stat could not be read: {text:?}"))
}

/// `pid (comm) state ppid …`, with the start time the 22nd field. The comm
/// may hold spaces and parentheses, so the fields start after its last `)`.
fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?.to_string();
    let f: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    Some(Stat {
        comm,
        state: f.first()?.chars().next()?,
        ppid: f.get(1)?.parse().ok()?,
        start: f.get(19)?.parse().ok()?,
    })
}

/// The file name of a process's `argv[0]`; None for an empty command line
/// (a zombie, a kernel thread).
fn argv0(pid: u32) -> Option<String> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let first = cmdline
        .split(|&b| b == 0)
        .next()
        .filter(|a| !a.is_empty())?;
    let first = String::from_utf8_lossy(first);
    Some(
        std::path::Path::new(first.as_ref())
            .file_name()
            .map_or_else(|| first.to_string(), |n| n.to_string_lossy().into_owned()),
    )
}

/// The processes that hold the client's end of a loopback TCP connection
/// (theseus-6qy). The client's row in `/proc/net/tcp` (or `tcp6`) is the one
/// whose local address is the client's and whose remote address is the
/// server's; its inode is the socket, and each `/proc/<pid>/fd` entry linked
/// to `socket:[inode]` holds it. Only this account's processes can be read,
/// so another account's client is found by no one.
pub fn loopback_owners(server: SocketAddr, client: SocketAddr) -> Result<Vec<u32>, String> {
    let table = if client.is_ipv4() {
        "/proc/net/tcp"
    } else {
        "/proc/net/tcp6"
    };
    let text = std::fs::read_to_string(table).map_err(|e| format!("{table}: {e}"))?;
    let Some((inode, uid)) = socket_row(&text, client, server) else {
        return Err(format!(
            "no socket on this machine is the client end of the connection from {client}"
        ));
    };
    if inode == 0 {
        return Err(format!("the connection from {client} has closed"));
    }
    let pids = holders(inode);
    if pids.is_empty() {
        return Err(format!(
            "no process this account can read holds the connection from {client} (its socket \
             belongs to uid {uid})"
        ));
    }
    Ok(pids)
}

/// An address as `/proc/net/tcp` and `tcp6` print it: each 32-bit word of the
/// address in the host's byte order, in hex, then the port.
fn proc_net_addr(a: SocketAddr) -> String {
    let words: Vec<u8> = match a.ip() {
        std::net::IpAddr::V4(ip) => ip.octets().to_vec(),
        std::net::IpAddr::V6(ip) => ip.octets().to_vec(),
    };
    let mut s = String::new();
    for w in words.chunks(4) {
        let word = u32::from_ne_bytes([w[0], w[1], w[2], w[3]]);
        s.push_str(&format!("{word:08X}"));
    }
    format!("{s}:{:04X}", a.port())
}

/// The inode and owner of the socket whose local end is `local` and whose
/// remote end is `remote`, from the text of `/proc/net/tcp` or `tcp6`.
fn socket_row(table: &str, local: SocketAddr, remote: SocketAddr) -> Option<(u64, u32)> {
    let (l, r) = (proc_net_addr(local), proc_net_addr(remote));
    table.lines().skip(1).find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        (f.get(1)?.eq_ignore_ascii_case(&l) && f.get(2)?.eq_ignore_ascii_case(&r))
            .then(|| Some((f.get(9)?.parse().ok()?, f.get(7)?.parse().ok()?)))
            .flatten()
    })
}

/// Every process whose file table holds `socket:[inode]`. This process is
/// the server, and is skipped.
fn holders(inode: u64) -> Vec<u32> {
    let want = format!("socket:[{inode}]");
    let me = std::process::id();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    let mut out = Vec::new();
    for e in procs.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else {
            continue;
        };
        if fds
            .flatten()
            .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l.as_os_str() == want.as_str()))
        {
            out.push(pid);
        }
    }
    out
}

/// A stand-in for a job wrapper, for tests (theseus-6qy). `flock` holds a
/// lock file and runs its command as a child. With the lock file named
/// `job-wrapper`, its command line is a wrapper's: the mode word first, then
/// `--correlation-id <id>` among its arguments. Under it, `sleep` stands for
/// a job's process.
#[cfg(test)]
pub(crate) struct Standin {
    group: u32,
    /// The stand-in wrapper's pid.
    pub wrapper: u32,
    /// The pid of the `sleep` under it.
    pub child: u32,
    _dir: tempfile::TempDir,
}

#[cfg(test)]
impl Standin {
    pub(crate) fn start(job: &str) -> Self {
        use std::os::unix::process::CommandExt;
        let dir = tempfile::tempdir().unwrap();
        let wrapper = std::process::Command::new("flock")
            .current_dir(dir.path())
            .args([
                theseus_kernel::job::WRAPPER_MODE,
                "sh",
                "-c",
                "sleep 60 & echo $! > child.pid; wait",
                "--correlation-id",
                job,
            ])
            .process_group(0)
            .spawn()
            .expect("flock, from util-linux, runs the stand-in wrapper")
            .id();
        let pid_file = dir.path().join("child.pid");
        let t0 = Instant::now();
        let child = loop {
            let pid = std::fs::read_to_string(&pid_file)
                .ok()
                .filter(|s| s.ends_with('\n'))
                .and_then(|s| s.trim().parse().ok());
            let exec_d = pid.is_some_and(|p: u32| {
                std::fs::read_to_string(format!("/proc/{p}/comm"))
                    .is_ok_and(|c| c.trim() == "sleep")
            });
            match pid {
                Some(p) if exec_d => break p,
                _ => {
                    assert!(t0.elapsed().as_secs() < 10, "the stand-in never started");
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        };
        Self {
            group: wrapper,
            wrapper,
            child,
            _dir: dir,
        }
    }
}

#[cfg(test)]
impl Drop for Standin {
    fn drop(&mut self) {
        let _ = std::process::Command::new("kill")
            .args(["-9", "--", &format!("-{}", self.group)])
            .status();
    }
}

/// For tests that need a process outside every job.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::{Peer, Traced};

    /// Whether this test process itself runs under a Theseus job, as when an
    /// agent runs the gate through `proc.run`. Then nothing it starts is
    /// outside a job, and a test that needs such a process says so and stops.
    pub(crate) fn inside_a_job() -> bool {
        match Peer::process(std::process::id()).trace() {
            Traced::Job { job, .. } => {
                eprintln!("skipped the rest: this test runs inside Theseus job {job}");
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::inside_a_job;
    use super::*;

    #[test]
    fn stat_reads_a_comm_with_spaces_and_parentheses() {
        let text = "4242 (a (b) c) S 17 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 \
                    1000 100 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        assert_eq!(
            parse_stat(text),
            Some(Stat {
                comm: "a (b) c".into(),
                state: 'S',
                ppid: 17,
                start: 987654,
            })
        );
        assert_eq!(parse_stat("12 (x) S"), None);
        let me = stat(std::process::id()).unwrap();
        assert!(!me.exited() && me.ppid > 0 && me.start > 0);
    }

    /// `/proc/net/tcp` and `tcp6` print each 32-bit word of an address in the
    /// host's byte order, and the port in hex.
    #[test]
    fn addresses_are_written_as_proc_net_writes_them() {
        let v4: SocketAddr = "127.0.0.1:7433".parse().unwrap();
        let v6: SocketAddr = "[::1]:7433".parse().unwrap();
        if cfg!(target_endian = "little") {
            assert_eq!(proc_net_addr(v4), "0100007F:1D09");
            assert_eq!(proc_net_addr(v6), "00000000000000000000000001000000:1D09");
        }
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 0100007F:1D09 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 11111 1 0000000000000000 100 0 0 10 0\n\
   1: 0100007F:1D09 0100007F:B41A 01 00000000:00000000 00:00000000 00000000  1000        0 22222 1 0000000000000000 20 4 30 10 -1\n\
   2: 0100007F:B41A 0100007F:1D09 01 00000000:00000000 00:00000000 00000000  1001        0 33333 1 0000000000000000 20 4 30 10 -1\n";
        let client: SocketAddr = "127.0.0.1:46106".parse().unwrap();
        if cfg!(target_endian = "little") {
            assert_eq!(socket_row(table, client, v4), Some((33333, 1001)));
            assert_eq!(socket_row(table, v4, client), Some((22222, 1000)));
        }
        assert_eq!(socket_row(table, "127.0.0.1:1".parse().unwrap(), v4), None);
    }

    /// A process under a live job wrapper is that job's, the wrapper itself
    /// included; the refusal names the job, the pid, and the program.
    #[test]
    fn a_process_under_a_job_wrapper_is_that_jobs() {
        let s = Standin::start("act_standin");
        let Traced::Job {
            asker,
            job,
            wrapper,
        } = Peer::process(s.child).trace()
        else {
            panic!("not the job's");
        };
        assert_eq!((asker.pid, asker.argv0.as_str()), (s.child, "sleep"));
        assert_eq!((job.as_str(), wrapper), ("act_standin", s.wrapper));
        let t = Peer::process(s.child).trace();
        assert_eq!(
            t.refusal().unwrap(),
            format!(
                "from a Theseus job's process (job act_standin, pid {}, sleep)",
                s.child
            )
        );
        assert_eq!(t.json()["job"], "act_standin");
        assert!(matches!(
            Peer::process(s.wrapper).trace(),
            Traced::Job { wrapper, .. } if wrapper == s.wrapper
        ));
    }

    /// A process under the daemon itself, with no live wrapper between, is a
    /// job's orphan, whose wrapper died (theseus-z4b), and is refused with its
    /// own reason, on the socket and through the web UI. The daemon is not
    /// under itself, and a wrapper met first still names the job. The test
    /// process stands for the daemon here.
    #[test]
    fn a_process_under_the_daemon_is_a_jobs_orphan() {
        use std::io::BufRead;
        let me = std::process::id();
        let mut sleep = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = sleep.id();
        let t = Peer::process(pid).trace_under(Some(me));
        let Traced::Orphan { asker, daemon } = &t else {
            panic!("{t:?}");
        };
        assert_eq!(
            (asker.pid, asker.argv0.as_str(), *daemon),
            (pid, "sleep", me)
        );
        assert_eq!(
            t.refusal().unwrap(),
            format!(
                "from a process under theseusd itself (pid {pid}, sleep), which is a job's orphan"
            )
        );
        assert_eq!(t.json()["under_daemon"], me);
        assert!(!matches!(
            Peer::process(me).trace_under(Some(me)),
            Traced::Orphan { .. }
        ));
        // A process that never adopted records no daemon.
        if !inside_a_job() {
            assert!(matches!(
                Peer::process(pid).trace_under(None),
                Traced::Outside(_)
            ));
        }
        let s = Standin::start("act_under_daemon");
        assert!(matches!(
            Peer::process(s.child).trace_under(Some(me)),
            Traced::Job { job, .. } if job == "act_under_daemon"
        ));
        // The web UI's client, a bash under the daemon holding its socket.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let mut bash = std::process::Command::new("bash")
            .args([
                "-c",
                &format!(
                    "exec 3<>/dev/tcp/127.0.0.1/{}; echo $$; read -r _ <&3",
                    server.port()
                ),
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(bash.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        let (stream, client) = listener.accept().unwrap();
        let t = Peer::Loopback { server, client }.trace_under(Some(me));
        assert!(
            matches!(&t, Traced::Orphan { asker, .. } if asker.pid.to_string() == line.trim() && asker.argv0 == "bash"),
            "{t:?}"
        );
        drop(stream);
        let _ = bash.wait();
        let _ = sleep.kill();
        let _ = sleep.wait();
    }

    /// The operator's own process is outside every job, and counts.
    #[test]
    fn a_process_outside_every_job_counts() {
        if inside_a_job() {
            return;
        }
        let t = Peer::process(std::process::id()).trace();
        let Traced::Outside(a) = &t else {
            panic!("{t:?}");
        };
        assert_eq!(a.pid, std::process::id());
        assert_eq!(t.refusal(), None);
        assert_eq!(Peer::None.trace(), Traced::NoProcess);
    }

    /// A peer that exited, whose pid was reused, or that could not be read
    /// counts as a job's.
    #[test]
    fn a_process_that_cannot_be_traced_counts_as_a_jobs() {
        let mut gone = std::process::Command::new("true").spawn().unwrap();
        let pid = gone.id();
        gone.wait().unwrap();
        let p = Peer::process(pid);
        assert!(
            matches!(&p, Peer::Unknown(w) if w.contains("no such process")),
            "{p:?}"
        );
        let why = |p: Peer| match p.trace() {
            Traced::Untraceable { why, .. } => why,
            t => panic!("{t:?}"),
        };
        assert!(why(p).contains("no such process"));
        let Peer::Process { start, .. } = Peer::process(std::process::id()) else {
            panic!("this process has a start time");
        };
        let reused = Peer::Process {
            pid: std::process::id(),
            start: start + 1,
        };
        assert!(why(reused).contains("its pid was reused"));
        let t = Peer::Unknown("the socket named no peer pid".into()).trace();
        assert_eq!(
            t.refusal().unwrap(),
            "from a process that could not be traced (the socket named no peer pid), which \
             counts as a Theseus job's"
        );
    }

    /// The web UI's client is the process that holds the client's end of the
    /// loopback connection: found from `/proc/net/tcp` and `/proc/*/fd`,
    /// under a job or outside one. A connection whose client end is gone is
    /// found by no one, and counts as a job's.
    #[test]
    fn the_web_uis_client_is_the_process_that_holds_its_socket() {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let script = format!(
            "exec 3<>/dev/tcp/127.0.0.1/{}; echo $$; read -r _ <&3",
            server.port()
        );
        let connect =
            |standin: Option<&str>| -> (std::process::Child, u32, SocketAddr, std::net::TcpStream) {
                let mut c = match standin {
                    None => std::process::Command::new("bash"),
                    Some(dir) => {
                        let mut c = std::process::Command::new("flock");
                        c.current_dir(dir)
                            .args([theseus_kernel::job::WRAPPER_MODE, "bash"]);
                        c
                    }
                };
                c.args(["-c", &script]);
                if standin.is_some() {
                    c.args(["--correlation-id", "act_web"]);
                }
                let mut child = c.stdout(std::process::Stdio::piped()).spawn().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(child.stdout.as_mut().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let (stream, client) = listener.accept().unwrap();
                (child, line.trim().parse().unwrap(), client, stream)
            };
        // Outside every job.
        if !inside_a_job() {
            let (mut child, bash, client, mut stream) = connect(None);
            let t0 = Instant::now();
            let t = Peer::Loopback { server, client }.trace();
            eprintln!("loopback trace: {} µs", t0.elapsed().as_micros());
            assert!(
                matches!(&t, Traced::Outside(a) if a.pid == bash && a.argv0 == "bash"),
                "{t:?}"
            );
            stream.write_all(b"done\n").unwrap();
            child.wait().unwrap();
            // The client is gone: its end has no holder.
            let t = Peer::Loopback { server, client }.trace();
            assert!(matches!(&t, Traced::Untraceable { .. }), "{t:?}");
        }
        // Under a job.
        let dir = tempfile::tempdir().unwrap();
        let (mut child, bash, client, mut stream) = connect(Some(dir.path().to_str().unwrap()));
        let t = Peer::Loopback { server, client }.trace();
        assert!(
            matches!(&t, Traced::Job { asker, job, wrapper } if asker.pid == bash && job == "act_web" && *wrapper == child.id()),
            "{t:?}"
        );
        stream.write_all(b"done\n").unwrap();
        child.wait().unwrap();
    }
}
