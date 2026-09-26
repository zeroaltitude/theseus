//! The protocol server (spec §3.18): JSON-RPC 2.0 over newline-delimited
//! JSON on any `AsyncRead + AsyncWrite` pair (stdio, a Unix socket). One
//! task per connection; notifications for a connection flow through its own
//! channel so a streaming turn never blocks another client.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::Value;
use theseus_protocol::{
    error_code, method, notify, HandlerInfo, HealthResult, HookInfo, HooksListResult,
    HooksRegisterParams, HooksRegisterResult, Id, LedgerEntry, LedgerTailParams, LedgerTailResult,
    Message, ProfileChanged, ProfileInfo, ProfileListResult, ProfileUseParams, ProviderErrorData,
    Request, Response, SessionKind, SessionListResult, SessionOpenParams, TurnSubmitParams, Usage,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::advancer::StopAfterOneLoop;
use crate::hooks::{HookEvent, Hooks};
use crate::ledger::LedgerRow;
use crate::provider::{Anthropic, Provider};
use crate::secrets::Secrets;
use crate::session::{SessionRecord, TurnLocks};
use crate::store::Store;
use crate::turn::{ToolchainManager, TurnError, TurnRunner};
use crate::Config;

pub struct Core {
    pub cfg: Arc<Config>,
    pub hooks: Hooks,
    pub store: Store,
    pub runner: TurnRunner,
    pub secret_names: Vec<String>,
    pub telemetry: Arc<crate::telemetry::Telemetry>,
    started: Instant,
    turns: AtomicU64,
    provider_errors: AtomicU64,
    /// The live profile and where it came from ("config" | "runtime").
    live: std::sync::RwLock<(String, String)>,
    pub shutdown: tokio::sync::Notify,
}

const META_LIVE_PROFILE: &str = "live_profile";

impl Core {
    pub fn new(cfg: Config, secrets: Secrets, store: Store) -> Result<Arc<Self>> {
        let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
        for (name, pc) in cfg.all_providers() {
            let key = secrets
                .get(&pc.api_key_secret)
                .with_context(|| {
                    format!(
                        "secret {} for provider {name} missing after resolution",
                        pc.api_key_secret
                    )
                })?
                .clone();
            let timeouts = pc
                .timeouts
                .clone()
                .unwrap_or_else(|| cfg.model.timeouts.clone());
            providers.insert(name, Arc::new(Anthropic::new(&pc.api_base, key, timeouts)?));
        }
        let headers = cfg
            .telemetry
            .headers_secret
            .as_deref()
            .and_then(|n| secrets.get(n));
        let telemetry = crate::telemetry::Telemetry::from_config(&cfg.telemetry, headers)?;
        match &telemetry.endpoint {
            Some(e) => tracing::info!(endpoint = %e, "telemetry: OTLP/HTTP export on"),
            None => tracing::info!("telemetry: no otlp_endpoint configured; nothing is exported"),
        }
        Self::with_providers_and_telemetry(cfg, providers, store, secrets.names(), telemetry)
    }

    /// Build a core around one provider registered under the config's default
    /// provider name (tests use `FakeProvider`).
    pub fn with_provider(
        cfg: Config,
        provider: Arc<dyn Provider>,
        store: Store,
        secret_names: Vec<String>,
    ) -> Result<Arc<Self>> {
        let mut providers = BTreeMap::new();
        providers.insert(cfg.model.provider.clone(), provider);
        Self::with_providers(cfg, providers, store, secret_names)
    }

    pub fn with_providers(
        cfg: Config,
        providers: BTreeMap<String, Arc<dyn Provider>>,
        store: Store,
        secret_names: Vec<String>,
    ) -> Result<Arc<Self>> {
        Self::with_providers_and_telemetry(
            cfg,
            providers,
            store,
            secret_names,
            crate::telemetry::Telemetry::disabled(),
        )
    }

    pub fn with_providers_and_telemetry(
        cfg: Config,
        providers: BTreeMap<String, Arc<dyn Provider>>,
        store: Store,
        secret_names: Vec<String>,
        telemetry: crate::telemetry::Telemetry,
    ) -> Result<Arc<Self>> {
        let cfg = Arc::new(cfg);
        let hooks = Hooks::new();
        let runner = TurnRunner {
            cfg: cfg.clone(),
            providers,
            hooks: hooks.clone(),
            store: store.clone(),
            locks: TurnLocks::default(),
            advancer: Arc::new(StopAfterOneLoop),
            toolchain: ToolchainManager,
        };
        // A persisted runtime switch wins over config, if it still names a profile.
        let profiles = cfg.all_profiles();
        let live = match store.get_meta::<String>(META_LIVE_PROFILE)? {
            Some(name) if profiles.contains_key(&name) => (name, "runtime".to_string()),
            Some(stale) => {
                tracing::warn!(profile = %stale, "persisted live profile no longer configured; using config");
                (cfg.model.live.clone(), "config".to_string())
            }
            None => (cfg.model.live.clone(), "config".to_string()),
        };
        tracing::info!(profile = %live.0, source = %live.1, "live profile");
        let core = Arc::new(Self {
            cfg,
            hooks,
            store,
            runner,
            secret_names,
            telemetry: Arc::new(telemetry),
            started: Instant::now(),
            turns: AtomicU64::new(0),
            provider_errors: AtomicU64::new(0),
            live: std::sync::RwLock::new(live),
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

    pub fn live_profile(&self) -> (String, String) {
        self.live.read().unwrap().clone()
    }

    pub fn profile_list(&self) -> ProfileListResult {
        let (live, live_source) = self.live_profile();
        let profiles = self
            .cfg
            .all_profiles()
            .into_iter()
            .map(|(name, p)| ProfileInfo {
                live: name == live,
                name,
                provider: p.provider,
                model: p.model,
                max_output_tokens: p.max_output_tokens,
                has_system: p.system.is_some(),
            })
            .collect();
        ProfileListResult {
            live,
            live_source,
            profiles,
        }
    }

    /// Switch the live profile; persisted so it survives restart.
    pub fn profile_use(&self, name: &str, by: &str) -> Result<ProfileChanged> {
        if !self.cfg.all_profiles().contains_key(name) {
            anyhow::bail!(
                "unknown profile {name:?}; configured: {}",
                self.cfg
                    .all_profiles()
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.store.put_meta(META_LIVE_PROFILE, &name.to_string())?;
        let previous = {
            let mut g = self.live.write().unwrap();
            let prev = g.0.clone();
            *g = (name.to_string(), "runtime".into());
            prev
        };
        let changed = ProfileChanged {
            previous,
            live: name.to_string(),
            by: by.to_string(),
        };
        self.store.append_ledger(&LedgerRow::new(
            "profile.changed",
            None,
            None,
            serde_json::to_value(&changed)?,
        ))?;
        Ok(changed)
    }

    pub fn health(&self) -> HealthResult {
        let (profile, _) = self.live_profile();
        let prof = self.cfg.all_profiles().get(&profile).cloned();
        HealthResult {
            name: crate::NAME.into(),
            version: crate::VERSION.into(),
            protocol: theseus_protocol::VERSION.into(),
            uptime_secs: self.started.elapsed().as_secs(),
            sessions: self.store.session_count().unwrap_or(0),
            turns: self.turns.load(Ordering::Relaxed),
            model: prof.as_ref().map(|p| p.model.clone()).unwrap_or_default(),
            profile,
            provider: prof.map(|p| p.provider).unwrap_or_default(),
            providers: self.runner.providers.keys().cloned().collect(),
            secrets_resolved: self.secret_names.clone(),
            usage_total: self.usage_total(),
            provider_errors: self.provider_errors.load(Ordering::Relaxed),
            ledger_rows: self.store.ledger_len().unwrap_or(0),
            telemetry: theseus_protocol::TelemetryStatus {
                enabled: self.telemetry.enabled(),
                otlp_endpoint: self.telemetry.endpoint.clone(),
            },
        }
    }

    fn usage_total(&self) -> Usage {
        let mut total = Usage::default();
        if let Ok(recs) = self.store.list_sessions::<SessionRecord>() {
            for r in recs {
                crate::turn::add_usage(&mut total, &r.usage);
            }
        }
        total
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
                        let resp = core.handle(req, tx, &client).await;
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
        tx: mpsc::UnboundedSender<Message>,
        client: &str,
    ) -> Response {
        let id = req.id.clone();
        match self.dispatch(req, tx, client).await {
            Ok(v) => Response::ok(id, v),
            Err(f) => Response::err_with(id, f.code, f.message, f.data),
        }
    }

    async fn dispatch(
        self: Arc<Self>,
        req: Request,
        tx: mpsc::UnboundedSender<Message>,
        client: &str,
    ) -> Result<Value, RpcFailure> {
        let bad = |e: anyhow::Error| RpcFailure::new(error_code::INTERNAL, e.to_string());
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
                    return Err(RpcFailure::new(
                        error_code::INVALID_PARAMS,
                        "input is empty",
                    ));
                }
                let session = match &p.session_id {
                    Some(id) => self
                        .store
                        .get_session::<SessionRecord>(id)
                        .map_err(bad)?
                        .ok_or_else(|| {
                            RpcFailure::new(error_code::NOT_FOUND, format!("no session {id}"))
                        })?,
                    None => self
                        .open_session(SessionOpenParams::default())
                        .map_err(bad)?,
                };
                let (live, _) = self.live_profile();
                let target = self
                    .runner
                    .resolve_target(
                        &live,
                        p.profile.as_deref(),
                        p.provider.as_deref(),
                        p.model.as_deref(),
                    )
                    .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e.to_string()))?;
                let (t_profile, t_provider, t_model) = (
                    target.profile.clone(),
                    target.provider.clone(),
                    target.model.clone(),
                );
                let result = match self.runner.run(session, p.input, target, tx).await {
                    Ok(r) => {
                        self.telemetry.record_turn(&r);
                        r
                    }
                    Err(e) => {
                        self.turns.fetch_add(1, Ordering::Relaxed);
                        return Err(match e.downcast::<TurnError>() {
                            Ok(te) => {
                                self.provider_errors.fetch_add(1, Ordering::Relaxed);
                                self.telemetry
                                    .record_failure(&crate::telemetry::FailedTurn {
                                        profile: &t_profile,
                                        provider: &t_provider,
                                        model: &t_model,
                                        class: &te.class,
                                        transient: te.transient,
                                        elapsed_ms: te.elapsed_ms,
                                        trace: te.trace.as_ref(),
                                    });
                                let data = serde_json::to_value(ProviderErrorData {
                                    class: te.class.clone(),
                                    transient: te.transient,
                                    usage_unknown: te.usage_unknown,
                                    turn_id: Some(te.turn_id.clone()),
                                    session_id: te.session_id.clone(),
                                    elapsed_ms: te.elapsed_ms,
                                    trace: te.trace.clone(),
                                })
                                .unwrap_or(Value::Null);
                                RpcFailure {
                                    code: error_code::PROVIDER,
                                    message: format!("{:#}", te.source),
                                    data,
                                }
                            }
                            Err(other) => RpcFailure {
                                code: error_code::INTERNAL,
                                message: format!("{other:#}"),
                                data: Value::Null,
                            },
                        });
                    }
                };
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
                    .map_err(|e: String| RpcFailure::new(error_code::INVALID_PARAMS, e))?;
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
                    .map_err(|e: String| RpcFailure::new(error_code::INVALID_PARAMS, e))?;
                let removed = self.hooks.unregister(event, &p.handler_id, client);
                Ok(serde_json::json!({"removed": removed}))
            }
            method::PROFILE_LIST => Ok(serde_json::to_value(self.profile_list()).unwrap()),
            method::PROFILE_USE => {
                let p: ProfileUseParams = parse(req.params)?;
                let changed = self
                    .profile_use(&p.name, client)
                    .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e.to_string()))?;
                // Tell this client; other clients learn on their next health/profile.list.
                let _ = tx.send(Message::Notification(theseus_protocol::Notification::new(
                    notify::PROFILE_CHANGED,
                    &changed,
                )));
                Ok(serde_json::to_value(changed).unwrap())
            }
            method::LEDGER_TAIL => {
                let p: LedgerTailParams = parse(req.params)?;
                let n = p.n.unwrap_or(20).min(1000);
                let scan = if p.kind.is_some() || p.session_id.is_some() {
                    n * 50
                } else {
                    n
                };
                let rows: Vec<(u64, LedgerRow)> = self.store.ledger_tail(scan).map_err(bad)?;
                let rows: Vec<LedgerEntry> = rows
                    .into_iter()
                    .filter(|(_, r)| p.kind.as_deref().is_none_or(|k| r.kind == k))
                    .filter(|(_, r)| {
                        p.session_id
                            .as_deref()
                            .is_none_or(|s| r.session_id.as_deref() == Some(s))
                    })
                    .map(|(position, r)| LedgerEntry {
                        position,
                        at_unix_ms: r.at_unix_ms,
                        kind: r.kind,
                        session_id: r.session_id,
                        turn_id: r.turn_id,
                        data: r.data,
                    })
                    .collect();
                let rows = rows[rows.len().saturating_sub(n)..].to_vec();
                Ok(serde_json::to_value(LedgerTailResult {
                    rows,
                    total: self.store.ledger_len().map_err(bad)?,
                })
                .unwrap())
            }
            method::SHUTDOWN => {
                self.hooks
                    .dispatch(HookEvent::ServerStopping, None, None, Value::Null);
                let _ = self.store.append_ledger(&LedgerRow::new(
                    "server.stopping",
                    None,
                    None,
                    Value::Null,
                ));
                self.telemetry.flush();
                let _ = self.store.checkpoint();
                self.shutdown.notify_waiters();
                Ok(serde_json::json!({"ok": true}))
            }
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
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: Value::Null,
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, RpcFailure> {
    serde_json::from_value(v)
        .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, format!("invalid params: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FakeProvider;
    use theseus_protocol::{notify, Notification, TurnSubmitResult};
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn test_core(reply: &str) -> Arc<Core> {
        let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
        let store = Store::open(&dir.join("store"), theseus_store::Engine::Redb).unwrap();
        let mut cfg = Config::example();
        cfg.server.state_dir = dir.to_string_lossy().into_owned();
        Core::with_provider(
            cfg,
            Arc::new(FakeProvider {
                reply: reply.into(),
                ..Default::default()
            }),
            store,
            vec!["anthropic_api_key".into()],
        )
        .unwrap()
    }

    /// Drive a connection over an in-memory duplex: returns (lines received) after `n_requests` responses.
    async fn roundtrip(core: Arc<Core>, requests: Vec<Request>) -> Vec<Message> {
        let (client, server) = duplex(64 * 1024);
        let (sr, sw) = tokio::io::split(server);
        let srv = tokio::spawn(core.serve_connection(sr, sw, "test".into()));
        let (cr, mut cw) = tokio::io::split(client);
        let want = requests.len();
        for r in requests {
            let mut line = serde_json::to_string(&r).unwrap();
            line.push('\n');
            cw.write_all(line.as_bytes()).await.unwrap();
        }
        let mut lines = BufReader::new(cr).lines();
        let mut got = Vec::new();
        let mut responses = 0;
        while responses < want {
            let line = lines.next_line().await.unwrap().unwrap();
            let m: Message = serde_json::from_str(&line).unwrap();
            if matches!(m, Message::Response(_)) {
                responses += 1;
            }
            got.push(m);
        }
        cw.shutdown().await.unwrap();
        drop(cw);
        drop(lines);
        let _ = srv.await;
        got
    }

    fn responses(msgs: &[Message]) -> Vec<&Response> {
        msgs.iter()
            .filter_map(|m| match m {
                Message::Response(r) => Some(r),
                _ => None,
            })
            .collect()
    }
    fn notifications(msgs: &[Message]) -> Vec<&Notification> {
        msgs.iter()
            .filter_map(|m| match m {
                Message::Notification(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn health_and_unknown_method() {
        let core = test_core("x");
        let msgs = roundtrip(
            core,
            vec![
                Request::new(Id::Num(1), method::HEALTH, Value::Null),
                Request::new(Id::Num(2), "nope.nothing", Value::Null),
            ],
        )
        .await;
        let rs = responses(&msgs);
        let health = rs.iter().find(|r| r.id == Id::Num(1)).unwrap();
        assert_eq!(health.result.as_ref().unwrap()["name"], "theseus");
        let nope = rs.iter().find(|r| r.id == Id::Num(2)).unwrap();
        assert_eq!(
            nope.error.as_ref().unwrap().code,
            error_code::METHOD_NOT_FOUND
        );
    }

    #[tokio::test]
    async fn one_turn_is_one_loop_with_streamed_deltas() {
        let core = test_core("hello there friend");
        let msgs = roundtrip(
            core.clone(),
            vec![Request::new(
                Id::Num(7),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: None,
                    input: "hi".into(),
                    profile: None,
                    provider: None,
                    model: None,
                },
            )],
        )
        .await;
        let ns: Vec<&str> = notifications(&msgs)
            .iter()
            .map(|n| n.method.as_str())
            .collect();
        assert_eq!(ns.first(), Some(&notify::TURN_STARTED));
        assert_eq!(ns.get(1), Some(&notify::LOOP_STARTED));
        assert!(ns.iter().filter(|m| **m == notify::MODEL_DELTA).count() >= 2);
        assert_eq!(ns[ns.len() - 2], notify::LOOP_ENDED);
        assert_eq!(ns[ns.len() - 1], notify::TURN_ENDED);
        let streamed: String = notifications(&msgs)
            .iter()
            .filter(|n| n.method == notify::MODEL_DELTA)
            .map(|n| n.params["text"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(streamed, "hello there friend");

        let r = responses(&msgs)[0];
        let result: TurnSubmitResult = serde_json::from_value(r.result.clone().unwrap()).unwrap();
        assert_eq!(result.loops, 1);
        assert_eq!(result.stop_reason, "stop_after_one_loop");
        assert_eq!(result.output, "hello there friend");
        assert_eq!(result.provider_stop_reason.as_deref(), Some("end_turn"));

        // Every hook site on the turn path was visited with zero handlers, and ledgered.
        let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(200).unwrap();
        let sites: Vec<String> = rows
            .iter()
            .filter(|(_, r)| r.kind == "hook.site" && r.turn_id.as_deref() == Some(&result.turn_id))
            .map(|(_, r)| r.data["event"].as_str().unwrap().to_string())
            .collect();
        for expected in [
            "turn.starting",
            "input.received",
            "context.built",
            "model.pre_call",
            "model.post_call",
            "advancer.decided",
            "loop.ended",
            "message.sending",
            "reply.claim",
            "turn.ended",
        ] {
            assert!(
                sites.contains(&expected.to_string()),
                "missing site {expected}: {sites:?}"
            );
        }
        assert!(rows
            .iter()
            .filter(|(_, r)| r.kind == "hook.site")
            .all(|(_, r)| r.data["handlers"] == 0));
        // The trace: turn > loop 0 > provider.call > first_token, with hooks as children.
        let tr = result.trace.as_ref().expect("trace");
        assert_eq!(tr.name, "turn");
        assert!(tr.end_us.is_some());
        let names: Vec<&str> = tr.children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"lock.wait"));
        assert!(names.contains(&"turn.starting"));
        assert!(names.contains(&"loop 0"));
        assert!(names.contains(&"session.write"));
        assert!(names.contains(&"turn.ended"));
        let lp = tr.children.iter().find(|c| c.name == "loop 0").unwrap();
        let lnames: Vec<&str> = lp.children.iter().map(|c| c.name.as_str()).collect();
        assert!(lnames.contains(&"compile"));
        assert!(lnames.contains(&"model.pre_call"));
        assert!(lnames.contains(&"provider.call"));
        assert!(lnames.contains(&"advancer"));
        let pc = lp
            .children
            .iter()
            .find(|c| c.name == "provider.call")
            .unwrap();
        assert!(pc.children.iter().any(|m| m.name == "first_token"));
        assert_eq!(pc.attrs["request_id"], "req_fake");
        assert!(rows.iter().any(|(_, r)| r.kind == "turn.trace"));
        assert_eq!(core.health().turns, 1);
        assert_eq!(core.health().sessions, 1);
    }

    #[tokio::test]
    async fn rejects_empty_input_and_unknown_session() {
        let core = test_core("x");
        let msgs = roundtrip(
            core,
            vec![
                Request::new(
                    Id::Num(1),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: None,
                        input: "   ".into(),
                        profile: None,
                        provider: None,
                        model: None,
                    },
                ),
                Request::new(
                    Id::Num(2),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: Some("ses_nope".into()),
                        input: "hi".into(),
                        profile: None,
                        provider: None,
                        model: None,
                    },
                ),
            ],
        )
        .await;
        let rs = responses(&msgs);
        let e1 = rs
            .iter()
            .find(|r| r.id == Id::Num(1))
            .unwrap()
            .error
            .as_ref()
            .unwrap();
        assert_eq!(e1.code, error_code::INVALID_PARAMS);
        let e2 = rs
            .iter()
            .find(|r| r.id == Id::Num(2))
            .unwrap()
            .error
            .as_ref()
            .unwrap();
        assert_eq!(e2.code, error_code::NOT_FOUND);
    }

    #[tokio::test]
    async fn remote_hook_observer_sees_turn_events() {
        let core = test_core("ok");
        let msgs = roundtrip(
            core,
            vec![
                Request::new(
                    Id::Num(1),
                    method::HOOKS_REGISTER,
                    HooksRegisterParams {
                        event: "turn.ended".into(),
                        handler_id: "obs".into(),
                    },
                ),
                Request::new(
                    Id::Num(2),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: None,
                        input: "hi".into(),
                        profile: None,
                        provider: None,
                        model: None,
                    },
                ),
                Request::new(Id::Num(3), method::HOOKS_LIST, Value::Null),
            ],
        )
        .await;
        let hook_events: Vec<&Notification> = notifications(&msgs)
            .into_iter()
            .filter(|n| n.method == notify::HOOK_EVENT)
            .collect();
        assert_eq!(hook_events.len(), 1);
        assert_eq!(hook_events[0].params["event"], "turn.ended");
        assert_eq!(hook_events[0].params["handler_id"], "obs");
        let list = responses(&msgs)
            .iter()
            .find(|r| r.id == Id::Num(3))
            .unwrap()
            .result
            .clone()
            .unwrap();
        let l: HooksListResult = serde_json::from_value(list).unwrap();
        assert_eq!(l.events.len(), HookEvent::ALL.len());
        assert_eq!(l.handlers.len(), 1);
        assert_eq!(l.handlers[0].client, "test");
    }

    #[tokio::test]
    async fn same_session_serializes_turns() {
        let core = test_core("r");
        let open = roundtrip(
            core.clone(),
            vec![Request::new(
                Id::Num(1),
                method::SESSION_OPEN,
                SessionOpenParams::default(),
            )],
        )
        .await;
        let sid = responses(&open)[0].result.as_ref().unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let reqs = (0..3)
            .map(|i| {
                Request::new(
                    Id::Num(10 + i),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: Some(sid.clone()),
                        input: format!("turn {i}"),
                        profile: None,
                        provider: None,
                        model: None,
                    },
                )
            })
            .collect();
        let msgs = roundtrip(core.clone(), reqs).await;
        assert_eq!(responses(&msgs).len(), 3);
        let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
        assert_eq!(rec.turns, 3);
        // turn.started/turn.ended never interleave: each started is followed by its own ended.
        let seq: Vec<&str> = notifications(&msgs)
            .iter()
            .filter(|n| n.method == notify::TURN_STARTED || n.method == notify::TURN_ENDED)
            .map(|n| n.method.as_str())
            .collect();
        assert_eq!(seq, [notify::TURN_STARTED, notify::TURN_ENDED].repeat(3));
    }

    #[tokio::test]
    async fn provider_failure_is_classified_and_ledgered() {
        let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
        let store = Store::open(&dir.join("store"), theseus_store::Engine::Redb).unwrap();
        let core = Core::with_provider(
            Config::example(),
            Arc::new(FakeProvider {
                fail_with: Some(crate::provider::ProviderError::Timeout {
                    phase: crate::provider::TimeoutPhase::StreamIdle,
                    elapsed_ms: 61_000,
                }),
                ..Default::default()
            }),
            store,
            vec![],
        )
        .unwrap();
        let msgs = roundtrip(
            core.clone(),
            vec![Request::new(
                Id::Num(1),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: None,
                    input: "hi".into(),
                    profile: None,
                    provider: None,
                    model: None,
                },
            )],
        )
        .await;
        let r = responses(&msgs)[0];
        let e = r.error.as_ref().expect("error response");
        assert_eq!(e.code, error_code::PROVIDER);
        assert_eq!(e.data["class"], "timeout");
        assert_eq!(e.data["transient"], true);
        assert_eq!(e.data["usage_unknown"], true);
        assert!(e.message.contains("stream_idle") || e.message.to_lowercase().contains("timeout"));
        let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(50).unwrap();
        assert!(rows
            .iter()
            .any(|(_, r)| r.kind == "provider.error" && r.data["class"] == "timeout"));
        assert!(rows.iter().any(|(_, r)| r.kind == "turn.failed"));
        let tr = &e.data["trace"];
        assert_eq!(tr["name"], "turn");
        assert_eq!(tr["attrs"]["outcome"], "failed");
        assert_eq!(core.health().provider_errors, 1);
        // The turn still counted and the session record was written.
        assert_eq!(core.health().turns, 1);
    }

    #[tokio::test]
    async fn usage_accumulates_per_session_and_globally() {
        let core = test_core("one two three");
        let open = roundtrip(
            core.clone(),
            vec![Request::new(
                Id::Num(1),
                method::SESSION_OPEN,
                SessionOpenParams::default(),
            )],
        )
        .await;
        let sid = responses(&open)[0].result.as_ref().unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let reqs = (0..2)
            .map(|i| {
                Request::new(
                    Id::Num(10 + i),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: Some(sid.clone()),
                        input: "a b".into(),
                        profile: None,
                        provider: None,
                        model: None,
                    },
                )
            })
            .collect();
        let msgs = roundtrip(core.clone(), reqs).await;
        let r: TurnSubmitResult =
            serde_json::from_value(responses(&msgs)[0].result.clone().unwrap()).unwrap();
        assert_eq!(r.usage.input_tokens, 2);
        assert_eq!(r.usage.output_tokens, 3);
        let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
        assert_eq!(rec.usage.input_tokens, 4);
        assert_eq!(rec.usage.output_tokens, 6);
        let h = core.health();
        assert_eq!(h.usage_total.output_tokens, 6);
        let tail = roundtrip(
            core.clone(),
            vec![Request::new(
                Id::Num(99),
                method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(5),
                    kind: Some("provider.call".into()),
                    session_id: None,
                },
            )],
        )
        .await;
        let t: LedgerTailResult =
            serde_json::from_value(responses(&tail)[0].result.clone().unwrap()).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert!(t.rows.iter().all(|r| r.kind == "provider.call"));
    }

    #[tokio::test]
    async fn per_turn_provider_and_model_selection() {
        let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
        let store = Store::open(&dir.join("store"), theseus_store::Engine::Redb).unwrap();
        let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
        providers.insert(
            "anthropic".into(),
            Arc::new(FakeProvider {
                reply: "from anthropic".into(),
                ..Default::default()
            }),
        );
        providers.insert(
            "zai".into(),
            Arc::new(FakeProvider {
                reply: "from zai".into(),
                ..Default::default()
            }),
        );
        let core = Core::with_providers(Config::example(), providers, store, vec![]).unwrap();
        let msgs = roundtrip(
            core.clone(),
            vec![
                Request::new(
                    Id::Num(1),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: None,
                        input: "hi".into(),
                        profile: None,
                        provider: None,
                        model: None,
                    },
                ),
                Request::new(
                    Id::Num(2),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: None,
                        input: "hi".into(),
                        profile: None,
                        provider: Some("zai".into()),
                        model: Some("glm-5.3-flash".into()),
                    },
                ),
                Request::new(
                    Id::Num(3),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: None,
                        input: "hi".into(),
                        profile: None,
                        provider: Some("nope".into()),
                        model: None,
                    },
                ),
            ],
        )
        .await;
        let rs = responses(&msgs);
        let r1: TurnSubmitResult = serde_json::from_value(
            rs.iter()
                .find(|r| r.id == Id::Num(1))
                .unwrap()
                .result
                .clone()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(r1.provider, "anthropic");
        assert_eq!(r1.output, "from anthropic");
        assert_eq!(r1.model, "claude-sonnet-5");
        let r2: TurnSubmitResult = serde_json::from_value(
            rs.iter()
                .find(|r| r.id == Id::Num(2))
                .unwrap()
                .result
                .clone()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(r2.provider, "zai");
        assert_eq!(r2.output, "from zai");
        assert_eq!(r2.model, "glm-5.3-flash");
        let e3 = rs
            .iter()
            .find(|r| r.id == Id::Num(3))
            .unwrap()
            .error
            .as_ref()
            .unwrap();
        assert_eq!(e3.code, error_code::INVALID_PARAMS);
        assert!(core.health().providers.contains(&"zai".to_string()));
    }

    #[tokio::test]
    async fn live_profile_switch_persists_and_routes() {
        let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
        let store = Store::open(&dir.join("store"), theseus_store::Engine::Redb).unwrap();
        let mk = |store: Store| {
            let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
            providers.insert(
                "anthropic".into(),
                Arc::new(FakeProvider {
                    reply: "from anthropic".into(),
                    ..Default::default()
                }),
            );
            providers.insert(
                "zai".into(),
                Arc::new(FakeProvider {
                    reply: "from zai".into(),
                    ..Default::default()
                }),
            );
            Core::with_providers(Config::example(), providers, store, vec![]).unwrap()
        };
        let core = mk(store.clone());
        assert_eq!(
            core.live_profile(),
            ("sonnet".to_string(), "config".to_string())
        );
        let ask = |id: u64, profile: Option<&str>| {
            Request::new(
                Id::Num(id),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id: None,
                    input: "hi".into(),
                    profile: profile.map(str::to_string),
                    provider: None,
                    model: None,
                },
            )
        };
        let msgs = roundtrip(
            core.clone(),
            vec![
                ask(1, None),
                Request::new(
                    Id::Num(2),
                    method::PROFILE_USE,
                    ProfileUseParams { name: "glm".into() },
                ),
                ask(3, None),
                ask(4, Some("sonnet")),
                Request::new(
                    Id::Num(5),
                    method::PROFILE_USE,
                    ProfileUseParams {
                        name: "nope".into(),
                    },
                ),
                Request::new(Id::Num(6), method::PROFILE_LIST, Value::Null),
            ],
        )
        .await;
        let rs = responses(&msgs);
        let get = |id: u64| -> TurnSubmitResult {
            serde_json::from_value(
                rs.iter()
                    .find(|r| r.id == Id::Num(id))
                    .unwrap()
                    .result
                    .clone()
                    .unwrap(),
            )
            .unwrap()
        };
        assert_eq!(get(1).profile, "sonnet");
        assert_eq!(get(1).output, "from anthropic");
        assert_eq!(get(3).profile, "glm");
        assert_eq!(get(3).provider, "zai");
        assert_eq!(get(3).model, "glm-5.3-flash");
        assert_eq!(get(3).output, "from zai");
        assert_eq!(get(4).profile, "sonnet", "explicit profile beats live");
        let bad = rs.iter().find(|r| r.id == Id::Num(5)).unwrap();
        assert_eq!(bad.error.as_ref().unwrap().code, error_code::INVALID_PARAMS);
        let list: ProfileListResult = serde_json::from_value(
            rs.iter()
                .find(|r| r.id == Id::Num(6))
                .unwrap()
                .result
                .clone()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(list.live, "glm");
        assert_eq!(list.live_source, "runtime");
        assert!(msgs
            .iter()
            .any(|m| matches!(m, Message::Notification(n) if n.method == notify::PROFILE_CHANGED)));

        // A fresh core over the same store comes up with the switched profile.
        drop(core);
        let core2 = mk(store);
        assert_eq!(
            core2.live_profile(),
            ("glm".to_string(), "runtime".to_string())
        );
        assert_eq!(core2.health().profile, "glm");
        assert_eq!(core2.health().model, "glm-5.3-flash");
    }

    #[tokio::test]
    async fn parse_error_gets_a_response() {
        let core = test_core("x");
        let (client, server) = duplex(4096);
        let (sr, sw) = tokio::io::split(server);
        let srv = tokio::spawn(core.serve_connection(sr, sw, "test".into()));
        let (cr, mut cw) = tokio::io::split(client);
        cw.write_all(b"this is not json\n").await.unwrap();
        let mut lines = BufReader::new(cr).lines();
        let line = lines.next_line().await.unwrap().unwrap();
        let m: Message = serde_json::from_str(&line).unwrap();
        match m {
            Message::Response(r) => assert_eq!(r.error.unwrap().code, error_code::PARSE),
            other => panic!("expected response, got {other:?}"),
        }
        cw.shutdown().await.unwrap();
        drop(cw);
        drop(lines);
        let _ = srv.await;
    }
}
