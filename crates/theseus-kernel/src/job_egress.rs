//! An L1 job's egress (M4 18c; design §2.4). For a job whose list is not
//! empty, the init opens a listener inside the job's network namespace and
//! hands it over; the wrapper serves it with `theseus_sandbox`'s proxy from
//! the host's namespace, and stops the proxy once the job has ended, a stop
//! or a deadline included. The proxy's records go into the completion's
//! `detail.egress` (`theseus_sandbox::egress::Summary`), which the core reads
//! for the `sandbox.egress` rows and for whether the result is outside text.
//! A job with no list gets no listener and no proxy, and pays nothing.
//!
//! **A name the daemon serves itself** (theseus-pw1q.21.5): a daemon can
//! [`register`] a [`Local`] service for one `host:port`. A job in the daemon's
//! own process (`probe_run`) whose list is that name alone gets the service
//! in place of the proxy: it is handed every connection that reaches the
//! job's listener, plain HTTP in the proxy's own absolute form included,
//! which the proxy refuses. Nothing is resolved or dialled, and no address
//! outside the daemon is reachable. A wrapper process holds no registry, so
//! the same list there reaches the proxy, which refuses the name.

use std::collections::BTreeMap;
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_sandbox::egress::{proxy_env, Allow, Proxy, Running, Summary, PORT};
use theseus_sandbox::{SandboxChild, Spec};

use crate::job::L1;

/// How long a stop waits for the job's tunnels to close by themselves
/// before it ends them: the job has gone, so nothing more reaches it.
const DRAIN: Duration = Duration::from_millis(250);

/// A service the daemon runs for one name of a job's egress list.
pub trait Local: Send + Sync {
    /// One connection from inside the job's network namespace. It must return
    /// at once: the connection is served on a task of the service's own.
    fn serve(&self, conn: TcpStream);
}

static LOCAL: Mutex<BTreeMap<String, Arc<dyn Local>>> = Mutex::new(BTreeMap::new());

/// A registered service, until it is dropped.
#[must_use = "a registration ends when it is dropped"]
pub struct Registered(String);

