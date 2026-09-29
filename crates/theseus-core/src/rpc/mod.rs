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

use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::bus::SessionBus;
use crate::catalog::Catalog;
use crate::ledger::LedgerRow;
use crate::narrative::Narrator;
use crate::provider::{Anthropic, Provider};
use crate::scrub::Scrubber;
use crate::secrets::Secrets;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::telemetry::Telemetry;
use crate::toolrun::{JobLauncher, ToolRuntime, WrapperLauncher};
use crate::turn::TurnRunner;
use crate::Config;
use theseus_kernel::job::WrapperEvidence;
use theseus_kernel::{Kernel, Spool};

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
    pub telemetry: Arc<Telemetry>,
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
}

/// What a `Core` is built from. `Core::new` resolves these from the config and
/// the vault's secrets; tests start from `Parts::for_tests`.
pub struct Parts {
    pub cfg: Config,
    pub providers: BTreeMap<String, Arc<dyn Provider>>,
    pub store: Store,
    /// The resolved secrets' names, for health; never their values.
    pub secret_names: Vec<String>,
    pub telemetry: Telemetry,
    pub scrubber: Arc<Scrubber>,
    pub launcher: Arc<dyn JobLauncher>,
}

const META_LIVE_PROFILE: &str = "live_profile";

impl Core {
    /// The daemon's core, from its config and the resolved secrets. It takes
    /// the secrets by value and they go when it returns: the providers, the
    /// telemetry headers, and the scrubber keep what each needs, and the daemon
    /// keeps no other copy.
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
            providers.insert(
                name.clone(),
                Arc::new(Anthropic::new(&pc.api_base, key, timeouts)?),
            );
        }
        let headers = cfg
            .telemetry
            .headers_secret
            .as_deref()
            .and_then(|n| secrets.get(n));
        let telemetry = crate::telemetry::Telemetry::from_config(&cfg.telemetry, headers)?;
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
        Self::build(Parts {
            cfg,
            providers,
            store,
            secret_names: secrets.names(),
            telemetry,
            scrubber,
            launcher,
        })
    }

    /// Build a core from its parts: the kernel opened on the store and started
    /// (its spool beside the store), then the catalog, the tool runtime, and
    /// the turn runner.
    pub fn build(parts: Parts) -> Result<Arc<Self>> {
        let Parts {
            cfg,
            providers,
            store,
            secret_names,
            telemetry,
            scrubber,
            launcher,
        } = parts;
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
        let tools = Arc::new(crate::toolrun::build_runtime(
            &cfg,
            Some(spool.clone()),
            scrubber,
            launcher,
        )?);
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
            secret_names,
            telemetry: Arc::new(telemetry),
            narrator,
            started: Instant::now(),
            provider_errors: AtomicU64::new(0),
            live: std::sync::RwLock::new(live),
            shutdown: tokio::sync::Notify::new(),
            bindings: BindingBoard::default(),
            approval,
        });
        core.store.append_ledger(&LedgerRow::new(
            "server.started",
            None,
            None,
            json!({"startup": core.startup_report}),
        ))?;
        Ok(core)
    }

    pub fn live_profile(&self) -> (String, String) {
        self.live.read().unwrap().clone()
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
            secret_names: vec![],
            telemetry: Telemetry::disabled(),
            scrubber: Arc::new(Scrubber::default()),
            launcher: Arc::new(crate::toolrun::InlineLauncher),
        }
    }
}
