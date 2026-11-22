//! "The loop is installing" (issue 41, round 2): one predicate, `Core::self_installing`, for the pipeline's view,
//! the long-hold line and anything that waits for a quiet loop. It follows the real install:
//! - the loop's own install is under way from the start of its phase (`join::joining`); once its unit is accepted
//!   the state names the unit, and a reader asks systemd whether that unit is still active (bounded, and kept for
//!   [`FRESH`]). A unit that has ended without the restart it makes clears the state, with a row that says so; a
//!   unit systemd cannot be asked about keeps it. Nothing clears it on a timer;
//! - at start the daemon looks for a still-running install unit of its own state directory (its script is under
//!   `<state>/self`) and shows it again before it answers anything;
//! - the root helper's install (`join::root_install`) is under way while its record says restarting or started.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use super::Installing;
use crate::fact::rsi_pipeline::{Ended, Moved};
use crate::rpc::Core;
use crate::rsi::join::root_install::Stage;

/// How long systemd's answer about a unit is kept before it is asked again.
pub const FRESH: Duration = Duration::from_secs(10);
/// The longest one question to systemd may take.
const ASK: Duration = Duration::from_secs(2);
/// The install units' names (`join::install`).
pub const UNIT_PREFIX: &str = "theseus-self-install-";

/// What a reader asks of the machine's units: a test's stand-in, or systemd.
pub trait Units: Send + Sync {
    /// systemd's state of `unit` (`active`, `activating`, `failed`, `inactive`, …); none when it cannot be asked.
    fn state(&self, unit: &str) -> Option<String>;
    /// The install units running now whose script is under `root` (`<state>/self`).
    fn running(&self, root: &Path) -> Vec<String>;
}

/// The machine's own: `systemctl --user`, each call bounded by [`ASK`].
pub struct Systemd;

