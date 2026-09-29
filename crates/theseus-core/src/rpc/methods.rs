//! The protocol's methods, one named function each (spec §3.18). `server`
//! routes a request to its method by name; each takes its typed params and
//! returns its typed result, and `?` on an internal error is `INTERNAL`.

use std::sync::atomic::Ordering;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::{
    error_code, notify, HealthResult, LedgerEntry, LedgerTailParams, LedgerTailResult, Message,
    ProfileChanged, ProfileInfo, ProfileListResult, ProfileUseParams, ProviderErrorData,
    SessionKind, SessionOpenParams, TurnSubmitParams, Usage,
};

use super::confirms::waiting_by_execution;
use super::server::{Conn, RpcFailure};
use super::{Core, META_LIVE_PROFILE};
use crate::approval::Refusal;
use crate::bus::EventSink;
use crate::compiler::Recompile;
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::session::SessionRecord;
use crate::turn::{TurnError, TurnRequest, OPERATOR};
use theseus_kernel::Authority;

impl Core {
    pub fn health(&self) -> HealthResult {
        let (profile, _) = self.live_profile();
        let prof = self.cfg.all_profiles().get(&profile).cloned();
        // The totals come from one read of every session record.
        let sessions = self
            .store
            .list_sessions::<SessionRecord>()
            .unwrap_or_default();
        let mut usage_total = Usage::default();
        for s in &sessions {
            crate::turn::add_usage(&mut usage_total, &s.usage);
        }
        HealthResult {
            name: crate::NAME.into(),
            version: crate::VERSION.into(),
            protocol: theseus_protocol::VERSION.into(),
            uptime_secs: self.started.elapsed().as_secs(),
            sessions: self.store.session_count().unwrap_or(0),
            turns: sessions.iter().map(|s| s.turns).sum(),
            model: prof.as_ref().map(|p| p.model.clone()).unwrap_or_default(),
            profile,
            provider: prof.map(|p| p.provider).unwrap_or_default(),
            providers: self.runner.providers.keys().cloned().collect(),
            secrets_resolved: self.secret_names.clone(),
            usage_total,
            provider_errors: self.provider_errors.load(Ordering::Relaxed),
            ledger_rows: self.store.ledger_len().unwrap_or(0),
            telemetry: theseus_protocol::TelemetryStatus {
                enabled: self.telemetry.enabled(),
                otlp_endpoint: self.telemetry.endpoint.clone(),
            },
            kernel: self.kernel_status(),
            cost_usd_total: sessions.iter().map(|s| s.cost_usd).sum(),
            catalog_version: self.catalog.version.clone(),
            bindings: self.bindings.all(),
            narrative: self.narrator.on(),
            approval: self.approval_status(),
            tightenings: self.tools.tightened.all(),
        }
    }

    /// `[approval]` for health: each listed channel's state, judged with the
    /// Discord binding's latest checks and state.
    pub fn approval_status(&self) -> theseus_protocol::ApprovalStatus {
        let discord = self
            .bindings
            .all()
            .into_iter()
            .find(|b| b.kind == "discord");
        self.approval.status(
            self.cfg.web.enabled,
            discord.as_ref().map(|b| b.state.as_str()),
        )
    }

    pub(super) fn session_open(
        &self,
        p: SessionOpenParams,
    ) -> Result<theseus_protocol::SessionInfo, RpcFailure> {
        let rec = self.open_session(p)?;
        let mut info = rec.info();
        info.execution_state = Some("waiting".into());
        Ok(info)
    }

    pub(super) fn open_session(&self, p: SessionOpenParams) -> Result<SessionRecord> {
        let mut rec = SessionRecord::new(p.kind.unwrap_or(SessionKind::Conversation), p.label);
        let exec = self.kernel.open_execution(
            &rec.session_id,
            rec.kind,
            Authority {
                principal: OPERATOR.to_string(),
                ..Default::default()
            },
            None,
            None,
        )?;
        if self.narrator.on() {
            self.narrator.first_sight(&rec.session_id);
            narrate!(
                self.narrator,
                Session,
                Some(&rec.session_id),
                None,
                "Session {} opened ({}); its execution {} has a spend limit of {}.",
                crate::narrative::short(&rec.session_id),
                rec.kind.as_str(),
                crate::narrative::short(&exec.id),
                crate::narrative::dollars(exec.budget.limit_micros)
            );
        }
        rec.execution_id = Some(exec.id);
        self.store.put_session(&rec.session_id, &rec)?;
        self.store.append_ledger(&LedgerRow::new(
            "session.opened",
            Some(&rec.session_id),
            None,
            json!({"execution_id": rec.execution_id}),
        ))?;
        Ok(rec)
    }

