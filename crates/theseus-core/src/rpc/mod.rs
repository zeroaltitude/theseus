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

mod aws;
mod bindings;
mod confirms;
pub(crate) use confirms::{expired_answer, Act};
mod driver;
mod info;
mod mcp;
mod memory;
mod methods;
mod ontology;
mod pages;
mod policy;
mod publish;
mod server;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ledger;
#[cfg(test)]
mod tests_lists;
mod trust;

pub use bindings::BindingBoard;

use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;
use theseus_protocol::LedgerKind;

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
    /// The export pipeline, once built after serving, when its headers
    /// secret resolves (`install_telemetry`). Until then nothing is exported.
    telemetry: std::sync::OnceLock<Telemetry>,
    telemetry_off: Telemetry,
    /// The narrative (`narrative = true`): live lines and a bounded tail.
    pub narrator: Arc<Narrator>,
    started: Instant,
    provider_errors: AtomicU64,
    /// The live profile and where it came from ("config" | "runtime").
    live: std::sync::RwLock<(String, String)>,
    pub shutdown: tokio::sync::Notify,
    /// Channel bindings: their status, for health.
    pub bindings: BindingBoard,
    /// What must reach a channel, written when it becomes true; a binding
    /// only delivers it (theseus-q4v).
    pub outbox: Arc<crate::outbox::Outbox>,
    /// Where the config came from, and what the vault said of the copy a
    /// start served from (theseus-2fo, theseus-zmgb).
    pub config_gate: Arc<ConfigGate>,
    /// Set once the daemon is to restart onto the vault's changed note.
    restart: tokio::sync::watch::Sender<Option<ConfigRestart>>,
    /// What the web UI refused: not its own page or address (theseus-70f).
    web_refusals: Arc<crate::webui::Refusals>,
    /// The spool's last sweep since the daemon started (theseus-2ij).
    last_sweep: std::sync::Mutex<Option<theseus_protocol::SpoolSweep>>,
    /// The push (theseus-in3): one view per execution, seeded on first need.
    pub push: crate::push::Push,
    /// The index tender's supervisor (roadmap row 51): the socket daemon runs
    /// it after serving; health and `index.*` ask the tender through it.
    pub index: Arc<crate::tender::IndexTender>,
    /// The MCP servers `[mcp.servers]` attaches (M7 36b): started after
    /// serving; their stored lists are offered from the start.
    pub mcp: Arc<crate::mcp::McpBoard>,
    /// The newest crash a start found (Review 2's consideration 1), for
    /// health: set after serving (`report_crash`).
    crash: std::sync::Mutex<Option<theseus_protocol::CrashStatus>>,
    /// Set as the stop writes its last checkpoint (theseus-81kk). From then on
    /// a row that work after serving writes on its own time (the secrets as
    /// they settle) is dropped, since the next start would replay it. A write
    /// holds it to read, so the checkpoint waits for one in progress.
    closed: std::sync::RwLock<bool>,
    /// The MCP server's health block, as its listener sets it (step 41b).
    pub mcp_server: crate::mcp_server::Board,
}

