//! The protocol's methods, one named function each (spec §3.18). `server`
//! routes a request to its method by name; each takes its typed params and
//! returns its typed result, and `?` on an internal error is `INTERNAL`.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use theseus_protocol::LedgerKind;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::{
    error_code, HealthResult, LedgerEntry, LedgerTailParams, LedgerTailResult, Message,
    ProfileChanged, ProfileInfo, ProfileListResult, ProfileUseParams, ProviderErrorData,
    SessionKind, SessionOpenParams, TurnSubmitParams, Usage,
};

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

/// `session.wait`'s timeout: 10 minutes by default, a day at most.
const WAIT_DEFAULT_MS: u64 = 10 * 60 * 1000;
const WAIT_MAX_MS: u64 = 24 * 60 * 60 * 1000;

/// Health's session totals (`Core::session_totals`).
struct SessionTotals {
    sessions: u64,
    turns: u64,
    usage: Usage,
    cost_usd: f64,
    /// The sessions that hold external text.
    holding: Vec<SessionRecord>,
}

impl Core {
    pub fn health(&self) -> HealthResult {
        let (profile, _) = self.live_profile();
        let prof = self.cfg.all_profiles().get(&profile).cloned();
        let totals = self.session_totals();
        HealthResult {
            name: crate::NAME.into(),
            version: crate::VERSION.into(),
            protocol: theseus_protocol::VERSION.into(),
            build: crate::build(),
            uptime_secs: self.started.elapsed().as_secs(),
            sessions: totals.sessions,
            turns: totals.turns,
            model: prof.as_ref().map(|p| p.model.clone()).unwrap_or_default(),
            profile,
            provider: prof.map(|p| p.provider).unwrap_or_default(),
            providers: self.runner.providers.keys().cloned().collect(),
            secrets_resolved: self.secrets.ready_names(),
            secrets: self.secrets.status(),
            config: self.config_gate.status(),
            startup: self.startup_log.snapshot(),
            usage_total: totals.usage,
            provider_errors: self.provider_errors.load(Ordering::Relaxed),
            ledger_rows: self.store.ledger_len().unwrap_or(0),
            telemetry: self.telemetry_status(),
            kernel: self.kernel_status(),
            children: self.children_status(),
            broker: self.tools.broker.status(),
            harness_only: Some(crate::broker::harness_only(&self.cfg, &self.tools.broker)),
            cost_usd_total: totals.cost_usd,
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
            tightenings: self.tools.tightened.all(),
            wakes: self.wakes(None, None).unwrap_or_default(),
            external_text: crate::external::listed(
                &totals.holding,
                theseus_protocol::now_unix_ms(),
            ),
            web: self.web_refusals.status(self.cfg.web.dev_origin.as_deref()),
            disk: self.tools.disk.status(),
            binary: crate::binary::status(),
            spool: self.spool_status(),
            push: Some(self.push.status(self.bus.all_watchers())),
            aws: self.tools.aws.as_ref().map(|a| a.status()),
            // Asked of the tender by `health_now`, never here: this answers
            // at once.
            index: None,
            mcp: self.mcp.status(),
            extensions: self.extend_health(),
            store: self.store_status(),
            crash: self.crash_status(),
            sandbox: self.tools.enabled().then(|| self.tools.sandbox.health()),
            cancels: self.tools.stops.counts(),
            places: Some(self.runner.place_rule.health(&self.cfg)),
            judge: Some(self.runner.judge.health()),
            terminals: self.tools.terms.all().iter().map(|t| t.info()).collect(),
            mcp_server: self.mcp_server.health(self.cfg.mcp_server.enabled),
            lsp: self.tools.lsp.as_ref().map(|b| b.health()),
            memory: self.runner.memory.on().then(|| self.memory_health()),
            tasks: Some(self.tasks_health()),
        }
    }

    /// Health's session totals (theseus-lv2): from the index's projection,
    /// which adds up every session's numbers as it is written, and the
    /// sessions holding external text by their term; so a health answer
    /// reads only those sessions. From one read of every session record
    /// while the projection is not whole (a store an older build wrote last,
    /// until its terms are built after serving).
    fn session_totals(&self) -> SessionTotals {
        use theseus_store::Store as _;
        let inner = self.store.inner();
        let projected = inner.totals(theseus_store::kinds::SESSION).ok().flatten();
        let holding = inner
            .latest_by_terms(
                theseus_store::kinds::SESSION,
                crate::store::EXTERNAL,
                &format!("{}\u{1}", crate::store::EXTERNAL),
            )
            .ok()
            .flatten();
        if let (Some(t), Some(holding)) = (projected, holding) {
            let n = |i: usize| u64::try_from(t[i]).unwrap_or(u64::MAX);
            return SessionTotals {
                sessions: n(0),
                turns: n(1),
                usage: Usage {
                    input_tokens: n(2),
                    output_tokens: n(3),
                    cache_read_input_tokens: n(4),
                    cache_creation_input_tokens: n(5),
                    cache_creation_1h_input_tokens: n(6),
                },
                cost_usd: crate::store::cost_usd(t[7]),
                holding: holding.iter().filter_map(|r| r.decode().ok()).collect(),
            };
        }
        let sessions = self
            .store
            .list_sessions::<SessionRecord>()
            .unwrap_or_default();
        let mut usage = Usage::default();
        for s in &sessions {
            crate::turn::add_usage(&mut usage, &s.usage);
        }
        SessionTotals {
            sessions: sessions.len() as u64,
            turns: sessions.iter().map(|s| s.turns).sum(),
            usage,
            cost_usd: sessions.iter().map(|s| s.cost_usd).sum(),
            holding: sessions
                .into_iter()
                .filter(|s| s.external.is_some())
                .collect(),
        }
    }

