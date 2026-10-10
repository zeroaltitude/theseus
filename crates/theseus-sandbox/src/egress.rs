//! 18b: L1's egress proxy (design §2.4).
//!
//! A job's network namespace holds `lo` alone. With egress, the init opens a
//! listener on 127.0.0.1:`PORT` inside it and hands the descriptor to the
//! wrapper over `SCM_RIGHTS` before the command starts
//! (`Spec::egress_port`, `SandboxChild::take_egress_listener`). The wrapper
//! serves HTTP `CONNECT` on it from the host's namespace. Each `CONNECT`, in
//! order:
//!
//! 1. `host:port` is matched against the job's list: a glob on the host, and
//!    the exact port;
//! 2. the name is resolved by DD5's public-only rule (`theseus_tools::net`):
//!    a name any of whose addresses is not public is refused, so no allowed
//!    name leads to the metadata service or to localhost;
//! 3. the proxy connects, and copies bytes both ways;
//! 4. it records the host, port, address, bytes each way, and milliseconds.
//!
//! A refused `CONNECT` gets a 403 whose body says why. A program that ignores
//! the proxy variables has no route at all. Plain `http://` forwarding is
//! not offered (filed): any other method gets a 405.
//!
//! 18c wired it in: the job wrapper's L1 path runs a `Proxy` for a job whose
//! list is not empty, stops it when the job ends, and puts its `Summary` in
//! the completion's `detail.egress`. Each `CONNECT`'s `Outcome` is a value
//! the proxy acts on. The proxy never reads or rewrites what a tunnel
//! carries: credentials as stand-ins, which would have ended TLS here, were
//! dropped for v1 (theseus-gh7, 2026-10-03). What a job holds leaves only
//! for the hosts its list names.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use theseus_tools::net::{private_kind, split_host_port};

/// The port the job's proxy listens on, inside its namespace.
pub const PORT: u16 = 3128;

/// The longest request head a client may send before its tunnel.
const MAX_HEAD: usize = 8 * 1024;
/// How long a client has to send its request head.
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
/// Tunnels open at once, per job: each holds two of the wrapper's threads.
const MAX_TUNNELS: usize = 64;
/// Records kept per job; past it they are counted (`Running::dropped`).
const MAX_RECORDS: usize = 1000;

/// The job's environment for its proxy: `HTTPS_PROXY`, `HTTP_PROXY`, and
/// `ALL_PROXY`, each in both cases (curl reads only the lowercase
/// `http_proxy`), and `NO_PROXY` for the job's own loopback.
pub fn proxy_env(port: u16) -> Vec<(String, String)> {
    let proxy = format!("http://127.0.0.1:{port}");
    let mut env: Vec<(String, String)> = [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .map(|k| (k.to_string(), proxy.clone()))
    .collect();
    for k in ["NO_PROXY", "no_proxy"] {
        env.push((k.into(), "localhost,127.0.0.1,::1".into()));
    }
    env
}

/// One entry of a job's list: `host:port`, with a glob on the host and the
/// exact port (`theseus_tools::net`, where `proc.run`'s plan and the gate
/// read it too).
pub use theseus_tools::net::Allow;

/// Why a name may not be reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// One of its addresses is not public (DD5).
    Private {
        host: String,
        ip: IpAddr,
        kind: &'static str,
    },
    /// It did not resolve.
    Unresolved { host: String, error: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Private { host, ip, kind } if *host == ip.to_string() => {
                write!(f, "{ip} is {kind}")
            }
            Refusal::Private { host, ip, kind } => write!(f, "{host} resolves to {ip}, {kind}"),
            Refusal::Unresolved { host, error } => write!(f, "{host} does not resolve: {error}"),
        }
    }
}

/// DD5's public-only resolver, as the proxy uses it: the very answer the
/// connection uses is the one checked, so a rebinding name has no second
/// answer to give.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolver {
    /// Tests: names answered here instead of by the system's resolver.
    pub hosts: BTreeMap<String, Vec<IpAddr>>,
    /// Tests: addresses taken as public (a test's server on 127.0.0.1).
    pub public: Vec<IpAddr>,
}

