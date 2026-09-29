//! The protocol server (spec §3.18): JSON-RPC 2.0 over newline-delimited
//! JSON on any `AsyncRead + AsyncWrite` pair (stdio, a Unix socket). One
//! task per connection; notifications for a connection flow through its own
//! channel so a streaming turn never blocks another client.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use theseus_protocol::{
    error_code, method, notify, HealthResult, Id, LedgerEntry, LedgerTailParams, LedgerTailResult,
    Message, ProfileChanged, ProfileInfo, ProfileListResult, ProfileUseParams, ProviderErrorData,
    Request, Response, SessionKind, SessionListResult, SessionOpenParams, TurnSubmitParams, Usage,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::bus::{EventSink, SessionBus};
use crate::catalog::Catalog;
use crate::compiler::Recompile;
use crate::ledger::LedgerRow;
use crate::narrative::{narrate, Narrator};
use crate::node::{Body, Node};
use crate::provider::{Anthropic, Provider};
use crate::scrub::Scrubber;
use crate::secrets::Secrets;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::toolrun::{InlineLauncher, JobLauncher, ToolRuntime, WrapperLauncher};
use crate::turn::{TurnError, TurnRequest, TurnRunner, OPERATOR};
use crate::Config;
use theseus_kernel::job::WrapperEvidence;
use theseus_kernel::{Authority, Execution, Kernel, Spool};

