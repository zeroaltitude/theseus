//! Whether the config a start serves from may act (theseus-2fo, spec §3.19).
//!
//! A start from the last-known-good copy of the vault's note answers every
//! method that only reads at once. Every method that acts waits here, bounded
//! like the secrets, and the actors (the harness loop, the driver, the web
//! UI, Discord, telemetry, the GitHub check) start only once the vault
//! confirms the copy. The secrets come from the same vault at the same
//! moment, and every action already waits for them, so the wait costs
//! nothing in practice.
//!
//! When the vault's note differs, the daemon ledgers `config.changed`,
//! rewrites the copy, and restarts onto the vault's version. A process that
//! began as such a restart and finds the note changed again holds instead,
//! so a restart never loops.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde_json::json;
use theseus_protocol::{ConfigRestart, ConfigStatus};
use tokio::sync::watch;

use crate::config_copy::{self, Compared};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::secrets::{OpReader, SecretRef};
use crate::{Config, Core};

/// The longest an acting method waits for the vault to confirm the copy: the
/// secrets' bound, since both come from the same vault at the same moment.
pub const CONFIG_WAIT: Duration = crate::turn::SECRET_WAIT;

/// In the environment of a process that restarted itself onto the vault's
/// changed note: what changed, as a `ConfigRestart` in JSON. It is what
/// stops a second restart.
pub const RESTARTED_ENV: &str = "THESEUS_RESTARTED_ONTO_VAULT";

/// A held read is tried again after 5 s, then at doubling intervals up to a
/// minute, as a failed secret is.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(60);

/// Where the gate stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// The config may act: a file, a vault read before serving, or a copy
    /// the vault confirmed.
    Open,
    /// Serving from the copy, and the vault has not answered yet.
    Confirming,
    /// The vault answered, and the copy may not act: why, in words.
    Held(String),
    /// The vault's note changed, and the daemon restarts onto it.
    Restarting,
}

#[derive(Default)]
struct Info {
    source: String,
    reference: String,
    started_from: String,
    copy: Option<PathBuf>,
    /// The text this start served from, while the vault has not confirmed it.
    copy_text: String,
    /// How it was confirmed, or why no copy is kept, in words.
    detail: Option<String>,
    confirmed: Option<Instant>,
    reads: u32,
    retry_at: Option<Instant>,
    /// This process began as a restart onto a changed note.
    restarted: Option<ConfigRestart>,
    /// The restart this process asked for.
    restarting: Option<ConfigRestart>,
}

pub struct ConfigGate {
    tx: watch::Sender<Gate>,
    /// Process start, for the times health reports.
    origin: Instant,
    info: Mutex<Info>,
    /// How long an acting method waits, and the first retry of a held read
    /// (tests shorten both).
    wait_max: Duration,
    retry_first: Duration,
}

impl ConfigGate {
    fn new(gate: Gate, origin: Instant, info: Info) -> Arc<Self> {
        Arc::new(Self {
            tx: watch::Sender::new(gate),
            origin,
            info: Mutex::new(info),
            wait_max: CONFIG_WAIT,
            retry_first: RETRY_FIRST,
        })
    }

    /// A config file: read as it is, with no copy and nothing to confirm.
    pub fn file(path: &str) -> Arc<Self> {
        Self::new(
            Gate::Open,
            Instant::now(),
            Info {
                source: "file".into(),
                reference: path.into(),
                started_from: "file".into(),
                ..Default::default()
            },
        )
    }

    /// A vault note read before serving (a first start, or one whose copy
    /// could not serve): it is the vault's own word, so it may act at once.
    pub fn vault(
        reference: &str,
        copy: PathBuf,
        origin: Instant,
        read_at: Instant,
        detail: String,
    ) -> Arc<Self> {
        Self::new(
            Gate::Open,
            origin,
            Info {
                source: "vault".into(),
                reference: reference.into(),
                started_from: "vault".into(),
                copy: Some(copy),
                detail: Some(detail),
                confirmed: Some(read_at),
                restarted: restarted_from_env(),
                ..Default::default()
            },
        )
    }

    /// Serving from the copy at `copy`, whose text is `text`, until the vault
    /// confirms it.
    pub fn from_copy(reference: &str, copy: PathBuf, text: String, origin: Instant) -> Arc<Self> {
        Self::new(
            Gate::Confirming,
            origin,
            Info {
                source: "vault".into(),
                reference: reference.into(),
                started_from: "copy".into(),
                copy: Some(copy),
                copy_text: text,
                restarted: restarted_from_env(),
                ..Default::default()
            },
        )
    }

