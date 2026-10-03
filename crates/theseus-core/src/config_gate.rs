//! Where the config came from, and the vault's word on the copy a start
//! served from (theseus-2fo, spec §3.19; theseus-zmgb).
//!
//! A start from the last-known-good copy of the vault's note acts on it at
//! once: `theseusd` serves from a copy only when its digest is the one the
//! daemon recorded as it wrote it (`config_copy::written_by_daemon`), and
//! reads the vault first otherwise. After serving, the vault is read once
//! (`check`):
//!
//! - the same text: nothing to do;
//! - only comments or formatting differ: the copy and its digest are
//!   rewritten;
//! - a changed note: `config.changed`, the copy and its digest rewritten, and
//!   a restart onto it, in place (`Core::restart_onto`);
//! - a note that changed again after such a restart, a note that does not
//!   load, or a vault that does not answer: ledgered, logged, narrated, and
//!   said in health, and the daemon keeps serving the copy. A restart never
//!   loops, and the vault is read again at the next start.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use theseus_protocol::LedgerKind;

use futures_util::future::BoxFuture;
use serde_json::json;
use theseus_protocol::{ConfigRestart, ConfigStatus};

use crate::config_copy::{self, Compared};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::secrets::{OpReader, SecretRef};
use crate::{Config, Core};

/// In the environment of a process that restarted itself onto the vault's
/// changed note: what changed, as a `ConfigRestart` in JSON. It is what
/// stops a second restart.
pub const RESTARTED_ENV: &str = "THESEUS_RESTARTED_ONTO_VAULT";

/// What the vault said of the config this start serves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum State {
    /// A file, a note read before serving, or a copy the vault agrees with.
    #[default]
    Confirmed,
    /// Acting on the copy, and the vault has not answered yet.
    Confirming,
    /// The vault's read did not settle the copy, which keeps serving: why,
    /// in words.
    Held(String),
    /// The vault's note changed, and the daemon restarts onto it.
    Restarting,
}

#[derive(Default)]
struct Info {
    state: State,
    source: String,
    reference: String,
    started_from: String,
    copy: Option<PathBuf>,
    /// The text this start served from, until the vault has answered.
    copy_text: String,
    /// How it was confirmed, why it is held, or why no copy is kept, in words.
    detail: Option<String>,
    confirmed: Option<Instant>,
    reads: u32,
    /// This process began as a restart onto a changed note.
    restarted: Option<ConfigRestart>,
    /// The restart this process asked for.
    restarting: Option<ConfigRestart>,
}

pub struct ConfigGate {
    /// Process start, for the times health reports.
    origin: Instant,
    info: Mutex<Info>,
}

impl ConfigGate {
    fn new(state: State, origin: Instant, info: Info) -> Arc<Self> {
        Arc::new(Self {
            origin,
            info: Mutex::new(Info { state, ..info }),
        })
    }

    /// A config file: read as it is, with no copy and nothing to check.
    pub fn file(path: &str) -> Arc<Self> {
        Self::new(
            State::Confirmed,
            Instant::now(),
            Info {
                source: "file".into(),
                reference: path.into(),
                started_from: "file".into(),
                ..Default::default()
            },
        )
    }

