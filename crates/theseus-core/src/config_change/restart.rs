//! The restart after an approved change, and the way back (issue 22, line 2).
//!
//! An approved change that needs a restart writes a marker, `pending.json`,
//! beside the numbered copies, and the daemon restarts in place by the
//! existing path (`Core::stop` and `theseusd`'s exec of its own image), which
//! passes the marker's path in `THESEUS_CONFIG_CHANGE`. The marker is the
//! whole memory of the change, in a file, because a start whose settings do
//! not load has no store to remember in.
//!
//! The start that follows reads it (`begin`):
//!
//! - **`restarting`**: the first start onto the change. It marks the marker
//!   `started` and goes on; once serving, [`Core::confirm_change`] waits up to
//!   [`HEALTHY_WITHIN_SECS`] for the daemon to be healthy (it serves its
//!   socket, its store opened, and the index tender is off or not `down` or
//!   `stalled`) and then marks it `confirmed` and tells the owner.
//! - **the settings do not load**: `theseusd` calls [`roll_back`] at once.
//! - **not healthy in time**: [`roll_back`], then one more restart.
//! - **`started`**: a start onto the change that never confirmed (it died) is
//!   found by the next one, which rolls back at once.
//!
//! [`roll_back`] puts the copy kept before the change back as the config, and
//! only when the file is still exactly what the change wrote and the copy is
//! exactly what was kept: an edit by the owner since is never overwritten, and
//! a copy someone replaced is never restored. It marks the marker
//! `rolled_back`, which no start acts on again, so **one change restarts at
//! most twice (onto it, and back) and never loops**. The start after a
//! rollback tells the owner and records `config.rolled_back`.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_protocol::{ConfigRestart, LedgerKind};

use super::write;
use crate::config_copy::sha256;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// The environment variable that carries the marker's path across the exec.
pub const CHANGE_ENV: &str = "THESEUS_CONFIG_CHANGE";
/// What a change's restart names in `ConfigRestart::reference`, before the
/// marker's path: `theseusd` execs onto a change, not onto the vault's note.
pub const REF_PREFIX: &str = "change:";
/// The marker's file name.
pub const MARKER: &str = "pending.json";
/// How long the daemon has, after the restart, to be healthy before it goes
/// back, in seconds. Said on the card's restart line and in the report.
pub const HEALTHY_WITHIN_SECS: u64 = 120;
/// How often health is looked at while waiting.
pub const LOOK_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Restarting,
    Started,
    Confirmed,
    RolledBack,
}

/// The change a restart is onto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    /// The question that approved it.
    pub id: String,
    pub file: PathBuf,
    /// The copy kept before the change, and its digest.
    pub previous: PathBuf,
    pub previous_sha256: String,
    pub digest_before: String,
    pub digest_after: String,
    pub paths: Vec<String>,
    pub session_id: String,
    pub place: Option<String>,
    pub approved_by: String,
    pub via: String,
    pub requested_ms: u64,
    pub stage: Stage,
    /// When it confirmed, how long the restart took, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    /// Why it was rolled back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// Where the marker is, for a state directory.
pub fn marker_path(state: &Path) -> PathBuf {
    write::dir(state).join(MARKER)
}

/// Keep the marker.
pub fn save(path: &Path, p: &Pending) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(p).map_err(std::io::Error::other)?;
    write::atomic(path, &text, None)
}

/// The marker at `path`: none when there is none, or it is not one.
pub fn load(path: &Path) -> Option<Pending> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The change is over: its marker is kept under its own name, so no later
/// start finds it pending.
pub fn finish(path: &Path, p: &Pending) {
    let done = path.with_file_name(format!("done-{}.json", p.id));
    if let Err(e) = std::fs::rename(path, &done) {
        tracing::warn!(error = %e, "the change's marker could not be put away");
    }
}

/// What a start does about a marker it finds.
#[derive(Debug, PartialEq)]
pub enum Begin {
    /// Nothing is pending.
    Normal,
    /// The first start onto the change: go on, and watch for health.
    Watch(Pending),
    /// A start onto the change that never confirmed: go back now.
    RollBackNow(Pending),
    /// This start is the one after a rollback: serve, and say so.
    Tell(Pending),
}

