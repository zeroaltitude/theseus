//! The protocol server (spec §3.18): JSON-RPC 2.0 over newline-delimited
//! JSON on any `AsyncRead + AsyncWrite` pair (stdio, a Unix socket). One
//! task per connection; notifications for a connection flow through its own
//! channel so a streaming turn never blocks another client.
//!
//! `Core` is the server's state, built here from its `Parts`. Its jobs have a
//! file each: serving connections and routing each method by name (`server`),
//! the methods (`methods`), the protocol's views of records (`info`), pending
//! confirms and their answers (`confirms`), "should have asked" and its undo
//! (`policy`), what the harness loop drives (`driver`), and channel bindings
//! (`bindings`).

mod bindings;
mod confirms;
mod driver;
mod info;
mod methods;
mod policy;
mod server;
#[cfg(test)]
mod tests;

pub use bindings::BindingBoard;
pub use server::ACTS;

use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::bus::SessionBus;
use crate::catalog::Catalog;
use crate::config_gate::ConfigGate;
use crate::ledger::LedgerRow;
use crate::narrative::{narrate, Narrator};
use crate::provider::{Anthropic, Provider};
use crate::scrub::Scrubber;
use crate::secrets::{SecretBoard, SecretState};
use crate::session::SessionRecord;
use crate::startup::StartupLog;
use crate::store::Store;
use crate::telemetry::Telemetry;
use crate::toolrun::{JobLauncher, ToolRuntime, WrapperLauncher};
use crate::turn::TurnRunner;
use crate::Config;
use theseus_kernel::job::WrapperEvidence;
use theseus_kernel::{Kernel, Spool};
use theseus_protocol::ConfigRestart;

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
    /// Each secret's state. The daemon serves before they resolve, and each
    /// consumer waits for its own (theseus-qa0).
    pub secrets: Arc<SecretBoard>,
    /// The last start's phases, for health and the Observatory.
    pub startup_log: Arc<StartupLog>,
    /// The export pipeline, once built after serving, when the config may act
    /// and its headers secret resolves (`install_telemetry`). Until then
    /// nothing is exported.
    telemetry: std::sync::OnceLock<Telemetry>,
    telemetry_off: Telemetry,
    /// The narrative (`narrative = true`): live lines and a bounded tail.
    pub narrator: Arc<Narrator>,
    started: Instant,
    provider_errors: AtomicU64,
    /// The live profile and where it came from ("config" | "runtime").
    live: std::sync::RwLock<(String, String)>,
    pub shutdown: tokio::sync::Notify,
    /// Channel bindings: their status for health, and how many still start.
    pub bindings: BindingBoard,
    /// `[approval]`: who may answer a waiting call, and through which
    /// channels, with the Discord binding's checks (theseus-sgh).
    pub approval: crate::approval::Approval,
    /// Whether the config may act: a start from the copy of the vault's note
    /// answers reads until the vault confirms it (theseus-2fo).
    pub config_gate: Arc<ConfigGate>,
    /// Set once the daemon is to restart onto the vault's changed note.
    restart: tokio::sync::watch::Sender<Option<ConfigRestart>>,
}

/// What a `Core` is built from. `Core::new` builds these from the config and
/// the secret board; tests start from `Parts::for_tests`.
pub struct Parts {
    pub cfg: Config,
    pub providers: BTreeMap<String, Arc<dyn Provider>>,
    pub store: Store,
    /// Each secret's state; values stay in it, and the providers, the
    /// scrubber, and the bindings read their own there.
    pub secrets: Arc<SecretBoard>,
    pub startup_log: Arc<StartupLog>,
    /// `None`: built by `install_telemetry` once its headers secret resolves.
    pub telemetry: Option<Telemetry>,
    pub scrubber: Arc<Scrubber>,
    pub launcher: Arc<dyn JobLauncher>,
    pub config_gate: Arc<ConfigGate>,
    /// Toollets registered after the config's, so one may stand in for a
    /// built-in (tests: one that takes a known time). The daemon adds none.
    pub toollets: Vec<Arc<dyn theseus_tools::Tool>>,
}

const META_LIVE_PROFILE: &str = "live_profile";

