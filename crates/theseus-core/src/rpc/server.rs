//! Serving a connection, and routing each request to its method by name.

use std::sync::Arc;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use theseus_protocol::method::TASK_NOT_THIS;
use theseus_protocol::{error_code, method, Id, Message, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch, Semaphore};

use super::Core;
use crate::approval::{Answerer, Client, Surface};
use crate::outbound::{Drain, Item, Outbound};
use crate::peer_proc::Behind;

/// The connection a request came in on: who it is, the surface its listener
/// named, and where its notifications go.
#[derive(Clone, Copy)]
pub(super) struct Conn<'a> {
    pub client: &'a str,
    pub surface: Surface,
    /// The process behind the connection (theseus-5rvss).
    pub peer: &'a crate::peer_proc::Peer,
    /// The socket peer's pid, as the daemon logs it for the connection: what a
    /// row of the vault's keeping says about which run it was (issue 29).
    pub pid: Option<u32>,
    /// The connection's one queue, with the backlog cap (theseus-in3).
    pub tx: &'a Outbound,
    /// Closed when the connection's reader ends: a parked `session.wait`
    /// ends with it (theseus-in3).
    pub closed: &'a watch::Receiver<()>,
    /// When the request arrived: a turn's trace starts there.
    pub arrived: std::time::Instant,
}

impl Conn<'_> {
    /// Who a cancel, a profile switch, or a recompile names (DD8, V8
    /// theseus-cny7): the author the request gives (the Discord
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
    /// `sock#32`; theseus-qiy), and the surface and the binding's Discord ids
    /// decide (the place rule, theseus-zmgb).
    /// Every such method builds it here, so whatever judges an answer judges
    /// the others the same way, and a content label's removal names the same
    /// one.
    pub fn answerer(
        &self,
        author: Option<String>,
        discord: Option<theseus_protocol::DiscordOrigin>,
    ) -> Answerer {
        // An answer from a process under an agent, or (on the CLI) with no
        // terminal, is that agent's: its name is the label, so every row and
        // line that says `by` says it (theseus-5rvss). The web UI's `/ws` is
        // read the same way, by the process holding the client's end of the
        // connection (theseus-iie8e): its port is open to every process of
        // the owner's own user, and `Host` and `Origin` are headers any of
        // them can send, so nothing else tells an agent's WebSocket from the
        // cockpit's page.
        let agent = match (self.surface, self.peer.behind()) {
            (Surface::Cli | Surface::Web, Behind::Agent(name)) => Some(name.clone()),
            _ => None,
        };
        Answerer {
            label: agent
                .as_deref()
                .map_or_else(|| self.actor(author.as_deref()), theseus_protocol::agent_by),
            surface: self.surface,
            discord,
            agent,
        }
    }
}

/// A connection as its requests see it: its label, the surface that accepted
/// it, and the process behind it.
pub(super) struct Link {
    label: String,
    surface: Surface,
    peer: crate::peer_proc::Peer,
    pid: Option<u32>,
}

impl Link {
    /// `agents` is `[policy] agent_processes`, which says who is an agent.
    fn new(client: Client, agents: &[String]) -> Self {
        let pid = match &client.peer {
            crate::peer_proc::Source::Pid(pid) => Some(*pid),
            _ => None,
        };
        let peer = crate::peer_proc::Peer::new(client.peer, agents.into());
        #[cfg(test)]
        let peer = client.behind.map_or(peer, crate::peer_proc::Peer::known);
        Self {
            label: client.label,
            surface: client.surface,
            peer,
            pid,
        }
    }
}

impl Link {
    fn conn<'a>(
        &'a self,
        tx: &'a Outbound,
        closed: &'a watch::Receiver<()>,
        arrived: std::time::Instant,
    ) -> Conn<'a> {
        Conn {
            client: &self.label,
            surface: self.surface,
            peer: &self.peer,
            pid: self.pid,
            tx,
            closed,
            arrived,
        }
    }
}

impl Core {
    /// The NDJSON byte budget, including its final newline. In-process clients
    /// use the same configured budget before writing to their sole connection.
    pub fn request_line_cap(&self) -> usize {
        request_cap(self.cfg.tools.max_attachment_bytes)
    }

