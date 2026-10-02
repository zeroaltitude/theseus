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
//! refused as well, as a job's orphan. So is one under any other serving
//! `theseusd`, known by its command line (theseus-6uo): a scratch daemon's
//! orphan, or a `--stdio` daemon's, which is a subreaper too, runs as the
//! operator's user and could otherwise answer this one.
//!
//! This is a speed bump before M4's sandbox (L1: no route to localhost
//! services), not a boundary. A job can still drive a process that is not
//! its descendant: a user systemd unit, a tmux server already running, `at`
//! or cron, or anything started outside the job that reads what it writes.
//!
//! Another user's process is a boundary, and is kept out before any of this:
//! the Unix socket is 0600, and the web UI refuses a connection whose client
//! socket another uid owns as it accepts it (`client_uid`, theseus-3qf).

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
    /// A process under another serving `theseusd`, with no live wrapper
    /// between: that daemon's orphan, or a process it runs (theseus-6uo).
    /// That daemon's pid.
    OtherDaemon { asker: Asker, daemon: u32 },
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
            Self::OtherDaemon { asker, daemon } => Some(format!(
                "from a process under another serving theseusd (pid {}, {}; the daemon is pid \
                 {daemon}), which counts as that daemon's job",
                asker.pid, asker.argv0
            )),
            Self::Untraceable { why, .. } => Some(format!(
                "from a process that could not be traced ({why}), which counts as a Theseus \
                 job's"
            )),
        }
    }

    /// The asker, as `approval.refused` and its ledger row name it; none
    /// when no process answered.
    pub fn asker(&self) -> Option<theseus_protocol::Asker> {
        use theseus_protocol::Asker as A;
        let seen = |a: &Asker| A {
            pid: Some(a.pid),
            argv0: Some(a.argv0.clone()),
            trace_us: a.trace_us,
            ..A::default()
        };
        Some(match self {
            Self::NoProcess => return None,
            Self::Outside(a) => seen(a),
            Self::Job {
                asker,
                job,
                wrapper,
            } => A {
                job: Some(job.clone()),
                wrapper_pid: Some(*wrapper),
                ..seen(asker)
            },
            Self::Orphan { asker, daemon } => A {
                under_daemon: Some(*daemon),
                ..seen(asker)
            },
            Self::OtherDaemon { asker, daemon } => A {
                under_other_daemon: Some(*daemon),
                ..seen(asker)
            },
            Self::Untraceable { why, trace_us } => A {
                untraceable: Some(why.clone()),
                trace_us: *trace_us,
                ..A::default()
            },
        })
    }

    /// The asker, for a ledger row.
    pub fn json(&self) -> serde_json::Value {
        serde_json::to_value(self.asker()).unwrap_or_default()
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
            Ok(Found::OtherDaemon { mut asker, daemon }) => {
                asker.trace_us = us();
                Traced::OtherDaemon { asker, daemon }
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
    OtherDaemon {
        asker: Asker,
        daemon: u32,
    },
}

/// The deepest process tree a walk follows.
const MAX_DEPTH: usize = 4096;

/// From `pid` up the parent chain to pid 1: the first live job wrapper met,
/// the asker itself included, or else `daemon`, or any other serving
/// `theseusd`, met above the asker. Each process's command line is read once.
/// `start`
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
            let cmdline = std::fs::read(format!("/proc/{p}/cmdline")).unwrap_or_default();
            if let Some(job) = theseus_kernel::job::job_in_cmdline(&cmdline) {
                return Ok(Found::Job {
                    asker,
                    job,
                    wrapper: p,
                });
            }
            if p != pid && daemon == Some(p) {
                return Ok(Found::Orphan { asker, daemon: p });
            }
            if p != pid && theseus_kernel::job::daemon_in_cmdline(&cmdline) {
                return Ok(Found::OtherDaemon { asker, daemon: p });
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
            Ok(j @ (Found::Job { .. } | Found::Orphan { .. } | Found::OtherDaemon { .. })) => {
                return Ok(j)
            }
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
    socket_rows(table, local, remote).next()
}

/// Every such row: a closed socket's (no inode) can stand beside a live one.
fn socket_rows(
    table: &str,
    local: SocketAddr,
    remote: SocketAddr,
) -> impl Iterator<Item = (u64, u32)> + '_ {
    let (l, r) = (proc_net_addr(local), proc_net_addr(remote));
    table.lines().skip(1).filter_map(move |line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        (f.get(1)?.eq_ignore_ascii_case(&l) && f.get(2)?.eq_ignore_ascii_case(&r))
            .then(|| Some((f.get(9)?.parse().ok()?, f.get(7)?.parse().ok()?)))
            .flatten()
    })
}