    /// The same gate with shorter waits, for tests.
    #[cfg(test)]
    pub fn with_timings(self: Arc<Self>, wait_max: Duration, retry_first: Duration) -> Arc<Self> {
        let me = Arc::try_unwrap(self).ok().expect("a new gate");
        Arc::new(Self {
            wait_max,
            retry_first,
            ..me
        })
    }

    /// The restart marker, for tests of a process that began as a restart.
    #[cfg(test)]
    pub fn began_as_restart(&self, r: ConfigRestart) {
        self.info.lock().unwrap().restarted = Some(r);
    }

    pub fn state(&self) -> Gate {
        self.tx.borrow().clone()
    }

    pub fn is_open(&self) -> bool {
        *self.tx.borrow() == Gate::Open
    }

    pub fn reference(&self) -> String {
        self.info.lock().unwrap().reference.clone()
    }

    /// Add to what health says of the config (a first start's copy).
    pub fn append_detail(&self, more: &str) {
        let mut i = self.info.lock().unwrap();
        i.detail = Some(match i.detail.take() {
            Some(d) => format!("{d}; {more}"),
            None => more.to_string(),
        });
    }

    /// An acting method's wait, bounded: how long it waited (zero when the
    /// config may act already, so a turn's trace shows no wait), or why it
    /// may not act (`config_unconfirmed`).
    pub async fn wait(&self) -> Result<Duration, String> {
        if self.is_open() {
            return Ok(Duration::ZERO);
        }
        let t0 = Instant::now();
        let mut rx = self.tx.subscribe();
        let settled = tokio::time::timeout(
            self.wait_max,
            rx.wait_for(|g| matches!(g, Gate::Open | Gate::Restarting)),
        )
        .await;
        match settled {
            Ok(Ok(g)) if *g == Gate::Open => Ok(t0.elapsed()),
            _ => Err(self.why_not(self.wait_max)),
        }
    }

    /// Why an acting method may not act now.
    fn why_not(&self, waited: Duration) -> String {
        let reference = self.reference();
        match self.state() {
            Gate::Restarting => "the vault's config note changed since the copy this daemon \
                                 started from, and it is restarting onto the vault's version: \
                                 send this again once it answers"
                .to_string(),
            Gate::Held(why) => format!(
                "the config is held: {why}; nothing acts until the vault confirms the copy of \
                 {reference} this daemon started from"
            ),
            Gate::Confirming | Gate::Open => format!(
                "the vault has not confirmed the config this daemon started from (its copy of \
                 {reference}) within {} s; nothing acts until it does",
                waited.as_secs()
            ),
        }
    }

    /// Wait, unbounded, until the config may act: true, or false when the
    /// daemon restarts instead. The actors start behind this.
    pub async fn opened(&self) -> bool {
        let mut rx = self.tx.subscribe();
        matches!(
            rx.wait_for(|g| matches!(g, Gate::Open | Gate::Restarting))
                .await
                .map(|g| g.clone()),
            Ok(Gate::Open)
        )
    }

    /// The restart marker this process began with.
    pub fn restarted(&self) -> Option<ConfigRestart> {
        self.info.lock().unwrap().restarted.clone()
    }

    fn copy_text(&self) -> String {
        self.info.lock().unwrap().copy_text.clone()
    }

    fn copy_path(&self) -> Option<PathBuf> {
        self.info.lock().unwrap().copy.clone()
    }

    fn begin_read(&self) {
        let mut i = self.info.lock().unwrap();
        i.reads += 1;
        i.retry_at = None;
    }

    fn confirm(&self, detail: String) {
        {
            let mut i = self.info.lock().unwrap();
            i.confirmed = Some(Instant::now());
            i.detail = Some(detail);
            i.copy_text.clear();
        }
        self.tx.send_replace(Gate::Open);
    }

    fn hold(&self, why: String, retry_at: Instant) {
        self.info.lock().unwrap().retry_at = Some(retry_at);
        self.tx.send_replace(Gate::Held(why));
    }

    pub(crate) fn restarting(&self, r: ConfigRestart) {
        self.info.lock().unwrap().restarting = Some(r);
        self.tx.send_replace(Gate::Restarting);
    }