    /// Serve one connection until EOF. `client` names the connection (its
    /// session watches are dropped when it goes away) and says which surface
    /// accepted it; a bare label is a surface no listener named.
    pub async fn serve_connection<R, W>(
        self: Arc<Self>,
        reader: R,
        writer: W,
        client: Client,
    ) -> Result<()>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let link = Arc::new(Link::new(client, &self.cfg.policy.agent_processes));
        let client = link.label.clone();
        // One ordered outbound queue: notifications and responses share it, so a
        // turn's events always precede its response on the wire. Past the
        // backlog cap its notifications are dropped until it drains, and the
        // writer then says what was lost (theseus-in3). The cap is
        // `BACKLOG_CAP`; only a test may lower it (theseus-0u6g).
        #[cfg(test)]
        let cap = self
            .push
            .backlog_cap
            .load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(test))]
        let cap = crate::outbound::BACKLOG_CAP;
        let (tx, rx) = crate::outbound::channel_capped(self.push.lost.clone(), cap);
        let resp_tx = tx.clone();
        // Asks to be told once everything queued before the ask is written
        // (theseus-ur0): a stop's answer, before the serving loops wake.
        let (flush_tx, flush_rx) = mpsc::unbounded_channel::<oneshot::Sender<()>>();
        // Dropped when the reader ends: a parked `session.wait` ends with it.
        let (closed_tx, closed_rx) = watch::channel(());

        let writer_task = tokio::spawn(write_connection(
            self.clone(),
            writer,
            rx,
            flush_rx,
            tx.closed_signal(),
        ));

        // The ordered lane (theseus-klo2): what changes a session applies in
        // the order it arrived.
        let lane = Arc::new(super::ordered::Lane::default());
        let requests = Admission::new();
        let mut reader = BufReader::new(reader);
        let line_cap = self.request_line_cap();
        let mut reader_closed = tx.closed_signal();
        loop {
            let next = tokio::select! {
                _ = connection_closed(&mut reader_closed) => break,
                line = request_line(&mut reader, line_cap) => line,
            };
            let mut line = match next {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(e) => {
                    reject_line(&client, line_cap, e, &tx, &flush_tx).await;
                    break;
                }
            };
            line.truncate(line.trim_end().len());
            let leading = line.len() - line.trim_start().len();
            line.drain(..leading);
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
                    let Some(permit) = requests.take(&req.method) else {
                        resp_tx.respond(Message::Response(Response::err(
                            req.id,
                            error_code::LIMIT,
                            "too many requests in flight on this connection",
                        )));
                        continue;
                    };
                    let core = self.clone();
                    let tx = tx.clone();
                    let resp_tx = resp_tx.clone();
                    let flush_tx = flush_tx.clone();
                    let link = link.clone();
                    let closed = closed_rx.clone();
                    let shutdown = req.method == method::SHUTDOWN;
                    // Entered here, in its arrival order (theseus-klo2).
                    let ticket = lane.ticket_for(&req.method);
                    tokio::spawn(async move {
                        let _permit = permit;
                        let resp = core.handle_in_lane(ticket, req, tx, &link, &closed).await;
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
        tx.stop_watching();
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

    /// `handle`, after the earlier requests on its lane have taken effect
    /// when it is an ordered one, with its mark in reach (theseus-klo2).
    async fn handle_in_lane(
        self: &Arc<Self>,
        ticket: Option<super::ordered::Ticket>,
        req: Request,
        tx: Outbound,
        link: &Link,
        closed: &watch::Receiver<()>,
    ) -> Response {
        let Some(t) = ticket else {
            return self.clone().handle(req, tx, link, closed).await;
        };
        let waited = t.wait(self.lane_key(&req.method, &req.params)).await;
        self.telemetry().record_lane_wait(&req.method, waited);
        let work = self.clone().handle(req, tx, link, closed);
        super::ordered::scope(t.applied(), work).await
    }

    async fn handle(
        self: Arc<Self>,
        req: Request,
        tx: Outbound,
        link: &Link,
        closed: &watch::Receiver<()>,
    ) -> Response {
        let id = req.id.clone();
        // What a trim that finds a peak names (issue 21): this method, its
        // wall time, and the size of its answer.
        let mut ran = self
            .resident
            .activity()
            .ran(crate::resident::activity::Kind::Rpc, &req.method);
        let answer = self.clone().dispatch(req, tx, link, closed).await;
        if let Ok(v) = &answer {
            ran.bytes(crate::resident::activity::json_len(v));
        }
        // A trim follows once the daemon has been quiet (theseus-9lxe).
        self.resident.mark();
        match answer {
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
        link: &Link,
        closed: &watch::Receiver<()>,
    ) -> Result<Value, RpcFailure> {
        let arrived = std::time::Instant::now();
        let surface = link.surface;
        let conn = link.conn(&tx, closed, arrived);
        let params = req.params;
        // The MCP server's connection (step 41b): its tools' methods, and
        // the owner's acts, which their judgment refuses.
        self.mcp_admit(surface, &req.method, &params)?;
        match req.method.as_str() {
            method::HEALTH => reply(self.health_now().await),
            method::SESSION_OPEN => route(params, |p| self.session_open_on(surface, p)),
            m if super::sessions::OWN.contains(&m) => self.rpc_sessions(m, params, conn),
            m if super::retry::OWN.contains(&m) => self.rpc_retry(m, params, conn),
            method::TURN_SUBMIT => reply(self.turn_submit(parse(params)?, conn).await?),
            method::PROFILE_LIST => reply(self.profile_list()),
            method::PROFILE_USE => route(params, |p| self.profile_use(p, conn)),
            method::PROFILE_RELEASE => reply(self.profile_release(conn)?),
            method::PROFILE_PINS => reply(self.profile_pins().map_err(RpcFailure::invalid)?),
            method::PROFILE_UNPIN => route(or_empty(params), |p| self.profile_unpin(p, conn)),
            method::SESSION_HISTORY => route(params, |p| self.session_history(p)),
            // The first watch of any kind seeds the push (theseus-tq04).
            method::SESSION_WATCH => reply(self.session_watch(parse(params)?, conn).await?),
            method::SESSION_UNWATCH => route(params, |p| Ok(self.session_unwatch(p, conn))),
            method::CATALOG_LIST => reply(self.catalog_list()),
            method::COMPILATION_LIST => route(params, |p| self.compilation_list(p)),
            method::NODE_LIST => route(params, |p| self.node_list(p)),
            method::NODE_REACH => self.rpc_node_reach(params).await,
            method::ACTION_CONFIRM => route(params, |p| self.action_confirm(p, conn)),
            method::WORK_ANSWER => route(params, |p| self.work_answer(p, conn)),
            method::POLICY_TIGHTEN => route(params, |p| self.policy_tighten(p, conn)),
            method::POLICY_UNTIGHTEN => route(params, |p| self.policy_untighten(p, conn)),
            method::CONTENT_UNLABEL => route(params, |p| self.content_unlabel(p, conn)),
            method::PLACE_PUBLISH => route(params, |p| self.place_publish(p, conn)),
            method::TASK_START => route(params, |p| self.task_start_rpc(p, conn)),
            m @ (method::OWNER_PROOF_ENROLL
            | method::OWNER_PROOF_CONFIRM
            | method::OWNER_PROOF_STATUS
            | method::OWNER_PROOF_RESET
            | method::OWNER_PROOF_LOCK) => self.rpc_owner_proof(m, params, conn),
            method::HANDS_LIST => route(params, |p| self.rpc_hands_list(&p)),
            method::BUDGET_LIST | method::POLICY_EXPLAIN => self.rpc_reads(&req.method, params),
            method::ONTOLOGY_LIST
            | method::ONTOLOGY_CATEGORY_ADD
            | method::ONTOLOGY_GUIDANCE_SET
            | method::ONTOLOGY_MEMBERSHIP_SET
            | method::ONTOLOGY_PROPOSALS
            | method::ONTOLOGY_PROPOSAL_ACCEPT
            | method::ONTOLOGY_PROPOSAL_REJECT => self.rpc_ontology(&req.method, params, conn),
            method::CONFIRM_LIST => route(or_empty(params), |p| self.confirm_list_for(p, conn)),
            method::TOOL_LIST => reply(self.tool_list()),
            method::EXECUTION_LIST => route(or_empty(params), |p| self.execution_list(p)),
            method::ACTION_LIST => route(params, |p| self.action_list(p)),
            // A cancel and a stop wait for the jobs they end on the runtime's
            // timer, never holding a worker (theseus-bzq).
            method::EXECUTION_CANCEL => reply(self.execution_cancel(parse(params)?, conn).await?),
            method::EXECUTION_STOP => reply(self.execution_stop(parse(params)?, conn).await?),
            // Its filters are optional: no params lists every task.
            method::TASK_LIST => route(or_empty(params), |p| self.task_list(p)),
            m @ (method::TASK_CANCEL | TASK_NOT_THIS) => self.task_stop(m, params, conn).await,
            m @ (method::TASK_PAUSE | method::TASK_RESUME) => self.rpc_task_pause(m, params, conn),
            method::TASK_REHOME => self.rpc_task_rehome(params, conn),
            method::TASK_RESCHEDULE => self.rpc_task_reschedule(params, conn),
            method::SESSION_PROFILE => self.rpc_session_profile(params, conn),
            // Its filters are optional: no params lists every wake.
            method::WAKE_LIST => route(or_empty(params), |p| self.wake_list(p)),
            method::WAKE_CANCEL => route(params, |p| self.wake_cancel(p, conn)),
            m @ (method::LEDGER_TAIL | method::LEDGER_WINDOW) => self.ledger_reads(m, params),
            method::NARRATIVE_WATCH => reply(self.narrative_watch(parse(or_empty(params))?, conn)),
            method::NARRATIVE_UNWATCH => reply(self.narrative_unwatch(conn)),
            m @ (method::EXECUTIONS_WATCH | method::STATUS_SHORT) => {
                self.executions_reads(m, params, conn).await
            }
            method::EXECUTIONS_UNWATCH => reply(self.executions_unwatch(conn)),
            // A wait parks on the board's feed, never holding a worker.
            method::SESSION_WAIT => reply(self.session_wait(parse(params)?, conn).await?),
            m if super::index_rebuild::OWN.contains(&m) => self.rpc_index(m, params, conn).await,
            m @ (method::MEMORY_SEARCH | method::MEMORY_LOOKUP | method::HISTORY_READ) => {
                self.mem(m, params, conn).await
            }
            m @ (method::MEMORY_RECALLS | method::MEMORY_CITES) => self.mem_reads(m, params),
            method::MEMORY_LABEL | method::FACT_RETRACT => self.rpc_acts(&req.method, params, conn),
            // Jev's judgments (M5 23b): their rows, and one with its state.
            method::JUDGE_LIST | method::JUDGE_GET => self.rpc_reads(&req.method, params),
            // The learning ledger (M5 25c): a label, and the report.
            method::JUDGE_LABEL => route(params, |p| self.rpc_judge_label(p, conn)),
            m @ (method::LEARNING_REPORT | method::JUDGE_PROVE) => self.rpc_ledger(m, params).await,
            m if super::judge_runs::RUNS.contains(&m) => self.rpc_judge_run(m, params, conn).await,
            m if super::import::prefixed(m) => self.clone().rpc_prefixed(m, params, conn).await,
            method::BENCH_HISTORY => reply(self.bench_history(params).await?),
            method::SANDBOX_USAGE => reply(self.sandbox_usage()),
            method::MCP_LIST => reply(self.mcp.list(&self.tools)),
            m @ (method::EXTEND_LIST | method::EXTENSION_REVOKE) => self.rpc_ext(m, params, conn),
            method::MCP_RESTART => reply(self.mcp_restart(parse(params)?)?),
            method::MCP_PROMPT_LIST => reply(self.mcp_prompt_list(params)?),
            // AWS's bootstrap (C2): the plan reads; the apply waits for the stacks.
            method::AWS_BOOTSTRAP => reply(self.aws_bootstrap(parse(params)?, conn).await?),
            method::AWS_CONFIRM_ALERTS => {
                reply(self.aws_confirm_alerts(parse(params)?, conn).await?)
            }
            // The loops wake once the answer is written (`serve_connection`).
            method::SHUTDOWN => reply(self.stopping()),
            other => Err(RpcFailure::new(
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }

    /// `node.reach`, from one arm of `dispatch` (the shape budget). A walk may
    /// read several sessions, so it runs off the serving workers. Its params
    /// are parsed first: none fails at once.
    async fn rpc_node_reach(self: &Arc<Self>, params: Value) -> Result<Value, RpcFailure> {
        let p = parse(params)?;
        let core = self.clone();
        reply(
            tokio::task::spawn_blocking(move || core.node_reach(p))
                .await
                .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))??,
        )
    }

    /// 43a's list and 43b's revoke, from one arm of `dispatch`, which stays
    /// within clippy's length that way (the shape budget).
    fn rpc_ext(&self, name: &str, params: Value, conn: Conn<'_>) -> Result<Value, RpcFailure> {
        match name {
            method::EXTEND_LIST => reply(self.extend_list_with(&parse(or_empty(params))?)?),
            _ => route(params, |p| self.rpc_extension_revoke(p, conn)),
        }
    }

    /// Reads from one arm of `dispatch` each, which stays within clippy's
    /// length that way (the shape budget): the operator's two (step 42a),
    /// and Jev's judgments (M5 23b).
    fn rpc_reads(&self, name: &str, params: Value) -> Result<Value, RpcFailure> {
        match name {
            method::BUDGET_LIST => reply(self.budget_list()?),
            method::POLICY_EXPLAIN => route(or_empty(params), |p| self.policy_explain(p)),
            method::JUDGE_LIST => reply(self.judge_list(parse(params)?)?),
            _ => reply(self.judge_get(parse(params)?)?),
        }
    }
}

/// One writer owns draining and byte-accounting; forced closure also
/// interrupts an in-progress socket write to a peer that stopped reading.
async fn write_connection<W: AsyncWrite + Unpin>(
    core: Arc<Core>,
    mut writer: W,
    mut rx: Drain,
    mut flush_rx: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
    mut closed: watch::Receiver<bool>,
) {
    let writing = async {
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
    };
    tokio::select! {
        _ = connection_closed(&mut closed) => {},
        _ = writing => {},
    }
    drop(rx);
    let _ = tokio::time::timeout(ANSWER_FLUSH, writer.shutdown()).await;
}

// Inline images are base64 encoded, and import batches carry many episodes.
// Nothing bounds how many files one `turn.submit` carries; the largest batch a
// bound surface sends is a Discord message's: ten attachments, each up to
// `[tools] max_attachment_bytes` (theseus-discord `files.rs`). Room for ten such
// files base64 encoded, plus one MiB of JSON envelope; a larger frame is refused
// with LIMIT. A wire budget, not permission to bypass each attachment's decoded
// cap (`attach::from_wire`). The 16 MiB floor keeps images/imports with a tiny cap.
const WIRE_ATTACHMENTS: u64 = 10;

fn request_cap(max_attachment_bytes: u64) -> usize {
    let encoded = max_attachment_bytes.div_ceil(3).saturating_mul(4);
    usize::try_from(
        encoded
            .saturating_mul(WIRE_ATTACHMENTS)
            .saturating_add(1024 * 1024),
    )
    .unwrap_or(usize::MAX - 1)
    .clamp(16 * 1024 * 1024, usize::MAX - 1)
}
const REQUEST_CAP: usize = 64;
const CONTROL_CAP: usize = 8;

struct Admission {
    requests: Arc<Semaphore>,
    controls: Arc<Semaphore>,
}

impl Admission {
    fn new() -> Self {
        Self {
            requests: Arc::new(Semaphore::new(REQUEST_CAP)),
            controls: Arc::new(Semaphore::new(CONTROL_CAP)),
        }
    }

    fn take(&self, method: &str) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let control = matches!(
            method,
            method::SHUTDOWN
                | method::EXECUTION_STOP
                | method::EXECUTION_CANCEL
                | method::TASK_CANCEL
                | method::TASK_PAUSE
                | method::TASK_NOT_THIS
                | method::JOB_STOP
                | method::WORK_ANSWER
                | method::ACTION_CONFIRM
                | method::CARD_ANSWER
        );
        let slots = if control {
            &self.controls
        } else {
            &self.requests
        };
        slots.clone().try_acquire_owned().ok()
    }
}

/// Say why a frame was refused before closing, without waiting forever
/// for a peer that stopped reading. No request id can be trusted yet.
async fn reject_line(
    client: &str,
    cap: usize,
    error: std::io::Error,
    tx: &Outbound,
    flush: &mpsc::UnboundedSender<oneshot::Sender<()>>,
) {
    tracing::warn!(client, limit_bytes = cap, reason = %error, "request line rejected");
    tx.respond(Message::Response(Response::err(
        Id::Num(0),
        error_code::LIMIT,
        error.to_string(),
    )));
    let (ack, written) = oneshot::channel();
    if flush.send(ack).is_ok() {
        let _ = tokio::time::timeout(ANSWER_FLUSH, written).await;
    }
    tx.close();
}

/// Take reads at most one bounded line, even if the peer never sends a newline.
async fn request_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    cap: usize,
) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let n = reader
        .take((cap + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    if n > cap {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("request line exceeds {cap} bytes (read {n} bytes)"),
        ));
    }
    if n == 0 {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

async fn connection_closed(closed: &mut watch::Receiver<bool>) {
    if !*closed.borrow() {
        let _ = closed.changed().await;
    }
}

/// The longest a client's `shutdown` waits for its answer to be written
/// before the serving loops wake (theseus-ur0). A write takes microseconds;
/// the bound is for a client that stopped reading.
const ANSWER_FLUSH: std::time::Duration = std::time::Duration::from_secs(1);

/// Write one item, and then, if the queue has drained after dropping
/// notifications, the `events.lost` that says so (theseus-in3). False once
/// the connection is gone.
async fn write_one<W: AsyncWrite + Unpin>(
    core: &Core,
    writer: &mut W,
    rx: &mut Drain,
    item: &Item,
) -> bool {
    if !write_item(writer, item).await {
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

/// Write one queued item: a message, serialized here, or a notification's
/// line, serialized once for every connection (theseus-celu.36) and written
/// as it is. False once the connection is gone.
pub(crate) async fn write_item<W: AsyncWrite + Unpin>(writer: &mut W, item: &Item) -> bool {
    match item {
        Item::Message(m) => write_line(writer, m).await,
        Item::Line(line) => write_bytes(writer, line.as_bytes()).await,
    }
}

/// Write one message as an NDJSON line and flush it. False once the
/// connection is gone; a message that does not serialize is skipped.
async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, m: &Message) -> bool {
    #[cfg(test)]
    crate::outbound::counts::serialized();
    let Ok(mut s) = serde_json::to_string(m) else {
        return true;
    };
    s.push('\n');
    write_bytes(writer, s.as_bytes()).await
}

/// Write bytes and flush them. False once the connection is gone.
async fn write_bytes<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> bool {
    if writer.write_all(bytes).await.is_err() {
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

/// Params a method may leave out: none reads as `{}`, so its filters take
/// their defaults.
pub(super) fn or_empty(v: Value) -> Value {
    if v.is_null() {
        json!({})
    } else {
        v
    }
}

/// Parse a method's params, run it, and serialize its result.
impl Core {
    /// `hands.list` (step 40 part 2), as the server answers it.
    fn rpc_hands_list(
        &self,
        p: &theseus_protocol::HandsListParams,
    ) -> Result<theseus_protocol::HandsListResult, RpcFailure> {
        self.hands_list(p)
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))
    }
}

pub(super) fn route<P: DeserializeOwned, R: Serialize>(
    params: Value,
    method: impl FnOnce(P) -> Result<R, RpcFailure>,
) -> Result<Value, RpcFailure> {
    reply(method(parse(params)?)?)
}

/// A method's result, serialized.
pub(super) fn reply<R: Serialize>(r: R) -> Result<Value, RpcFailure> {
    Ok(serde_json::to_value(r).unwrap())
}

#[cfg(test)]
mod connection_limits {
    use super::*;

    #[tokio::test]
    async fn a_request_without_a_newline_cannot_grow_past_the_cap() {
        let cap = request_cap(0);
        let bytes = vec![b'x'; cap + 100];
        let mut reader = BufReader::new(bytes.as_slice());
        assert_eq!(
            request_line(&mut reader, cap).await.unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        let mut reader = BufReader::new(&b"one\ntwo\n"[..]);
        assert_eq!(
            request_line(&mut reader, cap).await.unwrap(),
            Some("one\n".into())
        );
        assert_eq!(
            request_line(&mut reader, cap).await.unwrap(),
            Some("two\n".into())
        );
        assert!(request_line(&mut reader, cap).await.unwrap().is_none());
    }

    #[test]
    fn request_admission_is_bounded_and_keeps_control_capacity() {
        let admission = Admission::new();
        let holds: Vec<_> = (0..REQUEST_CAP)
            .map(|_| admission.take(method::SESSION_WAIT).unwrap())
            .collect();
        assert!(admission.take(method::SESSION_WAIT).is_none());
        let controls: Vec<_> = (0..CONTROL_CAP)
            .map(|_| admission.take(method::WORK_ANSWER).unwrap())
            .collect();
        assert!(admission.take(method::SHUTDOWN).is_none());
        drop(controls);
        assert!(admission.take(method::SHUTDOWN).is_some());
        drop(holds);
        assert!(admission.take(method::SESSION_WAIT).is_some());
    }
}

#[cfg(test)]
mod attachment_frames {
    use super::*;

    #[tokio::test]
    async fn ten_wire_attachments_fit_one_request() {
        use base64::Engine as _;
        let size = 4 * 1024 * 1024;
        let data = base64::engine::general_purpose::STANDARD.encode(vec![0; size]);
        let files: Vec<_> = (0..10)
            .map(|_| {
                json!({
                    "name": "report.pdf", "media_type": "application/pdf",
                    "size": size, "data": data
                })
            })
            .collect();
        let mut line = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "turn.submit",
            "params": {"input": "read these", "attachments": files}
        }))
        .unwrap();
        line.push('\n');
        let received = request_line(
            &mut BufReader::new(line.as_bytes()),
            request_cap(size as u64),
        )
        .await
        .expect("ten wire attachments must fit")
        .unwrap();
        assert_eq!(received, line);
    }

    #[tokio::test]
    async fn a_configured_attachment_fits_its_request_line() {
        use base64::Engine as _;
        for size in [32 * 1024 * 1024, 40 * 1024 * 1024] {
            let data = base64::engine::general_purpose::STANDARD.encode(vec![0; size]);
            let mut line = serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "turn.submit",
                "params": {"input": "read this", "attachments": [{
                    "name": "report.pdf", "media_type": "application/pdf",
                    "size": size, "data": data
                }]}
            }))
            .unwrap();
            line.push('\n');
            let mut reader = BufReader::new(line.as_bytes());
            let received = request_line(&mut reader, request_cap(size as u64))
                .await
                .expect("configured attachment must fit")
                .unwrap();
            assert_eq!(received, line);
        }
    }

    #[tokio::test]
    async fn attachment_at_cap_is_kept_and_one_over_is_not() {
        use base64::Engine as _;
        let cap = 32 * 1024 * 1024;
        let dir = tempfile::tempdir().unwrap();
        let blobs = crate::blobs::Blobs::new(dir.path());
        for size in [cap, cap + 1] {
            let mut bytes = vec![0; size];
            bytes[..4].copy_from_slice(b"PK\x03\x04");
            let wire = theseus_protocol::Attachment {
                name: "report.zip".into(),
                media_type: "application/zip".into(),
                size: size as u64,
                data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
                text: None,
                not_read: None,
            };
            let mut line = serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "turn.submit",
                "params": {"input": "read", "attachments": [wire]}
            }))
            .unwrap();
            line.push('\n');
            let received = request_line(
                &mut BufReader::new(line.as_bytes()),
                request_cap(cap as u64),
            )
            .await
            .unwrap()
            .unwrap();
            let request: Request = serde_json::from_str(&received).unwrap();
            let params: theseus_protocol::TurnSubmitParams =
                serde_json::from_value(request.params).unwrap();
            let (files, _) = crate::attach::from_wire(
                params.attachments,
                crate::attach::Caps {
                    max_file: cap as u64,
                    max_text: 1024,
                    page_images: None,
                },
                &blobs,
            );
            let description = crate::attach::header(&files[0], None);
            assert_eq!(
                description.contains("over the"),
                size > cap,
                "{description}"
            );
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[tokio::test]
    async fn an_oversize_request_gets_a_limit_answer_before_eof() {
        let core = super::super::tests::test_core_with("unused", |cfg| {
            cfg.tools.max_attachment_bytes = 0;
        });
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (sr, sw) = tokio::io::split(server);
        let task = tokio::spawn(core.serve_connection(sr, sw, "frame-test".into()));
        let (cr, mut cw) = tokio::io::split(client);
        let cap = request_cap(0);
        cw.write_all(&vec![b'x'; cap + 1]).await.unwrap();
        let mut lines = BufReader::new(cr).lines();
        let line = lines
            .next_line()
            .await
            .unwrap()
            .expect("limit response, not silent EOF");
        let answer: Response = serde_json::from_str(&line).unwrap();
        let error = answer.error.unwrap();
        assert_eq!(error.code, error_code::LIMIT);
        assert!(error.message.contains(&cap.to_string()));
        assert!(lines.next_line().await.unwrap().is_none());
        task.await.unwrap().unwrap();
    }
}