impl Drop for Registered {
    fn drop(&mut self) {
        LOCAL
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// Serve `entry` (`host:port`, as the job's list writes it) with `service`
/// for jobs of this process. The last registration of a name wins.
pub fn register(entry: &str, service: Arc<dyn Local>) -> Registered {
    LOCAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(entry.to_string(), service);
    Registered(entry.to_string())
}

fn local_for(l1: &L1) -> Option<Arc<dyn Local>> {
    let [only] = l1.egress.as_slice() else {
        return None;
    };
    LOCAL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(only)
        .cloned()
}

/// A job's egress, once it runs: the proxy, or the daemon's own service.
pub(crate) enum Egress {
    Proxy(Running),
    Local(Accepting),
}

/// The accept loop of a [`Local`] service on a job's listener, on a thread of
/// its own, until it is stopped.
pub(crate) struct Accepting {
    stop: UnixStream,
    thread: JoinHandle<()>,
}

impl Accepting {
    pub(crate) fn start(listener: TcpListener, service: Arc<dyn Local>) -> std::io::Result<Self> {
        let (quit, stop) = UnixStream::pair()?;
        let thread = std::thread::Builder::new()
            .name("egress-local".into())
            .spawn(move || accept(&listener, &quit, &*service))?;
        Ok(Self { stop, thread })
    }

    /// End the loop and wait for it. Connections it handed over are the
    /// service's, and end with the job's namespace.
    fn finish(self) {
        let _ = self.stop.shutdown(std::net::Shutdown::Both);
        let _ = self.thread.join();
    }
}

fn accept(listener: &TcpListener, quit: &UnixStream, service: &dyn Local) {
    loop {
        let mut fds = [
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: quit.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: two valid pollfds, and the count says so.
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if r == -1 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if r == -1 || fds[1].revents != 0 {
            return;
        }
        match listener.accept() {
            Ok((conn, _)) => service.serve(conn),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // A full descriptor table, or a connection reset in the queue.
            Err(_) if fds[0].revents & libc::POLLIN != 0 => {
                std::thread::sleep(Duration::from_millis(50));
            }
            // The job's namespace is gone.
            Err(_) => return,
        }
    }
}

/// The job's list, and its spec readied for it: a job with a list gets the
/// listener inside its namespace and the proxy's variables, in place of any
/// the call set; one without gets neither. An entry that does not parse is
/// dropped, so the list never widens: the core checked each one before the
/// job was planned.
pub(crate) fn prepare(spec: &mut Spec, l1: &L1) -> Vec<Allow> {
    let allow: Vec<Allow> = l1.egress.iter().filter_map(|s| s.parse().ok()).collect();
    if !allow.is_empty() {
        spec.egress_port = Some(PORT);
        let vars = proxy_env(PORT);
        spec.env.retain(|(k, _)| !vars.iter().any(|(v, _)| v == k));
        spec.env.extend(vars);
    }
    allow
}

/// The proxy, on the listener the init handed over, for a job with a list;
/// None for one without. A name the daemon serves itself ([`register`]) is
/// served on the listener instead of the proxy. A proxy that cannot start
/// closes the listener, so the job has no route out at all, and
/// `detail.egress` says why.
pub(crate) fn start(
    child: &mut SandboxChild,
    l1: &L1,
    allow: &[Allow],
    detail: &mut Value,
) -> Option<Egress> {
    let fd = child.take_egress_listener()?;
    if let Some(service) = local_for(l1) {
        return match Accepting::start(TcpListener::from(fd), service) {
            Ok(a) => Some(Egress::Local(a)),
            Err(e) => {
                let mut s = serde_json::to_value(Summary::of(allow, &[], 0)).unwrap_or_default();
                s["error"] = json!(format!("the egress service did not start: {e}"));
                detail["egress"] = s;
                None
            }
        };
    }
    let resolver = l1.egress_dns.clone().unwrap_or_default();
    match Proxy::new(fd, allow.to_vec(), resolver).start() {
        Ok(running) => Some(Egress::Proxy(running)),
        Err(e) => {
            let mut s = serde_json::to_value(Summary::of(allow, &[], 0)).unwrap_or_default();
            s["error"] = json!(format!("the egress proxy did not start: {e}"));
            detail["egress"] = s;
            None
        }
    }
}

/// Stops the job's proxy, once the job has ended, and puts what it recorded
/// in `detail.egress` (a local service records no connections). None: the job
/// had no proxy.
pub(crate) fn stop(running: Option<Egress>, allow: &[Allow], detail: &mut Value) {
    let (log, dropped) = match running {
        None => return,
        Some(Egress::Proxy(running)) => running.finish(DRAIN),
        Some(Egress::Local(accepting)) => {
            accepting.finish();
            (Vec::new(), 0)
        }
    };
    detail["egress"] = serde_json::to_value(Summary::of(allow, &log, dropped)).unwrap_or_default();
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A service that answers each connection with its running count.
    struct Count(AtomicUsize);

    impl Local for Count {
        fn serve(&self, mut conn: TcpStream) {
            let n = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = conn.write_all(format!("conn {n}\n").as_bytes());
        }
    }

    fn read_all(addr: std::net::SocketAddr) -> String {
        let mut s = String::new();
        TcpStream::connect(addr)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        s
    }

    #[test]
    fn a_local_service_is_handed_each_connection_until_it_is_stopped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let svc = Arc::new(Count(AtomicUsize::new(0)));
        let acc = Accepting::start(listener, svc.clone()).unwrap();
        assert_eq!(read_all(addr), "conn 1\n");
        assert_eq!(read_all(addr), "conn 2\n");
        acc.finish();
        assert!(
            TcpStream::connect(addr).is_err(),
            "the listener went with it"
        );
        assert_eq!(svc.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_registration_serves_a_job_whose_list_is_that_name_alone_until_it_is_dropped() {
        let l1 = |egress: &[&str]| L1 {
            egress: egress.iter().map(|s| s.to_string()).collect(),
            ..L1::default()
        };
        let name = "relay.sealed.invalid:80";
        assert!(local_for(&l1(&[name])).is_none());
        let reg = register(name, Arc::new(Count(AtomicUsize::new(0))));
        assert!(local_for(&l1(&[name])).is_some());
        assert!(
            local_for(&l1(&[name, "example.com:443"])).is_none(),
            "a list with another name keeps the proxy"
        );
        assert!(local_for(&l1(&["example.com:443"])).is_none());
        drop(reg);
        assert!(local_for(&l1(&[name])).is_none());
    }
}
