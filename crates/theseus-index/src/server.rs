//! The tender's socket: JSON-RPC 2.0, one request per line and one response
//! per line, on `<index>/sock` (mode 0600, in a 0700 directory). A thread
//! per connection, at most [`MAX_CONNECTIONS`] at once: the core is the only
//! client, and a recall holds one (its words' query, then its whole one, on
//! one connection: theseus-zo1y). These threads run at the daemon's
//! priority, not at the tender's background one (`priority`, theseus-y9a3).
//!
//! **A caller that has gone frees its slot.** A query whose caller closed its
//! connection (a recall past its deadline, or whose turn ended) stops at the
//! embedder's next layer ([`closed`], asked between layers), and its thread
//! ends: one recall's abandoned vector query no longer holds a slot for the
//! whole of its embedding.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use serde_json::Value;
use theseus_protocol::{error_code, Id, Request, Response};

use crate::proto::{
    method, EmbedParams, EntitiesParams, EntitiesResult, ForgetParams, NeighboursParams,
    QueryParams, RebuildResult,
};
use crate::tender::Shared;

pub const MAX_CONNECTIONS: usize = 16;

/// Bind the socket at `path` (a stale one from a killed tender is replaced:
/// the caller holds the directory's lock) and serve it on a thread.
pub fn spawn(path: &Path, shared: Arc<Shared>) -> io::Result<thread::JoinHandle<()>> {
    Ok(spawn_counted(path, shared)?.0)
}

/// [`spawn`], and the count of connections it serves now (tests count them).
pub fn spawn_counted(
    path: &Path,
    shared: Arc<Shared>,
) -> io::Result<(thread::JoinHandle<()>, Arc<AtomicUsize>)> {
    let listener = bind(path)?;
    let active = Arc::new(AtomicUsize::new(0));
    let count = active.clone();
    let served = thread::Builder::new()
        .name("index-socket".into())
        .spawn(move || accept(&listener, &shared, &active))?;
    Ok((served, count))
}

/// [`spawn`] from a thread started before the tender opens (theseus-y9a3):
/// the socket's thread, and each connection's, keep the priority of the
/// thread that calls this, while the caller lowers itself and opens the
/// tender (`priority`). Once `shared` comes, it binds `path` and says how
/// that went on the returned channel; a `shared` that never comes (the open
/// failed) ends it.
pub fn spawn_early(
    path: &Path,
    shared: mpsc::Receiver<Arc<Shared>>,
) -> io::Result<mpsc::Receiver<io::Result<()>>> {
    let (said, bound) = mpsc::channel();
    let path = path.to_path_buf();
    thread::Builder::new()
        .name("index-socket".into())
        .spawn(move || {
            let Ok(shared) = shared.recv() else { return };
            let listener = match bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    let _ = said.send(Err(e));
                    return;
                }
            };
            let _ = said.send(Ok(()));
            accept(&listener, &shared, &Arc::new(AtomicUsize::new(0)));
        })?;
    Ok(bound)
}