impl Resolver {
    /// Every address of `host`, each public; or why not.
    pub fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, Refusal> {
        let ips: Vec<IpAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![ip]
        } else if let Some(ips) = self.hosts.get(host) {
            ips.clone()
        } else {
            (host, port)
                .to_socket_addrs()
                .map_err(|e| Refusal::Unresolved {
                    host: host.into(),
                    error: e.to_string(),
                })?
                .map(|a| a.ip())
                .collect()
        };
        if ips.is_empty() {
            return Err(Refusal::Unresolved {
                host: host.into(),
                error: "no address".into(),
            });
        }
        for ip in &ips {
            if let Some(kind) = private_kind(*ip).filter(|_| !self.public.contains(ip)) {
                return Err(Refusal::Private {
                    host: host.into(),
                    ip: *ip,
                    kind,
                });
            }
        }
        Ok(ips
            .into_iter()
            .map(|ip| SocketAddr::new(ip, port))
            .collect())
    }
}

/// What the proxy does with one `CONNECT` (`Proxy::decide`). It is a value
/// the proxy then acts on, never an action taken inside the checks, so each
/// decision is tested as a value. Two outcomes, tunnel or refuse: nothing
/// reads inside a tunnel (theseus-gh7 dropped TLS interception for v1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Connect to the first of these addresses that answers, and copy bytes
    /// both ways.
    Tunnel(Vec<SocketAddr>),
    /// Answer with this status, and say why in its body.
    Refuse { code: u16, why: String },
}

/// One `CONNECT`, as the proxy saw it: 18c's `detail.egress`, and its
/// `sandbox.egress` and `sandbox.egress_refused` rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connection {
    pub host: String,
    pub port: u16,
    /// The address it reached; None when it was refused before a connect.
    pub addr: Option<SocketAddr>,
    /// Bytes from the job out, and back in.
    pub up: u64,
    pub down: u64,
    pub ms: u64,
    /// Why it was refused (the response's body), or None.
    pub refused: Option<String>,
}

/// A job's egress, as its completion's `detail.egress` keeps it (18c): the
/// list it ran with, each host it reached (one entry per `host:port`, its
/// connections and bytes and milliseconds summed), and each refusal (one
/// entry per host, port, and reason, counted). Small whatever the job did:
/// at most one entry per name it asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// The job's list: the operator's `[sandbox] egress`, and the hosts its
    /// call named.
    pub allow: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<Reached>,
    #[serde(default)]
    pub refused: Vec<Refused>,
    /// Connections past the records the proxy keeps, counted only.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One `host:port` a job reached: a tunnel was opened to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reached {
    pub host: String,
    pub port: u16,
    pub connections: u64,
    /// Bytes from the job out, and back in.
    pub up: u64,
    pub down: u64,
    /// The tunnels' milliseconds, together.
    pub ms: u64,
}

impl Reached {
    /// `github.com:443`.
    pub fn name(&self) -> String {
        show(&self.host, self.port)
    }
}

/// The `CONNECT`s refused for one reason, at one `host:port`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refused {
    pub host: String,
    pub port: u16,
    /// What the 403 (or 400, 405, 502, 503) said.
    pub why: String,
    pub count: u64,
}

impl Summary {
    /// The proxy's records for one job, with `allow` its list.
    pub fn of(allow: &[Allow], log: &[Connection], dropped: usize) -> Self {
        let mut hosts: Vec<Reached> = Vec::new();
        let mut refused: Vec<Refused> = Vec::new();
        for c in log {
            match &c.refused {
                None => {
                    let at = hosts
                        .iter()
                        .position(|h| h.host == c.host && h.port == c.port)
                        .unwrap_or_else(|| {
                            hosts.push(Reached {
                                host: c.host.clone(),
                                port: c.port,
                                ..Reached::default()
                            });
                            hosts.len() - 1
                        });
                    let h = &mut hosts[at];
                    h.connections += 1;
                    h.up += c.up;
                    h.down += c.down;
                    h.ms += c.ms;
                }
                Some(why) => {
                    match refused
                        .iter_mut()
                        .find(|r| r.host == c.host && r.port == c.port && &r.why == why)
                    {
                        Some(r) => r.count += 1,
                        None => refused.push(Refused {
                            host: c.host.clone(),
                            port: c.port,
                            why: why.clone(),
                            count: 1,
                        }),
                    }
                }
            }
        }
        Self {
            allow: allow.iter().map(ToString::to_string).collect(),
            hosts,
            refused,
            dropped: dropped as u64,
        }
    }