    /// For health.
    pub fn status(&self) -> ConfigStatus {
        let gate = self.state();
        let i = self.info.lock().unwrap();
        let ms = |t: Instant| t.saturating_duration_since(self.origin).as_millis() as u64;
        let (state, detail) = match &gate {
            Gate::Open => ("confirmed", i.detail.clone()),
            Gate::Confirming => (
                "confirming",
                Some("serving from the copy; nothing acts until the vault confirms it".into()),
            ),
            Gate::Held(why) => ("held", Some(why.clone())),
            Gate::Restarting => (
                "restarting",
                Some(format!(
                    "the vault's note changed since the copy ({}); restarting onto it",
                    i.restarting
                        .as_ref()
                        .map(|r| r.tables.join(", "))
                        .unwrap_or_default()
                )),
            ),
        };
        ConfigStatus {
            source: i.source.clone(),
            reference: i.reference.clone(),
            state: state.into(),
            started_from: i.started_from.clone(),
            detail,
            confirmed_ms: i.confirmed.map(ms),
            copy: i.copy.as_ref().map(|p| p.display().to_string()),
            reads: i.reads,
            retry_in_ms: matches!(gate, Gate::Held(_))
                .then_some(i.retry_at)
                .flatten()
                .map(|t| t.saturating_duration_since(Instant::now()).as_millis() as u64),
            restarted: i.restarted.clone(),
        }
    }
}

/// The marker a restart onto a changed note leaves in its environment.
fn restarted_from_env() -> Option<ConfigRestart> {
    let raw = std::env::var(RESTARTED_ENV).ok()?;
    serde_json::from_str(&raw)
        .map_err(|e| tracing::warn!(error = %e, "{RESTARTED_ENV} is not a restart marker; ignored"))
        .ok()
}

/// Where the config note is read from: the vault through `op read` in the
/// daemon, a scripted note in tests.
pub trait ReadNote: Send + Sync {
    fn read_note<'a>(&'a self, reference: &'a str) -> BoxFuture<'a, Result<String, String>>;
}

impl ReadNote for OpReader {
    fn read_note<'a>(&'a self, reference: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let r = SecretRef::parse(reference).map_err(|e| format!("{e:#}"))?;
            self.read(&r)
                .await
                .map(|s| s.expose().to_string())
                .map_err(|e| format!("{e:#}"))
        })
    }
}

/// What one read of the vault decided.
enum Step {
    Confirmed(String),
    Restart(ConfigRestart),
    /// Why, and the ledger row that says so.
    Held(String, LedgerRow),
}

/// Read the vault's note behind the socket until it confirms the copy this
/// start served from, or the daemon restarts onto a changed note. `first`
/// is the read begun at process start, beside the secrets' `op inject`; the
/// `config.vault` startup phase closes with the first answer.
pub async fn confirm(
    core: Arc<Core>,
    reader: Arc<dyn ReadNote>,
    first: Option<tokio::task::JoinHandle<Result<String, String>>>,
    began: Instant,
) {
    let gate = core.config_gate.clone();
    let reference = gate.reference();
    let phase = Some(core.startup_log.begin("config.vault", true, began));
    let mut phase = phase;
    if let Some(r) = gate.restarted() {
        narrate!(
            core.narrator,
            Config,
            None,
            None,
            "This daemon began as a restart onto the vault's config note, which had changed \
             since the copy it started from ({}). It serves from the rewritten copy, and nothing \
             acts until the vault confirms it.",
            r.tables.join(", ")
        );
    }
    let mut first = first;
    let mut delay = gate.retry_first;
    let mut said: Option<String> = None;
    loop {
        gate.begin_read();
        let t0 = Instant::now();
        let answer = match first.take() {
            Some(h) => h
                .await
                .unwrap_or_else(|e| Err(format!("the read stopped: {e}"))),
            None => reader.read_note(&reference).await,
        };
        let read_ms = t0.elapsed().as_millis() as u64;
        let step = judge(&core, &reference, answer);
        let outcome = match &step {
            Step::Confirmed(_) => "confirmed",
            Step::Restart(_) => "changed",
            Step::Held(..) => "held",
        };
        if let Some(p) = phase.take() {
            core.startup_log.end(
                p,
                json!({"outcome": outcome, "reference": reference, "read_ms": read_ms}),
            );
        }
        match step {
            Step::Confirmed(how) => {
                let reads = gate.status().reads;
                let restarted = gate.restarted();
                // The open sessions take the vault's spend limit before
                // anything may act (theseus-3pj).
                core.follow_spend_limit();
                gate.confirm(how.clone());
                ledger(
                    &core,
                    LedgerRow::new(
                        "config.confirmed",
                        None,
                        None,
                        json!({"reference": reference, "how": how, "ms": gate.status().confirmed_ms, "reads": reads, "restarted": restarted}),
                    ),
                );
                narrate!(
                    core.narrator,
                    Config,
                    None,
                    None,
                    "The vault confirmed the config this daemon started from ({how}), {} after \
                     the start; the actors start.",
                    crate::narrative::duration(gate.status().confirmed_ms.unwrap_or(0))
                );
                tracing::info!(reference = %reference, how = %how, reads, "config confirmed by the vault");
                return;
            }
            Step::Restart(r) => {
                core.restart_onto(r).await;
                return;
            }
            Step::Held(why, row) => {
                if said.as_deref() != Some(why.as_str()) {
                    tracing::warn!(reference = %reference, why = %why, "config held: answering reads only");
                    ledger(&core, row);
                    narrate!(
                        core.narrator,
                        Config,
                        None,
                        None,
                        "The config is held: {why}. The daemon answers reads only, and reads the \
                         vault again in {}.",
                        crate::narrative::duration(delay.as_millis() as u64)
                    );
                    said = Some(why.clone());
                }
                gate.hold(why, Instant::now() + delay);
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(RETRY_MAX);
            }
        }
    }
}

