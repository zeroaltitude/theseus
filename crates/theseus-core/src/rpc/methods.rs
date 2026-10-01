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
            // The records just read: `session_count` would read them all again.
            sessions: sessions.len() as u64,
            turns: sessions.iter().map(|s| s.turns).sum(),
            model: prof.as_ref().map(|p| p.model.clone()).unwrap_or_default(),
            profile,
            provider: prof.map(|p| p.provider).unwrap_or_default(),
            providers: self.runner.providers.keys().cloned().collect(),
            secrets_resolved: self.secrets.ready_names(),
            secrets: self.secrets.status(),
            config: self.config_gate.status(),
            startup: self.startup_log.snapshot(),
            usage_total,
            provider_errors: self.provider_errors.load(Ordering::Relaxed),
            ledger_rows: self.store.ledger_len().unwrap_or(0),
            telemetry: self.telemetry_status(),
            kernel: self.kernel_status(),
            children: self.children_status(),
            broker: self.tools.broker.status(),
            cost_usd_total: sessions.iter().map(|s| s.cost_usd).sum(),
            catalog_version: self.catalog.version.clone(),
            bindings: self
                .bindings
                .all()
                .into_iter()
                .map(|mut b| {
                    b.outbox = Some(self.outbox.status(&b.kind));
                    b
                })
                .collect(),
            narrative: self.narrator.on(),
            context: self.context_status(),
            approval: self.approval_status(),
            tightenings: self.tools.tightened.all(),
            wakes: self.wakes(None, None).unwrap_or_default(),
            external_text: crate::external::listed(&sessions),
            web: self.web_refusals.status(),
        }
    }

    /// The web UI refused a request that was not from its own page or
    /// address (theseus-70f): counted for health, and ledgered as
    /// `web.refused` at most once a minute per kind. Not narrated.
    pub fn web_refused(&self, why: crate::webui::Why, detail: Value) {
        self.web_refusals.refuse(&self.store, why, detail);
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
        let e = r
            .execution_id
            .as_deref()
            .and_then(|id| self.kernel.execution(id).ok().flatten());
        i.execution_state = e.as_ref().map(|e| e.state.as_str().to_string());
        if let Some(exec) = r.execution_id.as_deref() {
            i.pending_confirms = waiting.get(exec).copied().unwrap_or(0);
        }
        // A task (DD7): its parent, for the tree, and its carved limit.
        if let Some(t) = &r.task {
            i.parent_session_id = Some(t.parent_session.clone());
            i.limit_usd = e
                .as_ref()
                .map(|e| theseus_kernel::micros_to_usd(e.budget.limit_micros));
        }
        i
    }

    /// Tasks (DD7), the newest first: every one, or only those `session`
    /// started, or only those that report to `target`.
    pub fn tasks(
        &self,
        session: Option<&str>,
        target: Option<&str>,
    ) -> Result<Vec<theseus_protocol::TaskInfo>> {
        let parent = match session {
            Some(s) => match self.store.get_session::<SessionRecord>(s)? {
                Some(SessionRecord {
                    execution_id: Some(e),
                    ..
                }) => Some(e),
                _ => return Ok(vec![]),
            },
            None => None,
        };
        let waiting = waiting_by_execution(&self.kernel.pending_confirms()?);
        let mut out = Vec::new();
        for e in self.kernel.tasks(parent.as_deref())? {
            let rec: Option<SessionRecord> = self.store.get_session(&e.session_id)?;
            let info =
                crate::task::info(&e, rec.as_ref(), waiting.get(&e.id).copied().unwrap_or(0));
            if target.is_some_and(|t| info.target.as_deref() != Some(t)) {
                continue;
            }
            out.push(info);
        }
        out.reverse();
        Ok(out)
    }

    pub(super) fn task_list(
        &self,
        p: theseus_protocol::TaskListParams,
    ) -> Result<theseus_protocol::TaskListResult, RpcFailure> {
        Ok(theseus_protocol::TaskListResult {
            tasks: self.tasks(p.session_id.as_deref(), p.target.as_deref())?,
        })
    }

    /// Stop a task and its jobs, as `execution.cancel` does; the place hears
    /// it once (DD7). A task that has ended already is left as it is.
    pub(super) fn task_cancel(
        &self,
        p: theseus_protocol::TaskCancelParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::TaskCancelResult, RpcFailure> {
        self.task_cancel_by(&p.task, &conn.actor(p.author.as_deref()))
            .map_err(|e| match e.downcast::<crate::task::NoSuchTask>() {
                Ok(n) => RpcFailure::new(error_code::NOT_FOUND, n.0),
                Err(e) => RpcFailure::invalid(e),
            })
    }

    /// Pending wakes (DD8), soonest first: every one, or only `session`'s, or
    /// only those whose turns post to `target`.
    pub fn wakes(
        &self,
        session: Option<&str>,
        target: Option<&str>,
    ) -> Result<Vec<theseus_protocol::WakeInfo>> {
        let mut out = Vec::new();
        for (e, w) in self.kernel.pending_wakes()? {
            if session.is_some_and(|s| s != e.session_id) {
                continue;
            }
            let rec: Option<SessionRecord> = self.store.get_session(&e.session_id)?;
            let info = crate::wake::info(&e, &w, rec.and_then(|r| r.title));
            if target.is_some_and(|t| info.target.as_deref() != Some(t)) {
                continue;
            }
            out.push(info);
        }
        Ok(out)
    }

    pub(super) fn wake_list(
        &self,
        p: theseus_protocol::WakeListParams,
    ) -> Result<theseus_protocol::WakeListResult, RpcFailure> {
        Ok(theseus_protocol::WakeListResult {
            wakes: self.wakes(p.session_id.as_deref(), p.target.as_deref())?,
        })
    }

    /// Cancel a pending wake, so nothing fires (DD8).
    pub(super) fn wake_cancel(
        &self,
        p: theseus_protocol::WakeCancelParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::WakeCancelResult, RpcFailure> {
        self.wake_cancel_by(&p.wake, &conn.actor(p.author.as_deref()))
            .map_err(|e| match e.downcast::<crate::wake::NoSuchWake>() {
                Ok(n) => RpcFailure::new(error_code::NOT_FOUND, n.0),
                Err(e) => RpcFailure::invalid(e),
            })
    }

    /// Cancel the pending wake `name` names (its id, or the end of it), for
    /// `by`. A name that also names a task is refused, so a cancel never
    /// stops the wrong thing.
    pub fn wake_cancel_by(
        &self,
        name: &str,
        by: &str,
    ) -> Result<theseus_protocol::WakeCancelResult> {
        let pending = self.kernel.pending_wakes()?;
        let (e, w) = crate::wake::resolve(&pending, name).map_err(crate::wake::NoSuchWake)?;
        let tasks = self.kernel.tasks(None)?;
        if let Ok(t) = crate::task::resolve(&tasks, name) {
            anyhow::bail!(
                "`{name}` names a wake ({}) and a task ({}): give more of its id",
                crate::task::short(&w.id),
                crate::task::short(&t.session_id)
            );
        }
        let rec: Option<SessionRecord> = self.store.get_session(&e.session_id)?;
        let info = crate::wake::info(e, w, rec.and_then(|r| r.title));
        if self.kernel.cancel_wake(&e.id, &w.id, by)?.is_none() {
            return Err(crate::wake::NoSuchWake(format!(
                "wake {} is no longer pending: it ran, or was cancelled, just now",
                info.short
            ))
            .into());
        }
        crate::narrative::narrate!(
            self.narrator,
            Session,
            Some(&e.session_id),
            None,
            "Wake {} cancelled by {by}: \"{}\" will not run.",
            info.short,
            crate::session::title_from(&w.note)
        );
        Ok(theseus_protocol::WakeCancelResult { wake: info })
    }

    /// Cancel the task `name` names (its id, or the end of it), for `by`.
    pub fn task_cancel_by(
        &self,
        name: &str,
        by: &str,
    ) -> Result<theseus_protocol::TaskCancelResult> {
        let tasks = self.kernel.tasks(None)?;
        let id = crate::task::resolve(&tasks, name)
            .map_err(crate::task::NoSuchTask)?
            .id
            .clone();
        let (e, cancelled) = self.cancel_execution(&id, by)?;
        let rec: Option<SessionRecord> = self.store.get_session(&e.session_id)?;
        Ok(theseus_protocol::TaskCancelResult {
            task: crate::task::info(&e, rec.as_ref(), 0),
            cancelled_actions: cancelled,
        })
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
                arrived: Some(conn.arrived),
                config_wait_us: conn.config_wait_us,
                reply_to: p.reply_to,
            })
            .await;
        match result {
            Ok(r) => {
                self.telemetry().record_turn(&r);
                Ok(r)
            }
            Err(e) => Err(match e.downcast::<TurnError>() {
                Ok(te) => {
                    self.provider_errors.fetch_add(1, Ordering::Relaxed);
                    self.telemetry()
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
        self.session(&p.session_id)?;
        self.request_recompile(&p.session_id, strategy, conn.client)?;
        Ok(json!({"session_id": p.session_id, "pending": p.strategy}))
    }

    /// Ask for a recompile on the session's next turn: set under the
    /// record's lock, with its row in the same frame, so a turn running now
    /// cannot write it over (theseus-xeo). False when there is no session.
    pub fn request_recompile(
        &self,
        session_id: &str,
        strategy: Recompile,
        by: &str,
    ) -> Result<bool> {
        let row = LedgerRow::new(
            "context.recompile_requested",
            Some(session_id),
            None,
            json!({"strategy": strategy, "by": by}),
        );
        let written = self.store.update_session(session_id, |r| {
            r.pending_recompile = Some(strategy);
            Ok(vec![theseus_store::NewRecord::json(
                theseus_store::kinds::LEDGER,
                None,
                &row,
            )?])
        })?;
        Ok(written.is_some())
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
        self.confirm_action_with(
            &p.correlation_id,
            p.approve,
            p.note.as_deref(),
            who,
            p.trust,
        )
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
            self.cancel_execution(&p.execution_id, &conn.actor(p.author.as_deref()))?;
        Ok(theseus_protocol::ExecutionCancelResult {
            execution: Self::execution_info(&e),
            cancelled_actions: cancelled,
        })
    }

    /// `execution.stop` (W1): `/stop`, which halts the work and keeps the
    /// conversation. A task is refused: `task.cancel` stops it.
    pub(super) fn execution_stop(
        &self,
        p: theseus_protocol::ExecutionStopParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::ExecutionStopResult, RpcFailure> {
        if self.kernel.execution(&p.execution_id)?.is_none() {
            return Err(RpcFailure::new(
                error_code::NOT_FOUND,
                format!("no execution {}", p.execution_id),
            ));
        }
        let by = conn.actor(p.author.as_deref());
        match self.stop_execution(&p.execution_id, &by) {
            Ok(r) => Ok(r),
            Err(e) => match e.downcast_ref::<theseus_kernel::KernelError>() {
                Some(theseus_kernel::KernelError::StopTask { .. }) => {
                    Err(RpcFailure::new(error_code::REFUSED, e.to_string()))
                }
                _ => Err(e.into()),
            },
        }
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

    /// The daemon stops at once: ledgered, the index checkpointed, and the
    /// serving loops woken. `theseusd` flushes telemetry once its serving
    /// loop ends, bounded. A restart onto the vault's changed note stops so;
    /// a client's `shutdown` wakes the loops only once its answer is on the
    /// wire (`stopping`, theseus-ur0).
    pub(super) fn stop(&self) -> Value {
        let answer = self.stopping();
        self.shutdown.notify_waiters();
        answer
    }

    /// `shutdown`'s work before its answer: the row and the checkpoint. The
    /// connection that asked wakes the serving loops after it has written
    /// the answer (`serve_connection`), since the runtime's end could cancel
    /// that connection's writer first, and the client would see the
    /// connection close unanswered though the daemon stopped (theseus-ur0).
    pub(super) fn stopping(&self) -> Value {
        let _ =
            self.store
                .append_ledger(&LedgerRow::new("server.stopping", None, None, Value::Null));
        let _ = self.store.checkpoint();
        json!({"ok": true})
    }
}