    /// Whether the job connected out: a tunnel was opened to some host.
    pub fn connected(&self) -> bool {
        !self.hosts.is_empty()
    }

    /// The hosts it reached, as the outside-text hold names them:
    /// `api.github.com:443, pypi.org:443`.
    pub fn reached(&self) -> String {
        self.hosts
            .iter()
            .map(Reached::name)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The proxy for one job.
pub struct Proxy {
    listener: TcpListener,
    allow: Vec<Allow>,
    resolver: Resolver,
    connect_timeout: Duration,
}

impl Proxy {
    /// `listener` is the job's, from `SandboxChild::take_egress_listener`.
    pub fn new(listener: OwnedFd, allow: Vec<Allow>, resolver: Resolver) -> Self {
        Self {
            listener: TcpListener::from(listener),
            allow,
            resolver,
            connect_timeout: Duration::from_secs(10),
        }
    }

    /// Serves the listener on a thread of its own, each tunnel on its own
    /// threads, until `Running::stop`.
    pub fn start(self) -> io::Result<Running> {
        self.listener.set_nonblocking(true)?;
        let (stop_r, stop_w) = crate::sys::pipe()?;
        let shared = Arc::new(Shared {
            log: Mutex::new(Vec::new()),
            dropped: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            open: Mutex::default(),
            next: AtomicU64::new(0),
        });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("egress-proxy".into())
            .spawn(move || self.serve(&stop_r, &s))?;
        Ok(Running {
            shared,
            stop: Some(stop_w),
            thread: Some(thread),
        })
    }

    fn serve(self, stop: &OwnedFd, shared: &Arc<Shared>) {
        let me = Arc::new(self);
        loop {
            let mut fds = [
                libc::pollfd {
                    fd: me.listener.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if r == -1 && crate::sys::errno() == libc::EINTR {
                continue;
            }
            if r == -1 || fds[1].revents != 0 {
                return;
            }
            match me.listener.accept() {
                Ok((mut client, _)) => {
                    // A job may not tie up its wrapper's threads.
                    if shared.active.load(Ordering::SeqCst) >= MAX_TUNNELS {
                        let _ = client.set_write_timeout(Some(HEAD_TIMEOUT));
                        let why = format!("too many tunnels at once ({MAX_TUNNELS})");
                        respond(&mut client, 503, &why);
                        continue;
                    }
                    let (proxy, mine) = (me.clone(), shared.clone());
                    shared.active.fetch_add(1, Ordering::SeqCst);
                    let spawned = std::thread::Builder::new()
                        .name("egress-tunnel".into())
                        .spawn(move || {
                            let record = proxy.tunnel(client, &mine);
                            mine.record(record);
                            mine.active.fetch_sub(1, Ordering::SeqCst);
                        });
                    if spawned.is_err() {
                        shared.active.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // A full descriptor table, or a connection reset in the
                // queue, passes: wait and accept again (theseus-7vtp).
                Err(e) if transient_accept_error(&e) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                // The job's namespace is gone, or the listener broke.
                Err(_) => return,
            }
        }
    }

    /// What to do with a `CONNECT` to `host:port`, decided before anything
    /// is done: steps 1 and 2 of the order (the list, then the public-only
    /// resolver).
    pub fn decide(&self, host: &str, port: u16) -> Outcome {
        if !self.allow.iter().any(|a| a.permits(host, port)) {
            let why = format!("{} is not on this job's egress list", show(host, port));
            return Outcome::Refuse { code: 403, why };
        }
        match self.resolver.resolve(host, port) {
            Ok(addrs) => Outcome::Tunnel(addrs),
            Err(r @ Refusal::Private { .. }) => Outcome::Refuse {
                code: 403,
                why: r.to_string(),
            },
            Err(r) => Outcome::Refuse {
                code: 502,
                why: r.to_string(),
            },
        }
    }

    /// One client: its request, the decision, then what it decided.
    fn tunnel(&self, mut client: TcpStream, shared: &Shared) -> Connection {
        let t0 = Instant::now();
        let _ = client.set_nonblocking(false);
        let _ = client.set_read_timeout(Some(HEAD_TIMEOUT));
        let mut record = Connection {
            host: String::new(),
            port: 0,
            addr: None,
            up: 0,
            down: 0,
            ms: 0,
            refused: None,
        };
        let refuse = |client: &mut TcpStream, record: &mut Connection, code: u16, why: String| {
            respond(client, code, &why);
            record.refused = Some(why);
            record.ms = t0.elapsed().as_millis() as u64;
        };
        let (head, rest) = match read_head(&mut client) {
            Ok(h) => h,
            Err(why) => {
                refuse(&mut client, &mut record, 400, why);
                return record;
            }
        };
        let (host, port) = match parse_connect(&head) {
            Ok(t) => t,
            Err((code, why)) => {
                refuse(&mut client, &mut record, code, why);
                return record;
            }
        };
        record.host.clone_from(&host);
        record.port = port;
        let addrs = match self.decide(&host, port) {
            Outcome::Tunnel(addrs) => addrs,
            Outcome::Refuse { code, why } => {
                refuse(&mut client, &mut record, code, why);
                return record;
            }
        };
        let mut last = None;
        let server =
            addrs.iter().find_map(
                |a| match TcpStream::connect_timeout(a, self.connect_timeout) {
                    Ok(s) => Some((s, *a)),
                    Err(e) => {
                        last = Some(e);
                        None
                    }
                },
            );
        let Some((server, addr)) = server else {
            let e = last.map(|e| e.to_string()).unwrap_or_default();
            let why = format!("connecting to {}: {e}", show(&host, port));
            refuse(&mut client, &mut record, 502, why);
            return record;
        };
        record.addr = Some(addr);
        if client
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .is_err()
        {
            record.ms = t0.elapsed().as_millis() as u64;
            return record;
        }
        let _ = client.set_read_timeout(None);
        let id = shared.next.fetch_add(1, Ordering::SeqCst);
        if let (Ok(a), Ok(b)) = (client.try_clone(), server.try_clone()) {
            shared
                .open
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(id, [a, b]);
        }
        let (up, down) = relay(client, server, &rest);
        shared
            .open
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        record.up = up;
        record.down = down;
        record.ms = t0.elapsed().as_millis() as u64;
        record
    }
}

fn show(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

struct Shared {
    log: Mutex<Vec<Connection>>,
    /// Records past `MAX_RECORDS`, counted and not kept.
    dropped: AtomicUsize,
    active: AtomicUsize,
    /// Each open tunnel's two sockets, so a stop can end a tunnel whose
    /// server holds it open after the job has gone, and still record it.
    open: Mutex<BTreeMap<u64, [TcpStream; 2]>>,
    next: AtomicU64,
}

impl Shared {
    fn record(&self, c: Connection) {
        let mut log = self.log.lock().unwrap_or_else(|p| p.into_inner());
        if log.len() < MAX_RECORDS {
            log.push(c);
        } else {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// A proxy at work.
pub struct Running {
    shared: Arc<Shared>,
    stop: Option<OwnedFd>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    /// The connections so far: each tunnel is recorded once it closes.
    pub fn connections(&self) -> Vec<Connection> {
        self.shared
            .log
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Connections past the kept records, counted only.
    pub fn dropped(&self) -> usize {
        self.shared.dropped.load(Ordering::SeqCst)
    }

    /// Stops accepting, waits up to `grace` for open tunnels to close (a
    /// job's end closes its side of each), then ends any still open (a
    /// server may hold one past the job), and gives every connection: each
    /// tunnel opened is recorded, with the bytes it carried.
    pub fn stop(self, grace: Duration) -> Vec<Connection> {
        self.finish(grace).0
    }

    /// `stop`, with the connections past the kept records, counted.
    pub fn finish(mut self, grace: Duration) -> (Vec<Connection>, usize) {
        self.halt();
        let idle = |until: Instant| {
            while self.shared.active.load(Ordering::SeqCst) > 0 && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        idle(Instant::now() + grace);
        for [a, b] in self
            .shared
            .open
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            let _ = a.shutdown(Shutdown::Both);
            let _ = b.shutdown(Shutdown::Both);
        }
        idle(Instant::now() + Duration::from_secs(2));
        (self.connections(), self.dropped())
    }

    /// Tunnels open now.
    pub fn active(&self) -> usize {
        self.shared.active.load(Ordering::SeqCst)
    }

    fn halt(&mut self) {
        if let Some(w) = self.stop.take() {
            let _ = std::fs::File::from(w).write_all(b"x");
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The request head, up to its blank line, and whatever the client sent
/// after it (a TLS hello may follow at once).
fn read_head(client: &mut TcpStream) -> Result<(String, Vec<u8>), String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            let head = String::from_utf8_lossy(&buf).into_owned();
            return Ok((head, rest));
        }
        if buf.len() > MAX_HEAD {
            return Err(format!("the request head passed {MAX_HEAD} bytes"));
        }
        match client.read(&mut chunk) {
            Ok(0) => return Err("the client closed before its request ended".into()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("reading the request: {e}")),
        }
    }
}

/// `CONNECT host:port HTTP/1.x`: the target, or the status and why not.
fn parse_connect(head: &str) -> Result<(String, u16), (u16, String)> {
    let line = head.lines().next().unwrap_or("");
    let mut words = line.split_whitespace();
    let (method, target, version) = (words.next(), words.next(), words.next());
    if !version.is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err((400, format!("not an HTTP/1 request: {line:?}")));
    }
    if method != Some("CONNECT") {
        return Err((
            405,
            "only CONNECT is served: plain http:// forwarding is not offered".into(),
        ));
    }
    let target = target.unwrap_or("");
    // A target names one host: a `*` is the list's, never a request's.
    split_host_port(target)
        .filter(|(h, _)| !h.contains('*'))
        .ok_or_else(|| (400, format!("{target:?} is not host:port")))
}

fn respond(client: &mut TcpStream, code: u16, why: &str) {
    let reason = match code {
        400 => "Bad Request",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        503 => "Service Unavailable",
        _ => "Bad Gateway",
    };
    let body = format!("{why}\n");
    let msg = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = client.write_all(msg.as_bytes());
}

/// Copies both ways until each side is done: (bytes up, bytes down).
fn relay(client: TcpStream, server: TcpStream, rest: &[u8]) -> (u64, u64) {
    let (Ok(client_w), Ok(server_r)) = (client.try_clone(), server.try_clone()) else {
        return (0, 0);
    };
    let down = std::thread::spawn(move || copy(server_r, client_w));
    let mut server_w = server;
    let up = match server_w.write_all(rest) {
        Ok(()) => rest.len() as u64 + copy(client, server_w),
        Err(_) => {
            let _ = server_w.shutdown(Shutdown::Both);
            0
        }
    };
    (up, down.join().unwrap_or(0))
}

/// Copies `from` to `to` until `from` ends, then ends `to`'s write side.
fn copy(mut from: TcpStream, mut to: TcpStream) -> u64 {
    let mut buf = [0u8; 16 * 1024];
    let mut n = 0u64;
    loop {
        match from.read(&mut buf) {
            Ok(0) => break,
            Ok(k) => {
                if to.write_all(&buf[..k]).is_err() {
                    break;
                }
                n += k as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    let _ = to.shutdown(Shutdown::Write);
    n
}

/// An accept error that says the moment is bad, not that the listener is: out
/// of descriptors (`EMFILE`, `ENFILE`), of buffers or memory (`ENOBUFS`,
/// `ENOMEM`), or a peer that went before it was taken (`ECONNABORTED`,
/// `EPROTO`).
fn transient_accept_error(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(
            libc::EMFILE
                | libc::ENFILE
                | libc::ENOBUFS
                | libc::ENOMEM
                | libc::ECONNABORTED
                | libc::EPROTO
        )
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_full_descriptor_table_is_a_bad_moment_and_a_broken_listener_is_not() {
        for e in [
            libc::EMFILE,
            libc::ENFILE,
            libc::ENOBUFS,
            libc::ECONNABORTED,
        ] {
            assert!(super::transient_accept_error(
                &io::Error::from_raw_os_error(e)
            ));
        }
        for e in [libc::EBADF, libc::EINVAL, libc::ENOTSOCK] {
            assert!(!super::transient_accept_error(
                &io::Error::from_raw_os_error(e)
            ));
        }
    }

    use super::*;

    #[test]
    fn only_connect_is_served() {
        assert_eq!(
            parse_connect("CONNECT github.com:443 HTTP/1.1\r\nHost: github.com:443"),
            Ok(("github.com".into(), 443))
        );
        assert_eq!(
            parse_connect("CONNECT [::1]:22 HTTP/1.0"),
            Ok(("::1".into(), 22))
        );
        assert_eq!(
            parse_connect("GET http://x/ HTTP/1.1").map_err(|e| e.0),
            Err(405)
        );
        assert_eq!(
            parse_connect("CONNECT github.com HTTP/1.1").map_err(|e| e.0),
            Err(400)
        );
        assert_eq!(parse_connect("hello").map_err(|e| e.0), Err(400));
        assert_eq!(
            parse_connect("CONNECT *.crates.io:443 HTTP/1.1").map_err(|e| e.0),
            Err(400),
            "a request names one host"
        );
    }

    /// Each `CONNECT`'s outcome is a value decided
    /// before the proxy acts, and the job's summary keeps one entry per
    /// host reached and per refusal, whatever the number of connections.
    #[test]
    fn a_connect_is_decided_as_a_value_and_summed_per_host() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut resolver = Resolver::default();
        let public: IpAddr = "93.184.215.14".parse().unwrap();
        resolver.hosts.insert("fine.test".into(), vec![public]);
        resolver
            .hosts
            .insert("meta.test".into(), vec!["169.254.169.254".parse().unwrap()]);
        let allow: Vec<Allow> = ["fine.test:443", "meta.test:80"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        let p = Proxy::new(l.into(), allow.clone(), resolver);
        assert_eq!(
            p.decide("fine.test", 443),
            Outcome::Tunnel(vec![SocketAddr::new(public, 443)])
        );
        assert_eq!(
            p.decide("other.test", 443),
            Outcome::Refuse {
                code: 403,
                why: "other.test:443 is not on this job's egress list".into()
            }
        );
        assert_eq!(
            p.decide("meta.test", 80),
            Outcome::Refuse {
                code: 403,
                why: "meta.test resolves to 169.254.169.254, a link-local address".into()
            }
        );
        let conn = |host: &str, up, down, refused: Option<&str>| Connection {
            host: host.into(),
            port: 443,
            addr: None,
            up,
            down,
            ms: 5,
            refused: refused.map(str::to_string),
        };
        let log = [
            conn("fine.test", 10, 100, None),
            conn("fine.test", 20, 200, None),
            conn("other.test", 0, 0, Some("no")),
            conn("other.test", 0, 0, Some("no")),
        ];
        let s = Summary::of(&allow, &log, 3);
        assert_eq!(s.allow, ["fine.test:443", "meta.test:80"]);
        assert_eq!(
            s.hosts,
            [Reached {
                host: "fine.test".into(),
                port: 443,
                connections: 2,
                up: 30,
                down: 300,
                ms: 10
            }]
        );
        assert_eq!(s.refused.len(), 1);
        assert_eq!((s.refused[0].count, s.dropped), (2, 3));
        assert!(s.connected());
        assert_eq!(s.reached(), "fine.test:443");
        let none = Summary::of(&allow, &log[2..], 0);
        assert!(!none.connected(), "a refusal alone is no connection");
        let back: Summary = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn the_resolver_refuses_any_answer_that_is_not_public() {
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        let meta: IpAddr = "169.254.169.254".parse().unwrap();
        let public: IpAddr = "93.184.215.14".parse().unwrap();
        let mut r = Resolver::default();
        r.hosts.insert("rebind.test".into(), vec![lo]);
        r.hosts.insert("metadata.test".into(), vec![meta]);
        r.hosts.insert("mixed.test".into(), vec![public, lo]);
        r.hosts.insert("fine.test".into(), vec![public]);
        let why = |h: &str| r.resolve(h, 80).unwrap_err().to_string();
        assert_eq!(
            why("rebind.test"),
            "rebind.test resolves to 127.0.0.1, a loopback address"
        );
        assert_eq!(
            why("metadata.test"),
            "metadata.test resolves to 169.254.169.254, a link-local address"
        );
        assert_eq!(
            why("mixed.test"),
            "mixed.test resolves to 127.0.0.1, a loopback address"
        );
        assert_eq!(
            why("169.254.169.254"),
            "169.254.169.254 is a link-local address"
        );
        assert_eq!(why("::1"), "::1 is a loopback address");
        assert_eq!(
            r.resolve("fine.test", 443),
            Ok(vec![SocketAddr::new(public, 443)])
        );
        // localhost, by the system's own resolver.
        assert!(why("localhost").contains("a loopback address"));
        // A test's server, taken as public.
        r.public.push(lo);
        assert!(r.resolve("rebind.test", 80).is_ok());
    }

    #[test]
    fn the_proxy_env_names_every_spelling() {
        let env = proxy_env(PORT);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        for k in [
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            assert_eq!(get(k), Some("http://127.0.0.1:3128"), "{k}");
        }
        assert_eq!(get("NO_PROXY"), Some("localhost,127.0.0.1,::1"));
    }

    /// A server that echoes what it reads, on 127.0.0.1.
    fn echo() -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut w = s.try_clone().unwrap();
                    let _ = io::copy(&mut &s, &mut w);
                });
            }
        });
        addr
    }

    /// A request through `proxy_addr`: the response head, and the echo of
    /// `payload` once tunnelled (sent with the request, as a TLS hello is).
    fn ask(proxy_addr: SocketAddr, request: &str, payload: &[u8]) -> (String, Vec<u8>) {
        let mut c = TcpStream::connect(proxy_addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut msg = request.as_bytes().to_vec();
        msg.extend_from_slice(payload);
        c.write_all(&msg).unwrap();
        let (head, mut rest) = read_head(&mut c).unwrap();
        if head.starts_with("HTTP/1.1 200") {
            while rest.len() < payload.len() {
                let mut b = [0u8; 256];
                let n = c.read(&mut b).unwrap();
                if n == 0 {
                    break;
                }
                rest.extend_from_slice(&b[..n]);
            }
        } else {
            let _ = c.read_to_end(&mut rest);
        }
        (head, rest)
    }

    #[test]
    fn a_tunnel_carries_bytes_both_ways_and_is_recorded() {
        let target = echo();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = l.local_addr().unwrap();
        let mut resolver = Resolver::default();
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        resolver.hosts.insert("echo.test".into(), vec![lo]);
        resolver.public.push(lo);
        let allow = vec![format!("echo.test:{}", target.port()).parse().unwrap()];
        let running = Proxy::new(l.into(), allow, resolver).start().unwrap();
        let req = format!(
            "CONNECT echo.test:{} HTTP/1.1\r\nHost: echo.test\r\n\r\n",
            target.port()
        );
        let (head, echoed) = ask(proxy_addr, &req, b"hello, tunnel");
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(echoed, b"hello, tunnel");
        let req = format!("CONNECT other.test:{} HTTP/1.1\r\n\r\n", target.port());
        let (head, body) = ask(proxy_addr, &req, b"");
        assert!(head.starts_with("HTTP/1.1 403"), "{head}");
        assert_eq!(
            String::from_utf8_lossy(&body),
            format!(
                "other.test:{} is not on this job's egress list\n",
                target.port()
            )
        );
        let (head, _) = ask(proxy_addr, "GET http://echo.test/ HTTP/1.1\r\n\r\n", b"");
        assert!(head.starts_with("HTTP/1.1 405"), "{head}");
        let log = running.stop(Duration::from_secs(2));
        let ok = log
            .iter()
            .find(|c| c.refused.is_none())
            .expect("the tunnel's record");
        assert_eq!((ok.host.as_str(), ok.addr), ("echo.test", Some(target)));
        assert_eq!((ok.up, ok.down), (13, 13));
        assert_eq!(log.iter().filter(|c| c.refused.is_some()).count(), 2);
    }

    /// A tunnel whose server holds it open after the job has gone is ended
    /// by the stop and still recorded, with the bytes it carried (18c: the
    /// record decides whether the job's result is outside text).
    #[test]
    fn a_stop_records_a_tunnel_its_server_holds_open() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = l.local_addr().unwrap();
        // A server that reads, and never answers or closes.
        std::thread::spawn(move || {
            let held: Vec<TcpStream> = l.incoming().flatten().take(1).collect();
            std::thread::sleep(Duration::from_secs(30));
            drop(held);
        });
        let pl = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = pl.local_addr().unwrap();
        let mut resolver = Resolver::default();
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        resolver.hosts.insert("hold.test".into(), vec![lo]);
        resolver.public.push(lo);
        let allow = vec![format!("hold.test:{}", target.port()).parse().unwrap()];
        let running = Proxy::new(pl.into(), allow, resolver).start().unwrap();
        let mut c = TcpStream::connect(proxy_addr).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let req = format!("CONNECT hold.test:{} HTTP/1.1\r\n\r\n", target.port());
        c.write_all(req.as_bytes()).unwrap();
        let (head, _) = read_head(&mut c).unwrap();
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        c.write_all(b"12345").unwrap();
        let t0 = Instant::now();
        while running.active() == 0 && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(50));
        let log = running.stop(Duration::from_millis(50));
        assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
        assert_eq!(log.len(), 1, "{log:?}");
        assert_eq!((log[0].refused.as_deref(), log[0].up), (None, 5));
        drop(c);
    }

    #[test]
    fn a_job_may_hold_only_so_many_tunnels_at_once() {
        let target = echo();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = l.local_addr().unwrap();
        let mut resolver = Resolver::default();
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        resolver.hosts.insert("echo.test".into(), vec![lo]);
        resolver.public.push(lo);
        let allow = vec![format!("echo.test:{}", target.port()).parse().unwrap()];
        let running = Proxy::new(l.into(), allow, resolver).start().unwrap();
        let req = format!("CONNECT echo.test:{} HTTP/1.1\r\n\r\n", target.port());
        let open = |want: &str| {
            let mut c = TcpStream::connect(proxy_addr).unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            c.write_all(req.as_bytes()).unwrap();
            let (head, body) = read_head(&mut c).unwrap();
            assert!(head.starts_with(want), "{head}");
            (c, body)
        };
        let held: Vec<TcpStream> = (0..MAX_TUNNELS).map(|_| open("HTTP/1.1 200").0).collect();
        let (mut c, mut body) = open("HTTP/1.1 503");
        let _ = c.read_to_end(&mut body);
        assert_eq!(
            String::from_utf8_lossy(&body),
            "too many tunnels at once (64)\n"
        );
        drop(held);
        let log = running.stop(Duration::from_secs(5));
        assert_eq!(log.len(), MAX_TUNNELS);
    }
}
