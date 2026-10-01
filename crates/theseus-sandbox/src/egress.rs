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

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use theseus_tools::net::private_kind;

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

/// One entry of a job's list: `host:port`, with a glob on the host
/// (`*.crates.io:443`) and the exact port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allow {
    host: String,
    port: u16,
}

impl std::str::FromStr for Allow {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (host, port) = split_host_port(s).ok_or_else(|| format!("{s:?} is not host:port"))?;
        Ok(Self { host, port })
    }
}

impl std::fmt::Display for Allow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

impl Allow {
    pub fn permits(&self, host: &str, port: u16) -> bool {
        self.port == port && glob(&self.host, host)
    }
}

/// `name:port` or `[v6]:port`, the name lowercased and without a final dot.
fn split_host_port(s: &str) -> Option<(String, u16)> {
    let (host, port) = match s.strip_prefix('[') {
        Some(rest) => rest.split_once("]:")?,
        None => {
            let (h, p) = s.rsplit_once(':')?;
            if h.contains(':') {
                return None;
            }
            (h, p)
        }
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let port: u16 = port.parse().ok()?;
    (!host.is_empty() && port != 0).then_some((host, port))
}

/// `*` matches any run of characters, dots included (so `*.crates.io` is
/// every name under crates.io, and not crates.io itself); anything else
/// matches itself, without regard to case.
fn glob(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.as_bytes(), name.as_bytes());
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ni));
            pi += 1;
        } else if pi < p.len() && p[pi].eq_ignore_ascii_case(&n[ni]) {
            pi += 1;
            ni += 1;
        } else if let Some((s, m)) = star {
            pi = s + 1;
            ni = m + 1;
            star = Some((s, m + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

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
#[derive(Debug, Clone, Default)]
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
                            let record = proxy.tunnel(client);
                            mine.record(record);
                            mine.active.fetch_sub(1, Ordering::SeqCst);
                        });
                    if spawned.is_err() {
                        shared.active.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // The job's namespace is gone, or the listener broke.
                Err(_) => return,
            }
        }
    }

    /// One client: its request, the checks, then the tunnel.
    fn tunnel(&self, mut client: TcpStream) -> Connection {
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
        if !self.allow.iter().any(|a| a.permits(&host, port)) {
            let why = format!("{} is not on this job's egress list", show(&host, port));
            refuse(&mut client, &mut record, 403, why);
            return record;
        }
        let addrs = match self.resolver.resolve(&host, port) {
            Ok(a) => a,
            Err(r @ Refusal::Private { .. }) => {
                refuse(&mut client, &mut record, 403, r.to_string());
                return record;
            }
            Err(r) => {
                refuse(&mut client, &mut record, 502, r.to_string());
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
        let (up, down) = relay(client, server, &rest);
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
    /// job's end closes its side of each), and gives every connection.
    pub fn stop(mut self, grace: Duration) -> Vec<Connection> {
        self.halt();
        let deadline = Instant::now() + grace;
        while self.shared.active.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.connections()
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
    split_host_port(target).ok_or_else(|| (400, format!("{target:?} is not host:port")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glob_is_on_the_host_and_the_port_is_exact() {
        let a: Allow = "*.crates.io:443".parse().unwrap();
        assert!(a.permits("index.crates.io", 443));
        assert!(a.permits("a.b.crates.io", 443));
        assert!(!a.permits("crates.io", 443));
        assert!(!a.permits("index.crates.io", 80));
        assert!(!a.permits("evilcrates.io", 443));
        let a: Allow = "GitHub.com.:443".parse().unwrap();
        assert!(a.permits("github.com", 443));
        assert!(!a.permits("api.github.com", 443));
        assert_eq!(a.to_string(), "github.com:443");
        let a: Allow = "[2606:4700::1111]:443".parse().unwrap();
        assert!(a.permits("2606:4700::1111", 443));
        assert_eq!(a.to_string(), "[2606:4700::1111]:443");
        for bad in [
            "github.com",
            "github.com:0",
            ":443",
            "2606:4700::1111:443",
            "x:y",
        ] {
            assert!(bad.parse::<Allow>().is_err(), "{bad}");
        }
        assert!(glob("*", "anything.at.all"));
        assert!(glob("a*b*c", "aXXbYYc"));
        assert!(!glob("a*b*c", "aXXbYY"));
    }

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