    /// Every session record, the most recently active first.
    pub(super) fn sessions_by_activity(&self) -> Result<Vec<SessionRecord>> {
        let mut recs: Vec<SessionRecord> = self.store.list_sessions()?;
        recs.sort_by(|a, b| {
            b.last_active_ms
                .max(b.created_at_unix_ms)
                .cmp(&a.last_active_ms.max(a.created_at_unix_ms))
        });
        Ok(recs)
    }

    /// Every session, the most recently active first.
    pub fn session_list(&self) -> Result<Vec<theseus_protocol::SessionInfo>> {
        let waiting = waiting_by_execution(&self.kernel.pending_confirms().unwrap_or_default());
        Ok(self
            .sessions_by_activity()?
            .iter()
            .map(|r| self.session_info(r, &waiting))
            .collect())
    }

    fn session_info(
        &self,
        r: &SessionRecord,
        waiting: &std::collections::BTreeMap<String, u32>,
    ) -> theseus_protocol::SessionInfo {
        let mut i = r.info();
        i.execution_state = r
            .execution_id
            .as_deref()
            .and_then(|id| self.kernel.execution(id).ok().flatten())
            .map(|e| e.state.as_str().to_string());
        if let Some(exec) = r.execution_id.as_deref() {
            i.pending_confirms = waiting.get(exec).copied().unwrap_or(0);
        }
        i
    }

    /// Run one turn for a client, streaming its events to that connection.
    pub(super) async fn turn_submit(
        &self,
        p: TurnSubmitParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::TurnSubmitResult, RpcFailure> {
        if p.input.trim().is_empty() && p.attachments.is_empty() {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "input is empty",
            ));
        }
        let session = match &p.session_id {
            Some(id) => self.session(id)?,
            None => self.open_session(SessionOpenParams::default())?,
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
            .map_err(RpcFailure::invalid)?;
        let (t_profile, t_provider, t_model) = (
            target.profile.clone(),
            target.provider.clone(),
            target.model.clone(),
        );
        let sink = EventSink::new(
            self.bus.clone(),
            &session.session_id,
            Some((conn.client.to_string(), conn.tx.clone())),
        );
        let result = self
            .runner
            .run(TurnRequest {
                session,
                input: Some(p.input),
                target,
                sink,
                author: p.author.clone().unwrap_or_else(|| conn.client.to_string()),
                recompile: None,
                attachments: p.attachments,
            })
            .await;
        match result {
            Ok(r) => {
                self.telemetry.record_turn(&r);
                Ok(r)
            }
            Err(e) => Err(match e.downcast::<TurnError>() {
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
                        usage: te.usage.clone(),
                        cost_usd: te.cost_usd,
                        tool_calls: te.tool_calls,
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
            }),
        }
    }

    /// A session's record, or `NOT_FOUND`.
    fn session(&self, id: &str) -> Result<SessionRecord, RpcFailure> {
        self.store
            .get_session::<SessionRecord>(id)?
            .ok_or_else(|| RpcFailure::new(error_code::NOT_FOUND, format!("no session {id}")))
    }

    pub fn profile_list(&self) -> ProfileListResult {
        let (live, live_source) = self.live_profile();
        let profiles = self
            .cfg
            .all_profiles()
            .iter()
            .map(|(name, p)| ProfileInfo {
                live: *name == live,
                name: name.clone(),
                max_output_tokens: p.effective_max_tokens(&self.catalog),
                has_system: p.system.is_some(),
                provider: p.provider.clone(),
                model: p.model.clone(),
            })
            .collect();
        ProfileListResult {
            live,
            live_source,
            profiles,
        }
    }

    /// Switch the live profile; persisted so it survives restart. This client
    /// is told; others learn on their next `health` or `profile.list`.
    pub(super) fn profile_use(
        &self,
        p: ProfileUseParams,
        conn: Conn<'_>,
    ) -> Result<ProfileChanged, RpcFailure> {
        let name = p.name.as_str();
        self.cfg.profile(name).map_err(RpcFailure::invalid)?;
        let changed = self
            .switch_profile(name, conn.client)
            .map_err(RpcFailure::invalid)?;
        let _ = conn
            .tx
            .send(Message::Notification(theseus_protocol::Notification::new(
                notify::PROFILE_CHANGED,
                &changed,
            )));
        Ok(changed)
    }