/// Whose is the client's end of a loopback TCP connection to this process
/// (theseus-3qf): the uid that owns its socket, on its row in
/// `/proc/net/tcp` or `tcp6`. The web UI reads it as it accepts each
/// connection, before any request: its `Host` and `Origin` checks keep out
/// other pages in the operator's browser, but any local process can send
/// both, and the TCP port, unlike the Unix socket (0600), is open to every
/// user on the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientUid {
    /// The client's row was found: the uid that owns its socket.
    Uid(u32),
    /// The client's row holds no socket any more (no inode): it closed
    /// before the accept, as a port probe does. A closed socket's row may
    /// show uid 0 (TIME_WAIT), so its uid is not read.
    Closed,
    /// No row could be read for it, or no table at all: why.
    Unknown(String),
    /// This platform keeps no such table (not Linux).
    NoTable,
}

/// The owner of the client's end of the connection from `client` to
/// `server`, both as the server's socket names them: asked of the kernel by
/// the four-tuple (`sock_diag`, theseus-u6xg), or, where the kernel will not
/// say, read from `/proc/net/tcp` and `tcp6`.
pub fn client_uid(server: SocketAddr, client: SocketAddr) -> ClientUid {
    if !cfg!(target_os = "linux") {
        return ClientUid::NoTable;
    }
    match client_uid_by(sock_diag, server, client) {
        Ok(found) => found,
        Err(why) => {
            tracing::debug!(why = %why, "sock_diag would not say; reading /proc/net/tcp");
            client_uid_in(
                |t| std::fs::read_to_string(t).map_err(|e| format!("{t}: {e}")),
                server,
                client,
            )
        }
    }
}

/// The forms the pair can take, the accepted form first, each with the table
/// `/proc` keeps it in, and the client's socket's (local, remote): an IPv4
/// pair in `tcp`, and also as IPv4-mapped IPv6 in `tcp6` (a client on an
/// IPv6 socket that connected to the mapped address); an IPv6 pair in
/// `tcp6`, and a mapped one also in `tcp` (a client on an IPv4 socket, which
/// a dual-stack listener accepts as mapped).
fn forms(server: SocketAddr, client: SocketAddr) -> Vec<(&'static str, SocketAddr, SocketAddr)> {
    use std::net::IpAddr;
    let v4 = |a: SocketAddr| match a.ip() {
        IpAddr::V4(_) => Some(a),
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(|ip| SocketAddr::new(ip.into(), a.port())),
    };
    let v6 = |a: SocketAddr| match a.ip() {
        IpAddr::V4(ip) => SocketAddr::new(ip.to_ipv6_mapped().into(), a.port()),
        IpAddr::V6(_) => a,
    };
    let mut forms = vec![("/proc/net/tcp6", v6(client), v6(server))];
    if let (Some(c), Some(s)) = (v4(client), v4(server)) {
        let as_v4 = ("/proc/net/tcp", c, s);
        if client.is_ipv4() {
            forms.insert(0, as_v4);
        } else {
            forms.push(as_v4);
        }
    }
    forms
}