/// Bind the socket at `path`, mode 0600; a stale one is replaced.
fn bind(path: &Path) -> io::Result<UnixListener> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Serve each connection on a thread of its own, at most
/// [`MAX_CONNECTIONS`] at once.
fn accept(listener: &UnixListener, shared: &Arc<Shared>, active: &Arc<AtomicUsize>) {
    for conn in listener.incoming() {
        let Ok(conn) = conn else { continue };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            tracing::warn!("index: a connection over the limit, closed");
            continue;
        }
        let (shared, done) = (shared.clone(), active.clone());
        let spawned = thread::Builder::new()
            .name("index-conn".into())
            .spawn(move || {
                if let Err(e) = serve(conn, &shared) {
                    tracing::debug!(error = %e, "index: a connection ended");
                }
                done.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn serve(conn: UnixStream, shared: &Shared) -> io::Result<()> {
    let reader = BufReader::new(conn.try_clone()?);
    let fd = conn.as_raw_fd();
    let gone = move || closed(fd);
    let mut writer = conn;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut out = serde_json::to_vec(&answer_while(&line, shared, &gone))?;
        out.push(b'\n');
        writer.write_all(&out)?;
    }
    Ok(())
}

/// Whether the peer of the connection on `fd` has closed it (or it broke):
/// `poll` with no wait, for `POLLRDHUP`. Requests already sent and not yet
/// read (a recall's second query) leave it open.
pub fn closed(fd: RawFd) -> bool {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLRDHUP,
        revents: 0,
    };
    // SAFETY: one pollfd, owned here, and a zero timeout.
    let n = unsafe { libc::poll(&mut p, 1, 0) };
    n > 0 && p.revents & (libc::POLLRDHUP | libc::POLLHUP | libc::POLLERR) != 0
}

/// One request's response.
pub fn answer(line: &str, shared: &Shared) -> Response {
    answer_while(line, shared, &|| false)
}

/// [`answer`] for a caller that may go: a query stops embedding once `gone`
/// says so.
pub fn answer_while(line: &str, shared: &Shared, gone: &(dyn Fn() -> bool + Sync)) -> Response {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Response::err(Id::Num(0), error_code::PARSE, format!("not a request: {e}"))
        }
    };
    let id = req.id.clone();
    match req.method.as_str() {
        method::QUERY => {
            let p: QueryParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
            };
            match shared.query_while(&p, gone) {
                Ok(r) => Response::ok(id, r),
                Err(e) => Response::err(id, error_code::INVALID_PARAMS, format!("{e:#}")),
            }
        }
        method::STATUS => {
            let mut s = shared.status();
            // Asked on this connection's thread: the queries' own priority.
            s.queries_at = Some(crate::priority::current());
            Response::ok(id, s)
        }
        method::REBUILD => {
            shared.request_rebuild();
            Response::ok(id, RebuildResult { accepted: true })
        }
        method::NEIGHBOURS => {
            let p: NeighboursParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
            };
            match shared.neighbours(&p) {
                Ok(r) => Response::ok(id, r),
                Err(e) => Response::err(id, error_code::INVALID_PARAMS, format!("{e:#}")),
            }
        }
        method::EMBED => {
            let p: EmbedParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
            };
            match shared.embed(&p) {
                Ok(r) => Response::ok(id, r),
                Err(e) => Response::err(id, error_code::INVALID_PARAMS, format!("{e:#}")),
            }
        }
        method::WARM => Response::ok(id, shared.warm()),
        method::ENTITIES => {
            let p: EntitiesParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
            };
            match entities(&p) {
                Ok(r) => Response::ok(id, r),
                Err(e) => Response::err(id, error_code::INVALID_PARAMS, e),
            }
        }
        method::FORGET => {
            let p: ForgetParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => return Response::err(id, error_code::INVALID_PARAMS, e.to_string()),
            };
            match shared.forget(p) {
                Ok(r) => Response::ok(id, r),
                Err(e) => Response::err(id, error_code::INTERNAL, format!("{e:#}")),
            }
        }
        other => Response::err_with(
            id,
            error_code::METHOD_NOT_FOUND,
            format!("no method {other:?} on the index tender"),
            Value::Null,
        ),
    }
}

/// `index.entities`: each text through the one extractor the entity field
/// is built by (`entity::entities`). Pure: no index, no model.
pub fn entities(p: &EntitiesParams) -> Result<EntitiesResult, String> {
    if p.texts.len() > EntitiesParams::MAX_TEXTS {
        return Err(format!(
            "{} texts: index.entities takes at most {}",
            p.texts.len(),
            EntitiesParams::MAX_TEXTS
        ));
    }
    Ok(EntitiesResult {
        entities: p
            .texts
            .iter()
            .map(|t| crate::entity::entities(t).into_iter().collect())
            .collect(),
    })
}
