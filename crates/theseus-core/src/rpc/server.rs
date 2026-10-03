//! Serving a connection, and routing each request to its method by name.

use std::sync::Arc;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use theseus_protocol::{error_code, method, Id, Message, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

use super::Core;
use crate::approval::{Answerer, Client, Peer, Surface};
use crate::outbound::{Drain, Outbound};

/// The connection a request came in on: who it is, the surface its listener
/// named, and where its notifications go.
#[derive(Clone, Copy)]
pub(super) struct Conn<'a> {
    pub client: &'a str,
    pub surface: Surface,
    /// The process on the other end, as the listener read it (theseus-6qy).
    pub peer: &'a Peer,
    /// The connection's one queue, with the backlog cap (theseus-in3).
    pub tx: &'a Outbound,
    /// Closed when the connection's reader ends: a parked `session.wait`
    /// ends with it (theseus-in3).
    pub closed: &'a watch::Receiver<()>,
    /// When the request arrived: a turn's trace starts there.
    pub arrived: std::time::Instant,
}

impl Conn<'_> {
    /// Who a cancel names (DD8): the author the request gives (the Discord
    /// binding gives the person), else the surface (`the CLI`, `the web
    /// UI`), else, on a surface no listener named, the connection's label.
    /// A connection's own label (`sock#13`) names no one a reader knows.
    pub fn actor(&self, author: Option<&str>) -> String {
        match (author, self.surface) {
            (Some(a), _) => a.to_string(),
            (None, Surface::Unnamed) => self.client.to_string(),
            (None, s) => s.name().to_string(),
        }
    }

    /// Who makes an approval-like act on this connection (an answer, a
    /// "should have asked" press, an undo, a trust), as the connection knows
    /// it: the label names, as a cancel's actor does (`the CLI`, never
    /// `sock#32`; theseus-qiy), and the surface, the binding's Discord ids,
    /// and the process on the other end decide (theseus-sgh, theseus-6qy).
    /// Every such method builds it here, so whatever judges an answer judges
    /// the others the same way, and an approval's trust and `policy.trust`
    /// name the same one.
    pub fn answerer(
        &self,
        author: Option<String>,
        discord: Option<theseus_protocol::DiscordOrigin>,
    ) -> Answerer {
        Answerer {
            label: self.actor(author.as_deref()),
            surface: self.surface,
            discord,
            peer: self.peer.clone(),
        }
    }
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
            peer,
        } = client;
        // One ordered outbound queue: notifications and responses share it, so a
        // turn's events always precede its response on the wire. Past the
        // backlog cap its notifications are dropped until it drains, and the
        // writer then says what was lost (theseus-in3).
        let (tx, mut rx) = crate::outbound::channel(self.push.lost.clone());
        let resp_tx = tx.clone();
        // Asks to be told once everything queued before the ask is written
        // (theseus-ur0): a stop's answer, before the serving loops wake.
        let (flush_tx, mut flush_rx) = mpsc::unbounded_channel::<oneshot::Sender<()>>();
        // Dropped when the reader ends: a parked `session.wait` ends with it.
        let (closed_tx, closed_rx) = watch::channel(());

        let core = self.clone();
        let writer_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    m = rx.recv() => match m {
                        Some(m) => {
                            if !write_one(&core, &mut writer, &mut rx, &m).await {
                                break;
                            }
                        }
                        None => break,
                    },
                    Some(ack) = flush_rx.recv() => {
                        // An ask is sent after what it waits for was queued,
                        // so all of that can be taken now.
                        let mut open = true;
                        while let Some(m) = rx.try_recv() {
                            open = write_one(&core, &mut writer, &mut rx, &m).await;
                            if !open {
                                break;
                            }
                        }
                        let _ = ack.send(());
                        if !open {
                            break;
                        }
                    }
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
                    resp_tx.respond(Message::Response(Response::err(
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
                    let flush_tx = flush_tx.clone();
                    let client = client.clone();
                    let peer = peer.clone();
                    let closed = closed_rx.clone();
                    let shutdown = req.method == method::SHUTDOWN;
                    tokio::spawn(async move {
                        let resp = core
                            .clone()
                            .handle(req, tx, &client, surface, &peer, &closed)
                            .await;
                        let stopping = shutdown && resp.error.is_none();
                        resp_tx.respond(Message::Response(resp));
                        if stopping {
                            core.wake_after_answer(&flush_tx).await;
                        }
                    });
                }
                Message::Notification(n) => {
                    tracing::debug!(method = %n.method, "client notification ignored");
                }
                Message::Response(_) => {}
            }
        }
        drop(closed_tx);
        drop(tx);
        drop(resp_tx);
        drop(flush_tx);
        self.bus.drop_conn(&client);
        self.narrator.unwatch(&client);
        let _ = writer_task.await;
        Ok(())
    }

    /// Wake the serving loops for a client's `shutdown` once its answer is on
    /// the wire (theseus-ur0): the connection's writer says when everything
    /// queued before the ask is written, or that it is gone. Bounded, so a
    /// client that never reads cannot hold the stop.
    async fn wake_after_answer(&self, flush: &mpsc::UnboundedSender<oneshot::Sender<()>>) {
        let (ack, written) = oneshot::channel();
        if flush.send(ack).is_ok() {
            let _ = tokio::time::timeout(ANSWER_FLUSH, written).await;
        }
        self.shutdown.notify_waiters();
    }

    async fn handle(
        self: Arc<Self>,
        req: Request,
        tx: Outbound,
        client: &str,
        surface: Surface,
        peer: &Peer,
        closed: &watch::Receiver<()>,
    ) -> Response {
        let id = req.id.clone();
        match self.dispatch(req, tx, client, surface, peer, closed).await {
            Ok(v) => Response::ok(id, v),
            Err(f) => Response::err_with(id, f.code, f.message, f.data),
        }
    }

    /// Every method, by name: each parses its params, runs, and serializes
    /// its result (`route`, `reply`).
    async fn dispatch(
        self: Arc<Self>,
        req: Request,
        tx: Outbound,
        client: &str,
        surface: Surface,
        peer: &Peer,
        closed: &watch::Receiver<()>,
    ) -> Result<Value, RpcFailure> {
        let arrived = std::time::Instant::now();
        let conn = Conn {
            client,
            surface,
            peer,
            tx: &tx,
            closed,
            arrived,
        };
        let params = req.params;
        match req.method.as_str() {
            method::HEALTH => reply(self.health_now().await),
            method::SESSION_OPEN => route(params, |p| self.session_open(p)),
            method::SESSION_LIST => {
                // Its filter is optional: no params lists every session.
                let params = if params.is_null() { json!({}) } else { params };
                route(params, |p| self.session_list_of(p))
            }
            method::TURN_SUBMIT => reply(self.turn_submit(parse(params)?, conn).await?),
            method::PROFILE_LIST => reply(self.profile_list()),
            method::PROFILE_USE => route(params, |p| self.profile_use(p, conn)),
            method::SESSION_HISTORY => route(params, |p| self.session_history(p)),
            // The first watch of any kind seeds the push (theseus-tq04).
            method::SESSION_WATCH => reply(self.session_watch(parse(params)?, conn).await?),
            method::SESSION_UNWATCH => route(params, |p| Ok(self.session_unwatch(p, conn))),
            method::SESSION_RECOMPILE => route(params, |p| self.session_recompile(p, conn)),
            method::CATALOG_LIST => reply(self.catalog_list()),
            method::COMPILATION_LIST => route(params, |p| self.compilation_list(p)),
            method::NODE_LIST => route(params, |p| self.node_list(p)),
            // A walk may read several sessions, so it runs off the serving
            // workers. Its params are parsed first: none fails at once.
            method::NODE_REACH => {
                let p = parse(params)?;
                let core = self.clone();
                reply(
                    tokio::task::spawn_blocking(move || core.node_reach(p))
                        .await
                        .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))??,
                )
            }
            method::ACTION_CONFIRM => route(params, |p| self.action_confirm(p, conn)),
            method::POLICY_TIGHTEN => route(params, |p| self.policy_tighten(p, conn)),
            method::POLICY_UNTIGHTEN => route(params, |p| self.policy_untighten(p, conn)),
            method::POLICY_TRUST => route(params, |p| self.policy_trust(p, conn)),
            method::PLACE_PUBLISH => route(params, |p| self.place_publish(p, conn)),
            method::CONFIRM_LIST => reply(theseus_protocol::ConfirmListResult {
                confirms: self.confirm_list()?,
            }),
            method::TOOL_LIST => reply(self.tool_list()),
            method::EXECUTION_LIST => reply(self.execution_list()?),
            method::ACTION_LIST => route(params, |p| self.action_list(p)),
            // A cancel and a stop wait for the jobs they end on the runtime's
            // timer, never holding a worker (theseus-bzq).
            method::EXECUTION_CANCEL => reply(self.execution_cancel(parse(params)?, conn).await?),
            method::EXECUTION_STOP => reply(self.execution_stop(parse(params)?, conn).await?),
            method::TASK_LIST => {
                // Its filters are optional: no params lists every task.
                let params = if params.is_null() { json!({}) } else { params };
                route(params, |p| self.task_list(p))
            }
            method::TASK_CANCEL => reply(self.task_cancel(parse(params)?, conn).await?),
            method::WAKE_LIST => {
                // Its filters are optional: no params lists every wake.
                let params = if params.is_null() { json!({}) } else { params };
                route(params, |p| self.wake_list(p))
            }
            method::WAKE_CANCEL => route(params, |p| self.wake_cancel(p, conn)),
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
            method::EXECUTIONS_WATCH => {
                // Its limit is optional: no params takes the default.
                let params = if params.is_null() { json!({}) } else { params };
                reply(self.executions_watch(parse(params)?, conn).await?)
            }
            method::EXECUTIONS_UNWATCH => reply(self.executions_unwatch(conn)),
            // A wait parks on the board's feed, never holding a worker.
            method::SESSION_WAIT => reply(self.session_wait(parse(params)?, conn).await?),
            // The index tender (roadmap row 51), asked on its socket, bounded.
            method::INDEX_STATUS => reply(self.index.health(crate::tender::STATUS_DEADLINE).await),
            method::INDEX_QUERY => reply(self.index_query(parse(params)?).await?),
            method::BENCH_HISTORY => reply(self.bench_history(params).await?),
            method::SANDBOX_USAGE => reply(self.sandbox_usage()),
            // AWS's bootstrap (C2): the plan reads; the apply waits for the stacks.
            method::AWS_BOOTSTRAP => reply(self.aws_bootstrap(parse(params)?, conn).await?),
            // The loops wake once the answer is written (`serve_connection`).
            method::SHUTDOWN => reply(self.stopping()),
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }
}