/// A moment for the answers to the requests that waited at the gate to reach
/// their clients before a restart closes their connections.
const RESTART_GRACE: std::time::Duration = std::time::Duration::from_millis(100);

impl Core {
    /// The daemon's core, from its config and the secret board, which may
    /// still be resolving: nothing here waits for a secret (theseus-qa0).
    /// Each provider and the scrubber read their values from the board. The
    /// telemetry pipeline is built after serving (`install_telemetry`), once
    /// the vault confirms the config (theseus-2fo) and its headers secret
    /// resolves, so its client stays off the start path.
    pub fn new(
        cfg: Config,
        secrets: Arc<SecretBoard>,
        store: Store,
        startup_log: Arc<StartupLog>,
        config_gate: Arc<ConfigGate>,
    ) -> Result<Arc<Self>> {
        let t = Instant::now();
        let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
        for (name, pc) in cfg.all_providers() {
            let timeouts = pc
                .timeouts
                .clone()
                .unwrap_or_else(|| cfg.model.timeouts.clone());
            providers.insert(
                name.clone(),
                Arc::new(Anthropic::new(
                    &pc.api_base,
                    secrets.clone(),
                    &pc.api_key_secret,
                    timeouts,
                )?),
            );
        }
        let scrubber = Arc::new(Scrubber::from_board(secrets.clone()));
        // Wrappers run this very image: after an in-place upgrade (copy, then
        // rename over the old file) the path on disk is a newer binary, or
        // `current_exe()` names a deleted file; `/proc/self/exe` is still us.
        let self_exe = match std::path::Path::new("/proc/self/exe") {
            p if p.exists() => p.to_path_buf(),
            _ => std::env::current_exe()
                .context("locating the theseusd binary for the job wrapper")?,
        };
        let launcher: Arc<dyn JobLauncher> = Arc::new(WrapperLauncher { self_exe });
        // The start path's phases follow one another: providers, kernel, core.
        startup_log.record("providers", false, t, json!({"providers": providers.len()}));
        Self::build(Parts {
            cfg,
            providers,
            store,
            secrets,
            startup_log,
            telemetry: None,
            scrubber,
            launcher,
            config_gate,
            toollets: vec![],
        })
    }

    /// The telemetry headers secret, when an export would carry it.
    fn telemetry_headers(cfg: &Config) -> Option<String> {
        cfg.telemetry
            .headers_secret
            .clone()
            .filter(|_| cfg.telemetry.endpoint().is_some())
    }

    /// The export pipeline, or a disabled one until it is built.
    pub fn telemetry(&self) -> &Telemetry {
        self.telemetry.get().unwrap_or(&self.telemetry_off)
    }

    /// Telemetry for health: the pipeline's own status once it is built;
    /// before that, `off` without an endpoint, or `waiting` and for what.
    pub fn telemetry_status(&self) -> theseus_protocol::TelemetryStatus {
        if let Some(t) = self.telemetry.get() {
            return t.status();
        }
        let Some(endpoint) = self.cfg.telemetry.endpoint() else {
            return theseus_protocol::TelemetryStatus::off();
        };
        let detail = match Self::telemetry_headers(&self.cfg) {
            _ if !self.config_gate.is_open() => {
                "the vault has not confirmed the config yet".to_string()
            }
            Some(name) => match self.secrets.states().get(&name) {
                Some(SecretState::Failed(why)) => {
                    format!("its headers secret {name} did not resolve: {why}")
                }
                _ => format!("its headers secret {name} is resolving"),
            },
            None => "starting".to_string(),
        };
        theseus_protocol::TelemetryStatus {
            otlp_endpoint: Some(endpoint.to_string()),
            state: "waiting".into(),
            detail: Some(detail),
            ..Default::default()
        }
    }

    /// Build the pipeline, or keep why it could not be built.
    fn build_telemetry(&self, headers: Option<&crate::secrets::Secret>) -> Result<(), String> {
        let (t, out) = match Telemetry::from_config(&self.cfg.telemetry, headers) {
            Ok(t) => (t, Ok(())),
            Err(e) => {
                let why = format!("{e:#}");
                tracing::error!(error = %why, "telemetry: building the exporter failed; nothing is exported");
                let endpoint = self.cfg.telemetry.endpoint().unwrap_or_default();
                (Telemetry::failed(endpoint, why.clone()), Err(why))
            }
        };
        let _ = self.telemetry.set(t);
        out
    }