/// Read the marker at `path` and mark a first start `started`. Writing the
/// mark is what makes a second start find it.
pub fn begin(path: &Path) -> Begin {
    let Some(mut p) = load(path) else {
        return Begin::Normal;
    };
    match p.stage {
        Stage::Confirmed => Begin::Normal,
        Stage::RolledBack => Begin::Tell(p),
        Stage::Started => Begin::RollBackNow(p),
        Stage::Restarting => {
            p.stage = Stage::Started;
            match save(path, &p) {
                Ok(()) => Begin::Watch(p),
                // A marker that cannot be marked would let a dying start loop.
                Err(_) => Begin::RollBackNow(p),
            }
        }
    }
}

/// Put the copy kept before the change back as the config, and mark the
/// marker `rolled_back` with `why`. Refused, with the reason, when the file is
/// no longer what the change wrote or the copy is not what was kept.
pub fn roll_back(path: &Path, p: &mut Pending, why: &str) -> Result<(), String> {
    let now = std::fs::read_to_string(&p.file)
        .map_err(|e| format!("the settings file cannot be read: {e}"))?;
    if sha256(&now) != p.digest_after {
        return Err(
            "the settings file is no longer the one the change wrote, so it is not put back over \
             a later edit"
                .into(),
        );
    }
    let before = std::fs::read_to_string(&p.previous)
        .map_err(|e| format!("the copy kept before the change cannot be read: {e}"))?;
    if sha256(&before) != p.previous_sha256 {
        return Err("the copy kept before the change is not the one that was kept".into());
    }
    write::write_config(&p.file, &before)
        .map_err(|e| format!("the settings could not be put back: {e}"))?;
    p.stage = Stage::RolledBack;
    p.why = Some(why.to_string());
    save(path, p).map_err(|e| format!("the marker could not be kept: {e}"))
}

/// Wait up to `within` for `probe` to say the daemon is healthy, looking every
/// `every`: how long it took, or the last reason it was not.
pub async fn watch_health<F, Fut>(
    probe: F,
    within: Duration,
    every: Duration,
) -> Result<Duration, String>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let t0 = tokio::time::Instant::now();
    loop {
        let why = match probe().await {
            Ok(()) => return Ok(t0.elapsed()),
            Err(why) => why,
        };
        if t0.elapsed() + every > within {
            return Err(why);
        }
        tokio::time::sleep(every).await;
    }
}