pub struct Core {
    pub cfg: Arc<Config>,
    pub store: Store,
    /// The durable kernel (M2), sharing the store's WAL and index.
    pub kernel: Arc<Kernel>,
    /// Completion spool beside the store (`<state>/spool`).
    pub spool: Spool,
    /// Woken when a turn ends or an execution changes (admission waiters).
    pub admission: Arc<tokio::sync::Notify>,
    /// The last startup report, as JSON, for health.
    pub startup_report: Value,
    pub catalog: Arc<Catalog>,
    pub bus: Arc<SessionBus>,
    pub tools: Arc<ToolRuntime>,
    pub runner: TurnRunner,
    pub secret_names: Vec<String>,
    pub telemetry: Arc<crate::telemetry::Telemetry>,
    /// The narrative (`narrative = true`): live lines and a bounded tail.
    pub narrator: Arc<Narrator>,
    started: Instant,
    turns: AtomicU64,
    provider_errors: AtomicU64,
    /// The live profile and where it came from ("config" | "runtime").
    live: std::sync::RwLock<(String, String)>,
    pub shutdown: tokio::sync::Notify,
    /// Channel bindings report here (by kind) and health shows them.
    bindings: std::sync::RwLock<BTreeMap<String, theseus_protocol::BindingStatus>>,
    /// Bindings still starting. The continuation driver waits (bounded) until
    /// this is zero, so a turn the kernel resumes at startup is watched by its
    /// channel from its first event.
    bindings_pending: AtomicU64,
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
        let scrubber = Arc::new(Scrubber::from_secrets(&secrets));
        // Wrappers run this very image: after an in-place upgrade (copy, then
        // rename over the old file) the path on disk is a newer binary, or
        // `current_exe()` names a deleted file; `/proc/self/exe` is still us.
        let self_exe = match std::path::Path::new("/proc/self/exe") {
            p if p.exists() => p.to_path_buf(),
            _ => std::env::current_exe()
                .context("locating the theseusd binary for the job wrapper")?,
        };
        let launcher: Arc<dyn JobLauncher> = Arc::new(WrapperLauncher { self_exe });
        Self::build(
            cfg,
            providers,
            store,
            secrets.names(),
            telemetry,
            scrubber,
            launcher,
        )
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
        Self::build(
            cfg,
            providers,
            store,
            secret_names,
            telemetry,
            Arc::new(Scrubber::default()),
            Arc::new(InlineLauncher),
        )
    }

    pub fn build(
        cfg: Config,
        providers: BTreeMap<String, Arc<dyn Provider>>,
        store: Store,
        secret_names: Vec<String>,
        telemetry: crate::telemetry::Telemetry,
        scrubber: Arc<Scrubber>,
        launcher: Arc<dyn JobLauncher>,
    ) -> Result<Arc<Self>> {
        let cfg = Arc::new(cfg);
        // The kernel shares the store. Its spool sits beside the store dir:
        // `store` → `spool`, `store-stdio` → `spool-stdio`.
        let spool_dir = {
            let name = store
                .dir()
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "store".into());
            store
                .dir()
                .with_file_name(name.replacen("store", "spool", 1))
        };
        let spool = Spool::open(&spool_dir).context("opening completion spool")?;
        let kernel = Arc::new(Kernel::new(
            store.shared(),
            Arc::new(theseus_kernel::RealClock),
            cfg.kernel.to_kernel_config(),
        ));
        let startup = kernel
            .startup(
                Some(&spool),
                &WrapperEvidence {
                    spool: spool.clone(),
                },
            )
            .context("kernel startup")?;
        for st in &startup.steps {
            tracing::info!(step = st.step, name = %st.name, us = st.elapsed_us, "kernel startup step");
        }
        tracing::info!(
            requeued_interrupted = startup.requeued_interrupted.len(),
            spool_drained = startup.spool_drained,
            spool_malformed = startup.spool_quarantined,
            woke_due = startup.reconcile.woke_due.len(),
            marked_unknown = startup.reconcile.marked_unknown.len(),
            settled_from_evidence = startup.reconcile.settled_from_evidence.len(),
            total_us = startup.elapsed_us,
            spool = %spool_dir.display(),
            "kernel accepting events"
        );
        let admission = Arc::new(tokio::sync::Notify::new());
        let catalog = Arc::new(Catalog::with_overrides(&cfg.catalog));
        for (name, p) in cfg.all_profiles() {
            if catalog.get(&p.model).is_none() {
                tracing::warn!(profile = %name, model = %p.model, "model is not in the catalog: it runs, but cost is unknown and limits are defaults");
            }
        }
        let bus = Arc::new(SessionBus::default());
        let narrator = Arc::new(Narrator::new(cfg.narrative));
        let tools = Arc::new(crate::toolrun::build_runtime(
            &cfg,
            Some(spool.clone()),
            scrubber,
            launcher,
        )?);
        tracing::info!(
            tools = tools.registry.len(),
            roots = ?tools.ctx.roots,
            enforcement = tools.policy.enforcement.as_str(),
            overrides = ?tools.policy.tools,
            mcp = ?tools.policy.mcp,
            floor = ?tools.policy.floor_paths,
            catalog = %catalog.version,
            "tools and catalog"
        );
        let runner = TurnRunner {
            cfg: cfg.clone(),
            providers,
            store: store.clone(),
            kernel: kernel.clone(),
            admission: admission.clone(),
            catalog: catalog.clone(),
            tools: tools.clone(),
            bus: bus.clone(),
            narrator: narrator.clone(),
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
            store,
            kernel,
            spool,
            admission,
            startup_report: serde_json::to_value(&startup).unwrap_or(Value::Null),
            catalog,
            bus,
            tools,
            runner,
            secret_names,
            telemetry: Arc::new(telemetry),
            narrator,
            started: Instant::now(),
            turns: AtomicU64::new(0),
            provider_errors: AtomicU64::new(0),
            live: std::sync::RwLock::new(live),
            shutdown: tokio::sync::Notify::new(),
            bindings: std::sync::RwLock::new(BTreeMap::new()),
            bindings_pending: AtomicU64::new(0),
        });
        core.store.append_ledger(&LedgerRow::new(
            "server.started",
            None,
            None,
            json!({"startup": core.startup_report}),
        ))?;
        Ok(core)
    }

    /// The kernel's view for health and `execution.list`.
    pub fn kernel_status(&self) -> theseus_protocol::KernelStatus {
        let st = self.kernel.stats().unwrap_or_default();
        theseus_protocol::KernelStatus {
            accepting: st.accepting,
            admission_ceiling: st.admission_ceiling,
            turns_held: st.turns_held,
            executions_by_state: st.executions_by_state,
            actions_by_state: st.actions_by_state,
            quarantined_completions: st.quarantined_completions,
            startup: self.startup_report.clone(),
        }
    }

    pub fn execution_info(e: &Execution) -> theseus_protocol::ExecutionInfo {
        theseus_protocol::ExecutionInfo {
            execution_id: e.id.clone(),
            session_id: e.session_id.clone(),
            kind: e.kind.as_str().into(),
            state: e.state.as_str().into(),
            turns: e.turns,
            interrupted: e.interrupted,
            outstanding: e.outstanding.len() as u32,
            queued_results: e.queued_results.len() as u32,
            budget: theseus_protocol::BudgetInfo {
                limit: e.budget.limit,
                spent: e.budget.spent,
                reserved: e.budget.reserved,
                held_unknown: e.budget.held_unknown,
                available: e.budget.available(),
            },
            wake: serde_json::to_value(&e.wake).unwrap_or(Value::Null),
            reports_to: e.reports_to.clone(),
            ended_reason: e.ended_reason.clone(),
            created_at_ms: e.created_at_ms,
            updated_at_ms: e.updated_at_ms,
        }
    }

    pub fn action_info(a: &theseus_kernel::Action) -> theseus_protocol::ActionInfo {
        theseus_protocol::ActionInfo {
            correlation_id: a.correlation_id.clone(),
            execution_id: a.execution_id.clone(),
            session_id: a.session_id.clone(),
            tool: a.tool.clone(),
            state: a.state.as_str().into(),
            retry_class: match &a.retry_class {
                theseus_kernel::RetryClass::SafeToRepeat => "safe_to_repeat".into(),
                theseus_kernel::RetryClass::NonRepeatable => "non_repeatable".into(),
            },
            planned_at_ms: a.planned_at_ms,
            authorized_at_ms: a.authorized_at_ms,
            dispatched_at_ms: a.dispatched_at_ms,
            settled_at_ms: a.settled_at_ms,
            deadline_at_ms: a.deadline_at_ms,
            reserved_units: a.reserved_units,
            confirmed: a.confirm.is_some(),
            cancel: a
                .cancel
                .and_then(|c| serde_json::to_value(c).ok())
                .and_then(|v| v.as_str().map(str::to_string)),
            external_op_id: a.external_op_id.clone(),
            result_ref: a.result_ref.clone(),
            resolution: a.resolution.clone(),
            completions_seen: a.completions_seen,
        }
    }

    /// Heartbeat: drain the spool, reconcile against the wrapper evidence.
    /// Called by the harness loop on its timer and when a wrapper pokes the
    /// notify socket.
    pub fn heartbeat(&self, why: &str) {
        let t0 = Instant::now();
        let drained = self.drain_spool();
        let ev = WrapperEvidence {
            spool: self.spool.clone(),
        };
        match self.kernel.reconcile(&ev) {
            Ok(rep) => {
                let changed = !rep.woke_due.is_empty()
                    || !rep.settled_from_evidence.is_empty()
                    || !rep.marked_unknown.is_empty()
                    || !rep.resolved_unknown.is_empty()
                    || drained > 0;
                if changed {
                    tracing::info!(
                        why,
                        drained,
                        woke_due = rep.woke_due.len(),
                        settled_from_evidence = rep.settled_from_evidence.len(),
                        marked_unknown = rep.marked_unknown.len(),
                        resolved_unknown = rep.resolved_unknown.len(),
                        open_actions = rep.open_actions,
                        open_executions = rep.open_executions,
                        us = t0.elapsed().as_micros() as u64,
                        "heartbeat"
                    );
                    let (due, evidence, unknown) = (
                        rep.woke_due.len() as u64,
                        rep.settled_from_evidence.len() as u64,
                        rep.marked_unknown.len() as u64,
                    );
                    if self.narrator.on() && due + evidence + unknown > 0 {
                        narrate!(
                            self.narrator,
                            Job,
                            None,
                            None,
                            "Heartbeat ({why}): {} woke because a wait came due, {} \
                             settled from a job wrapper's evidence, {} marked unknown.",
                            crate::narrative::count(due, "execution", "executions"),
                            crate::narrative::count(evidence, "action", "actions"),
                            unknown
                        );
                    }
                    self.admission.notify_waiters();
                } else {
                    tracing::debug!(
                        why,
                        open_actions = rep.open_actions,
                        open_executions = rep.open_executions,
                        us = t0.elapsed().as_micros() as u64,
                        "heartbeat: nothing to do"
                    );
                }
            }
            Err(e) => tracing::warn!(error = %e, "reconcile failed"),
        }
    }

    /// The narrative's line for a job's completion that came from the spool.
    fn narrate_spooled(&self, c: &theseus_kernel::Completion) {
        let Ok(Some(a)) = self.kernel.action(&c.correlation_id) else {
            return;
        };
        let outcome = match c.outcome {
            theseus_kernel::Outcome::Succeeded => "succeeded",
            theseus_kernel::Outcome::Failed => "failed",
            theseus_kernel::Outcome::Unknown => "an unknown outcome",
        };
        let exit = c
            .detail
            .as_ref()
            .and_then(|d| d.get("exit_code"))
            .and_then(Value::as_i64)
            .map(|x| format!(", exit code {x}"))
            .unwrap_or_default();
        narrate!(
            self.narrator,
            Job,
            Some(&a.session_id),
            None,
            "Job {} ({}) finished: {outcome}{exit}; its completion came \
             from the spool.",
            crate::narrative::short(&c.correlation_id),
            a.tool
        );
    }

    /// Accept every spooled completion, removing each file after its frame.
    pub fn drain_spool(&self) -> u32 {
        let mut n = 0;
        match self.spool.drain() {
            Ok(d) => {
                if d.malformed > 0 {
                    tracing::warn!(
                        malformed = d.malformed,
                        "spool: unparseable completions moved to malformed/"
                    );
                }
                for (path, c) in d.completions {
                    match self.kernel.accept_completion(&c) {
                        Ok(acc) => {
                            tracing::info!(correlation_id = %c.correlation_id, producer = %c.producer, result = ?acc, "completion accepted from spool");
                            if self.narrator.on() {
                                self.narrate_spooled(&c);
                            }
                            if let Err(e) = self.spool.remove(&path) {
                                tracing::warn!(error = %e, "spool remove failed");
                            }
                            n += 1;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, correlation_id = %c.correlation_id, "completion refused")
                        }
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "spool drain failed"),
        }
        n
    }

    /// `/cancel <execution>`: deterministic control path. Terminates wrapper
    /// processes the spool knows about and walks each action's cancel lifecycle.
    pub fn cancel_execution(&self, id: &str, by: &str) -> Result<(Execution, Vec<String>)> {
        let to_kill = self.kernel.cancel_execution(id, by)?;
        for corr in &to_kill {
            match self.spool.read_pid(corr) {
                Some(pid) => {
                    let _ = self.kernel.cancel_acknowledged(corr);
                    if theseus_kernel::job::terminate(pid, Duration::from_secs(2)) {
                        let _ = self.kernel.cancel_verified(corr);
                    } else {
                        let _ = self.kernel.cancel_uncertain(corr);
                    }
                }
                None => {
                    // In-process or already gone: nothing to reach.
                    let _ = self.kernel.cancel_unsupported(corr);
                }
            }
        }
        self.admission.notify_waiters();
        let e = self
            .kernel
            .execution(id)?
            .ok_or_else(|| anyhow::anyhow!("execution {id} vanished"))?;
        narrate!(
            self.narrator,
            Session,
            Some(&e.session_id),
            None,
            "Execution {} cancelled by {by}: {} stopped; it is {} now.",
            crate::narrative::short(id),
            crate::narrative::count(to_kill.len() as u64, "action", "actions"),
            e.state.as_str()
        );
        Ok((e, to_kill))
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
                max_output_tokens: p.effective_max_tokens(&self.catalog),
                has_system: p.system.is_some(),
                provider: p.provider,
                model: p.model,
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
            turns: self.turns_total(),
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
            kernel: self.kernel_status(),
            cost_usd_total: self.cost_total(),
            catalog_version: self.catalog.version.clone(),
            bindings: self.bindings.read().unwrap().values().cloned().collect(),
            narrative: self.narrator.on(),
        }
    }

    /// A binding is about to start: continuations wait for it (see `wait_for_bindings`).
    pub fn expect_binding(&self) {
        self.bindings_pending.fetch_add(1, Ordering::SeqCst);
    }

    /// A binding is watching its sessions (or gave up): continuations may run.
    pub fn binding_started(&self) {
        let _ = self
            .bindings_pending
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
    }

    /// Wait until every expected binding has started, at most `max`. True when
    /// they all did; false on timeout (the caller goes ahead anyway).
    pub async fn wait_for_bindings(&self, max: Duration) -> bool {
        let deadline = Instant::now() + max;
        while self.bindings_pending.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        true
    }

    /// A channel binding reports its state; health shows the latest report.
    pub fn set_binding_status(&self, status: theseus_protocol::BindingStatus) {
        self.bindings
            .write()
            .unwrap()
            .insert(status.kind.clone(), status);
    }

    /// A ledger row written on behalf of a binding (`discord.*`), so its traffic
    /// sits in the same readable history as everything else.
    pub fn binding_ledger(&self, kind: &str, session_id: Option<&str>, data: Value) {
        if let Err(e) = self
            .store
            .append_ledger(&LedgerRow::new(kind, session_id, None, data))
        {
            tracing::warn!(error = %e, kind, "binding ledger append failed");
        }
    }

    /// Turns across every session, from the store (survives restarts).
    fn turns_total(&self) -> u64 {
        self.store
            .list_sessions::<SessionRecord>()
            .map(|v| v.iter().map(|s| s.turns).sum())
            .unwrap_or_else(|_| self.turns.load(Ordering::Relaxed))
    }

    fn cost_total(&self) -> f64 {
        self.store
            .list_sessions::<SessionRecord>()
            .map(|v| v.iter().map(|s| s.cost_usd).sum())
            .unwrap_or(0.0)
    }

    pub fn node_info(position: u64, n: &Node) -> theseus_protocol::NodeInfo {
        let (text, thinking, detail, bytes) = match &n.body {
            Body::UserMessage { text } => {
                (text.clone(), String::new(), Value::Null, text.len() as u64)
            }
            Body::AssistantMessage {
                blocks,
                model,
                provider,
                stop_reason,
                usage,
                cost_usd,
                request_id,
                correlation_id,
                compilation_id,
                ..
            } => {
                let calls: Vec<Value> = crate::provider::tool_uses_in(blocks)
                    .into_iter()
                    .map(|u| json!({"id": u.id, "name": u.name, "input": u.input}))
                    .collect();
                (
                    crate::provider::text_of(blocks),
                    crate::provider::thinking_of(blocks),
                    json!({"model": model, "provider": provider, "stop_reason": stop_reason, "usage": usage, "cost_usd": cost_usd, "request_id": request_id, "correlation_id": correlation_id, "compilation_id": compilation_id, "tool_calls": calls, "blocks": blocks.len()}),
                    serde_json::to_string(blocks)
                        .map(|s| s.len() as u64)
                        .unwrap_or(0),
                )
            }
            Body::ToolCall {
                tool_use_id,
                tool,
                input,
                correlation_id,
                gate,
                ..
            } => (
                String::new(),
                String::new(),
                json!({"tool_use_id": tool_use_id, "tool": tool, "input": input, "correlation_id": correlation_id, "decision": gate.get("decision"), "result": gate.get("result"), "plan": gate.get("plan")}),
                0,
            ),
            Body::ToolResult {
                tool_use_id,
                tool,
                status,
                is_error,
                content,
                correlation_id,
                bytes_total,
                truncated,
                full_ref,
                duration_ms,
                late,
                meta,
            } => (
                content.clone(),
                String::new(),
                json!({"tool_use_id": tool_use_id, "tool": tool, "status": status.as_str(), "is_error": is_error, "correlation_id": correlation_id, "truncated": truncated, "full_ref": full_ref, "duration_ms": duration_ms, "late": late, "meta": meta}),
                *bytes_total,
            ),
        };
        theseus_protocol::NodeInfo {
            node_id: n.id.clone(),
            kind: n.kind_str().into(),
            session_id: n.session_id.clone(),
            position,
            at_unix_ms: n.created_at_ms,
            turn_id: n.turn_id.clone(),
            loop_index: n.loop_index,
            author: n.author.clone(),
            text,
            thinking,
            detail,
            bytes,
        }
    }

    /// Tool calls in a session still waiting for the operator.
    pub fn pending_confirms(
        &self,
        session_id: &str,
    ) -> Result<Vec<theseus_protocol::ConfirmRequest>> {
        let mut out = Vec::new();
        let nodes = self.store.session_nodes(session_id)?;
        let ttl = self.kernel.config().confirm_ttl_ms;
        for (_, n) in &nodes {
            if let Body::ToolCall {
                tool,
                input,
                correlation_id: Some(c),
                gate,
                ..
            } = &n.body
            {
                if let Some(a) = self.kernel.action(c)? {
                    if a.state == theseus_kernel::ActionState::Planned && a.confirm.is_none() {
                        out.push(theseus_protocol::ConfirmRequest {
                            correlation_id: c.clone(),
                            session_id: session_id.into(),
                            execution_id: a.execution_id.clone(),
                            tool: tool.clone(),
                            input: input.clone(),
                            resource: a.resource.clone(),
                            reason: gate["decision"]["reason"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            by: OPERATOR.into(),
                            requested_at_ms: a.planned_at_ms,
                            expires_at_ms: a.planned_at_ms + ttl,
                            floor: gate["decision"]["floor"].as_bool().unwrap_or(false),
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    /// Every session, the most recently active first.
    pub fn session_list(&self) -> Result<Vec<theseus_protocol::SessionInfo>> {
        let mut recs: Vec<SessionRecord> = self.store.list_sessions()?;
        recs.sort_by(|a, b| {
            b.last_active_ms
                .max(b.created_at_unix_ms)
                .cmp(&a.last_active_ms.max(a.created_at_unix_ms))
        });
        let waiting = self.waiting_confirms();
        Ok(recs
            .iter()
            .map(|r| self.session_info(r, &waiting))
            .collect())
    }

    /// Tool calls waiting for the operator, by execution: planned actions with
    /// no bound confirmation. One scan of the open actions answers every
    /// session of a list.
    fn waiting_confirms(&self) -> BTreeMap<String, u32> {
        let mut by_execution = BTreeMap::new();
        for a in self.kernel.open_actions().unwrap_or_default() {
            if a.state == theseus_kernel::ActionState::Planned
                && a.confirm.is_none()
                && a.tool != crate::turn::PROVIDER_TOOL
            {
                *by_execution.entry(a.execution_id).or_insert(0) += 1;
            }
        }
        by_execution
    }

    fn session_info(
        &self,
        r: &SessionRecord,
        waiting: &BTreeMap<String, u32>,
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

    /// Answer a confirm: bind it (approve) or decline the action, then wake the
    /// execution so the driver resumes the turn exactly where it parked.
    pub fn confirm_action(
        &self,
        correlation_id: &str,
        approve: bool,
        note: Option<&str>,
        by: &str,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let a = self
            .kernel
            .action(correlation_id)?
            .ok_or_else(|| anyhow::anyhow!("no action {correlation_id}"))?;
        if a.state != theseus_kernel::ActionState::Planned {
            anyhow::bail!(
                "action {correlation_id} is {}, not waiting for confirmation",
                a.state.as_str()
            );
        }
        if approve {
            let call = self
                .store
                .session_nodes(&a.session_id)?
                .into_iter()
                .find_map(|(_, n)| match n.body {
                    Body::ToolCall {
                        correlation_id: Some(c),
                        gate,
                        ..
                    } if c == correlation_id => Some(gate),
                    _ => None,
                })
                .ok_or_else(|| anyhow::anyhow!("no tool call node for {correlation_id}"))?;
            let proposal: theseus_kernel::Proposal =
                serde_json::from_value(call["proposal"].clone())
                    .map_err(|e| anyhow::anyhow!("stored proposal unreadable: {e}"))?;
            self.kernel
                .bind_confirm(correlation_id, OPERATOR, &proposal)?;
        } else {
            self.kernel.decline_action(
                correlation_id,
                OPERATOR,
                note.unwrap_or("the operator declined"),
            )?;
        }
        self.store.append_ledger(&LedgerRow::new(
            "action.confirm_answered",
            Some(&a.session_id),
            None,
            json!({"correlation_id": correlation_id, "approved": approve, "note": note, "by": by}),
        ))?;
        narrate!(
            self.narrator,
            Approval,
            Some(&a.session_id),
            None,
            "{} {} by {by}{}.",
            a.tool,
            if approve { "approved" } else { "declined" },
            if note.is_some_and(|n| !n.trim().is_empty()) {
                ", with a note"
            } else {
                ""
            }
        );
        if self
            .kernel
            .wake(
                &a.execution_id,
                if approve { "confirmed" } else { "declined" },
            )
            .is_ok()
        {
            narrate!(
                self.narrator,
                Session,
                Some(&a.session_id),
                None,
                "Woken by the {}; the driver resumes the turn.",
                if approve { "approval" } else { "decline" }
            );
        }
        self.bus.publish(
            &a.session_id,
            &Message::Notification(theseus_protocol::Notification::new(
                notify::CONFIRM_RESOLVED,
                json!({"session_id": a.session_id, "correlation_id": correlation_id, "approved": approve, "by": by}),
            )),
            None,
        );
        self.admission.notify_waiters();
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: correlation_id.into(),
            approved: approve,
            session_id: a.session_id,
            execution_id: a.execution_id,
        })
    }

    /// The harness driver: take a continuation turn for an execution that is
    /// runnable without human input. Returns quickly if it is not ready.
    pub async fn continue_execution(
        self: &Arc<Self>,
        execution_id: &str,
    ) -> Result<Option<theseus_protocol::TurnSubmitResult>> {
        let Some(e) = self.kernel.execution(execution_id)? else {
            return Ok(None);
        };
        let Some(session) = self.store.get_session::<SessionRecord>(&e.session_id)? else {
            return Ok(None);
        };
        narrate!(
            self.narrator,
            Session,
            Some(&session.session_id),
            None,
            "The driver resumes execution {}: {}.",
            crate::narrative::short(&e.id),
            match e.queued_results.len() {
                0 if e.resume_pending => "it was woken".to_string(),
                0 => "it is queued".to_string(),
                n => format!(
                    "{} arrived",
                    crate::narrative::count(n as u64, "result", "results")
                ),
            }
        );
        let (live, _) = self.live_profile();
        let target = self.runner.target_for_session(&session, &live)?;
        let sink = EventSink::new(self.bus.clone(), &session.session_id, None);
        let r = self
            .runner
            .run(TurnRequest {
                session,
                input: None,
                target,
                sink,
                author: "harness".into(),
                recompile: None,
            })
            .await;
        match r {
            Ok(res) => {
                self.telemetry.record_turn(&res);
                Ok(Some(res))
            }
            Err(e) => Err(e),
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

    fn open_session(&self, p: SessionOpenParams, by: &str) -> Result<SessionRecord> {
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
        let _ = by;
        if self.narrator.on() {
            self.narrator.first_sight(&rec.session_id);
            narrate!(
                self.narrator,
                Session,
                Some(&rec.session_id),
                None,
                "Session {} opened ({}); its execution {} has a budget of \
                 {} units.",
                crate::narrative::short(&rec.session_id),
                rec.kind.as_str(),
                crate::narrative::short(&exec.id),
                crate::narrative::thousands(exec.budget.limit)
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

    /// Serve one connection until EOF. `client` names the connection: its
    /// session watches are dropped when it goes away.
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
                let rec = self.open_session(p, client).map_err(bad)?;
                let mut info = rec.info();
                info.execution_state = Some("waiting".into());
                Ok(serde_json::to_value(info).unwrap())
            }
            method::SESSION_LIST => Ok(serde_json::to_value(SessionListResult {
                sessions: self.session_list().map_err(bad)?,
            })
            .unwrap()),
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
                        .open_session(SessionOpenParams::default(), client)
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
                let sink = EventSink::new(
                    self.bus.clone(),
                    &session.session_id,
                    Some((client.to_string(), tx.clone())),
                );
                let result = match self
                    .runner
                    .run(TurnRequest {
                        session,
                        input: Some(p.input),
                        target,
                        sink,
                        author: p.author.clone().unwrap_or_else(|| client.to_string()),
                        recompile: None,
                    })
                    .await
                {
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
                        });
                    }
                };
                self.turns.fetch_add(1, Ordering::Relaxed);
                Ok(serde_json::to_value(result).unwrap())
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
            method::SESSION_HISTORY => {
                let p: theseus_protocol::SessionHistoryParams = parse(req.params)?;
                let rec = self
                    .store
                    .get_session::<SessionRecord>(&p.session_id)
                    .map_err(bad)?
                    .ok_or_else(|| {
                        RpcFailure::new(
                            error_code::NOT_FOUND,
                            format!("no session {}", p.session_id),
                        )
                    })?;
                let nodes = self.store.session_nodes(&p.session_id).map_err(bad)?;
                let skip = p.n.map(|n| nodes.len().saturating_sub(n)).unwrap_or(0);
                Ok(
                    serde_json::to_value(theseus_protocol::SessionHistoryResult {
                        session: self.session_info(&rec, &self.waiting_confirms()),
                        nodes: nodes[skip..]
                            .iter()
                            .map(|(pos, n)| Self::node_info(*pos, n))
                            .collect(),
                        pending_confirms: self.pending_confirms(&p.session_id).map_err(bad)?,
                    })
                    .unwrap(),
                )
            }
            method::SESSION_WATCH => {
                let p: theseus_protocol::SessionRef = parse(req.params)?;
                self.bus.watch(&p.session_id, client, tx.clone());
                Ok(json!({"watching": p.session_id, "watchers": self.bus.watchers(&p.session_id)}))
            }
            method::SESSION_UNWATCH => {
                let p: theseus_protocol::SessionRef = parse(req.params)?;
                self.bus.unwatch(&p.session_id, client);
                Ok(json!({"watching": Value::Null}))
            }
            method::SESSION_RECOMPILE => {
                let p: theseus_protocol::SessionRecompileParams = parse(req.params)?;
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
                let mut rec = self
                    .store
                    .get_session::<SessionRecord>(&p.session_id)
                    .map_err(bad)?
                    .ok_or_else(|| {
                        RpcFailure::new(
                            error_code::NOT_FOUND,
                            format!("no session {}", p.session_id),
                        )
                    })?;
                rec.pending_recompile = Some(strategy);
                self.store.put_session(&rec.session_id, &rec).map_err(bad)?;
                self.store
                    .append_ledger(&LedgerRow::new(
                        "context.recompile_requested",
                        Some(&rec.session_id),
                        None,
                        json!({"strategy": p.strategy, "by": client}),
                    ))
                    .map_err(bad)?;
                Ok(json!({"session_id": rec.session_id, "pending": p.strategy}))
            }
            method::CATALOG_LIST => {
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
                Ok(serde_json::to_value(theseus_protocol::CatalogListResult {
                    version: self.catalog.version.clone(),
                    models,
                })
                .unwrap())
            }
            method::COMPILATION_LIST => {
                let p: theseus_protocol::CompilationListParams = parse(req.params)?;
                let n = p.n.unwrap_or(50).min(500);
                let list = match &p.session_id {
                    Some(sid) => self.store.session_compilations(sid).map_err(bad)?,
                    None => self.store.recent_compilations(n).map_err(bad)?,
                };
                let current: std::collections::HashSet<String> = self
                    .store
                    .list_sessions::<SessionRecord>()
                    .map_err(bad)?
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
                Ok(
                    serde_json::to_value(theseus_protocol::CompilationListResult {
                        compilations: out,
                    })
                    .unwrap(),
                )
            }
            method::NODE_LIST => {
                let p: theseus_protocol::NodeListParams = parse(req.params)?;
                let n = p.n.unwrap_or(100).min(2000);
                let mut nodes = match &p.session_id {
                    Some(sid) => self.store.session_nodes(sid).map_err(bad)?,
                    None => self
                        .store
                        .recent_nodes(if p.kind.is_some() { n * 10 } else { n })
                        .map_err(bad)?,
                };
                if let Some(k) = &p.kind {
                    nodes.retain(|(_, node)| node.kind_str() == k);
                }
                let skip = nodes.len().saturating_sub(n);
                Ok(serde_json::to_value(theseus_protocol::NodeListResult {
                    nodes: nodes[skip..]
                        .iter()
                        .rev()
                        .map(|(pos, node)| Self::node_info(*pos, node))
                        .collect(),
                    total: self.store.node_count().map_err(bad)?,
                })
                .unwrap())
            }
            method::ACTION_CONFIRM => {
                let p: theseus_protocol::ActionConfirmParams = parse(req.params)?;
                if p.watch {
                    // Subscribe before the answer wakes the execution: the
                    // continuation turn is then seen from its first event.
                    if let Some(a) = self.kernel.action(&p.correlation_id).map_err(bad)? {
                        self.bus.watch(&a.session_id, client, tx.clone());
                    }
                }
                let r = self
                    .confirm_action(
                        &p.correlation_id,
                        p.approve,
                        p.note.as_deref(),
                        p.author.as_deref().unwrap_or(client),
                    )
                    .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e.to_string()))?;
                Ok(serde_json::to_value(r).unwrap())
            }
            method::TOOL_LIST => {
                let calls = self.tools.calls.lock().unwrap().clone();
                let total: u64 = calls.values().sum();
                let proc_calls = calls.get("proc.run").copied().unwrap_or(0);
                let tools = self
                    .tools
                    .registry
                    .all()
                    .map(|t| theseus_protocol::ToolInfo {
                        name: t.name().into(),
                        wire_name: theseus_tools::wire_name(t.name()),
                        family: t.family().into(),
                        description: t.description().into(),
                        class: t.class().as_str().into(),
                        backend: t.backend().as_str().into(),
                        policy: self.tools.policy.posture(t.name()).0.as_str().into(),
                        input_schema: t.input_schema(),
                        calls: calls.get(t.name()).copied().unwrap_or(0),
                    })
                    .collect();
                Ok(serde_json::to_value(theseus_protocol::ToolListResult {
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
                })
                .unwrap())
            }
            method::EXECUTION_LIST => {
                let execs = self.kernel.executions().map_err(bad)?;
                Ok(serde_json::to_value(theseus_protocol::ExecutionListResult {
                    executions: execs.iter().map(Self::execution_info).collect(),
                })
                .unwrap())
            }
            method::ACTION_LIST => {
                let p: theseus_protocol::ActionListParams = parse(req.params)?;
                let n = p.n.unwrap_or(200).min(2000);
                let mut actions = self.kernel.actions().map_err(bad)?;
                let total = actions.len() as u64;
                if let Some(x) = &p.execution_id {
                    actions.retain(|a| &a.execution_id == x);
                }
                actions.sort_by_key(|a| std::cmp::Reverse(a.planned_at_ms));
                actions.truncate(n);
                Ok(serde_json::to_value(theseus_protocol::ActionListResult {
                    actions: actions.iter().map(Self::action_info).collect(),
                    total,
                })
                .unwrap())
            }
            method::EXECUTION_CANCEL => {
                let p: theseus_protocol::ExecutionCancelParams = parse(req.params)?;
                if self
                    .kernel
                    .execution(&p.execution_id)
                    .map_err(bad)?
                    .is_none()
                {
                    return Err(RpcFailure::new(
                        error_code::NOT_FOUND,
                        format!("no execution {}", p.execution_id),
                    ));
                }
                let (e, cancelled) = self
                    .cancel_execution(&p.execution_id, p.author.as_deref().unwrap_or(client))
                    .map_err(bad)?;
                Ok(
                    serde_json::to_value(theseus_protocol::ExecutionCancelResult {
                        execution: Self::execution_info(&e),
                        cancelled_actions: cancelled,
                    })
                    .unwrap(),
                )
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
                Ok(serde_json::to_value(LedgerTailResult {
                    rows,
                    total: self.store.ledger_len().map_err(bad)?,
                })
                .unwrap())
            }
            method::NARRATIVE_WATCH | method::NARRATIVE_UNWATCH if !self.narrator.on() => {
                Err(RpcFailure::new(
                    error_code::DISABLED,
                    "narration is off: put `narrative = true` at the top of the config, \
                     before any [table], and restart the daemon",
                ))
            }
            method::NARRATIVE_WATCH => {
                let lines = self.narrator.watch(client, tx.clone()).unwrap_or_default();
                Ok(
                    serde_json::to_value(theseus_protocol::NarrativeWatchResult {
                        lines,
                        capacity: self.narrator.capacity() as u32,
                    })
                    .unwrap(),
                )
            }
            method::NARRATIVE_UNWATCH => {
                self.narrator.unwatch(client);
                Ok(json!({"watching": false}))
            }
            method::SHUTDOWN => {
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
    async fn continuations_wait_for_expected_bindings_but_not_forever() {
        let core = test_core("x");
        assert!(
            core.wait_for_bindings(Duration::from_millis(10)).await,
            "none expected"
        );
        core.expect_binding();
        assert!(
            !core.wait_for_bindings(Duration::from_millis(120)).await,
            "times out while a binding is still starting"
        );
        let c = core.clone();
        let waiter = tokio::spawn(async move { c.wait_for_bindings(Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(80)).await;
        core.binding_started();
        assert!(waiter.await.unwrap(), "released when the binding starts");
        core.binding_started(); // a second report never underflows
        assert!(core.wait_for_bindings(Duration::from_millis(10)).await);
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
                    author: None,
                },
            )],
        )
        .await;
        let ns: Vec<&str> = notifications(&msgs)
            .iter()
            .map(|n| n.method.as_str())
            .collect();
        assert_eq!(ns.first(), Some(&notify::TURN_STARTED));
        // The input becomes a node, the context compiles, the loop starts.
        let pos = |m: &str| {
            ns.iter()
                .position(|x| *x == m)
                .unwrap_or_else(|| panic!("no {m} in {ns:?}"))
        };
        assert!(pos(notify::NODE_WRITTEN) < pos(notify::CONTEXT_COMPILED));
        assert!(pos(notify::CONTEXT_COMPILED) < pos(notify::LOOP_STARTED));
        assert!(pos(notify::LOOP_STARTED) < pos(notify::MODEL_DELTA));
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
        assert_eq!(result.stop_reason, "no_tool_calls");
        assert_eq!(result.output, "hello there friend");
        // The exchange is content now: a user node and an assistant node.
        let nodes = core.store.session_nodes(&result.session_id).unwrap();
        let kinds: Vec<&str> = nodes.iter().map(|(_, n)| n.kind_str()).collect();
        assert_eq!(kinds, vec!["user_message", "assistant_message"]);
        assert_eq!(result.provider_stop_reason.as_deref(), Some("end_turn"));

        // No hook rows or hook spans (theseus-hco removed the hook system).
        let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(200).unwrap();
        assert!(!rows.iter().any(|(_, r)| r.kind.starts_with("hook")));
        // The trace: turn > loop 0 > provider.call > first_token.
        let tr = result.trace.as_ref().expect("trace");
        assert!(!serde_json::to_string(tr)
            .unwrap()
            .contains(r#""kind":"hook""#));
        assert_eq!(tr.name, "turn");
        assert!(tr.end_us.is_some());
        let names: Vec<&str> = tr.children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"admission.wait"));
        assert!(names.contains(&"loop 0"));
        assert!(names.contains(&"session.write"));
        let lp = tr.children.iter().find(|c| c.name == "loop 0").unwrap();
        let lnames: Vec<&str> = lp.children.iter().map(|c| c.name.as_str()).collect();
        assert!(lnames.contains(&"compile"));
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
                        author: None,
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
                        author: None,
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

    /// Rows written by the hook system (removed in theseus-hco) stay in old
    /// stores. They still read through `ledger.tail`, which `theseus ledger`
    /// and the Observatory use, and an old trace's hook spans still decode.
    #[tokio::test]
    async fn rows_from_the_hook_system_still_read() {
        let core = test_core("ok");
        // Byte for byte what the hook system wrote.
        let old = [
            r#"{"at_unix_ms":1759100000000,"kind":"server.started","data":{"hooks":{"event":"server.started","kind":"observe","handlers":0,"outcome":"proceed"},"startup":{}}}"#,
            r#"{"at_unix_ms":1759100000001,"kind":"session.opened","session_id":"ses_old","data":{"event":"session.opened","kind":"observe","handlers":0,"outcome":"proceed"}}"#,
            r#"{"at_unix_ms":1759100000002,"kind":"hooks.registered","data":{"event":"turn_ended","handler_id":"cli-watch","client":"cli#1"}}"#,
            r#"{"at_unix_ms":1759100000003,"kind":"hook.site","session_id":"ses_old","turn_id":"turn_old","data":{"event":"turn.starting","kind":"gate","handlers":0,"outcome":"proceed"}}"#,
            r#"{"at_unix_ms":1759100000004,"kind":"turn.trace","session_id":"ses_old","turn_id":"turn_old","data":{"name":"turn","kind":"turn","start_us":0,"end_us":900,"children":[{"name":"turn.starting","kind":"hook","start_us":10,"end_us":12,"attrs":{"kind":"gate","handlers":0,"outcome":"proceed"}}]}}"#,
        ];
        for row in old {
            let row: Value = serde_json::from_str(row).unwrap();
            core.store.append_ledger(&row).unwrap();
        }
        let tail = |id, kind: Option<&str>| {
            Request::new(
                Id::Num(id),
                method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(10),
                    kind: kind.map(str::to_string),
                    session_id: None,
                },
            )
        };
        let msgs = roundtrip(
            core,
            vec![
                tail(1, None),
                tail(2, Some("hook.site")),
                Request::new(Id::Num(3), "hooks.list", Value::Null),
            ],
        )
        .await;
        let rs = responses(&msgs);
        let result = |id| -> LedgerTailResult {
            let r = rs.iter().find(|r| r.id == Id::Num(id)).unwrap();
            serde_json::from_value(r.result.clone().unwrap()).unwrap()
        };
        let all = result(1);
        let kinds: Vec<&str> = all.rows.iter().map(|r| r.kind.as_str()).collect();
        for k in [
            "server.started",
            "session.opened",
            "hooks.registered",
            "hook.site",
            "turn.trace",
        ] {
            assert!(kinds.contains(&k), "{k} missing from {kinds:?}");
        }
        let sites = result(2).rows;
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].data["event"], "turn.starting");
        assert_eq!(sites[0].turn_id.as_deref(), Some("turn_old"));
        let trace = all.rows.iter().find(|r| r.kind == "turn.trace").unwrap();
        let span: theseus_protocol::Span = serde_json::from_value(trace.data.clone()).unwrap();
        assert_eq!(span.children[0].kind, "hook");
        // An old client asking for the hook list is told plainly.
        let gone = rs.iter().find(|r| r.id == Id::Num(3)).unwrap();
        assert_eq!(
            gone.error.as_ref().unwrap().code,
            error_code::METHOD_NOT_FOUND
        );
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
                        author: None,
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
                    author: None,
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
                        author: None,
                    },
                )
            })
            .collect();
        let msgs = roundtrip(core.clone(), reqs).await;
        let mut rs: Vec<TurnSubmitResult> = responses(&msgs)
            .iter()
            .map(|r| serde_json::from_value(r.result.clone().unwrap()).unwrap())
            .collect();
        rs.sort_by_key(|r| r.usage.input_tokens);
        assert_eq!(rs[0].usage.output_tokens, 3);
        // The second turn carries the first exchange: the session has memory.
        assert!(
            rs[1].usage.input_tokens > rs[0].usage.input_tokens,
            "{rs:?}"
        );
        let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
        assert_eq!(
            rec.usage.input_tokens,
            rs[0].usage.input_tokens + rs[1].usage.input_tokens
        );
        assert_eq!(rec.usage.output_tokens, 6);
        assert_eq!(rec.turns, 2);
        let h = core.health();
        assert_eq!(h.usage_total.output_tokens, 6);
        assert_eq!(h.turns, 2);
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

    /// A turn that fails after its first loop still books that loop: its usage,
    /// cost, and tool call reach the session, the `turn.failed` row, and
    /// `error.data`. Both failure exits: the provider fails in loop 1, or the
    /// budget runs out planning loop 1 (loop 0's 30,000 words spend it).
    #[tokio::test]
    async fn a_turn_that_fails_after_its_first_loop_keeps_that_loops_books() {
        use crate::provider::{ProviderError, Scripted};
        let mut wrong = Vec::new();
        for exit in ["provider", "budget"] {
            let first = Scripted::tools(
                &"word ".repeat(30_000),
                &[("t1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
            );
            let second = match exit {
                "provider" => Scripted::Fail(ProviderError::Overloaded {
                    message: "busy".into(),
                }),
                _ => Scripted::text("never asked"),
            };
            let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
            let store = Store::open(&dir.join("store"), theseus_store::Engine::Redb).unwrap();
            let fake = FakeProvider {
                chunk: usize::MAX,
                ..FakeProvider::scripted(vec![first, second])
            };
            let core =
                Core::with_provider(Config::example(), Arc::new(fake), store, vec![]).unwrap();
            // The budget fits loop 0's reservation, and not loop 1's once loop 0 is spent.
            let (live, _) = core.live_profile();
            let target = core.runner.resolve_target(&live, None, None, None).unwrap();
            let limit = (exit == "budget")
                .then(|| core.kernel.config().control_reserve + target.max_tokens as u64 + 20_000);
            let mut rec = SessionRecord::new(SessionKind::Conversation, None);
            let authority = Authority {
                principal: OPERATOR.into(),
                ..Default::default()
            };
            let e = core
                .kernel
                .open_execution(&rec.session_id, rec.kind, authority, limit, None)
                .unwrap();
            rec.execution_id = Some(e.id);
            let sid = rec.session_id.clone();
            core.store.put_session(&sid, &rec).unwrap();
            let msgs = roundtrip(
                core.clone(),
                vec![Request::new(
                    Id::Num(1),
                    method::TURN_SUBMIT,
                    TurnSubmitParams {
                        session_id: Some(sid.clone()),
                        input: "diff these".into(),
                        profile: None,
                        provider: None,
                        model: None,
                        author: None,
                    },
                )],
            )
            .await;
            let err = responses(&msgs)[0].error.clone().expect("the turn fails");
            let rows: Vec<(u64, LedgerRow)> = core.store.ledger_tail(500).unwrap();
            let row = |kind: &str| {
                rows.iter()
                    .find(|(_, r)| r.kind == kind)
                    .map(|(_, r)| r.data.clone())
                    .unwrap_or_default()
            };
            let (call, failed) = (row("provider.call"), row("turn.failed"));
            let (usage, cost) = (call["usage"].clone(), call["cost_usd"].clone());
            assert!(
                cost.as_f64().unwrap_or(0.0) > 0.0,
                "{exit}: loop 0 has a cost: {call}"
            );
            let s: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
            let class = if exit == "budget" {
                "budget_exhausted"
            } else {
                "overloaded"
            };
            for (what, got, want) in [
                ("error.data class", err.data["class"].clone(), json!(class)),
                ("session turns", json!(s.turns), json!(1)),
                ("session usage", json!(s.usage), usage.clone()),
                ("session cost_usd", json!(s.cost_usd), cost.clone()),
                ("session tool_calls", json!(s.tool_calls), json!(1)),
                (
                    "health cost_usd_total",
                    json!(core.health().cost_usd_total),
                    cost.clone(),
                ),
                (
                    "turn.failed usage_so_far",
                    failed["usage_so_far"].clone(),
                    usage.clone(),
                ),
                (
                    "turn.failed cost_usd",
                    failed["cost_usd"].clone(),
                    cost.clone(),
                ),
                (
                    "turn.failed tool_calls",
                    failed["tool_calls"].clone(),
                    json!(1),
                ),
                ("error.data usage", err.data["usage"].clone(), usage),
                ("error.data cost_usd", err.data["cost_usd"].clone(), cost),
                (
                    "error.data tool_calls",
                    err.data["tool_calls"].clone(),
                    json!(1),
                ),
            ] {
                if got != want {
                    wrong.push(format!("{exit}: {what} = {got}, want {want}"));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "the failed turn lost:\n{}",
            wrong.join("\n")
        );
    }

    /// Rows stored before theseus-8az renamed the decline vocabulary: a tool
    /// result with status `denied` and an `action.denied` ledger row. Both
    /// still decode, and the history and ledger reads serve them.
    #[tokio::test]
    async fn rows_stored_with_the_old_denied_names_still_decode() {
        use crate::node::ResultStatus;
        let core = test_core("hi");
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let n = Node::tool_result(
            &sid,
            Some("turn_1"),
            Some(0),
            Body::ToolResult {
                tool_use_id: "toolu_1".into(),
                tool: "fs.write".into(),
                status: ResultStatus::Declined,
                is_error: true,
                content: "Not run: the operator declined this call (not now).".into(),
                correlation_id: Some("act_1".into()),
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: Value::Null,
            },
        );
        let mut old = serde_json::to_value(&n).unwrap();
        old["body"]["status"] = json!("denied");
        let r = theseus_store::NewRecord::json(theseus_store::kinds::NODE, Some(&n.id), &old)
            .unwrap()
            .scoped(&sid);
        assert!(String::from_utf8_lossy(&r.payload).contains(r#""status":"denied""#));
        core.store.append(vec![r]).unwrap();
        let old_row = json!({"correlation_id": "act_1", "tool": "fs.write", "by": "operator", "reason": "not now"});
        for kind in ["action.denied", "action.declined"] {
            core.store
                .append_ledger(&LedgerRow::new(kind, Some(&sid), None, old_row.clone()))
                .unwrap();
        }

        let nodes = core.store.session_nodes(&sid).unwrap();
        assert!(
            matches!(
                &nodes[0].1.body,
                Body::ToolResult {
                    status: ResultStatus::Declined,
                    ..
                }
            ),
            "{nodes:?}"
        );
        let tail = |id, kind: &str| {
            Request::new(
                Id::Num(id),
                method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(10),
                    kind: Some(kind.into()),
                    session_id: None,
                },
            )
        };
        let got = roundtrip(
            core.clone(),
            vec![
                Request::new(
                    Id::Num(1),
                    method::SESSION_HISTORY,
                    theseus_protocol::SessionHistoryParams {
                        session_id: sid.clone(),
                        n: None,
                    },
                ),
                tail(2, "action.declined"),
                tail(3, "action.denied"),
            ],
        )
        .await;
        let rs = responses(&got);
        let result = |id| {
            rs.iter()
                .find(|r| r.id == Id::Num(id))
                .and_then(|r| r.result.clone())
                .unwrap()
        };
        let h: theseus_protocol::SessionHistoryResult = serde_json::from_value(result(1)).unwrap();
        assert_eq!(h.nodes[0].detail["status"], "declined");
        // Either name reads the rows stored under both.
        for id in [2, 3] {
            let t: LedgerTailResult = serde_json::from_value(result(id)).unwrap();
            let kinds: Vec<&str> = t.rows.iter().map(|r| r.kind.as_str()).collect();
            assert_eq!(kinds, ["action.denied", "action.declined"], "{t:?}");
            assert_eq!(t.rows[0].data, old_row);
        }
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
                        author: None,
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
                        author: None,
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
                        author: None,
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
        assert_eq!(r1.model, "claude-sonnet-5-5");
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
                    author: None,
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

    /// Send one request on an open connection and read to its response,
    /// keeping the notifications that came before it.
    async fn ask<R, W>(
        w: &mut W,
        lines: &mut tokio::io::Lines<BufReader<R>>,
        req: Request,
    ) -> (Response, Vec<Notification>)
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut line = serde_json::to_string(&req).unwrap();
        line.push('\n');
        w.write_all(line.as_bytes()).await.unwrap();
        let mut notes = Vec::new();
        loop {
            let l = lines.next_line().await.unwrap().unwrap();
            match serde_json::from_str::<Message>(&l).unwrap() {
                Message::Response(r) if r.id == req.id => return (r, notes),
                Message::Notification(n) => notes.push(n),
                _ => {}
            }
        }
    }

    /// `narrative.watch` returns the tail, then streams every line as
    /// `narrative.line` on the same connection until `narrative.unwatch`; a
    /// connection that arrives late gets the same lines as its tail. With
    /// narration off, both methods refuse with DISABLED and health says so.
    #[tokio::test]
    async fn narrative_watch_streams_a_turn_and_refuses_when_off() {
        use theseus_protocol::{NarrativeLine, NarrativePart, NarrativeWatchResult};
        let core = test_core("hello there");
        assert!(core.health().narrative, "the template turns it on");
        let (client, server) = duplex(256 * 1024);
        let (sr, sw) = tokio::io::split(server);
        let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "watcher".into()));
        let (cr, mut cw) = tokio::io::split(client);
        let mut lines = BufReader::new(cr).lines();
        let watch = Request::new(Id::Num(1), method::NARRATIVE_WATCH, Value::Null);
        let (r, _) = ask(&mut cw, &mut lines, watch).await;
        let w: NarrativeWatchResult = serde_json::from_value(r.result.unwrap()).unwrap();
        assert!(w.lines.is_empty(), "{:?}", w.lines);
        assert_eq!(w.capacity, 500);
        let submit = |id, session_id: Option<String>| {
            Request::new(
                Id::Num(id),
                method::TURN_SUBMIT,
                TurnSubmitParams {
                    session_id,
                    input: "hi".into(),
                    profile: None,
                    provider: None,
                    model: None,
                    author: Some("discord:eddie".into()),
                },
            )
        };
        let (r, notes) = ask(&mut cw, &mut lines, submit(2, None)).await;
        let res: TurnSubmitResult = serde_json::from_value(r.result.unwrap()).unwrap();
        let narrated: Vec<NarrativeLine> = notes
            .iter()
            .filter(|n| n.method == notify::NARRATIVE_LINE)
            .map(|n| serde_json::from_value(n.params.clone()).unwrap())
            .collect();
        let says = |part: NarrativePart, needle: &str| {
            narrated
                .iter()
                .any(|l| l.part == part && l.text.contains(needle))
        };
        for (part, needle) in [
            (NarrativePart::Session, "opened (conversation)"),
            (NarrativePart::Session, "budget of 20,000,000 units"),
            (NarrativePart::Turn, "started by discord:eddie"),
            (NarrativePart::Turn, "2 characters of input"),
            (NarrativePart::Turn, "ended after 1 loop"),
            (NarrativePart::Session, "Parked until the next input"),
        ] {
            assert!(
                says(part, needle),
                "no {part:?} line says {needle:?}: {narrated:#?}"
            );
        }
        assert!(narrated
            .iter()
            .all(|l| l.session_id.as_deref() == Some(res.session_id.as_str())));
        // A connection that arrives late gets those lines as its tail.
        let late = roundtrip(
            core.clone(),
            vec![Request::new(
                Id::Num(3),
                method::NARRATIVE_WATCH,
                Value::Null,
            )],
        )
        .await;
        let tail: NarrativeWatchResult =
            serde_json::from_value(responses(&late)[0].result.clone().unwrap()).unwrap();
        assert_eq!(tail.lines, narrated);
        // Unwatched, the next turn's lines no longer come.
        let unwatch = Request::new(Id::Num(4), method::NARRATIVE_UNWATCH, Value::Null);
        let (r, _) = ask(&mut cw, &mut lines, unwatch).await;
        assert_eq!(r.result.unwrap()["watching"], false);
        let (r, notes) = ask(&mut cw, &mut lines, submit(5, Some(res.session_id.clone()))).await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(notes.iter().all(|n| n.method != notify::NARRATIVE_LINE));
        assert!(
            core.narrator.tail().len() > narrated.len(),
            "still narrated"
        );
        cw.shutdown().await.unwrap();
        drop(cw);
        drop(lines);
        let _ = srv.await;

        // Off: the methods refuse and health says so.
        let dir = std::env::temp_dir().join(format!("theseus-test-{}", crate::new_id("t")));
        let store = Store::open(&dir.join("store"), theseus_store::Engine::Redb).unwrap();
        let mut cfg = Config::example();
        cfg.server.state_dir = dir.to_string_lossy().into_owned();
        cfg.narrative = false;
        let fake = FakeProvider {
            reply: "x".into(),
            ..Default::default()
        };
        let off = Core::with_provider(cfg, Arc::new(fake), store, vec![]).unwrap();
        let msgs = roundtrip(
            off.clone(),
            vec![
                Request::new(Id::Num(1), method::HEALTH, Value::Null),
                Request::new(Id::Num(2), method::NARRATIVE_WATCH, Value::Null),
                Request::new(Id::Num(3), method::NARRATIVE_UNWATCH, Value::Null),
            ],
        )
        .await;
        let rs = responses(&msgs);
        let by = |id| rs.iter().find(|r| r.id == Id::Num(id)).unwrap();
        assert_eq!(by(1).result.as_ref().unwrap()["narrative"], false);
        for id in [2, 3] {
            let e = by(id).error.as_ref().expect("refused");
            assert_eq!(e.code, error_code::DISABLED);
            assert!(e.message.contains("narrative = true"), "{}", e.message);
        }
        assert!(off.narrator.tail().is_empty());
    }
}