    fn switch_profile(&self, name: &str, by: &str) -> Result<ProfileChanged> {
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

    pub(super) fn session_history(
        &self,
        p: theseus_protocol::SessionHistoryParams,
    ) -> Result<theseus_protocol::SessionHistoryResult, RpcFailure> {
        let rec = self.session(&p.session_id)?;
        let nodes = self.store.session_nodes(&p.session_id)?;
        let skip = p.n.map(|n| nodes.len().saturating_sub(n)).unwrap_or(0);
        let pending = self.kernel.pending_confirms()?;
        Ok(theseus_protocol::SessionHistoryResult {
            session: self.session_info(&rec, &waiting_by_execution(&pending)),
            nodes: nodes[skip..]
                .iter()
                .map(|(pos, n)| Self::node_info(*pos, n))
                .collect(),
            pending_confirms: self.confirms_of(&pending, &rec, &nodes),
        })
    }

    pub(super) fn session_watch(&self, p: theseus_protocol::SessionRef, conn: Conn<'_>) -> Value {
        self.bus.watch(&p.session_id, conn.client, conn.tx.clone());
        json!({"watching": p.session_id, "watchers": self.bus.watchers(&p.session_id)})
    }

    pub(super) fn session_unwatch(&self, p: theseus_protocol::SessionRef, conn: Conn<'_>) -> Value {
        self.bus.unwatch(&p.session_id, conn.client);
        json!({"watching": Value::Null})
    }

    pub(super) fn session_recompile(
        &self,
        p: theseus_protocol::SessionRecompileParams,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let strategy = match p.strategy.as_str() {
            "fresh" => Recompile::Fresh,
            "transcript" => Recompile::Transcript,
            other => {
                return Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!("strategy {other:?} is not fresh or transcript"),
                ))
            }
        };
        let mut rec = self.session(&p.session_id)?;
        rec.pending_recompile = Some(strategy);
        self.store.put_session(&rec.session_id, &rec)?;
        self.store.append_ledger(&LedgerRow::new(
            "context.recompile_requested",
            Some(&rec.session_id),
            None,
            json!({"strategy": p.strategy, "by": conn.client}),
        ))?;
        Ok(json!({"session_id": rec.session_id, "pending": p.strategy}))
    }

    pub(super) fn catalog_list(&self) -> theseus_protocol::CatalogListResult {
        let profiles = self.cfg.all_profiles();
        let models = self
            .catalog
            .entries
            .iter()
            .map(|(m, e)| theseus_protocol::CatalogModel {
                model: m.clone(),
                entry: serde_json::to_value(e).unwrap_or(Value::Null),
                profiles: profiles
                    .iter()
                    .filter(|(_, p)| &p.model == m)
                    .map(|(n, _)| n.clone())
                    .collect(),
            })
            .collect();
        theseus_protocol::CatalogListResult {
            version: self.catalog.version.clone(),
            models,
        }
    }

    pub(super) fn compilation_list(
        &self,
        p: theseus_protocol::CompilationListParams,
    ) -> Result<theseus_protocol::CompilationListResult, RpcFailure> {
        let n = p.n.unwrap_or(50).min(500);
        let list = match &p.session_id {
            Some(sid) => self.store.session_compilations(sid)?,
            None => self.store.recent_compilations(n)?,
        };
        let current: std::collections::HashSet<String> = self
            .store
            .list_sessions::<SessionRecord>()?
            .into_iter()
            .filter_map(|s| s.compilation_id)
            .collect();
        let mut out: Vec<theseus_protocol::CompilationInfo> = list
            .iter()
            .map(|c| theseus_protocol::CompilationInfo {
                compilation_id: c.id.clone(),
                session_id: c.session_id.clone(),
                created_at_ms: c.created_at_ms,
                trigger: c.trigger.clone(),
                strategy: c.strategy.clone(),
                as_of: c.as_of,
                includes: c.includes.len() as u32,
                derived_from: c.derived_from.clone(),
                manifest: serde_json::to_value(&c.manifest).unwrap_or(Value::Null),
                current: current.contains(&c.id),
            })
            .collect();
        out.reverse();
        out.truncate(n);
        Ok(theseus_protocol::CompilationListResult { compilations: out })
    }

    pub(super) fn node_list(
        &self,
        p: theseus_protocol::NodeListParams,
    ) -> Result<theseus_protocol::NodeListResult, RpcFailure> {
        let n = p.n.unwrap_or(100).min(2000);
        let mut nodes = match &p.session_id {
            Some(sid) => self.store.session_nodes(sid)?,
            None => self
                .store
                .recent_nodes(if p.kind.is_some() { n * 10 } else { n })?,
        };
        if let Some(k) = &p.kind {
            nodes.retain(|(_, node)| node.kind_str() == k);
        }
        let skip = nodes.len().saturating_sub(n);
        Ok(theseus_protocol::NodeListResult {
            nodes: nodes[skip..]
                .iter()
                .rev()
                .map(|(pos, node)| Self::node_info(*pos, node))
                .collect(),
            total: self.store.node_count()?,
        })
    }

    pub(super) fn action_confirm(
        &self,
        p: theseus_protocol::ActionConfirmParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::ActionConfirmResult, RpcFailure> {
        if p.watch {
            // Subscribe before the answer wakes the execution: the
            // continuation turn is then seen from its first event.
            if let Some(a) = self.kernel.action(&p.correlation_id)? {
                self.bus.watch(&a.session_id, conn.client, conn.tx.clone());
            }
        }
        let who = conn.answerer(p.author, p.discord);
        self.confirm_action(&p.correlation_id, p.approve, p.note.as_deref(), who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: r.to_string(),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }

    pub(super) fn tool_list(&self) -> theseus_protocol::ToolListResult {
        let calls = self.tools.calls.lock().unwrap().clone();
        let total: u64 = calls.values().sum();
        let proc_calls = calls.get("proc.run").copied().unwrap_or(0);
        let tools = self
            .tools
            .registry
            .all()
            .map(|t| {
                let now = self.tools.posture_now(t.name());
                theseus_protocol::ToolInfo {
                    name: t.name().into(),
                    wire_name: theseus_tools::wire_name(t.name()),
                    family: t.family().into(),
                    description: t.description().into(),
                    class: t.class().as_str().into(),
                    backend: t.backend().as_str().into(),
                    policy: now.posture.as_str().into(),
                    setting: now.setting,
                    config_posture: now.config.as_str().into(),
                    config_setting: now.config_setting,
                    tightened: self.tools.tightened.get(t.name()),
                    input_schema: t.input_schema(),
                    calls: calls.get(t.name()).copied().unwrap_or(0),
                }
            })
            .collect();
        theseus_protocol::ToolListResult {
            tools,
            roots: self
                .tools
                .ctx
                .roots
                .iter()
                .map(|r| r.display().to_string())
                .collect(),
            shell_fallback_ratio: if total == 0 {
                0.0
            } else {
                proc_calls as f64 / total as f64
            },
            calls_total: total,
        }
    }

    pub(super) fn execution_list(
        &self,
    ) -> Result<theseus_protocol::ExecutionListResult, RpcFailure> {
        let execs = self.kernel.executions()?;
        Ok(theseus_protocol::ExecutionListResult {
            executions: execs.iter().map(Self::execution_info).collect(),
        })
    }

    pub(super) fn action_list(
        &self,
        p: theseus_protocol::ActionListParams,
    ) -> Result<theseus_protocol::ActionListResult, RpcFailure> {
        let n = p.n.unwrap_or(200).min(2000);
        let mut actions = self.kernel.actions()?;
        let total = actions.len() as u64;
        if let Some(x) = &p.execution_id {
            actions.retain(|a| &a.execution_id == x);
        }
        actions.sort_by_key(|a| std::cmp::Reverse(a.planned_at_ms));
        actions.truncate(n);
        Ok(theseus_protocol::ActionListResult {
            actions: actions.iter().map(Self::action_info).collect(),
            total,
        })
    }

    pub(super) fn execution_cancel(
        &self,
        p: theseus_protocol::ExecutionCancelParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::ExecutionCancelResult, RpcFailure> {
        if self.kernel.execution(&p.execution_id)?.is_none() {
            return Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no execution {}", p.execution_id),
            ));
        }
        let (e, cancelled) =
            self.cancel_execution(&p.execution_id, p.author.as_deref().unwrap_or(conn.client))?;
        Ok(theseus_protocol::ExecutionCancelResult {
            execution: Self::execution_info(&e),
            cancelled_actions: cancelled,
        })
    }

    pub(super) fn ledger_tail(&self, p: LedgerTailParams) -> Result<LedgerTailResult, RpcFailure> {
        let n = p.n.unwrap_or(20).min(1000);
        let scan = if p.kind.is_some() || p.session_id.is_some() {
            n * 50
        } else {
            n
        };
        let rows: Vec<(u64, LedgerRow)> = self.store.ledger_tail(scan)?;
        let rows: Vec<LedgerEntry> = rows
            .into_iter()
            .filter(|(_, r)| p.kind.as_deref().is_none_or(|k| r.is_kind(k)))
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
        Ok(LedgerTailResult {
            rows,
            total: self.store.ledger_len()?,
        })
    }

    pub(super) fn narrative_watch(&self, conn: Conn<'_>) -> theseus_protocol::NarrativeWatchResult {
        let lines = self
            .narrator
            .watch(conn.client, conn.tx.clone())
            .unwrap_or_default();
        theseus_protocol::NarrativeWatchResult {
            lines,
            capacity: self.narrator.capacity() as u32,
        }
    }

    pub(super) fn narrative_unwatch(&self, conn: Conn<'_>) -> Value {
        self.narrator.unwatch(conn.client);
        json!({"watching": false})
    }

    /// `shutdown`: ledgered, telemetry flushed, the index checkpointed, and the
    /// daemon told to stop.
    pub(super) fn stop(&self) -> Value {
        let _ =
            self.store
                .append_ledger(&LedgerRow::new("server.stopping", None, None, Value::Null));
        self.telemetry.flush();
        let _ = self.store.checkpoint();
        self.shutdown.notify_waiters();
        json!({"ok": true})
    }
}
