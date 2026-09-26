//! The protocol server (spec §3.18): JSON-RPC 2.0 over newline-delimited
//! JSON on any `AsyncRead + AsyncWrite` pair (stdio, a Unix socket). One
//! task per connection; notifications for a connection flow through its own
//! channel so a streaming turn never blocks another client.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::Value;
use theseus_protocol::{
    error_code, method, HandlerInfo, HealthResult, HookInfo, HooksListResult, HooksRegisterParams,
    HooksRegisterResult, Id, Message, Notification, Request, Response, SessionKind,
    SessionListResult, SessionOpenParams, TurnSubmitParams,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::advancer::StopAfterOneLoop;
use crate::hooks::{HookEvent, Hooks};
use crate::ledger::LedgerRow;
use crate::provider::Anthropic;
use crate::secrets::Secrets;
use crate::session::{SessionRecord, TurnLocks};
use crate::store::Store;
use crate::turn::{ToolchainManager, TurnRunner};
use crate::Config;

pub struct Core {
    pub cfg: Arc<Config>,
    pub hooks: Hooks,
    pub store: Store,
    pub runner: TurnRunner,
    pub secret_names: Vec<String>,
    started: Instant,
    turns: AtomicU64,
    pub shutdown: tokio::sync::Notify,
}

impl Core {
    pub fn new(cfg: Config, secrets: Secrets, store: Store) -> Result<Arc<Self>> {
        let key = secrets
            .get(&cfg.model.api_key_secret)
            .with_context(|| {
                format!(
                    "secret {} missing after resolution",
                    cfg.model.api_key_secret
                )
            })?
            .clone();
        let provider = Anthropic::new(&cfg.model.api_base, key)?;
        let cfg = Arc::new(cfg);
        let hooks = Hooks::new();
        let runner = TurnRunner {
            cfg: cfg.clone(),
            provider,
            hooks: hooks.clone(),
            store: store.clone(),
            locks: TurnLocks::default(),
            advancer: Arc::new(StopAfterOneLoop),
            toolchain: ToolchainManager,
        };
        let core = Arc::new(Self {
            cfg,
            hooks,
            store,
            runner,
            secret_names: secrets.names(),
            started: Instant::now(),
            turns: AtomicU64::new(0),
            shutdown: tokio::sync::Notify::new(),
        });
        let (_, visit) = core
            .hooks
            .dispatch(HookEvent::ServerStarted, None, None, Value::Null);
        core.store.append_ledger(&LedgerRow::new(
            "server.started",
            None,
            None,
            serde_json::to_value(visit)?,
        ))?;
        Ok(core)
    }

    pub fn health(&self) -> HealthResult {
        HealthResult {
            name: crate::NAME.into(),
            version: crate::VERSION.into(),
            protocol: theseus_protocol::VERSION.into(),
            uptime_secs: self.started.elapsed().as_secs(),
            sessions: self.store.session_count().unwrap_or(0),
            turns: self.turns.load(Ordering::Relaxed),
            model: self.cfg.model.model.clone(),
            secrets_resolved: self.secret_names.clone(),
        }
    }

    fn open_session(&self, p: SessionOpenParams) -> Result<SessionRecord> {
        let rec = SessionRecord::new(p.kind.unwrap_or(SessionKind::Conversation), p.label);
        self.store.put_session(&rec.session_id, &rec)?;
        let (_, visit) = self.hooks.dispatch(
            HookEvent::SessionOpened,
            None,
            Some(&rec.session_id),
            serde_json::to_value(rec.info())?,
        );
        self.store.append_ledger(&LedgerRow::new(
            "session.opened",
            Some(&rec.session_id),
            None,
            serde_json::to_value(visit)?,
        ))?;
        Ok(rec)
    }

    /// Serve one connection until EOF. `client` labels handlers this
    /// connection registers so they are dropped when it goes away.
    pub async fn serve_connection<R, W>(
        self: Arc<Self>,
        reader: R,
        mut writer: W,
        client: String,
    ) -> Result<()>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<Notification>();
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<Response>();

        // Single writer task: notifications and responses share the stream.
        let writer_task = tokio::spawn(async move {
            loop {
                let line = tokio::select! {
                    Some(n) = rx.recv() => serde_json::to_string(&n),
                    Some(r) = resp_rx.recv() => serde_json::to_string(&r),
                    else => break,
                };
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
                    let _ = resp_tx.send(Response::err(
                        Id::Num(0),
                        error_code::PARSE,
                        format!("parse error: {e}"),
                    ));
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
                        let resp = core.handle(req, tx, &client).await;
                        let _ = resp_tx.send(resp);
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
        let n = self.hooks.unregister_client(&client);
        if n > 0 {
            tracing::info!(client = %client, dropped = n, "unregistered handlers of departed client");
        }
        let _ = writer_task.await;
        Ok(())
    }

    async fn handle(
        self: Arc<Self>,
        req: Request,
        tx: mpsc::UnboundedSender<Notification>,
        client: &str,
    ) -> Response {
        let id = req.id.clone();
        match self.dispatch(req, tx, client).await {
            Ok(v) => Response::ok(id, v),
            Err(e) => {
                let (code, msg) = e;
                Response::err(id, code, msg)
            }
        }
    }

    async fn dispatch(
        self: Arc<Self>,
        req: Request,
        tx: mpsc::UnboundedSender<Notification>,
        client: &str,
    ) -> Result<Value, (i64, String)> {
        let bad = |e: anyhow::Error| (error_code::INTERNAL, e.to_string());
        let params = |v: Value| -> Result<_, (i64, String)> { Ok(v) };
        match req.method.as_str() {
            method::HEALTH => Ok(serde_json::to_value(self.health()).unwrap()),
            method::SESSION_OPEN => {
                let p: SessionOpenParams = parse(req.params)?;
                let rec = self.open_session(p).map_err(bad)?;
                Ok(serde_json::to_value(rec.info()).unwrap())
            }
            method::SESSION_LIST => {
                let recs: Vec<SessionRecord> = self.store.list_sessions().map_err(bad)?;
                Ok(serde_json::to_value(SessionListResult {
                    sessions: recs.iter().map(SessionRecord::info).collect(),
                })
                .unwrap())
            }
            method::TURN_SUBMIT => {
                let p: TurnSubmitParams = parse(req.params)?;
                if p.input.trim().is_empty() {
                    return Err((error_code::INVALID_PARAMS, "input is empty".into()));
                }
                let session = match &p.session_id {
                    Some(id) => self
                        .store
                        .get_session::<SessionRecord>(id)
                        .map_err(bad)?
                        .ok_or_else(|| (error_code::NOT_FOUND, format!("no session {id}")))?,
                    None => self
                        .open_session(SessionOpenParams::default())
                        .map_err(bad)?,
                };
                let result = self
                    .runner
                    .run(session, p.input, tx)
                    .await
                    .map_err(|e| (error_code::PROVIDER, format!("{e:#}")))?;
                self.turns.fetch_add(1, Ordering::Relaxed);
                Ok(serde_json::to_value(result).unwrap())
            }
            method::HOOKS_LIST => {
                let events = HookEvent::ALL
                    .iter()
                    .map(|e| HookInfo {
                        event: e.name().into(),
                        kind: e.kind().as_str().into(),
                        handlers: self.hooks.count(*e),
                    })
                    .collect();
                let handlers = self
                    .hooks
                    .handlers()
                    .into_iter()
                    .map(|h| HandlerInfo {
                        event: h.event.name().into(),
                        handler_id: h.handler_id,
                        client: h.client,
                    })
                    .collect();
                Ok(serde_json::to_value(HooksListResult { events, handlers }).unwrap())
            }
            method::HOOKS_REGISTER => {
                let p: HooksRegisterParams = parse(req.params)?;
                let event: HookEvent = p
                    .event
                    .parse()
                    .map_err(|e: String| (error_code::INVALID_PARAMS, e))?;
                let rec = self
                    .hooks
                    .register_remote(event, p.handler_id, client.to_string(), tx);
                self.store
                    .append_ledger(&LedgerRow::new(
                        "hooks.registered",
                        None,
                        None,
                        serde_json::to_value(&rec).unwrap_or(Value::Null),
                    ))
                    .map_err(bad)?;
                Ok(serde_json::to_value(HooksRegisterResult {
                    event: event.name().into(),
                    kind: event.kind().as_str().into(),
                    handler_id: rec.handler_id,
                })
                .unwrap())
            }
            method::HOOKS_UNREGISTER => {
                let p: HooksRegisterParams = parse(req.params)?;
                let event: HookEvent = p
                    .event
                    .parse()
                    .map_err(|e: String| (error_code::INVALID_PARAMS, e))?;
                let removed = self.hooks.unregister(event, &p.handler_id, client);
                Ok(serde_json::json!({"removed": removed}))
            }
            method::SHUTDOWN => {
                let _ = params(Value::Null);
                self.hooks
                    .dispatch(HookEvent::ServerStopping, None, None, Value::Null);
                let _ = self.store.append_ledger(&LedgerRow::new(
                    "server.stopping",
                    None,
                    None,
                    Value::Null,
                ));
                self.shutdown.notify_waiters();
                Ok(serde_json::json!({"ok": true}))
            }
            other => Err((
                error_code::METHOD_NOT_FOUND,
                format!("unknown method {other:?}"),
            )),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, (i64, String)> {
    serde_json::from_value(v)
        .map_err(|e| (error_code::INVALID_PARAMS, format!("invalid params: {e}")))
}