    /// Build the telemetry pipeline after serving, once the vault confirms
    /// the config (theseus-2fo) and its headers secret resolves. Fail
    /// closed: nothing is exported without its headers, and a failure waits
    /// for the secret's retry.
    pub async fn install_telemetry(self: Arc<Self>) {
        if !self.config_gate.opened().await {
            return;
        }
        let Some(name) = Self::telemetry_headers(&self.cfg) else {
            if self.telemetry.get().is_none() {
                let _ = self.build_telemetry(None);
            }
            return;
        };
        let t0 = std::time::Instant::now();
        let phase = self.startup_log.begin("telemetry.headers", true, t0);
        let mut rx = self.secrets.subscribe();
        // A failure is said once per reason, not on every change to the board.
        let mut said: Option<String> = None;
        loop {
            let state = rx.borrow_and_update().get(&name).cloned();
            match state {
                Some(SecretState::Ready(headers)) => {
                    let detail = match self.build_telemetry(Some(&headers)) {
                        Ok(()) => {
                            json!({"secret": name, "waited_ms": t0.elapsed().as_millis() as u64, "outcome": "ready", "enabled": self.telemetry().enabled()})
                        }
                        Err(why) => json!({"secret": name, "outcome": "error", "error": why}),
                    };
                    self.startup_log.end(phase, detail);
                    return;
                }
                Some(SecretState::Failed(why)) if said.as_deref() != Some(why.as_str()) => {
                    tracing::error!(secret = %name, error = %why, "telemetry: its headers secret did not resolve; nothing is exported until it does");
                    self.startup_log.end(
                        phase,
                        json!({"secret": name, "waited_ms": t0.elapsed().as_millis() as u64, "outcome": "failed", "error": why}),
                    );
                    said = Some(why);
                }
                // Config validation keeps an unknown name out of a daemon.
                None => return,
                _ => {}
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    /// Ledger the secrets as they settle (theseus-qa0): the first round's
    /// outcome, then each retry that makes one ready. Closes the `secrets`
    /// startup phase.
    pub async fn watch_secrets(self: Arc<Self>) {
        let start = self
            .secrets
            .started_at()
            .unwrap_or_else(std::time::Instant::now);
        let phase = self.startup_log.begin("secrets", true, start);
        let mut rx = self.secrets.subscribe();
        self.secrets.settle_all().await;
        let st = self.secrets.status();
        let failed: Vec<&str> = st.failed.iter().map(|f| f.name.as_str()).collect();
        self.startup_log.end(
            phase,
            json!({"state": st.state, "method": st.method, "ready": st.ready.len(), "failed": failed}),
        );
        let row = if st.failed.is_empty() {
            LedgerRow::new(
                "secrets.resolved",
                None,
                None,
                json!({"names": st.ready, "ms": st.settled_ms, "method": st.method, "rounds": st.rounds}),
            )
        } else {
            LedgerRow::new(
                "secrets.failed",
                None,
                None,
                json!({"failed": st.failed, "ready": st.ready, "ms": st.settled_ms, "method": st.method, "rounds": st.rounds, "retry_in_ms": st.retry_in_ms}),
            )
        };
        if let Err(e) = self.store.append_ledger(&row) {
            tracing::warn!(error = %e, "ledger append failed");
        }
        let mut ready: std::collections::BTreeSet<String> = st.ready.into_iter().collect();
        while self.secrets.status().state == "failed" && rx.changed().await.is_ok() {
            let newly: Vec<String> = self
                .secrets
                .ready_names()
                .into_iter()
                .filter(|n| !ready.contains(n))
                .collect();
            if newly.is_empty() {
                continue;
            }
            let st = self.secrets.status();
            tracing::info!(secrets = ?newly, rounds = st.rounds, "secrets resolved on a retry");
            let row = LedgerRow::new(
                "secrets.resolved",
                None,
                None,
                json!({"names": newly, "ms": self.startup_log.us(std::time::Instant::now()) / 1000, "method": st.method, "rounds": st.rounds, "still_failed": st.failed}),
            );
            if let Err(e) = self.store.append_ledger(&row) {
                tracing::warn!(error = %e, "ledger append failed");
            }
            ready.extend(newly);
        }
    }

    /// Build a core from its parts: the kernel opened on the store and started
    /// (its spool beside the store), then the catalog, the tool runtime, and
    /// the turn runner.
    pub fn build(parts: Parts) -> Result<Arc<Self>> {
        let Parts {
            cfg,
            providers,
            store,
            secrets,
            startup_log,
            telemetry,
            scrubber,
            launcher,
            config_gate,
            toollets,
        } = parts;
        let k0 = std::time::Instant::now();
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
        // An execution stored with a unit budget takes its dollar spend from
        // its session's recorded cost when startup rewrites it (theseus-0sg).
        let sessions = store.clone();
        let legacy_spend: theseus_kernel::LegacySpend = Arc::new(move |sid: &str| {
            sessions
                .get_session::<SessionRecord>(sid)
                .ok()
                .flatten()
                .map_or(0, |s| theseus_kernel::usd_to_micros(s.cost_usd))
        });
        // Startup writes nothing a copy's word decides (theseus-2fo): with a
        // config the vault has not confirmed, a store that still holds unit
        // budgets refuses to start, since their dollar limit is the config's.
        let kernel_cfg = theseus_kernel::KernelConfig {
            unconfirmed_config: !config_gate.is_open(),
            ..cfg.kernel.to_kernel_config()
        };
        let kernel = Arc::new(
            Kernel::new(
                store.shared(),
                Arc::new(theseus_kernel::RealClock),
                kernel_cfg,
            )
            .with_legacy_spend(legacy_spend),
        );
        let startup = kernel
            .startup(
                Some(&spool),
                &WrapperEvidence {
                    spool: spool.clone(),
                },
            )
            .context("kernel startup")?;
        startup_log.record(
            "kernel",
            false,
            k0,
            json!({"steps": startup.steps.iter().map(|s| json!({"name": s.name, "us": s.elapsed_us})).collect::<Vec<_>>()}),
        );
        let c0 = std::time::Instant::now();
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
        let unpriced = Catalog::missing_from(&cfg.catalog);
        if !unpriced.is_empty() {
            tracing::warn!(
                models = %unpriced.join(", "),
                "catalog: {} built-in model(s) have no [catalog] table in the config, so they run at the built-in prices; paste the tables from `theseusd example-config`",
                unpriced.len()
            );
        }
        for (name, p) in cfg.all_profiles() {
            if catalog.get(&p.model).is_none() {
                tracing::warn!(profile = %name, model = %p.model, "model is not in the catalog: it runs, but cost is unknown and limits are defaults");
            }
        }
        let bus = Arc::new(SessionBus::default());
        let narrator = Arc::new(Narrator::new(cfg.narrative));
        let mut tools =
            crate::toolrun::build_runtime(&cfg, Some(spool.clone()), scrubber, launcher)?;
        for t in toollets {
            tools.registry.register(t);
        }
        let tools = Arc::new(tools);
        // "Should have asked" presses are the store's, not the config's.
        tools
            .tightened
            .load(&store)
            .context("reading the tool tightenings")?;
        tracing::info!(
            tools = tools.registry.len(),
            roots = ?tools.ctx.roots,
            enforcement = tools.policy.enforcement.as_str(),
            overrides = ?tools.policy.tools,
            mcp = ?tools.policy.mcp,
            tightened = ?tools.tightened.all().iter().map(|t| t.tool.as_str()).collect::<Vec<_>>(),
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
            // Empty: nothing is read until a turn compiles (FAST).
            context_files: Default::default(),
            secrets: secrets.clone(),
            startup_log: startup_log.clone(),
        };
        let telemetry_cell = std::sync::OnceLock::new();
        if let Some(t) = telemetry {
            let _ = telemetry_cell.set(t);
        }
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
        let approval = crate::approval::Approval::new(cfg.approval.as_ref());
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
            secrets,
            startup_log,
            telemetry: telemetry_cell,
            telemetry_off: Telemetry::disabled(),
            narrator,
            started: Instant::now(),
            provider_errors: AtomicU64::new(0),
            live: std::sync::RwLock::new(live),
            shutdown: tokio::sync::Notify::new(),
            bindings: BindingBoard::default(),
            approval,
            config_gate,
            restart: tokio::sync::watch::Sender::new(None),
        });
        // `server.started` waits for `announce_serving`: nothing on the start
        // path needs it durable, and its frame is an fsync (theseus-qa0).
        core.startup_log.record("core", false, c0, Value::Null);
        // Under a config that may act, startup's step 2 gave the open
        // sessions a changed spend limit (theseus-3pj); under a copy, the
        // vault's confirmation does (`config_gate::confirm`).
        core.said_limits_followed(&startup.limits_followed);
        Ok(core)
    }

    /// Once the socket answers: the kernel's startup report
    /// (`server.started`) and the start path's phases (`server.serving`),
    /// in one frame, off the start path (theseus-qa0).
    pub fn announce_serving(&self, serving_us: u64) {
        let phases: Vec<_> = self
            .startup_log
            .snapshot()
            .into_iter()
            .filter(|p| !p.background)
            .collect();
        let rows = [
            LedgerRow::new(
                "server.started",
                None,
                None,
                json!({"startup": self.startup_report}),
            ),
            LedgerRow::new(
                "server.serving",
                None,
                None,
                json!({"serving_us": serving_us, "phases": phases}),
            ),
        ];
        let frame: Result<Vec<_>> = rows
            .iter()
            .map(|r| theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, r))
            .collect();
        if let Err(e) = frame.and_then(|f| self.store.append(&f)) {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    pub fn live_profile(&self) -> (String, String) {
        self.live.read().unwrap().clone()
    }

    /// Restart onto the vault's changed config note (theseus-2fo). The
    /// requests waiting at the gate are answered, then the clean shutdown
    /// path runs, and `theseusd` execs its own image with its own arguments
    /// once it sees `restart_requested`.
    pub async fn restart_onto(&self, r: ConfigRestart) {
        tracing::warn!(
            reference = %r.reference,
            tables = %r.tables.join(", "),
            copy_sha256 = %r.copy_sha256,
            vault_sha256 = %r.vault_sha256,
            "the vault's config note changed since the copy; restarting onto it"
        );
        narrate!(
            self.narrator,
            Config,
            None,
            None,
            "The vault's config note changed since the copy this daemon started from ({}). \
             Nothing acted on the copy; the daemon restarts onto the vault's version.",
            r.tables.join(", ")
        );
        self.config_gate.restarting(r.clone());
        tokio::time::sleep(RESTART_GRACE).await;
        self.restart.send_replace(Some(r));
        self.stop();
    }

    /// The restart asked for, if any.
    pub fn restart_requested(&self) -> Option<ConfigRestart> {
        self.restart.borrow().clone()
    }

    /// Resolves once a restart is asked for: the serving loops end on it.
    pub async fn restart_asked(&self) {
        let mut rx = self.restart.subscribe();
        let _ = rx.wait_for(Option::is_some).await;
    }
}

#[cfg(test)]
impl Parts {
    /// Parts for tests: one provider (a `FakeProvider`) under the config's
    /// default provider name, no secrets, no telemetry, and jobs run on a thread.
    pub fn for_tests(cfg: Config, provider: Arc<dyn Provider>, store: Store) -> Self {
        let mut providers = BTreeMap::new();
        providers.insert(cfg.model.provider.clone(), provider);
        Self {
            cfg,
            providers,
            store,
            secrets: SecretBoard::empty(),
            startup_log: Arc::default(),
            telemetry: Some(Telemetry::disabled()),
            scrubber: Arc::new(Scrubber::default()),
            launcher: Arc::new(crate::toolrun::InlineLauncher),
            config_gate: ConfigGate::file("test"),
            toollets: vec![],
        }
    }
}
