//! The tender's socket: JSON-RPC 2.0, one request per line and one response
//! per line, on `<index>/sock` (mode 0600, in a 0700 directory). A thread
//! per connection, at most [`MAX_CONNECTIONS`] at once: the core is the only
//! client, and a recall holds one (its words' query, then its whole one, on
//! one connection: theseus-zo1y).
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
use std::sync::Arc;
use std::thread;

use serde_json::Value;
use theseus_protocol::{error_code, Id, Request, Response};

use crate::proto::{
    method, EmbedParams, EntitiesParams, EntitiesResult, ForgetParams, NeighboursParams,
    QueryParams, RebuildResult,
};
use crate::tender::Shared;

pub const MAX_CONNECTIONS: usize = 16;

/// Connections past [`MAX_CONNECTIONS`] served at once for `index.status`
/// alone (theseus-uazd): health asks under 100 ms, so a tender whose slots
/// are all held (queries waiting on the model, an `index.embed` its caller
/// left) still answers it, and says why it refuses anything else.
pub const STATUS_LANE: usize = 4;

/// How long a connection in the status lane may take to send its request.
const LANE_READ: std::time::Duration = std::time::Duration::from_secs(2);

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
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let active = Arc::new(AtomicUsize::new(0));
    let lane = Arc::new(AtomicUsize::new(0));
    let count = active.clone();
    let served = thread::Builder::new()
        .name("index-socket".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { continue };
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    status_lane(conn, &shared, &lane);
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
        })?;
    Ok((served, count))
}

/// A connection past the limit: served on a thread of its own for one
/// `index.status`, while fewer than [`STATUS_LANE`] are; else closed.
fn status_lane(conn: UnixStream, shared: &Arc<Shared>, lane: &Arc<AtomicUsize>) {
    if lane.fetch_add(1, Ordering::SeqCst) >= STATUS_LANE {
        lane.fetch_sub(1, Ordering::SeqCst);
        tracing::warn!("index: a connection over the limit, closed");
        return;
    }
    let (shared, done) = (shared.clone(), lane.clone());
    let spawned = thread::Builder::new()
        .name("index-status".into())
        .spawn(move || {
            if let Err(e) = serve_status(conn, &shared) {
                tracing::debug!(error = %e, "index: a status connection ended");
            }
            done.fetch_sub(1, Ordering::SeqCst);
        });
    if spawned.is_err() {
        lane.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One request on a connection past the limit: `index.status` answered,
/// anything else refused with why.
fn serve_status(conn: UnixStream, shared: &Shared) -> io::Result<()> {
    conn.set_read_timeout(Some(LANE_READ))?;
    let mut line = String::new();
    BufReader::new(conn.try_clone()?).read_line(&mut line)?;
    let answer = match serde_json::from_str::<Request>(&line) {
        Ok(r) if r.method == method::STATUS => Response::ok(r.id, shared.status()),
        Ok(r) => Response::err(
            r.id,
            error_code::INTERNAL,
            format!(
                "the index tender is serving {MAX_CONNECTIONS} connections; past them it answers index.status alone"
            ),
        ),
        Err(e) => Response::err(Id::Num(0), error_code::PARSE, format!("not a request: {e}")),
    };
    let mut out = serde_json::to_vec(&answer)?;
    out.push(b'\n');
    (&conn).write_all(&out)
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
        method::STATUS => Response::ok(id, shared.status()),
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