/// Where the index tender's supervisor writes its facts' rows
/// (`crate::fact::index`, all `index.tender`): the ledger, off the
/// runtime's workers (a row is an fsync). It holds the core by `Weak`, so
/// the tender's task never keeps the store open past a stop.
fn index_ledger(core: &Arc<Core>) -> crate::tender::Ledger {
    let core = Arc::downgrade(core);
    Arc::new(move |row: LedgerRow| {
        let core = core.clone();
        let write = move || {
            let Some(core) = core.upgrade() else { return };
            if let Err(e) = core.store.append_ledger(&row) {
                tracing::warn!(error = %e, "ledger append failed");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    })
}

/// A store's completion spool, the directory beside it: `store` → `spool`,
/// `store-stdio` → `spool-stdio`.
pub fn spool_dir(store_dir: &std::path::Path) -> std::path::PathBuf {
    let name = store_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "store".into());
    store_dir.with_file_name(name.replacen("store", "spool", 1))
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
    /// The CPU pool's permits for the in-process toollets (tests: more than the
    /// host's cores, so a test of seven calls at once holds on a four-core host).
    /// The daemon sets none: a permit per core.
    pub cpu_cores: Option<usize>,
}

const META_LIVE_PROFILE: &str = "live_profile";

/// This very image, for the children that run it in a role (a job's wrapper,
/// an MCP server's L1 holder): after an in-place upgrade (copy, then rename
/// over the old file) the path on disk is a newer binary, or `current_exe()`
/// names a deleted file; `/proc/self/exe` is still us.
fn self_exe() -> std::path::PathBuf {
    match std::path::Path::new("/proc/self/exe") {
        p if p.exists() => p.to_path_buf(),
        _ => std::env::current_exe().unwrap_or_else(|_| "/proc/self/exe".into()),
    }
}

/// How old the last whole check of the WAL's history may be before a start
/// checks the whole log again, not only what was written since
/// (theseus-0dq): a day, for what rots where nothing writes.
pub const HISTORY_WHOLE_EVERY_MS: u64 = 24 * 60 * 60 * 1000;

/// Keys a stretch of the index's terms build reads and writes at once
/// (theseus-lv2): a few milliseconds of work, so a stop waits for little.
const TERMS_STRETCH: usize = 512;

/// Records a stretch of the index's shape build reads (theseus-vm3n.5).
const SHAPE_STRETCH: usize = 2048;

impl Core {
    /// The daemon's core, from its config and the secret board, which may
    /// still be resolving: nothing here waits for a secret (theseus-qa0).
    /// Each provider and the scrubber read their values from the board. The
    /// telemetry pipeline is built after serving (`install_telemetry`), once
    /// its headers secret resolves, so its client stays off the start path.
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
        // Wrappers run this very image (`self_exe`).
        let self_exe = self_exe();
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
            cpu_cores: None,
        })
    }

    /// The telemetry headers secret, when an export would carry it.
    fn telemetry_headers(cfg: &Config) -> Option<String> {
        cfg.telemetry
            .headers_secret
            .clone()
            .filter(|_| cfg.telemetry.endpoint().is_some())
    }

    /// Where the core's own facts go, outside a turn (`crate::fact`), for
    /// `session` or for none: their rows are written now, and their
    /// notifications go to the session's watchers (none: to no one).
    pub(crate) fn rec<'a>(&'a self, session: Option<&'a str>) -> crate::fact::Rec<'a> {
        crate::fact::Rec {
            narrator: &self.narrator,
            session,
            turn: None,
            to: session.map_or(crate::fact::To::Nobody, |s| {
                crate::fact::To::Session(&self.bus, s)
            }),
            store: &self.store,
        }
    }

    /// A session's own facts, outside a turn (`rec`).
    pub(crate) fn session_rec<'a>(&'a self, session: &'a str) -> crate::fact::Rec<'a> {
        self.rec(Some(session))
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

    /// Build the telemetry pipeline after serving, once its headers secret
    /// resolves. Fail closed: nothing is exported without its headers, and a
    /// failure waits for the secret's retry.
    pub async fn install_telemetry(self: Arc<Self>) {
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
                LedgerKind::SecretsResolved,
                None,
                None,
                json!({"names": st.ready, "ms": st.settled_ms, "method": st.method, "rounds": st.rounds}),
            )
        } else {
            LedgerRow::new(
                LedgerKind::SecretsFailed,
                None,
                None,
                json!({"failed": st.failed, "ready": st.ready, "ms": st.settled_ms, "method": st.method, "rounds": st.rounds, "retry_in_ms": st.retry_in_ms}),
            )
        };
        self.ledger_unless_closed(&row);
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
                LedgerKind::SecretsResolved,
                None,
                None,
                json!({"names": newly, "ms": self.startup_log.us(std::time::Instant::now()) / 1000, "method": st.method, "rounds": st.rounds, "still_failed": st.failed}),
            );
            self.ledger_unless_closed(&row);
            ready.extend(newly);
        }
    }

    /// Ledger `row`, unless the stop has written its last checkpoint
    /// (theseus-81kk): then it is dropped with a debug line, since a row
    /// after that checkpoint is replayed by the next start, and a clean
    /// stop leaves nothing to replay.
    pub(crate) fn ledger_unless_closed(&self, row: &LedgerRow) {
        let closed = self
            .closed
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closed {
            tracing::debug!(kind = %row.kind, "the stop's last checkpoint is written: a row after it is dropped");
            return;
        }
        if let Err(e) = self.store.append_ledger(row) {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    /// The stop's last checkpoint is next: from now on `ledger_unless_closed`
    /// writes nothing, and a write in progress ends first.
    pub(crate) fn close_late_rows(&self) {
        theseus_store::blocking(|| {
            *self
                .closed
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        });
    }

    /// Build a core from its parts: the kernel opened on the store and started
    /// (its spool beside the store), then the catalog, the tool runtime, and
    /// the turn runner.
    #[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
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
            cpu_cores,
        } = parts;
        let k0 = std::time::Instant::now();
        let cfg = Arc::new(cfg);
        // The kernel shares the store. Its spool sits beside the store dir.
        let spool = Spool::open(&spool_dir(store.dir())).context("opening completion spool")?;
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
        let kernel = Arc::new(
            Kernel::new(
                store.shared(),
                Arc::new(theseus_kernel::RealClock),
                cfg.kernel.to_kernel_config(),
            )
            .with_legacy_spend(legacy_spend),
        );
        let startup = kernel
            .startup(
                Some(&spool),
                &crate::aws::hands::overdue::Evidence(&WrapperEvidence {
                    spool: spool.clone(),
                }),
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
            spool = %spool.dir().display(),
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
        let mut tools = crate::toolrun::build_runtime(
            &cfg,
            Some(spool.clone()),
            scrubber,
            launcher,
            secrets.clone(),
        )?;
        for t in toollets {
            tools.registry.register(t);
        }
        if let Some(n) = cpu_cores {
            tools.cpu = crate::cpu::CpuPool::new(n);
        }
        let tools = Arc::new(tools);
        // The MCP servers' stored lists, one META key each, offered at once;
        // nothing starts until after serving (M7 36b).
        let mcp = crate::mcp::McpBoard::new(
            &cfg.mcp,
            tools.mcp.clone(),
            tools.broker.clone(),
            secrets.clone(),
            Arc::new(crate::mcp::Spawn {
                log_dir: crate::mcp::log_dir(store.dir()),
                cwd: tools.ctx.cwd.clone(),
                base_env: tools.proc_env.clone(),
                l1: crate::mcp::l1::L1Spawn {
                    exe: self_exe(),
                    sandbox: tools.sandbox.clone(),
                    umask: tools.ctx.umask,
                },
            }),
            |name| crate::mcp::read_stored(&store, name),
        );
        // The prompts' stored lists, and each prompt's definition as last
        // used (36c): the same, one key each.
        mcp.seed_prompts(
            |name| crate::mcp::prompts::read_stored(&store, name),
            |name| crate::mcp::prompts::read_used(&store, name),
        );
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
        // Read from the store on its first use, never on the start path.
        let outbox = Arc::new(crate::outbox::Outbox::new(store.clone(), kernel.clone()));
        // Nothing starts, and nothing is looked for, until after serving.
        let index = Arc::new(crate::tender::IndexTender::new(
            cfg.index.clone(),
            store.dir(),
            None,
            Arc::new(crate::tender::ChildrenOs),
        ));
        let runner = TurnRunner {
            memory: Arc::new(crate::recall::Memory::new(
                cfg.memory.clone(),
                Some(index.clone()),
            )),
            outbox: outbox.clone(),
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
            stops: Default::default(),
            latest_stops: Default::default(),
            // Told by the binding as it starts (the place rule).
            place_rule: Default::default(),
            // Built after serving, by one META scan (theseus-8kk.1).
            ontology: Default::default(),
            // Builds nothing until its first judgment (FAST).
            judge: crate::judge::JudgeService::new(
                cfg.judge.clone(),
                store.clone(),
                secrets.clone(),
                tools.scrubber.clone(),
            ),
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
            outbox,
            config_gate,
            restart: tokio::sync::watch::Sender::new(None),
            web_refusals: Arc::default(),
            last_sweep: Default::default(),
            push: crate::push::Push::default(),
            index,
            mcp,
            crash: Default::default(),
            closed: Default::default(),
            mcp_server: Default::default(),
        });
        core.index.set_ledger(index_ledger(&core));
        core.mcp.attach(Arc::downgrade(&core));
        // The language servers' rows go the same way (L2).
        if let Some(lsp) = &core.tools.lsp {
            lsp.set_ledger(index_ledger(&core));
        }
        core.tools.extend.attach(&core.mcp);
        // `server.started` waits for `announce_serving`: nothing on the start
        // path needs it durable, and its frame is an fsync (theseus-qa0).
        core.startup_log.record("core", false, c0, Value::Null);
        // Startup's step 2 gave the open sessions a changed spend limit
        // (theseus-3pj).
        core.said_limits_followed(&startup.limits_followed);
        Ok(core)
    }

    /// Once the socket answers: the kernel's startup report and the binary's
    /// build (`server.started`, theseus-9o5n) and the start path's phases
    /// (`server.serving`),
    /// in one frame, off the start path (theseus-qa0). When the store's open
    /// found an index that was not a database, moved it aside, and built it
    /// again from the WAL, `store.index_replaced` says so in the same frame
    /// (theseus-0b8).
    pub fn announce_serving(&self, serving_us: u64) {
        let phases: Vec<_> = self
            .startup_log
            .snapshot()
            .into_iter()
            .filter(|p| !p.background)
            .collect();
        let mut rows = vec![
            LedgerRow::new(
                LedgerKind::ServerStarted,
                None,
                None,
                json!({"startup": self.startup_report, "build": crate::build()}),
            ),
            LedgerRow::new(
                LedgerKind::ServerServing,
                None,
                None,
                json!({"serving_us": serving_us, "phases": phases}),
            ),
        ];
        if let Ok(st) = self.store.stats() {
            if let Some(m) = st.index_moved_aside {
                rows.push(LedgerRow::new(
                    LedgerKind::StoreIndexReplaced,
                    None,
                    None,
                    json!({"moved_aside": m.path, "bytes": m.bytes, "why": m.why,
                           "replayed_into_index": st.replayed_into_index,
                           "last_position": st.last_position}),
                ));
            }
        }
        let frame: Result<Vec<_>> = rows
            .iter()
            .map(|r| theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, r))
            .collect();
        if let Err(e) = frame.and_then(|f| self.store.append(&f)) {
            tracing::warn!(error = %e, "ledger append failed");
        }
    }

    /// The WAL's history, which the store's open left unchecked
    /// (theseus-8ni): checked once, after serving, on a thread of its own at
    /// about 5 % of one core (it sleeps 19 times each stretch's work). The
    /// background startup phase `store.verify` carries the outcome to health
    /// and the Observatory. A corrupt frame is loud: an error in the log, a
    /// `store.corrupt` ledger row, and its records' reads refused.
    ///
    /// It starts where the last check proved the log to (theseus-0dq): that
    /// check's last frame, checked again, then only what was written since.
    /// The whole log is checked again when the last whole check is a day
    /// old (`HISTORY_WHOLE_EVERY_MS`), for what rots where nothing writes.
    /// Its mark goes to the store's next checkpoint.
    ///
    /// The thread holds neither the core nor the store, only the check, the
    /// mark's slot, and a weak reference, so a daemon that stops meanwhile
    /// still drops its store and closes the index cleanly.
    pub fn check_store_history(self: &Arc<Self>) -> Option<std::thread::JoinHandle<()>> {
        let phase = self.startup_log.begin("store.verify", true, Instant::now());
        let inner = self.store.inner();
        let now = theseus_protocol::now_unix_ms();
        let mark = inner
            .verified()
            .filter(|v| now.saturating_sub(v.full_at_unix_ms) < HISTORY_WHOLE_EVERY_MS);
        let check = inner.history_check().from_mark(mark);
        let slot = inner.verified_slot();
        let log = self.startup_log.clone();
        let core = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("store-verify".into())
            .spawn(move || {
                let checked = check.run(|took| std::thread::sleep(took * 19));
                let detail = match checked {
                    Ok(h) => {
                        if let Some(v) = h.verified {
                            slot.set(v);
                        }
                        json!({
                            "outcome": "ok",
                            "segments": h.segments,
                            "frames": h.frames,
                            "records": h.records,
                            "bytes": h.bytes,
                            "busy_ms": (h.busy_ms * 10.0).round() / 10.0,
                            "checked_at_open": h.checked_at_open,
                            "from_position": h.from_position,
                        })
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "store: the WAL's history does not check; reads from the corrupt frame on are refused"
                        );
                        let row = LedgerRow::new(
                            LedgerKind::StoreCorrupt,
                            None,
                            None,
                            json!({"error": e.to_string()}),
                        );
                        if let Some(core) = core.upgrade() {
                            if let Err(e) = core.store.append_ledger(&row) {
                                tracing::warn!(error = %e, "ledger append failed");
                            }
                        }
                        json!({"outcome": "corrupt", "error": e.to_string()})
                    }
                };
                log.end(phase, detail);
            });
        match spawned {
            Ok(h) => Some(h),
            Err(e) => {
                self.startup_log
                    .end(phase, json!({"outcome": "not run", "error": e.to_string()}));
                None
            }
        }
    }

    /// The index's terms, built again after serving when the store's open
    /// found them not whole: a store an older build wrote last
    /// (theseus-lv2). A stretch of `TERMS_STRETCH` keys at a time on the
    /// blocking pool, so a stop waits for one stretch at most; until the
    /// last, the kernel's readers by state read every record, as before. The
    /// background startup phase `store.terms` says how many stretches, and
    /// how long. Then the index's shape, the same way (`build_store_shape`).
    /// Nothing when both are whole.
    pub fn build_store_terms(self: &Arc<Self>) {
        let store = self.store.inner();
        if store.terms_whole() && store.shaped() {
            return;
        }
        let core = Arc::downgrade(self);
        let log = self.startup_log.clone();
        let terms = (!store.terms_whole())
            .then(|| self.startup_log.begin("store.terms", true, Instant::now()));
        tokio::spawn(async move {
            if let Some(phase) = terms {
                let outcome = Self::build_terms_stretches(&core).await;
                log.end(phase, outcome);
            }
            let Some(c) = core.upgrade() else { return };
            c.build_store_shape();
        });
    }

    async fn build_terms_stretches(core: &std::sync::Weak<Self>) -> Value {
        let t0 = Instant::now();
        let (mut at, mut stretches) = (None, 0u64);
        loop {
            let Some(store) = core.upgrade().map(|c| c.store.inner().clone()) else {
                break json!({"outcome": "stopped", "stretches": stretches});
            };
            let from = at.take();
            match tokio::task::spawn_blocking(move || store.build_terms(from, TERMS_STRETCH)).await
            {
                Ok(Ok(Some(next))) => {
                    at = Some(next);
                    stretches += 1;
                }
                Ok(Ok(None)) => {
                    break json!({"outcome": "whole", "stretches": stretches,
                                 "ms": (t0.elapsed().as_secs_f64() * 1000.0).round()});
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %format!("{e:#}"), "store: building the index's terms failed; the kernel reads every record");
                    break json!({"outcome": "failed", "error": format!("{e:#}")});
                }
                Err(e) => break json!({"outcome": "failed", "error": e.to_string()}),
            }
        }
    }

    /// The index's shape (its counts, clocks, and tags), built again after
    /// serving when an older build wrote the index last (theseus-vm3n.5): a
    /// stretch of `SHAPE_STRETCH` records at a time on the blocking pool, so
    /// a stop waits for one stretch at most. Until the last, the counts walk
    /// and `ledger.tail`'s filtered reads scan, as before. The background
    /// startup phase `store.shape` says how many stretches, and how long.
    pub fn build_store_shape(self: &Arc<Self>) {
        if self.store.inner().shaped() {
            return;
        }
        let phase = self.startup_log.begin("store.shape", true, Instant::now());
        let log = self.startup_log.clone();
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            let t0 = Instant::now();
            let (mut at, mut stretches) = (None, 0u64);
            let outcome = loop {
                let Some(store) = core.upgrade().map(|c| c.store.inner().clone()) else {
                    break json!({"outcome": "stopped", "stretches": stretches});
                };
                let from = at.take();
                match tokio::task::spawn_blocking(move || store.build_shape(from, SHAPE_STRETCH))
                    .await
                {
                    Ok(Ok(Some(next))) => {
                        at = Some(next);
                        stretches += 1;
                    }
                    Ok(Ok(None)) => {
                        break json!({"outcome": "whole", "stretches": stretches,
                                     "ms": (t0.elapsed().as_secs_f64() * 1000.0).round()});
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(error = %format!("{e:#}"), "store: building the index's shape failed; its counts walk");
                        break json!({"outcome": "failed", "error": format!("{e:#}")});
                    }
                    Err(e) => break json!({"outcome": "failed", "error": e.to_string()}),
                }
            };
            log.end(phase, outcome);
        });
    }

    /// Sweep the spool's raw job output (theseus-2ij), then say so: a
    /// `spool.swept` row, never content, for a sweep that removed a file, and
    /// for the first sweep of a start that found any; and health's last
    /// sweep, always. An empty spool writes nothing, and neither does an
    /// hourly sweep that only keeps what a turn may still read.
    pub fn sweep_spool(&self, first: bool) -> theseus_protocol::SpoolSweep {
        let s = crate::sweep::sweep(
            &self.kernel,
            &self.store,
            &self.spool,
            std::time::SystemTime::now(),
        );
        if s.removed > 0 || (first && s.kept > 0) {
            let row = LedgerRow::new(
                LedgerKind::SpoolSwept,
                None,
                None,
                serde_json::to_value(&s).unwrap_or(Value::Null),
            );
            if let Err(e) = self.store.append_ledger(&row) {
                tracing::warn!(error = %e, "ledger append failed");
            }
        }
        if s.removed > 0 {
            tracing::info!(
                removed = s.removed,
                bytes = s.removed_bytes,
                kept = s.kept,
                "the spool's sweep removed raw job output no result will absorb"
            );
        }
        *self.last_sweep.lock().unwrap() = Some(s.clone());
        s
    }

    /// The spool's last sweep, for health.
    pub fn spool_status(&self) -> theseus_protocol::SpoolStatus {
        theseus_protocol::SpoolStatus {
            last_sweep: self.last_sweep.lock().unwrap().clone(),
        }
    }

    /// The store's refused reads, for health (R4, theseus-15g): the records
    /// list reads skipped because their frame is corrupt, and what repairs it.
    pub fn store_status(&self) -> theseus_protocol::StoreStatus {
        let st = self.store.stats().ok();
        let refused_records = st.as_ref().map_or(0, |s| s.refused_records);
        theseus_protocol::StoreStatus {
            refused_records,
            refused_positions: st.map(|s| s.refused_positions).unwrap_or_default(),
            repair: (refused_records > 0).then(|| crate::restore::REPAIR.into()),
        }
    }

    /// The crash file the last run left (Review 2's consideration 1), taken
    /// after serving: moved into `crashes/`, said in the log and in a
    /// `server.crashed` row, and kept for health. A start that finds none
    /// keeps the newest one an earlier start found, for health.
    pub fn report_crash(&self, state_dir: &std::path::Path, mode: &str) {
        let status = |c: &crate::crash::Crash, file: &std::path::Path, this_start: bool| {
            theseus_protocol::CrashStatus {
                at_unix_ms: c.at_unix_ms,
                pid: c.pid,
                version: c.version.clone(),
                thread: c.thread.clone(),
                location: c.location.clone(),
                file: file.display().to_string(),
                this_start,
            }
        };
        let found = crate::crash::take(state_dir, mode).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), "the last run's crash file could not be read");
            None
        });
        let kept = match found {
            Some((c, file)) => {
                tracing::warn!(at_unix_ms = c.at_unix_ms, pid = c.pid, thread = %c.thread,
                    location = %c.location, file = %file.display(),
                    "the last run crashed: a panic; its crash file is kept");
                self.rec(None).record(&crate::fact::start::CrashFound {
                    crash: &c,
                    file: &file.display().to_string(),
                });
                Some(status(&c, &file, true))
            }
            None => crate::crash::last(state_dir, mode).map(|(c, f)| status(&c, &f, false)),
        };
        *self.crash.lock().unwrap() = kept;
    }

    /// The newest crash a start found, for health.
    pub fn crash_status(&self) -> Option<theseus_protocol::CrashStatus> {
        self.crash.lock().unwrap().clone()
    }

    /// The spool's sweeps as a tender after serving (theseus-2ij), never on
    /// the start path: the first now, then one every hour, each on the
    /// blocking pool. Between sweeps it holds the core only weakly, so a
    /// stopped daemon's core is dropped as before.
    pub fn sweep_spool_after_serving(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut first = true;
            loop {
                let Some(c) = core.upgrade() else {
                    return;
                };
                let _ = tokio::task::spawn_blocking(move || c.sweep_spool(first)).await;
                first = false;
                tokio::time::sleep(crate::sweep::EVERY).await;
            }
        });
    }

    pub fn live_profile(&self) -> (String, String) {
        self.live.read().unwrap().clone()
    }

    /// Restart onto the vault's changed config note (theseus-2fo): the clean
    /// shutdown path runs, and `theseusd` execs its own image with its own
    /// arguments once it sees `restart_requested`. A turn the stop interrupts
    /// resumes after it, as after any stop (theseus-zmgb).
    pub fn restart_onto(&self, r: ConfigRestart) {
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
             The daemon restarts onto the vault's version.",
            r.tables.join(", ")
        );
        self.config_gate.restarting(r.clone());
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
            cpu_cores: None,
        }
    }
}