    /// `health`, with the index tender's block (roadmap row 51): a tender
    /// that runs is asked under `tender::HEALTH_DEADLINE`, so health never
    /// waits long on it, and none is asked before one runs, so a start's
    /// first answer waits on none.
    pub async fn health_now(&self) -> HealthResult {
        let mut h = self.health();
        h.index = Some(self.index.health_block().await);
        h
    }

    /// The daemon stops (roadmap row 51): the index tender gets SIGTERM and
    /// is never waited for, unless the daemon restarts in place onto the
    /// vault's changed note, whose next image takes the tender over.
    pub fn stop_index_tender(&self) {
        self.index.stop(self.restart_requested().is_some());
    }

    /// The daemon stops (L2): each language server's group gets SIGTERM,
    /// never waited for.
    pub fn stop_lsp(&self) {
        if let Some(lsp) = &self.tools.lsp {
            lsp.stop_all();
        }
    }

    /// `index.query` (roadmap row 51): forwarded to the tender as it came,
    /// bounded. An error it answers is the client's, with its code.
    pub async fn index_query(
        &self,
        p: theseus_protocol::index::IndexQueryParams,
    ) -> Result<theseus_protocol::index::IndexQueryResult, RpcFailure> {
        self.index
            .query(&p)
            .await
            .map_err(|(code, message)| RpcFailure::new(code, message))
    }

    /// The web UI refused a request that was not from its own page or
    /// address (theseus-70f): counted for health, and ledgered as
    /// `web.refused` at most once a minute per kind. Not narrated.
    pub fn web_refused(&self, why: crate::webui::Why, detail: Value) {
        self.web_refusals.refuse(&self.store, why, detail);
    }

    /// The web UI served a `/ws` upgrade for `[web] dev_origin`
    /// (theseus-zab): counted for health, and ledgered as `web.dev_origin` at
    /// most once a minute. Not narrated.
    pub fn web_dev_origin(&self, detail: Value) {
        self.web_refusals.dev_origin(&self.store, detail);
    }

