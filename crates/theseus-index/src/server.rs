//! The tender's socket: JSON-RPC 2.0, one request per line and one response
//! per line, on `<index>/sock` (mode 0600, in a 0700 directory). A thread
//! per connection, at most [`MAX_CONNECTIONS`] at once: the core is the only
//! client, and holds one or two.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
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

/// Bind the socket at `path` (a stale one from a killed tender is replaced:
/// the caller holds the directory's lock) and serve it on a thread.
pub fn spawn(path: &Path, shared: Arc<Shared>) -> io::Result<thread::JoinHandle<()>> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let active = Arc::new(AtomicUsize::new(0));
    thread::Builder::new()
        .name("index-socket".into())
        .spawn(move || {
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
        })
}

fn serve(conn: UnixStream, shared: &Shared) -> io::Result<()> {
    let reader = BufReader::new(conn.try_clone()?);
    let mut writer = conn;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut out = serde_json::to_vec(&answer(&line, shared))?;
        out.push(b'\n');
        writer.write_all(&out)?;
    }
    Ok(())
}

/// One request's response.
pub fn answer(line: &str, shared: &Shared) -> Response {
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
            match shared.query(&p) {
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