/// The longest a client's `shutdown` waits for its answer to be written
/// before the serving loops wake (theseus-ur0). A write takes microseconds;
/// the bound is for a client that stopped reading.
const ANSWER_FLUSH: std::time::Duration = std::time::Duration::from_secs(1);

/// Write one message, and then, if the queue has drained after dropping
/// notifications, the `events.lost` that says so (theseus-in3). False once
/// the connection is gone.
async fn write_one<W: AsyncWrite + Unpin>(
    core: &Core,
    writer: &mut W,
    rx: &mut Drain,
    m: &Message,
) -> bool {
    if !write_line(writer, m).await {
        return false;
    }
    match rx.written() {
        Some(lost) => {
            core.telemetry().record_push_lost(lost.dropped);
            tracing::info!(
                dropped = lost.dropped,
                streams = ?lost.streams,
                "a connection fell behind; it heard what it lost"
            );
            write_line(
                writer,
                &Message::from(theseus_protocol::Event::EventsLost(lost)),
            )
            .await
        }
        None => true,
    }
}

/// Write one message as an NDJSON line and flush it. False once the
/// connection is gone; a message that does not serialize is skipped.
async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, m: &Message) -> bool {
    let Ok(mut s) = serde_json::to_string(m) else {
        return true;
    };
    s.push('\n');
    if writer.write_all(s.as_bytes()).await.is_err() {
        return false;
    }
    let _ = writer.flush().await;
    true
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
