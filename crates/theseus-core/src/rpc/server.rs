//! Serving a connection, and routing each request to its method by name.

use std::sync::Arc;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use theseus_protocol::{error_code, method, Id, Message, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use super::Core;
use crate::approval::{Client, Surface};

/// The connection a request came in on: who it is, the surface its listener
/// named, and where its notifications go.
#[derive(Clone, Copy)]
pub(super) struct Conn<'a> {
    pub client: &'a str,
    pub surface: Surface,
    pub tx: &'a mpsc::UnboundedSender<Message>,
}

impl Core {
    /// Serve one connection until EOF. `client` names the connection (its
    /// session watches are dropped when it goes away) and says which surface
    /// accepted it; a bare label is a surface no listener named.
    pub async fn serve_connection<R, W>(
        self: Arc<Self>,
        reader: R,
        mut writer: W,
        client: Client,
    ) -> Result<()>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let Client {
            label: client,
            surface,
        } = client;
        // One ordered outbound queue: notifications and responses share it, so a
        // turn's events always precede its response on the wire.
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let resp_tx = tx.clone();

        let writer_task = tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                let line = serde_json::to_string(&m);
                match line {
                    Ok(mut s) => {
                        s.push('\n');
                        if writer.write_all(s.as_bytes()).await.is_err() {
                            break;
                        }
                        let _ = writer.flush().await;
                    }
                    Err(_) => continue,
                }
            }
            let _ = writer.shutdown().await;
        });

        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            let msg: Message = match serde_json::from_str(&line) {
                Ok(m) => m,
                Err(e) => {
                    let _ = resp_tx.send(Message::Response(Response::err(
                        Id::Num(0),
                        error_code::PARSE,
                        format!("parse error: {e}"),
                    )));
                    continue;
                }
            };
            match msg {
                Message::Request(req) => {
                    let core = self.clone();
                    let tx = tx.clone();
                    let resp_tx = resp_tx.clone();
                    let client = client.clone();
                    tokio::spawn(async move {
                        let resp = core.handle(req, tx, &client, surface).await;
                        let _ = resp_tx.send(Message::Response(resp));
                    });
                }
                Message::Notification(n) => {
                    tracing::debug!(method = %n.method, "client notification ignored");
                }
                Message::Response(_) => {}
            }
        }
        drop(tx);
        drop(resp_tx);
        self.bus.drop_conn(&client);
        self.narrator.unwatch(&client);
        let _ = writer_task.await;
        Ok(())
    }

    async fn handle(
        self: Arc<Self>,
        req: Request,
        tx: mpsc::UnboundedSender<Message>,
        client: &str,
        surface: Surface,
    ) -> Response {
        let id = req.id.clone();
        match self.dispatch(req, tx, client, surface).await {
            Ok(v) => Response::ok(id, v),
            Err(f) => Response::err_with(id, f.code, f.message, f.data),
        }
    }

    /// Every method, by name: each parses its params, runs, and serializes
    /// its result (`route`, `reply`).
    async fn dispatch(
        self: Arc<Self>,
        req: Request,
        tx: mpsc::UnboundedSender<Message>,
        client: &str,
        surface: Surface,
    ) -> Result<Value, RpcFailure> {
        let conn = Conn {
            client,
            surface,
            tx: &tx,
        };
        let params = req.params;
        match req.method.as_str() {
            method::HEALTH => reply(self.health()),
            method::SESSION_OPEN => route(params, |p| self.session_open(p)),
            method::SESSION_LIST => reply(theseus_protocol::SessionListResult {
                sessions: self.session_list()?,
            }),
            method::TURN_SUBMIT => reply(self.turn_submit(parse(params)?, conn).await?),
            method::PROFILE_LIST => reply(self.profile_list()),
            method::PROFILE_USE => route(params, |p| self.profile_use(p, conn)),
            method::SESSION_HISTORY => route(params, |p| self.session_history(p)),
            method::SESSION_WATCH => route(params, |p| Ok(self.session_watch(p, conn))),
            method::SESSION_UNWATCH => route(params, |p| Ok(self.session_unwatch(p, conn))),
            method::SESSION_RECOMPILE => route(params, |p| self.session_recompile(p, conn)),
            method::CATALOG_LIST => reply(self.catalog_list()),
            method::COMPILATION_LIST => route(params, |p| self.compilation_list(p)),
            method::NODE_LIST => route(params, |p| self.node_list(p)),
            method::ACTION_CONFIRM => route(params, |p| self.action_confirm(p, conn)),
            method::CONFIRM_LIST => reply(theseus_protocol::ConfirmListResult {
                confirms: self.confirm_list()?,
            }),
            method::TOOL_LIST => reply(self.tool_list()),
            method::EXECUTION_LIST => reply(self.execution_list()?),
            method::ACTION_LIST => route(params, |p| self.action_list(p)),
            method::EXECUTION_CANCEL => route(params, |p| self.execution_cancel(p, conn)),
            method::LEDGER_TAIL => route(params, |p| self.ledger_tail(p)),
            method::NARRATIVE_WATCH | method::NARRATIVE_UNWATCH if !self.narrator.on() => {
                Err(RpcFailure::new(
                    error_code::DISABLED,
                    "narration is off: put `narrative = true` at the top of the config, \
                     before any [table], and restart the daemon",
                ))
            }
            method::NARRATIVE_WATCH => reply(self.narrative_watch(conn)),
            method::NARRATIVE_UNWATCH => reply(self.narrative_unwatch(conn)),
            method::SHUTDOWN => reply(self.stop()),
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }
}

/// A failed request: JSON-RPC code, human message, structured data.
#[derive(Debug)]
pub struct RpcFailure {
    pub code: i64,
    pub message: String,
    pub data: Value,
}

impl RpcFailure {
    pub(super) fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: Value::Null,
        }
    }

    /// The caller asked for something that cannot be: `INVALID_PARAMS`.
    pub(super) fn invalid(e: anyhow::Error) -> Self {
        Self::new(error_code::INVALID_PARAMS, e.to_string())
    }
}

/// Anything else that fails inside a method is `INTERNAL`.
impl From<anyhow::Error> for RpcFailure {
    fn from(e: anyhow::Error) -> Self {
        Self::new(error_code::INTERNAL, e.to_string())
    }
}

/// A method's params, or `INVALID_PARAMS`.
pub(super) fn parse<T: DeserializeOwned>(v: Value) -> Result<T, RpcFailure> {
    serde_json::from_value(v)
        .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, format!("invalid params: {e}")))
}

/// Parse a method's params, run it, and serialize its result.
fn route<P: DeserializeOwned, R: Serialize>(
    params: Value,
    method: impl FnOnce(P) -> Result<R, RpcFailure>,
) -> Result<Value, RpcFailure> {
    reply(method(parse(params)?)?)
}

/// A method's result, serialized.
fn reply<R: Serialize>(r: R) -> Result<Value, RpcFailure> {
    Ok(serde_json::to_value(r).unwrap())
}