impl Core {
    /// The marker for a change the owner approved, kept and the restart asked:
    /// after a running turn has ended, or `wait` has passed and it is cut. A
    /// turn the stop interrupts resumes after the restart, as after any stop.
    pub(crate) fn restart_for_change(&self, p: &Pending, wait: Duration) {
        let marker = marker_path(&self.tools.config_change.state);
        let r = ConfigRestart {
            reference: format!("{REF_PREFIX}{}", marker.display()),
            at_unix_ms: theseus_protocol::now_unix_ms(),
            tables: p.paths.clone(),
            copy_sha256: p.digest_before.clone(),
            vault_sha256: p.digest_after.clone(),
        };
        let Some(core) = self.tools.config_change.core() else {
            // No handle on itself (a test's core that was not attached): ask now.
            self.ask_restart(r);
            return;
        };
        tokio::spawn(async move {
            let t0 = tokio::time::Instant::now();
            while core.tools.admission.load().0 > 0 && t0.elapsed() < wait {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            core.ask_restart(r);
        });
    }

    /// The restart, asked: the clean stop, after which `theseusd` execs its
    /// own image with the marker's path.
    fn ask_restart(&self, r: ConfigRestart) {
        tracing::warn!(
            tables = %r.tables.join(", "),
            "restarting to apply a change to Theseus's settings"
        );
        self.restart_for(r);
    }

    /// After a restart onto a change, once serving: wait for health, then
    /// either confirm and tell the owner, or go back and restart once more.
    pub async fn confirm_change(self: Arc<Self>, marker: PathBuf, mut p: Pending, socket: PathBuf) {
        let probe = || {
            let core = self.clone();
            let socket = socket.clone();
            async move { core.healthy_now(&socket).await }
        };
        let within = Duration::from_secs(HEALTHY_WITHIN_SECS);
        match watch_health(probe, within, LOOK_EVERY).await {
            Ok(took) => {
                p.stage = Stage::Confirmed;
                p.took_ms = Some(took.as_millis() as u64);
                match save(&marker, &p) {
                    Ok(()) => finish(&marker, &p),
                    Err(e) => {
                        tracing::warn!(error = %e, "the change's marker could not be marked confirmed")
                    }
                }
                self.tell(
                    &p,
                    format!(
                        "Theseus is back, on the settings you approved ({}). The restart took \
                         {} s.",
                        p.paths.join(", "),
                        took.as_secs().max(1)
                    ),
                );
            }
            Err(why) => self.go_back(
                &marker,
                &mut p,
                &format!("it was not healthy within {HEALTHY_WITHIN_SECS} s: {why}"),
            ),
        }
    }

    /// Roll the settings back and restart onto them, once.
    pub(crate) fn go_back(&self, marker: &Path, p: &mut Pending, why: &str) {
        match roll_back(marker, p, why) {
            Ok(()) => {
                let r = ConfigRestart {
                    reference: format!("{REF_PREFIX}{}", marker.display()),
                    at_unix_ms: theseus_protocol::now_unix_ms(),
                    tables: p.paths.clone(),
                    copy_sha256: p.digest_after.clone(),
                    vault_sha256: p.digest_before.clone(),
                };
                self.ask_restart(r);
            }
            Err(e) => {
                tracing::warn!(error = %e, "the settings were not put back");
                self.tell(
                    p,
                    format!(
                        "The new settings ({}) are not healthy ({why}), and Theseus could not \
                         put the old ones back: {e}. Look at the settings file.",
                        p.paths.join(", ")
                    ),
                );
            }
        }
    }

    /// Whether this daemon is healthy as a restart onto a change judges it: it
    /// serves its socket, its store is open (it is built on it), and the index
    /// tender is off or not `down` or `stalled`.
    async fn healthy_now(&self, socket: &Path) -> Result<(), String> {
        tokio::net::UnixStream::connect(socket)
            .await
            .map_err(|e| format!("it does not serve its socket yet: {e}"))?;
        let h = self.health_now().await;
        match h.index.as_ref().map(|i| i.state.as_str()) {
            Some(s @ ("down" | "stalled")) => Err(format!("the index tender is {s}")),
            _ => Ok(()),
        }
    }

    /// The start after a rollback: record `config.rolled_back` and tell the
    /// owner. Serving on the old settings, whatever happens: nothing restarts.
    pub fn tell_rolled_back(&self, p: &Pending) {
        let why = p.why.clone().unwrap_or_default();
        let row = LedgerRow::new(
            LedgerKind::ConfigRolledBack,
            Some(&p.session_id),
            None,
            json!({
                "question": p.id, "paths": p.paths, "why": why,
                "digest_before": p.digest_before, "digest_after": p.digest_after,
            }),
        );
        if let Err(e) = self.store.append_ledger(&row) {
            tracing::warn!(error = %e, "ledger append failed");
        }
        self.tell(
            p,
            format!(
                "Theseus went back to the settings it had before the change ({}): {why}. It is \
                 running on the old settings, and will not try the change again by itself.",
                p.paths.join(", ")
            ),
        );
    }

    /// A line to the owner, in the place they asked, or where approvals go.
    fn tell(&self, p: &Pending, text: String) {
        if let Err(e) = self
            .outbox
            .to_operator(Some(&p.session_id), json!({"kind": "notice", "text": text}))
        {
            tracing::warn!(error = %format!("{e:#}"), "the owner's notice about the settings change was not written");
        }
    }
}

#[cfg(test)]
mod tests;