    /// What the web UI's rows still hold in their span, written now: a
    /// clean stop's (theseus-sqpx), before its last checkpoint.
    pub(crate) fn flush_web_rows(&self) {
        self.web_refusals.flush(&self.store);
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

    pub(crate) fn open_session(&self, p: SessionOpenParams) -> Result<SessionRecord> {
        let operator = Authority {
            principal: OPERATOR.to_string(),
            ..Default::default()
        };
        self.open_session_as(p, operator, None)
    }

    /// `open_session` under `authority`, with a spend limit other than the
    /// default: an MCP client's (step 41b, `rpc::mcp`).
    pub(super) fn open_session_as(
        &self,
        p: SessionOpenParams,
        authority: Authority,
        limit_micros: Option<theseus_kernel::Micros>,
    ) -> Result<SessionRecord> {
        // A session that a holding session's job opens holds what that one
        // holds, from the frame that writes it (theseus-b5cl). Read before
        // anything is opened, so a record that cannot be read leaves nothing.
        let now = theseus_protocol::now_unix_ms();
        let inherited = crate::external::from_job(&self.store, p.opened_from.as_deref(), now)?;
        let mut rec = SessionRecord::new(p.kind.unwrap_or(SessionKind::Conversation), p.label);
        let exec =
            self.kernel
                .open_execution(&rec.session_id, rec.kind, authority, limit_micros, None)?;
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
        let mut data = json!({"execution_id": rec.execution_id});
        if let Some(from) = &p.opened_from {
            data["opened_from"] = json!(from);
        }
        let opened = LedgerRow::new(LedgerKind::SessionOpened, Some(&rec.session_id), None, data);
        let Some(h) = inherited else {
            self.store.put_session(&rec.session_id, &rec)?;
            self.store.append_ledger(&opened)?;
            return Ok(rec);
        };
        // The row that opens it, then the hold's row and the record, in one
        // frame.
        let mut frame = vec![theseus_store::NewRecord::json(
            theseus_store::kinds::LEDGER,
            None,
            &opened,
        )?];
        frame.extend(crate::external::hold(rec.clone(), h.clone(), None)?.unwrap_or_default());
        self.store.append(&frame)?;
        rec.external = Some(h.clone());
        self.session_rec(&rec.session_id)
            .record(&crate::fact::tool::HoldTaken {
                hold: &h,
                mode: self.tools.external_text,
            });
        Ok(rec)
    }

    /// A turn that a holding session's job sends to another session
    /// (`opened_from`, theseus-b5cl): that session takes the sender's hold,
    /// in a frame of its own before the turn writes the input, so no crash
    /// leaves the input without it. Nothing when it holds one already, or the
    /// sender holds none.
    fn take_from_job(&self, session_id: &str, from: Option<&str>) -> Result<()> {
        if from == Some(session_id) {
            return Ok(());
        }
        let now = theseus_protocol::now_unix_ms();
        let Some(h) = crate::external::from_job(&self.store, from, now)? else {
            return Ok(());
        };
        let newly = self.store.with_session(session_id, |rec| {
            let Some(frame) = crate::external::hold(rec, h.clone(), None)? else {
                return Ok(false);
            };
            self.store.append(&frame)?;
            Ok(true)
        })?;
        if newly == Some(true) {
            self.session_rec(session_id)
                .record(&crate::fact::tool::HoldTaken {
                    hold: &h,
                    mode: self.tools.external_text,
                });
        }
        Ok(())
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

    /// `session.list`: every session, or only those `ids` names
    /// (theseus-in3), read by id: a client that meets a new session in
    /// `execution.changed` asks for its title.
    pub(super) fn session_list_of(
        &self,
        p: theseus_protocol::SessionListParams,
    ) -> Result<theseus_protocol::SessionListResult, RpcFailure> {
        let Some(ids) = p.ids else {
            if let Some(n) = p.n {
                return self.session_page(n.min(1000), p.before);
            }
            return Ok(theseus_protocol::SessionListResult {
                sessions: self.session_list()?,
                older: None,
            });
        };
        let mut recs = Vec::new();
        for id in &ids {
            if let Some(r) = self.store.get_session::<SessionRecord>(id)? {
                recs.push(r);
            }
        }
        let pending = self.pending_by_execution(
            &self
                .kernel
                .pending_confirms()?
                .into_iter()
                .filter(|a| ids.contains(&a.session_id))
                .collect::<Vec<_>>(),
            None,
        );
        Ok(theseus_protocol::SessionListResult {
            sessions: recs
                .iter()
                .map(|r| self.session_info(r, &pending))
                .collect(),
            older: None,
        })
    }

    /// `session.list { n, before }` (theseus-96w2): the newest `n` sessions
    /// by when each was opened, through the index's births, each with its
    /// execution and questions as `session.list` gives them. While the
    /// index's shape is built after serving, every session is read, the
    /// newest `n` by when each was opened kept, and no cursor is given.
    fn session_page(
        &self,
        n: usize,
        before: Option<u64>,
    ) -> Result<theseus_protocol::SessionListResult, RpcFailure> {
        let (recs, older) = match self.sessions_paged(n, before)? {
            Some(page) => page,
            None => {
                let mut all: Vec<SessionRecord> = self.store.list_sessions()?;
                all.retain(|r| r.imported.is_none());
                all.sort_by_key(|r| std::cmp::Reverse(r.created_at_unix_ms));
                all.truncate(n);
                (all, None)
            }
        };
        let ids: Vec<&str> = recs.iter().map(|r| r.session_id.as_str()).collect();
        let pending = self.pending_by_execution(
            &self
                .kernel
                .pending_confirms()?
                .into_iter()
                .filter(|a| ids.contains(&a.session_id.as_str()))
                .collect::<Vec<_>>(),
            None,
        );
        Ok(theseus_protocol::SessionListResult {
            sessions: recs
                .iter()
                .map(|r| self.session_info(r, &pending))
                .collect(),
            older,
        })
    }

    /// `executions.watch` (theseus-in3): seed the board if nothing has yet,
    /// subscribe this connection, then read the board, so no change falls
    /// between the snapshot and the events.
    pub(super) async fn executions_watch(
        self: &std::sync::Arc<Self>,
        p: theseus_protocol::ExecutionsWatchParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::ExecutionsWatchResult, RpcFailure> {
        self.push.ensure(self).await?;
        self.bus.watch_all(conn.client, conn.tx.clone());
        let (position, executions, total) = self.push.snapshot(p.limit.unwrap_or(200) as usize);
        Ok(theseus_protocol::ExecutionsWatchResult {
            position,
            executions,
            confirms: self.confirm_list()?,
            total,
        })
    }

    /// `session.wait` (theseus-in3, design `stage2` §2.6): until the
    /// session's execution is what `until` names. The board's feed wakes it,
    /// so it costs nothing while parked, and it subscribes before it reads,
    /// so no change falls between. A wait already satisfied answers at once
    /// (`already`). It ends with its connection, and a connection holds at
    /// most `push::MAX_WAITS`.
    pub(super) async fn session_wait(
        self: &std::sync::Arc<Self>,
        p: theseus_protocol::SessionWaitParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::SessionWaitResult, RpcFailure> {
        use theseus_protocol::WaitUntil;
        let rec = self.session(&p.session_id)?;
        if p.until == WaitUntil::Terminal && rec.kind == SessionKind::Conversation {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "a conversation never ends; wait for settled",
            ));
        }
        let timeout = std::time::Duration::from_millis(
            p.timeout_ms.unwrap_or(WAIT_DEFAULT_MS).min(WAIT_MAX_MS),
        );
        let Some(_slot) = self.push.wait_slot(conn.client) else {
            return Err(RpcFailure::new(
                error_code::LIMIT,
                format!(
                    "this connection holds {} waits already: end one, or wait on another \
                     connection",
                    crate::push::MAX_WAITS
                ),
            ));
        };
        self.push.ensure(self).await?;
        let mut feed = self.push.feed();
        let mut closed = conn.closed.clone();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut first = true;
        loop {
            feed.borrow_and_update();
            let view = self.push.view_of_session(&p.session_id);
            if let Some(v) = view.as_ref().filter(|v| {
                p.until.reached_by(v) && p.after_position.is_none_or(|a| v.position > a)
            }) {
                return Ok(self.waited(p.until.as_str(), first, Some(v.clone()), &p.session_id));
            }
            first = false;
            tokio::select! {
                changed = feed.changed() => {
                    if changed.is_err() {
                        return Ok(self.waited("timeout", false, view, &p.session_id));
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return Ok(self.waited("timeout", false, view, &p.session_id));
                }
                _ = closed.changed() => {
                    return Err(RpcFailure::new(error_code::INTERNAL, "the connection closed"));
                }
            }
        }
    }

    /// `session.wait`'s answer: what was reached, and the session's view and
    /// questions as the wait ended.
    fn waited(
        &self,
        reached: &str,
        already: bool,
        execution: Option<theseus_protocol::ExecutionView>,
        session_id: &str,
    ) -> theseus_protocol::SessionWaitResult {
        theseus_protocol::SessionWaitResult {
            reached: reached.to_string(),
            already,
            execution,
            confirms: self.pending_confirms(session_id).unwrap_or_default(),
        }
    }

    /// `executions.unwatch`: the connection's all-session watch ends.
    pub(super) fn executions_unwatch(&self, conn: Conn<'_>) -> Value {
        json!({"watching": false, "was": self.bus.unwatch_all(conn.client)})
    }

    /// Every session, the most recently active first.
    pub fn session_list(&self) -> Result<Vec<theseus_protocol::SessionInfo>> {
        let pending =
            self.pending_by_execution(&self.kernel.pending_confirms().unwrap_or_default(), None);
        Ok(self
            .sessions_by_activity()?
            .iter()
            // Imported sessions are the import's to list (theseus-0lrr.6).
            .filter(|r| r.imported.is_none())
            .map(|r| self.session_info(r, &pending))
            .collect())
    }

    /// A session as `session.list` shows it, with its execution's state and
    /// attention (theseus-in3). `pending` is `pending_by_execution`'s.
    fn session_info(
        &self,
        r: &SessionRecord,
        pending: &std::collections::BTreeMap<String, Vec<theseus_protocol::PendingConfirm>>,
    ) -> theseus_protocol::SessionInfo {
        let mut i = r.info();
        let e = r
            .execution_id
            .as_deref()
            .and_then(|id| self.kernel.execution(id).ok().flatten());
        i.execution_state = e.as_ref().map(|e| e.state.as_str().to_string());
        let asks = r
            .execution_id
            .as_deref()
            .and_then(|exec| pending.get(exec))
            .cloned()
            .unwrap_or_default();
        i.pending_confirms = asks.len() as u32;
        // A task (DD7): its parent, for the tree, and its carved limit.
        if let Some(t) = &r.task {
            i.parent_session_id = Some(t.parent_session.clone());
            i.limit_usd = e
                .as_ref()
                .map(|e| theseus_kernel::micros_to_usd(e.budget.limit_micros));
        }
        i.attention = e.as_ref().map(|e| {
            crate::push::view(e, asks, i.parent_session_id.clone(), 0, e.updated_at_ms).attention
        });
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
        let pending = self.pending_by_execution(&self.kernel.pending_confirms()?, None);
        let mut out = Vec::new();
        for e in self.kernel.tasks(parent.as_deref())? {
            let rec: Option<SessionRecord> = self.store.get_session(&e.session_id)?;
            let asks = pending.get(&e.id).cloned().unwrap_or_default();
            let mut info = crate::task::info(&e, rec.as_ref(), asks.len() as u32);
            if target.is_some_and(|t| info.target.as_deref() != Some(t)) {
                continue;
            }
            info.arrangement = crate::task::arrangement_of(&self.store, rec.as_ref());
            let parent = Some(info.parent_session_id.clone()).filter(|p| !p.is_empty());
            info.attention =
                Some(crate::push::view(&e, asks, parent, 0, e.updated_at_ms).attention);
            out.push(info);
        }
        out.reverse();
        Ok(out)
    }

    pub fn task_list(
        &self,
        p: theseus_protocol::TaskListParams,
    ) -> Result<theseus_protocol::TaskListResult, RpcFailure> {
        Ok(theseus_protocol::TaskListResult {
            tasks: self.tasks(p.session_id.as_deref(), p.target.as_deref())?,
            records: self.task_records(p.session_id.as_deref())?,
        })
    }

    /// The task records (39a), each as it reads now: every one, or the ones
    /// a session's turns see.
    pub fn task_records(
        &self,
        session: Option<&str>,
    ) -> Result<Vec<crate::task_graph::TaskRecord>> {
        let all = crate::task_graph::all_shown(&self.store, &self.kernel)?;
        Ok(match session {
            Some(s) => crate::task_graph::scope(&all, s)
                .into_iter()
                .cloned()
                .collect(),
            None => all,
        })
    }

    /// One task record (39a), by its id or the end of it, with its
    /// children.
    pub fn task_get(
        &self,
        p: theseus_protocol::tasks::TaskGetParams,
    ) -> Result<theseus_protocol::tasks::TaskGetResult, RpcFailure> {
        let all = crate::task_graph::all_shown(&self.store, &self.kernel)?;
        let task = crate::task_graph::resolve(&all, &p.id)
            .map_err(|e| RpcFailure::new(error_code::NOT_FOUND, e))?
            .clone();
        let children = crate::task_graph::children(&all, &task.id)
            .into_iter()
            .cloned()
            .collect();
        Ok(theseus_protocol::tasks::TaskGetResult { task, children })
    }

    /// Stop a task and its jobs, as `execution.cancel` does; the place hears
    /// it once (DD7). A task that has ended already is left as it is.
    pub(super) async fn task_cancel(
        &self,
        p: theseus_protocol::TaskCancelParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::TaskCancelResult, RpcFailure> {
        self.task_cancel_by(&p.task, &conn.actor(p.author.as_deref()))
            .await
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
    pub async fn task_cancel_by(
        &self,
        name: &str,
        by: &str,
    ) -> Result<theseus_protocol::TaskCancelResult> {
        let tasks = self.kernel.tasks(None)?;
        let id = crate::task::resolve(&tasks, name)
            .map_err(crate::task::NoSuchTask)?
            .id
            .clone();
        let (e, cancelled, verdicts) = self.cancel_execution_judged(&id, by).await?;
        let rec: Option<SessionRecord> = self.store.get_session(&e.session_id)?;
        Ok(theseus_protocol::TaskCancelResult {
            task: theseus_protocol::TaskInfo {
                arrangement: crate::task::arrangement_of(&self.store, rec.as_ref()),
                ..crate::task::info(&e, rec.as_ref(), 0)
            },
            cancelled_actions: cancelled,
            verdicts,
        })
    }

    /// Run one turn for a client, streaming its events to that connection.
    /// What the owner named is their choice, never routed (25e); the pane's
    /// carried profile is not.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub(super) async fn turn_submit(
        &self,
        p: TurnSubmitParams,
        conn: Conn<'_>,
    ) -> Result<theseus_protocol::TurnSubmitResult, RpcFailure> {
        if p.prompt.is_some() && !(p.input.trim().is_empty() && p.attachments.is_empty()) {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "a prompt is the turn's whole input: send no input or attachments with it",
            ));
        }
        if p.prompt.is_none() && p.input.trim().is_empty() && p.attachments.is_empty() {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "input is empty",
            ));
        }
        // An imported session takes no turn (theseus-0lrr.6).
        if let Some(why) = p.session_id.as_deref().and_then(crate::import::refusal) {
            return Err(RpcFailure::new(error_code::REFUSED, why));
        }
        // An MCP server's prompt (36c): asked first, so a refusal (a shared
        // place, a missing argument, a server that is down) leaves no
        // session and no node behind.
        let prompt = match &p.prompt {
            Some(r) => Some(self.resolve_prompt(r, p.session_id.as_deref()).await?),
            None => None,
        };
        // A turn from a holding session's job holds what that one holds, in
        // the session it opens or the one it names (theseus-b5cl).
        let session = match &p.session_id {
            Some(id) => {
                self.take_from_job(id, p.opened_from.as_deref())?;
                self.session(id)?
            }
            None => self.open_session(SessionOpenParams {
                opened_from: p.opened_from.clone(),
                ..SessionOpenParams::default()
            })?,
        };
        // A place's profile, unless the turn names one (step 38a).
        let live = self.place_profile(&session.session_id, self.live_profile().0);
        // The pane carries the profile the last turn ran on: when that is
        // only where routing moved the session, it stands for the base the
        // move was made from (`Routed.from`, theseus-0j2.17), never a new
        // one, so a `-P` pane's profile runs once routing no longer acts; a
        // record without it names nothing (theseus-9yyr).
        let routed = session.routed.as_ref();
        let moved = routed.and_then(|r| r.profile.as_deref());
        let only_routed =
            p.carried && p.provider.is_none() && p.model.is_none() && p.profile.as_deref() == moved;
        let named = match only_routed {
            // A base no longer configured names nothing.
            true => routed
                .and_then(|r| r.from.as_deref())
                .filter(|f| self.cfg.all_profiles().contains_key(*f)),
            false => p.profile.as_deref(),
        };
        let mut target = self
            .runner
            .resolve_target(&live, named, p.provider.as_deref(), p.model.as_deref())
            .map_err(RpcFailure::invalid)?;
        target.chosen = crate::routing::chosen(&p);
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
                input: Some(prompt.as_ref().map_or(p.input, |pi| pi.text())),
                target,
                sink,
                author: prompt.as_ref().map_or_else(
                    || p.author.clone().unwrap_or_else(|| conn.client.to_string()),
                    |pi| pi.author.clone(),
                ),
                recompile: None,
                attachments: p.attachments,
                arrived: Some(conn.arrived),
                reply_to: p.reply_to,
                prompt,
            })
            .await;
        match result {
            Ok(r) => {
                self.telemetry().record_turn(&r);
                self.telemetry()
                    .record_node_cache(&self.store.node_cache().health());
                Ok(r)
            }
            Err(e) => Err(match e.downcast::<TurnError>() {
                Ok(te) => {
                    self.count_failed_turn(&t_profile, &t_provider, &t_model, &te);
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

    /// A turn that failed with a classified error, on the target it ran on,
    /// whoever ran it: a client's `turn.submit`, or the driver's continuation
    /// (theseus-yf1: until then a continuation's failure was counted
    /// nowhere). Health's count, and telemetry's turns, provider errors, and
    /// the calls its trace holds.
    pub(crate) fn count_failed_turn(
        &self,
        profile: &str,
        provider: &str,
        model: &str,
        te: &TurnError,
    ) {
        self.provider_errors.fetch_add(1, Ordering::Relaxed);
        self.telemetry()
            .record_failure(&crate::telemetry::FailedTurn {
                profile,
                provider,
                model,
                class: &te.class,
                transient: te.transient,
                elapsed_ms: te.elapsed_ms,
                trace: te.trace.as_ref(),
                usage: &te.usage,
                cost_usd: te.cost_usd,
            });
    }

    /// A session's record, or `NOT_FOUND`.
    pub(super) fn session(&self, id: &str) -> Result<SessionRecord, RpcFailure> {
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
            .switch_profile(name, &conn.actor(None))
            .map_err(RpcFailure::invalid)?;
        let m = Message::from(theseus_protocol::Event::ProfileChanged(changed.clone()));
        conn.tx.notify(&crate::outbound::Note::new(&m), "policy");
        Ok(changed)
    }

    fn switch_profile(&self, name: &str, by: &str) -> Result<ProfileChanged> {
        // The switch and when it was, in one frame: a session routing moved
        // before it runs on the new live profile at its next message
        // (theseus-9yyr).
        let at = theseus_protocol::now_unix_ms();
        let meta = theseus_store::kinds::META;
        self.store.append(&[
            theseus_store::NewRecord::json(meta, Some(META_LIVE_PROFILE), &name.to_string())?,
            theseus_store::NewRecord::json(meta, Some(crate::turn::SWITCHED), &at)?,
        ])?;
        self.runner.live_switched.moved(at);
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
            LedgerKind::ProfileChanged,
            None,
            None,
            serde_json::to_value(&changed)?,
        ))?;
        Ok(changed)
    }

    pub(crate) fn session_history(
        &self,
        p: theseus_protocol::SessionHistoryParams,
    ) -> Result<theseus_protocol::SessionHistoryResult, RpcFailure> {
        let rec = self.session(&p.session_id)?;
        let pending = self.kernel.pending_confirms()?;
        let mine: Vec<theseus_kernel::Action> = pending
            .iter()
            .filter(|a| a.session_id == p.session_id)
            .cloned()
            .collect();
        // A page, through the session's tag (theseus-96w2, theseus-xo0m).
        // A question waiting in it reads its card's gate record on its
        // call's node, wherever that is: the whole session when that is
        // what was asked for, else its tool calls back to the call's.
        let page = self.history_page(&p.session_id, p.n, p.after, p.before)?;
        let whole = p.n.is_none() && p.after.is_none() && p.before.is_none();
        let cards = if whole || mine.is_empty() {
            None
        } else {
            Some(self.card_nodes(&p.session_id, &mine)?)
        };
        // An imported node by its newest record, once (soul-import, theseus-0lrr.6): the
        // page's cursors were taken from the page as read (history-pages, theseus-xo0m).
        let mut page = page;
        page.nodes = crate::import::shown(&self.store, std::mem::take(&mut page.nodes))?;
        let known = cards.as_deref().unwrap_or(&page.nodes);
        let asks = self.pending_by_execution(&mine, Some((&p.session_id, known)));
        Ok(theseus_protocol::SessionHistoryResult {
            session: self.session_info(&rec, &asks),
            pending_confirms: self.confirms_of(&pending, &rec, known),
            nodes: page
                .nodes
                .iter()
                .map(|(pos, n)| Self::node_info(*pos, n))
                .collect(),
            next: page.next,
            older: page.older,
        })
    }

    /// `session.watch`: subscribe this connection to a session's events.
    /// A session's watchers get its `execution.changed` as well as the
    /// turn's events, so the first watch of any kind seeds the push, as
    /// `executions.watch` and `session.wait` do (theseus-tq04): before, a
    /// daemon nothing else watched sent a session's watcher no push at all.
    /// Seeded first, so a failed seed leaves no subscription behind.
    pub(super) async fn session_watch(
        self: &std::sync::Arc<Self>,
        p: theseus_protocol::SessionRef,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        self.push.ensure(self).await?;
        self.bus.watch(&p.session_id, conn.client, conn.tx.clone());
        Ok(json!({"watching": p.session_id, "watchers": self.bus.watchers(&p.session_id)}))
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
        self.request_recompile(&p.session_id, strategy, &conn.actor(None))?;
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
            LedgerKind::ContextRecompileRequested,
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

    /// Every model, as the code has it with the config's tables over it,
    /// and each table beside the code's own row (theseus-vwar).
    pub(super) fn catalog_list(&self) -> theseus_protocol::CatalogListResult {
        let profiles = self.cfg.all_profiles();
        let code = crate::catalog::Catalog::builtin();
        let models = self
            .catalog
            .entries
            .iter()
            .map(|(m, e)| {
                let config = self.cfg.catalog.get(m);
                theseus_protocol::CatalogModel {
                    model: m.clone(),
                    entry: serde_json::to_value(e).unwrap_or(Value::Null),
                    profiles: profiles
                        .iter()
                        .filter(|(_, p)| &p.model == m)
                        .map(|(n, _)| n.clone())
                        .collect(),
                    config: config.map(|r| serde_json::to_value(r).unwrap_or(Value::Null)),
                    code: config
                        .and(code.get(m))
                        .map(|c| serde_json::to_value(c).unwrap_or(Value::Null)),
                }
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
        // A kind or a session reads its tag's newest `n` (theseus-96w2);
        // while the index's shape is built, as before: the session's every
        // node, or ten times `n` of every session's, filtered here.
        let filtered = p.kind.is_some() || p.session_id.is_some();
        let paged = if filtered {
            self.nodes_paged(p.kind.as_deref(), p.session_id.as_deref(), n)?
        } else {
            None
        };
        let nodes: Vec<(u64, crate::node::Node)> = match paged {
            Some(newest_first) => newest_first,
            None => {
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
                nodes.drain(..skip);
                nodes.reverse();
                nodes
            }
        };
        let nodes = crate::import::shown(&self.store, nodes)?;
        Ok(theseus_protocol::NodeListResult {
            nodes: nodes
                .iter()
                .map(|(pos, node)| Self::node_info(*pos, node))
                .collect(),
            total: self.store.node_count()?,
        })
    }

    /// `node.reach` (theseus-n4m, step 12a): see `reach.rs`. A node is named
    /// by its whole id, or by its id's end when one node's ends so
    /// (theseus-glyw, `reach::resolve`).
    pub(super) fn node_reach(
        &self,
        p: theseus_protocol::NodeReachParams,
    ) -> Result<theseus_protocol::NodeReachResult, RpcFailure> {
        use crate::reach::Named;
        if let Some(r) = crate::reach::reach(&self.store, &p.node_id, p.max_generations)? {
            return Ok(r);
        }
        let id = match crate::reach::resolve(&self.store, &p.node_id)? {
            Named::One(id) => id,
            Named::Unknown(why) => return Err(RpcFailure::new(error_code::NOT_FOUND, why)),
            Named::Refused(why) => return Err(RpcFailure::new(error_code::INVALID_PARAMS, why)),
        };
        crate::reach::reach(&self.store, &id, p.max_generations)?
            .ok_or_else(|| RpcFailure::new(error_code::NOT_FOUND, format!("no node {id:?}")))
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
        // The built-ins, then the MCP servers' tools (M7 36b).
        let mut all: Vec<Arc<dyn theseus_tools::Tool>> =
            self.tools.registry.all().cloned().collect();
        all.extend(
            self.tools
                .mcp
                .all()
                .iter()
                .map(|t| t.clone() as Arc<dyn theseus_tools::Tool>),
        );
        let tools = all
            .iter()
            .map(|t| {
                let now = self.tools.posture_now(t.name());
                theseus_protocol::ToolInfo {
                    name: t.name().into(),
                    wire_name: t.wire_name(),
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

    /// Every execution, with its attention (theseus-in3): a task's parent
    /// session is its parent execution's.
    pub(crate) fn execution_list(
        &self,
    ) -> Result<theseus_protocol::ExecutionListResult, RpcFailure> {
        let execs = self.kernel.executions()?;
        let pending = self.pending_by_execution(&self.kernel.pending_confirms()?, None);
        let session_of: std::collections::HashMap<&str, &str> = execs
            .iter()
            .map(|e| (e.id.as_str(), e.session_id.as_str()))
            .collect();
        Ok(theseus_protocol::ExecutionListResult {
            executions: execs
                .iter()
                .map(|e| {
                    let parent = e
                        .parent
                        .as_deref()
                        .and_then(|p| session_of.get(p))
                        .map(|s| s.to_string());
                    let asks = pending.get(&e.id).cloned().unwrap_or_default();
                    let mut info = Self::execution_info(e);
                    info.attention =
                        Some(crate::push::view(e, asks, parent, 0, e.updated_at_ms).attention);
                    info
                })
                .collect(),
        })
    }

    pub(super) fn action_list(
        &self,
        p: theseus_protocol::ActionListParams,
    ) -> Result<theseus_protocol::ActionListResult, RpcFailure> {
        let n = p.n.unwrap_or(200).min(2000);
        // The newest `n` through the index's births, or an execution's
        // through its tag (theseus-96w2); while the index's shape is built,
        // every action, as before.
        let (actions, total) = match self.actions_paged(p.execution_id.as_deref(), n)? {
            Some(page) => page,
            None => {
                let mut actions = self.kernel.actions()?;
                let total = actions.len() as u64;
                if let Some(x) = &p.execution_id {
                    actions.retain(|a| &a.execution_id == x);
                }
                actions.sort_by_key(|a| std::cmp::Reverse(a.planned_at_ms));
                actions.truncate(n);
                (actions, total)
            }
        };
        use theseus_store::Store as _;
        let info = |a: &theseus_kernel::Action| theseus_protocol::ActionInfo {
            // A job's egress, from its completion (18c).
            egress: (a.tool == crate::sandbox::PROC_RUN)
                .then(|| {
                    self.store
                        .inner()
                        .as_ref()
                        .latest_by_key(theseus_store::kinds::COMPLETION, &a.correlation_id)
                })
                .and_then(|r| r.ok().flatten())
                .and_then(|r| r.decode::<theseus_kernel::Completion>().ok())
                .and_then(|c| c.detail?.get("egress").cloned()),
            ..Self::action_info(a)
        };
        Ok(theseus_protocol::ActionListResult {
            actions: actions.iter().map(info).collect(),
            total,
        })
    }

    pub(super) async fn execution_cancel(
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
        let (e, cancelled, verdicts) = self
            .cancel_execution_judged(&p.execution_id, &conn.actor(p.author.as_deref()))
            .await?;
        Ok(theseus_protocol::ExecutionCancelResult {
            execution: Self::execution_info(&e),
            cancelled_actions: cancelled,
            verdicts,
        })
    }

    /// `execution.stop` (W1): `/stop`, which halts the work and keeps the
    /// conversation. A task is refused: `task.cancel` stops it.
    pub(super) async fn execution_stop(
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
        match self.stop_execution(&p.execution_id, &by).await {
            Ok(r) => Ok(r),
            Err(e) => match e.downcast_ref::<theseus_kernel::KernelError>() {
                Some(theseus_kernel::KernelError::StopTask { .. }) => {
                    Err(RpcFailure::new(error_code::REFUSED, e.to_string()))
                }
                _ => Err(e.into()),
            },
        }
    }

    /// The newest `n` rows, or with `after` the first `n` after it (theseus-xo0m).
    /// A filter scans up to 50 rows for each one asked: the newest that many,
    /// or the next that many after `after`, whose last position is `next`.
    /// `ledger.tail`: one page through the store's index (theseus-vm3n.5):
    /// a kind or session filter reads its tag's postings, a window its time
    /// index, and a cursor its bound, so the read costs about its answer,
    /// never the history before it. The page and `total` are one snapshot
    /// (theseus-tphr).
    pub(super) fn ledger_tail(&self, p: LedgerTailParams) -> Result<LedgerTailResult, RpcFailure> {
        let n = p.n.unwrap_or(20).min(1000);
        let page = theseus_store::Page {
            kind: theseus_store::kinds::LEDGER,
            tags: ledger_tags(p.kind.as_deref(), p.session_id.as_deref()),
            after: p.after,
            before: p.before,
            since_ms: p.since_ms,
            until_ms: p.until_ms,
            limit: n,
        };
        let Some(out) = self.store.ledger_page(&page)? else {
            return self.ledger_tail_scanned(&p, n);
        };
        let rows = out
            .records
            .iter()
            .map(|r| {
                let row: crate::ledger::LedgerRow = r.decode()?;
                Ok(LedgerEntry {
                    position: r.position,
                    at_unix_ms: row.at_unix_ms,
                    kind: row.kind,
                    session_id: row.session_id,
                    turn_id: row.turn_id,
                    data: row.data,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(LedgerTailResult {
            rows,
            total: out.count,
            next: p.after.and(out.more.then_some(out.last).flatten()),
            older: p.before.and(out.more.then_some(out.first).flatten()),
        })
    }

    /// `ledger.tail` while the index's shape is built after serving (an
    /// older build wrote the index last, theseus-vm3n.5): today's read, a
    /// window of `n × 50` rows filtered here, with `before` and the times
    /// applied to it too. Its `total` is a second read.
    fn ledger_tail_scanned(
        &self,
        p: &LedgerTailParams,
        n: usize,
    ) -> Result<LedgerTailResult, RpcFailure> {
        let filtered = p.kind.is_some()
            || p.session_id.is_some()
            || p.since_ms.is_some()
            || p.until_ms.is_some();
        let scan = if filtered { n * 50 } else { n };
        let read: Vec<(u64, crate::ledger::LedgerRow)> = match p.after {
            Some(after) => self.store.ledger_after(after, scan)?,
            None => self.store.ledger_tail(scan)?,
        };
        let last_read = read.last().map(|(position, _)| *position);
        let window_full = read.len() == scan;
        let rows: Vec<LedgerEntry> = read
            .into_iter()
            .filter(|(_, r)| p.kind.as_deref().is_none_or(|k| r.is_kind(k)))
            .filter(|(_, r)| {
                p.session_id
                    .as_deref()
                    .is_none_or(|s| r.session_id.as_deref() == Some(s))
            })
            .filter(|(position, r)| {
                p.before.is_none_or(|b| *position < b)
                    && p.since_ms.is_none_or(|t| r.at_unix_ms >= t)
                    && p.until_ms.is_none_or(|t| r.at_unix_ms <= t)
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
        let (rows, next) = match p.after {
            Some(_) if rows.len() >= n && n > 0 => {
                let rows = rows[..n].to_vec();
                let next = rows.last().map(|r| r.position);
                (rows, next)
            }
            Some(_) => (rows, if window_full { last_read } else { None }),
            None => (rows[rows.len().saturating_sub(n)..].to_vec(), None),
        };
        Ok(LedgerTailResult {
            rows,
            total: self.store.ledger_len()?,
            next,
            older: None,
        })
    }

    /// `bench.history`: the gates' bench CSV, read off the serving workers.
    /// Its param is optional: no params reads the newest 500 runs.
    pub(super) async fn bench_history(
        &self,
        params: Value,
    ) -> Result<theseus_protocol::bench::BenchHistoryResult, RpcFailure> {
        let params = if params.is_null() { json!({}) } else { params };
        let p: theseus_protocol::bench::BenchHistoryParams = serde_json::from_value(params)
            .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e.to_string()))?;
        tokio::task::spawn_blocking(move || {
            crate::bench::read(crate::bench::path().as_deref(), p.last)
        })
        .await
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))
    }

    /// `sandbox.usage`: the L1 jobs running now. A daemon whose tools are
    /// off runs none.
    pub(super) fn sandbox_usage(&self) -> theseus_protocol::sandbox::SandboxUsage {
        self.tools.sandbox.usage()
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
        self.stop_record(Value::Null);
        json!({"ok": true})
    }

    /// A stop no client asked for (theseus-bv5): SIGINT, or SIGTERM, which is
    /// systemd's stop and `kill`'s default. The same work as a client's
    /// `shutdown` before its answer, the row naming the signal, so the next
    /// start replays nothing.
    pub fn stopping_on(&self, signal: &str) {
        self.stop_record(json!({ "signal": signal }));
    }

    /// The stop's row, then the checkpoint. From here no post is dispatched,
    /// and the posts already sent have until the stop's grace ends to settle
    /// (`Core::finish_stop`, theseus-pfv). The checkpoint syncs nothing of
    /// its own: redb's close, as the store drops, makes it durable in its
    /// own commit (theseus-02k), and a kill before that only lengthens the
    /// next start's replay by the stop's own frames.
    fn stop_record(&self, data: Value) {
        crate::startup::stop_began();
        self.outbox.stop_sending();
        // The MCP servers get SIGTERM, never waited for (M7 36b).
        self.mcp.stop();
        let _ = self.store.append_ledger(&LedgerRow::new(
            LedgerKind::ServerStopping,
            None,
            None,
            data,
        ));
        let _ = self.store.inner().checkpoint_for_close();
        crate::startup::stop_phase("row and checkpoint");
    }
}

/// The ledger's tags a `ledger.tail` filter reads (`theseus_store::pages`):
/// its kind under each of its names (a renamed kind reads the rows stored
/// under its old one too), in its session when it names one; the session's
/// alone without a kind; none without either.
pub(crate) fn ledger_tags(kind: Option<&str>, session: Option<&str>) -> Vec<String> {
    use theseus_store::pages::{ledger_kind, ledger_kind_session, ledger_session};
    let Some(kind) = kind else {
        return session.map(ledger_session).into_iter().collect();
    };
    let mut names = vec![kind.to_string()];
    for &(now, before) in theseus_protocol::LedgerKind::RENAMED {
        if kind == now.as_str() {
            names.push(before.to_string());
        } else if kind == before {
            names.push(now.as_str().to_string());
        }
    }
    names
        .iter()
        .map(|k| match session {
            Some(s) => ledger_kind_session(k, s),
            None => ledger_kind(k),
        })
        .collect()
}