/// `client_uid` by a lookup of each form's socket (`find`: the kernel's,
/// or a test's), which answers (inode, uid): a live socket wins over a
/// closed one, and no socket is unknown. `Err` when the lookup could not
/// answer, and the caller reads the tables instead.
fn client_uid_by(
    find: impl Fn(SocketAddr, SocketAddr) -> Result<Option<(u64, u32)>, String>,
    server: SocketAddr,
    client: SocketAddr,
) -> Result<ClientUid, String> {
    let mut closed = false;
    for (_, local, remote) in forms(server, client) {
        match find(local, remote)? {
            Some((inode, uid)) if inode != 0 => return Ok(ClientUid::Uid(uid)),
            Some(_) => closed = true,
            None => {}
        }
    }
    if closed {
        return Ok(ClientUid::Closed);
    }
    Ok(ClientUid::Unknown(format!(
        "no socket on this machine is the client end of the connection from {client}"
    )))
}

/// The socket whose local end is `local` and whose remote end is `remote`,
/// asked of the kernel by its four-tuple (theseus-u6xg): one netlink
/// `SOCK_DIAG_BY_FAMILY` request with no dump flag, which the kernel answers
/// from its hash of connections, where a read of `/proc/net/tcp` walks every
/// bucket of it (about 2 ms here). `Ok(Some((inode, uid)))`: found, its inode
/// 0 once it has closed (TIME_WAIT); `Ok(None)`: no such socket; `Err`: the
/// kernel would not say (no sock_diag), and the caller reads the tables.
fn sock_diag(local: SocketAddr, remote: SocketAddr) -> Result<Option<(u64, u32)>, String> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    const NETLINK_SOCK_DIAG: libc::c_int = 4;
    const SOCK_DIAG_BY_FAMILY: u16 = 20;
    const NLM_F_REQUEST: u16 = 1;
    const NLMSG_ERROR: u16 = 2;
    const NLMSG_DONE: u16 = 3;
    const HEADER: usize = 16;
    const REQUEST: usize = 56;
    // inet_diag_msg: family, state, timer, retrans (4); the socket's id
    // (48); expires, rqueue, wqueue (12); uid; inode.
    const UID_AT: usize = HEADER + 4 + 48 + 12;
    if local.is_ipv4() != remote.is_ipv4() {
        return Ok(None);
    }
    let family = if local.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    // An address as the kernel keeps it: in network order.
    let words = |a: SocketAddr| -> [u8; 16] {
        let mut w = [0u8; 16];
        match a.ip() {
            std::net::IpAddr::V4(ip) => w[..4].copy_from_slice(&ip.octets()),
            std::net::IpAddr::V6(ip) => w.copy_from_slice(&ip.octets()),
        }
        w
    };
    let mut req = Vec::with_capacity(HEADER + REQUEST);
    req.extend_from_slice(&((HEADER + REQUEST) as u32).to_ne_bytes());
    req.extend_from_slice(&SOCK_DIAG_BY_FAMILY.to_ne_bytes());
    req.extend_from_slice(&NLM_F_REQUEST.to_ne_bytes());
    req.extend_from_slice(&1u32.to_ne_bytes()); // seq
    req.extend_from_slice(&0u32.to_ne_bytes()); // pid: the kernel's
    req.push(family as u8);
    req.push(libc::IPPROTO_TCP as u8);
    req.push(0); // no extensions
    req.push(0);
    req.extend_from_slice(&u32::MAX.to_ne_bytes()); // every state
    req.extend_from_slice(&local.port().to_be_bytes());
    req.extend_from_slice(&remote.port().to_be_bytes());
    req.extend_from_slice(&words(local));
    req.extend_from_slice(&words(remote));
    req.extend_from_slice(&0u32.to_ne_bytes()); // any interface
    req.extend_from_slice(&u32::MAX.to_ne_bytes()); // no cookie
    req.extend_from_slice(&u32::MAX.to_ne_bytes());

    // SAFETY: socket(2) takes no pointers; a negative result is an error.
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            NETLINK_SOCK_DIAG,
        )
    };
    if fd < 0 {
        return Err(format!("sock_diag: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: `fd` is the socket just opened, owned from here on.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let timeout = libc::timeval {
        tv_sec: 1,
        tv_usec: 0,
    };
    // SAFETY: the value is a timeval that outlives the call, of the size given.
    unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&raw const timeout).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    // SAFETY: all zeroes is a valid sockaddr_nl: the kernel, no groups.
    let mut kernel: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    // SAFETY: the buffer and the address are valid for their lengths for the
    // call's duration.
    let sent = unsafe {
        libc::sendto(
            fd.as_raw_fd(),
            req.as_ptr().cast(),
            req.len(),
            0,
            (&raw const kernel).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if sent < 0 {
        return Err(format!("sock_diag: {}", std::io::Error::last_os_error()));
    }
    let mut buf = [0u8; 8192];
    // SAFETY: the buffer is valid for its length for the call's duration.
    let n = unsafe { libc::recv(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
    if n < 0 {
        return Err(format!("sock_diag: {}", std::io::Error::last_os_error()));
    }
    let b = &buf[..n as usize];
    if b.len() < HEADER {
        return Err("sock_diag: a short answer".into());
    }
    let u32_at = |i: usize| u32::from_ne_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    match u16::from_ne_bytes([b[4], b[5]]) {
        SOCK_DIAG_BY_FAMILY if b.len() >= UID_AT + 8 => {
            Ok(Some((u64::from(u32_at(UID_AT + 4)), u32_at(UID_AT))))
        }
        NLMSG_ERROR if b.len() >= HEADER + 4 => match -(u32_at(HEADER) as i32) {
            libc::ENOENT => Ok(None),
            errno => Err(format!(
                "sock_diag: {}",
                std::io::Error::from_raw_os_error(errno)
            )),
        },
        NLMSG_DONE => Ok(None),
        t => Err(format!("sock_diag: an answer of type {t}")),
    }
}

/// `client_uid` over the tables `read` gives (tests pass fixtures): each
/// form's table, a live row over a closed one. A table read while sockets
/// come and go can skip a row, so a miss is read once more before it counts.
fn client_uid_in(
    read: impl Fn(&str) -> Result<String, String>,
    server: SocketAddr,
    client: SocketAddr,
) -> ClientUid {
    let forms = forms(server, client);
    let mut why = String::new();
    for _ in 0..2 {
        let (mut read_one, mut unread, mut closed) = (false, None, false);
        for (table, local, remote) in &forms {
            match read(table) {
                Ok(text) => {
                    read_one = true;
                    for (inode, uid) in socket_rows(&text, *local, *remote) {
                        if inode != 0 {
                            return ClientUid::Uid(uid);
                        }
                        closed = true;
                    }
                }
                Err(e) => {
                    unread.get_or_insert(e);
                }
            }
        }
        if closed {
            return ClientUid::Closed;
        }
        why = match unread.filter(|_| !read_one) {
            Some(e) => e,
            None => format!(
                "no socket on this machine is the client end of the connection from {client}"
            ),
        };
    }
    ClientUid::Unknown(why)
}

/// This process's effective uid: the owner its own sockets carry.
pub fn own_uid() -> u32 {
    // SAFETY: geteuid takes nothing and cannot fail.
    unsafe { libc::geteuid() }
}

/// The web UI's verdict on a connection (theseus-3qf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admit {
    /// Its client's socket is the daemon's own uid's, or the platform keeps
    /// no table of owners.
    Serve,
    /// The client closed before the accept: no one is there to serve, or to
    /// refuse, so nothing is counted.
    Gone,
    /// Another uid's, or its owner could not be read: why, and the client's
    /// uid when known.
    Refuse { why: String, uid: Option<u32> },
}

/// The verdict on a connection whose client socket `owner` owns, for a
/// daemon running as `me`.
pub fn admit(owner: &ClientUid, me: u32) -> Admit {
    match owner {
        ClientUid::Uid(uid) if *uid == me => Admit::Serve,
        ClientUid::Uid(uid) => Admit::Refuse {
            why: format!("its socket belongs to uid {uid}, not the daemon's ({me})"),
            uid: Some(*uid),
        },
        ClientUid::Closed => Admit::Gone,
        ClientUid::Unknown(why) => Admit::Refuse {
            why: format!("its socket's owner could not be read: {why}"),
            uid: None,
        },
        ClientUid::NoTable => Admit::Serve,
    }
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

    /// One `/proc/net/tcp` or `tcp6` row: the socket from `local` to
    /// `remote`, owned by `uid`.
    fn row(n: usize, local: SocketAddr, remote: SocketAddr, uid: u32, inode: u64) -> String {
        format!(
            "  {n:>2}: {} {} 01 00000000:00000000 00:00000000 00000000  {uid:>4}        0 {inode} \
             1 0000000000000000 20 4 30 10 -1\n",
            proc_net_addr(local),
            proc_net_addr(remote)
        )
    }

    const HEAD: &str =
        "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   \
                        uid  timeout inode\n";

    /// The web UI's check at accept (theseus-3qf), from table fixtures: the
    /// daemon's own uid passes; another uid is refused, with it; a missing
    /// row or no table at all is refused, with why; a client that closed
    /// first is gone, not refused; the tcp6 and IPv4-mapped forms are found.
    #[test]
    fn a_web_client_of_another_uid_is_refused_from_the_tables() {
        let s4: SocketAddr = "127.0.0.1:7436".parse().unwrap();
        let c4: SocketAddr = "127.0.0.1:46106".parse().unwrap();
        let s6: SocketAddr = "[::1]:7436".parse().unwrap();
        let c6: SocketAddr = "[::1]:46107".parse().unwrap();
        let mapped = |a: SocketAddr| match a.ip() {
            std::net::IpAddr::V4(ip) => SocketAddr::new(ip.to_ipv6_mapped().into(), a.port()),
            _ => a,
        };
        let (me, other) = (1000, 65534);
        // The server's end is in the table too, owned by the daemon: only the
        // client's end counts.
        let tcp = format!(
            "{HEAD}{}{}",
            row(0, s4, c4, me, 11111),
            row(1, c4, s4, other, 22222)
        );
        let tcp6 = format!(
            "{HEAD}{}{}{}",
            row(0, s6, c6, me, 33333),
            row(1, c6, s6, me, 44444),
            row(2, mapped(c4), mapped(s4), other, 55555)
        );
        let tables = |tcp: Option<&str>, tcp6: Option<&str>| {
            let (tcp, tcp6) = (tcp.map(str::to_string), tcp6.map(str::to_string));
            move |t: &str| {
                let found = if t.ends_with("tcp6") { &tcp6 } else { &tcp };
                found
                    .clone()
                    .ok_or_else(|| format!("{t}: No such file or directory"))
            }
        };
        let both = || tables(Some(&tcp), Some(&tcp6));
        let verdict = |owner: ClientUid| admit(&owner, me);

        // Another uid's client on IPv4: refused, with its uid.
        let o = client_uid_in(both(), s4, c4);
        assert_eq!(o, ClientUid::Uid(other));
        assert_eq!(
            verdict(o),
            Admit::Refuse {
                why: "its socket belongs to uid 65534, not the daemon's (1000)".into(),
                uid: Some(other)
            }
        );
        // The daemon's own uid on IPv6: served.
        assert_eq!(client_uid_in(both(), s6, c6), ClientUid::Uid(me));
        assert_eq!(verdict(client_uid_in(both(), s6, c6)), Admit::Serve);
        // An IPv4-mapped pair, as a dual-stack listener accepts it: found in
        // tcp6 as mapped, and in tcp as plain IPv4 when the client's socket
        // is IPv4's.
        assert_eq!(
            client_uid_in(both(), mapped(s4), mapped(c4)),
            ClientUid::Uid(other)
        );
        let only_v4 = format!("{HEAD}{}", row(0, c4, s4, me, 66666));
        assert_eq!(
            client_uid_in(tables(Some(&only_v4), Some(HEAD)), mapped(s4), mapped(c4)),
            ClientUid::Uid(me)
        );
        // A v4 pair whose client is on an IPv6 socket: found in tcp6 as mapped.
        assert_eq!(
            client_uid_in(tables(Some(HEAD), Some(&tcp6)), s4, c4),
            ClientUid::Uid(other)
        );
        // A missing row: refused, and why.
        let gone: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let o = client_uid_in(both(), s4, gone);
        assert_eq!(
            o,
            ClientUid::Unknown(
                "no socket on this machine is the client end of the connection from \
                 127.0.0.1:40000"
                    .into()
            )
        );
        assert_eq!(
            verdict(o),
            Admit::Refuse {
                why: "its socket's owner could not be read: no socket on this machine is the \
                      client end of the connection from 127.0.0.1:40000"
                    .into(),
                uid: None
            }
        );
        // A row skipped by one read (sockets coming and going) is found by
        // the second.
        let reads = std::cell::Cell::new(0);
        let late = |t: &str| {
            reads.set(reads.get() + 1);
            match (t.ends_with("tcp6"), reads.get() > 2) {
                (false, true) => Ok(tcp.clone()),
                _ => Ok(HEAD.to_string()),
            }
        };
        assert_eq!(client_uid_in(late, s4, c4), ClientUid::Uid(other));
        assert_eq!(reads.get(), 3);
        // A client that closed before the accept: its row holds no socket
        // (and a TIME_WAIT row says uid 0). Gone: not served, not refused.
        let closed = format!("{HEAD}{}", row(0, c4, s4, 0, 0));
        let o = client_uid_in(tables(Some(&closed), None), s4, c4);
        assert_eq!(o, ClientUid::Closed);
        assert_eq!(verdict(o), Admit::Gone);
        // A live row beside a closed one with the same ends wins.
        let reused = format!(
            "{HEAD}{}{}",
            row(0, c4, s4, 0, 0),
            row(1, c4, s4, me, 77777)
        );
        assert_eq!(
            client_uid_in(tables(Some(&reused), None), s4, c4),
            ClientUid::Uid(me)
        );
        // No table could be read: refused, with the error.
        assert!(matches!(
            client_uid_in(tables(None, None), s4, c4),
            ClientUid::Unknown(w) if w.starts_with("/proc/net/tcp: No such file")
        ));
        // A platform with no table: served (and said once, in health and the
        // log).
        assert_eq!(verdict(ClientUid::NoTable), Admit::Serve);
        if cfg!(target_endian = "little") {
            assert_eq!(
                proc_net_addr(mapped(s4)),
                "0000000000000000FFFF00000100007F:1D0C"
            );
        }
    }

    /// The real tables: this process's own connection to itself is its own
    /// uid's, as an IPv4 pair and an IPv6 one; once the client has closed,
    /// before the accept, it is gone.
    #[test]
    fn this_users_own_loopback_client_is_found_as_its_own() {
        let connect = |server: &SocketAddr| {
            std::net::TcpStream::connect_timeout(server, std::time::Duration::from_secs(5)).unwrap()
        };
        for bind in ["127.0.0.1:0", "[::1]:0"] {
            let Ok(listener) = std::net::TcpListener::bind(bind) else {
                continue; // no IPv6 on this machine
            };
            let server = listener.local_addr().unwrap();
            let _client = connect(&server);
            let (_stream, client) = listener.accept().unwrap();
            let o = client_uid(server, client);
            assert_eq!(o, ClientUid::Uid(own_uid()), "{bind}");
            assert_eq!(admit(&o, own_uid()), Admit::Serve);
            assert!(matches!(
                admit(&o, own_uid() + 1),
                Admit::Refuse { uid: Some(u), .. } if u == own_uid()
            ));
            drop(connect(&server));
            let (_stream, client) = listener.accept().unwrap();
            assert_eq!(client_uid(server, client), ClientUid::Closed, "{bind}");
        }
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

    /// theseus-u6xg: the kernel answers a live loopback connection's client
    /// end by its four-tuple, with this process's uid, and a tuple no socket
    /// has with nothing; `client_uid` takes its answer. Both ways are timed
    /// here for the report (`--no-capture`).
    #[test]
    fn sock_diag_finds_a_live_connection_by_its_four_tuple() {
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap();
        let _client = TcpStream::connect(server).unwrap();
        let (_conn, client) = listener.accept().unwrap();
        match sock_diag(client, server) {
            Ok(Some((inode, uid))) => {
                assert_ne!(inode, 0, "a live socket has an inode");
                assert_eq!(uid, own_uid());
            }
            // A kernel without sock_diag: the tables answer instead.
            Err(e) => eprintln!("sock_diag is not here ({e}); the tables answer"),
            Ok(None) => panic!("the kernel did not find a live connection"),
        }
        assert_eq!(client_uid(server, client), ClientUid::Uid(own_uid()));
        // No socket has this tuple: nothing, never an error.
        let nowhere = |p: u16| SocketAddr::from(([127, 0, 0, 1], p));
        if let Ok(found) = sock_diag(nowhere(1), nowhere(2)) {
            assert_eq!(found, None);
        }
        let time = |f: &dyn Fn()| {
            let t = Instant::now();
            for _ in 0..200 {
                f();
            }
            t.elapsed().as_secs_f64() * 1e6 / 200.0
        };
        let diag_us = time(&|| {
            let _ = sock_diag(client, server);
        });
        let tables_us = time(&|| {
            let _ = client_uid_in(
                |t| std::fs::read_to_string(t).map_err(|e| format!("{t}: {e}")),
                server,
                client,
            );
        });
        eprintln!("client_uid: sock_diag {diag_us:.1} µs, /proc/net/tcp {tables_us:.1} µs, each");
    }

    /// The lookup's verdicts: a live socket over a closed one, nothing found
    /// is unknown, and a lookup that fails is an error the caller turns into
    /// a read of the tables. A mapped form is asked too.
    #[test]
    fn client_uid_by_takes_the_live_socket_and_says_what_it_did_not_find() {
        let a = |p: u16| SocketAddr::from(([127, 0, 0, 1], p));
        let (server, client) = (a(7433), a(51000));
        let live = |_: SocketAddr, _: SocketAddr| Ok(Some((42, 1000)));
        assert_eq!(
            client_uid_by(live, server, client),
            Ok(ClientUid::Uid(1000))
        );
        let closed = |_: SocketAddr, _: SocketAddr| Ok(Some((0, 0)));
        assert_eq!(client_uid_by(closed, server, client), Ok(ClientUid::Closed));
        // Closed as IPv4, live as IPv4-mapped IPv6: live.
        let mapped =
            |l: SocketAddr, _: SocketAddr| Ok(Some(if l.is_ipv4() { (0, 0) } else { (7, 1000) }));
        assert_eq!(
            client_uid_by(mapped, server, client),
            Ok(ClientUid::Uid(1000))
        );
        let none = |_: SocketAddr, _: SocketAddr| Ok(None);
        assert!(matches!(
            client_uid_by(none, server, client),
            Ok(ClientUid::Unknown(_))
        ));
        let fails = |_: SocketAddr, _: SocketAddr| Err("no sock_diag".to_string());
        assert!(client_uid_by(fails, server, client).is_err());
    }
}