fn ledger(core: &Core, row: LedgerRow) {
    if let Err(e) = core.store.append_ledger(&row) {
        tracing::warn!(error = %e, "ledger append failed");
    }
}

/// Judge one answer from the vault against the copy this start served from.
fn judge(core: &Core, reference: &str, answer: Result<String, String>) -> Step {
    let gate = &core.config_gate;
    let row = |kind: &str, data: serde_json::Value| LedgerRow::new(kind, None, None, data);
    let text = match answer {
        Ok(t) => t,
        Err(why) => {
            return Step::Held(
                format!("the vault did not answer: {why}"),
                row(
                    "config.unreachable",
                    json!({"reference": reference, "error": why}),
                ),
            )
        }
    };
    let vault = match Config::parse(&text) {
        Ok((cfg, _)) => cfg,
        Err(e) => {
            let e = format!("{e:#}");
            return Step::Held(
                format!("the vault's note does not load ({e}); the copy is left as it was"),
                row(
                    "config.invalid",
                    json!({"reference": reference, "vault_sha256": config_copy::sha256(&text), "error": e}),
                ),
            );
        }
    };
    let copy = gate.copy_text();
    let path = gate.copy_path();
    let write = |text: &str| match &path {
        Some(p) => config_copy::write(p, reference, text).map_err(|e| format!("{e:#}")),
        None => Ok(()),
    };
    match config_copy::compare(&copy, &text) {
        Compared::Same => Step::Confirmed("the same text as the copy".into()),
        Compared::Comments => {
            if let Err(e) = write(&text) {
                tracing::warn!(error = %e, "the copy could not be rewritten");
            }
            Step::Confirmed(
                "only comments or formatting differed, and the copy was rewritten".into(),
            )
        }
        Compared::Changed(tables) => {
            let r = ConfigRestart {
                reference: reference.into(),
                at_unix_ms: theseus_protocol::now_unix_ms(),
                tables,
                copy_sha256: config_copy::sha256(&copy),
                vault_sha256: config_copy::sha256(&text),
            };
            if gate.restarted().is_some() {
                return Step::Held(
                    "the vault's note changed again since the restart; restart to apply".into(),
                    row(
                        "config.held",
                        json!({"why": "changed again since the restart", "change": r}),
                    ),
                );
            }
            ledger(
                core,
                row(
                    "config.changed",
                    serde_json::to_value(&r).unwrap_or_default(),
                ),
            );
            // The restart starts from the vault's version: the copy, or,
            // for a note that may not be kept, no copy and a read first.
            let kept = match (vault.credential_in_url(), &path) {
                (Some(field), Some(p)) => {
                    tracing::warn!(field = %field, "the vault's note carries a credential in a URL: no copy is kept, and the restart reads the vault first");
                    match std::fs::remove_file(p) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                        _ => Ok(()),
                    }
                }
                _ => write(&text),
            };
            match kept {
                Ok(()) => Step::Restart(r),
                Err(e) => Step::Held(
                    format!(
                        "the vault's note changed ({}), and the copy could not be rewritten ({e}); \
                         restart to apply",
                        r.tables.join(", ")
                    ),
                    row(
                        "config.held",
                        json!({"why": "the copy could not be rewritten", "error": e, "change": r}),
                    ),
                ),
            }
        }
    }
}