    /// A vault note read before serving: a first start, one whose copy
    /// could not serve, or a restart onto a changed note whose copy could
    /// not be kept. `detail` says why it read the vault first.
    pub fn vault(
        reference: &str,
        copy: PathBuf,
        origin: Instant,
        read_at: Instant,
        detail: String,
    ) -> Arc<Self> {
        Self::new(
            State::Confirmed,
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

    /// Serving from the copy at `copy`, whose text is `text`, and acting on
    /// it, while the vault is read.
    pub fn from_copy(reference: &str, copy: PathBuf, text: String, origin: Instant) -> Arc<Self> {
        Self::new(
            State::Confirming,
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

    /// The restart marker, for tests of a process that began as a restart.
    #[cfg(test)]
    pub fn began_as_restart(&self, r: ConfigRestart) {
        self.info.lock().unwrap().restarted = Some(r);
    }

    pub fn state(&self) -> State {
        self.info.lock().unwrap().state.clone()
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

    fn settle(&self, state: State, detail: String) {
        let mut i = self.info.lock().unwrap();
        i.reads += 1;
        if state == State::Confirmed {
            i.confirmed = Some(Instant::now());
        }
        i.state = state;
        i.detail = Some(detail);
        i.copy_text.clear();
    }

    pub(crate) fn restarting(&self, r: ConfigRestart) {
        let mut i = self.info.lock().unwrap();
        i.reads += 1;
        i.restarting = Some(r);
        i.state = State::Restarting;
    }

    /// For health.
    pub fn status(&self) -> ConfigStatus {
        let i = self.info.lock().unwrap();
        let ms = |t: Instant| t.saturating_duration_since(self.origin).as_millis() as u64;
        let (state, detail) = match i.state.clone() {
            State::Confirmed => ("confirmed", i.detail.clone()),
            State::Confirming => (
                "confirming",
                Some(
                    "acting on the copy, which is the one the daemon wrote; the vault is being \
                     read"
                        .into(),
                ),
            ),
            State::Held(why) => ("held", Some(why)),
            State::Restarting => (
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

/// What the one read of the vault decided.
enum Step {
    Confirmed(String),
    Restart(ConfigRestart),
    /// Why, and the ledger row that says so.
    Held(String, LedgerRow),
}

/// Read the vault's note once, behind the socket, and settle the copy this
/// start served from: confirmed, rewritten, a restart onto a changed note,
/// or held, serving the copy. `first` is the read begun at process start,
/// beside the secrets' `op inject`; the `config.vault` startup phase closes
/// with its answer.
pub async fn check(
    core: Arc<Core>,
    reader: Arc<dyn ReadNote>,
    first: Option<tokio::task::JoinHandle<Result<String, String>>>,
    began: Instant,
) {
    let gate = core.config_gate.clone();
    let reference = gate.reference();
    let phase = core.startup_log.begin("config.vault", true, began);
    if let Some(r) = gate.restarted() {
        narrate!(
            core.narrator,
            Config,
            None,
            None,
            "This daemon began as a restart onto the vault's config note, which had changed \
             since the copy it started from ({}). It serves from the rewritten copy.",
            r.tables.join(", ")
        );
    }
    let t0 = Instant::now();
    let answer = match first {
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
    core.startup_log.end(
        phase,
        json!({"outcome": outcome, "reference": reference, "read_ms": read_ms}),
    );
    match step {
        Step::Confirmed(how) => {
            tracing::info!(reference = %reference, how = %how, "the vault agrees with the config copy");
            gate.settle(State::Confirmed, how);
        }
        Step::Restart(r) => core.restart_onto(r),
        Step::Held(why, row) => {
            tracing::warn!(reference = %reference, why = %why, "the vault's read did not settle the config copy; serving it");
            ledger(&core, row);
            narrate!(
                core.narrator,
                Config,
                None,
                None,
                "The vault's read did not settle the config copy: {why}. The daemon keeps \
                 serving the copy it started from, and reads the vault again at its next start."
            );
            gate.settle(State::Held(why.clone()), why);
        }
    }
}

fn ledger(core: &Core, row: LedgerRow) {
    if let Err(e) = core.store.append_ledger(&row) {
        tracing::warn!(error = %e, "ledger append failed");
    }
}

/// Judge the vault's answer against the copy this start served from.
fn judge(core: &Core, reference: &str, answer: Result<String, String>) -> Step {
    let gate = &core.config_gate;
    let row = |kind: LedgerKind, data: serde_json::Value| LedgerRow::new(kind, None, None, data);
    let text = match answer {
        Ok(t) => t,
        Err(why) => {
            return Step::Held(
                format!("the vault did not answer: {why}"),
                row(
                    LedgerKind::ConfigUnreachable,
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
                    LedgerKind::ConfigInvalid,
                    json!({"reference": reference, "vault_sha256": config_copy::sha256(&text), "error": e}),
                ),
            );
        }
    };
    let copy = gate.copy_text();
    let path = gate.copy_path();
    let keep = |text: &str| match &path {
        Some(p) => config_copy::keep(&core.store, p, reference, text).map_err(|e| format!("{e:#}")),
        None => Ok(()),
    };
    match config_copy::compare(&copy, &text) {
        Compared::Same => Step::Confirmed("the same text as the copy".into()),
        Compared::Comments => {
            if let Err(e) = keep(&text) {
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
                        LedgerKind::ConfigHeld,
                        json!({"why": "changed again since the restart", "change": r}),
                    ),
                );
            }
            ledger(
                core,
                row(
                    LedgerKind::ConfigChanged,
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
                _ => keep(&text),
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
                        LedgerKind::ConfigHeld,
                        json!({"why": "the copy could not be rewritten", "error": e, "change": r}),
                    ),
                ),
            }
        }
    }
}