/// `systemctl --user <args>`'s output, or none when it fails or takes longer than [`ASK`].
fn systemctl(args: &[&str]) -> Option<String> {
    let mut child = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let until = Instant::now() + ASK;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

impl Units for Systemd {
    fn state(&self, unit: &str) -> Option<String> {
        // `is-active` prints the state whatever its exit code; an empty answer is no answer.
        systemctl(&["is-active", unit]).filter(|s| !s.is_empty())
    }

    fn running(&self, root: &Path) -> Vec<String> {
        let pattern = format!("{UNIT_PREFIX}*");
        let Some(list) = systemctl(&[
            "list-units",
            "--plain",
            "--no-legend",
            "--state=active,activating",
            &pattern,
        ]) else {
            return Vec::new();
        };
        let script = root.join("install-").to_string_lossy().into_owned();
        list.lines()
            .filter_map(|l| l.split_whitespace().next())
            .map(|u| u.trim_end_matches(".service").to_string())
            .filter(|u| u.starts_with(UNIT_PREFIX))
            .filter(|u| {
                systemctl(&["show", "-p", "ExecStart", "--value", u])
                    .is_some_and(|exec| exec.contains(&script))
            })
            .collect()
    }
}

/// Whether systemd's `state` is a unit still running.
fn running_state(state: &str) -> bool {
    matches!(state, "active" | "activating" | "reloading")
}

impl Core {
    fn units(&self) -> Arc<dyn Units> {
        self.rsi
            .pipeline
            .units
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or_else(|| Arc::new(Systemd))
    }

    /// A test's stand-in for systemd.
    #[cfg(test)]
    pub(crate) fn set_units(&self, u: Arc<dyn Units>) {
        *self
            .rsi
            .pipeline
            .units
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(u);
    }

    /// systemd's state of `unit`, kept for [`FRESH`].
    fn unit_state(&self, unit: &str) -> Option<String> {
        let now = Instant::now();
        let mut seen = self
            .rsi
            .pipeline
            .unit_seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((u, at, state)) = seen.as_ref() {
            if u == unit && now.duration_since(*at) < FRESH {
                return state.clone();
            }
        }
        let state = self.units().state(unit);
        *seen = Some((unit.to_string(), now, state.clone()));
        state
    }

    /// The install the loop has under way, if any: the module's doc.
    pub(crate) fn self_installing(&self) -> Option<Installing> {
        if let Some(p) = self
            .root_pending()
            .filter(|p| matches!(p.stage, Stage::Restarting | Stage::Started))
        {
            return Some(Installing {
                item: String::new(),
                title: p.what.clone(),
                branch: p.branch.clone(),
                merge: p.merge.clone(),
                since_ms: p.requested_ms,
                unit: None,
                root_helper: true,
            });
        }
        let mine = self
            .rsi
            .pipeline
            .installing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        let Some(unit) = mine.unit.clone() else {
            // Its build, in this daemon: under way until the phase says otherwise.
            return Some(mine);
        };
        match self.unit_state(&unit) {
            Some(s) if !running_state(&s) => {
                self.install_ended(&mine, &unit, &s);
                None
            }
            // Running, or systemd cannot be asked: under way still.
            _ => Some(mine),
        }
    }

    /// The loop's install's unit ended with no restart of this daemon: no install is under way, and a row says why.
    fn install_ended(&self, i: &Installing, unit: &str, state: &str) {
        {
            let mut held = self
                .rsi
                .pipeline
                .installing
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if held.as_ref() != Some(i) {
                return;
            }
            *held = None;
        }
        let short = &i.merge[..i.merge.len().min(12)];
        let m = Moved::new(
            "",
            format!("the install of {short} ended without the restart it makes"),
        )
        .with("unit", unit)
        .with("merge", i.merge.as_str())
        .with("branch", i.branch.as_str())
        .with("state", state)
        .why(format!(
            "its unit {unit} is no longer running (systemd says {state}) and this daemon was not \
                 restarted onto the new build: the install did not finish"
        ));
        self.rec(None).record(&Ended(m));
        tracing::warn!(%unit, %state, merge = %i.merge, "self: the install's unit ended without a restart");
    }

    /// At start: a still-running install unit of this state directory is the loop's install under way again.
    /// Whether one was found.
    pub fn recover_installing(&self) -> bool {
        if self.cfg.self_improve.repo.is_none() {
            return false;
        }
        let root = self.cfg.state_dir().join("self");
        let Some(unit) = self.units().running(&root).into_iter().next() else {
            return false;
        };
        let short = unit.trim_start_matches(UNIT_PREFIX).to_string();
        let joined = crate::rsi::join::rows_of(&self.store, &["self.joined"])
            .ok()
            .flatten()
            .and_then(|rows| {
                rows.into_iter().rev().find(|r| {
                    r.data["merge"]
                        .as_str()
                        .is_some_and(|m| m.starts_with(&short))
                })
            });
        let s = |k: &str| {
            joined
                .as_ref()
                .and_then(|r| r.data[k].as_str())
                .unwrap_or_default()
                .to_string()
        };
        let merge = match s("merge") {
            m if m.is_empty() => short,
            m => m,
        };
        let i = Installing {
            item: s("item"),
            title: String::new(),
            branch: s("branch"),
            merge,
            since_ms: joined.as_ref().map_or(0, |r| r.at_unix_ms),
            unit: Some(unit.clone()),
            root_helper: false,
        };
        tracing::info!(%unit, "self: an install unit of this state directory still runs: the loop is installing");
        *self
            .rsi
            .pipeline
            .installing
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(i);
        true
    }

    /// The unit an install handed itself to (`join::joining`): its state names it from now on.
    pub(crate) fn install_handed_to(&self, unit: &str) {
        if let Some(i) = self
            .rsi
            .pipeline
            .installing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            i.unit = Some(unit.to_string());
        }
    }
}

/// A unit's line for a reader: how it stands.
pub fn said(i: &Installing) -> String {
    let short = &i.merge[..i.merge.len().min(12)];
    match (&i.unit, i.root_helper) {
        (_, true) => format!("installing {short} through the root helper: it restarts the service"),
        (Some(u), _) => {
            format!("installing {short}: its unit {u} copies the build in and restarts the daemon")
        }
        (None, _) => {
            format!("installing {short}: its build in L1, then its unit, which restarts the daemon")
        }
    }
}
