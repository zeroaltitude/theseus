//! The web UI's other-user check (theseus-3qf). The Unix socket is mode
//! 0600, so only the account that runs Theseus reaches it; the web UI's TCP
//! port is open to every user on the machine, so it refuses a connection
//! whose client socket another uid owns, as it accepts it (`client_uid`,
//! `admit`). The owner is read from `/proc/net/tcp` and `tcp6`, about 2 ms
//! per accept, off the runtime's workers (theseus-zmgb, core review §C4).
//!
//! Who asks is not traced any more (theseus-zmgb retired theseus-6qy's walk
//! of the asking process's ancestry and the web UI's scan of every process's
//! file descriptors). L1 is the boundary: an L1 job's view has no route to
//! the daemon's socket or its web UI. At L0 a job runs as the operator's own
//! user, and the CLI refuses the operator's acts when its environment carries
//! a job's marker (`THESEUS_SESSION`): a speed bump, since a job can strip
//! its environment.

use std::net::SocketAddr;

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
/// `server`, both as the server's socket names them, read from
/// `/proc/net/tcp` and `tcp6`; on a platform with no such table, none.
pub fn client_uid(server: SocketAddr, client: SocketAddr) -> ClientUid {
    if !cfg!(target_os = "linux") {
        return ClientUid::NoTable;
    }
    client_uid_in(
        |t| std::fs::read_to_string(t).map_err(|e| format!("{t}: {e}")),
        server,
        client,
    )
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

/// The inode of the client's end of a loopback TCP connection to this
/// process, as `client_uid` finds its row: what a scan of a process's file
/// descriptors names as `socket:[<inode>]`. The MCP server reads it to find
/// a job's process among the daemon's own descendants (step 41b). None when
/// no live row is found, or this platform keeps no table.
pub fn client_inode(server: SocketAddr, client: SocketAddr) -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    for _ in 0..2 {
        for (table, local, remote) in forms(server, client) {
            let Ok(text) = std::fs::read_to_string(table) else {
                continue;
            };
            let live = socket_rows(&text, local, remote).find(|(i, _)| *i != 0);
            if let Some((inode, _)) = live {
                return Some(inode);
            }
        }
    }
    None
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
        let t0 = std::time::Instant::now();
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

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(socket_rows(table, client, v4).next(), Some((33333, 1001)));
            assert_eq!(socket_rows(table, v4, client).next(), Some((22222, 1000)));
        }
        assert_eq!(
            socket_rows(table, "127.0.0.1:1".parse().unwrap(), v4).next(),
            None
        );
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
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
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
}
