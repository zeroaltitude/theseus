//! "The loop is installing" (issue 41, rounds 2 and 3): one predicate, `Core::self_install_state`, for the pipeline's
//! view, the long-hold line and the loop's own start of an install. It has three answers, and it never says quiet
//! before it knows:
//! - **busy**: the loop's own install is under way (from the start of its phase, `join::joining`, or the root
//!   helper's build, staging and helper run, `join::root_install`), or the root helper's record says restarting or
//!   started. Once the loop's install unit is accepted the state names it, and a reader asks systemd whether it
//!   still runs (bounded, kept for [`FRESH`]). A unit that has ended clears it: with no row when the running build
//!   is the install's commit (the install worked: this daemon is its restart), with a row that says it did not
//!   finish otherwise. A unit systemd cannot be asked about keeps it. Nothing clears it on a timer;
//! - **not known**: until the start's discovery (an install unit of this state directory still running, its exact
//!   script argument, named by a join of this loop) has once succeeded. Succeeded means all of it: `list-units` and
//!   each `show` exited 0 (their exit status is kept apart from what they print), no more than [`MOST_UNITS`] units
//!   ran, and the loop's joins were read (not an index still being built). Anything less keeps it not known, and it
//!   is asked again, backed off, in the background, [`TRIES`] times, then at the next start: no turn waits on it;
//! - **quiet**: none of the above.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use theseus_kernel::children::{self, Kind};

use super::Installing;
use crate::fact::rsi_pipeline::{Ended, Moved};
use crate::rpc::Core;
use crate::rsi::join::root_install::Stage;

/// How long systemd's answer about a unit is kept before it is asked again.
pub const FRESH: Duration = Duration::from_secs(10);
/// The longest one question to systemd may take.
const ASK: Duration = Duration::from_secs(2);
/// The most units the start's discovery looks at.
const MOST_UNITS: usize = 16;
/// The start's discovery: how many tries, and the first wait between them (doubled each time, at most [`LAST_WAIT`]).
const TRIES: u32 = 12;
const FIRST_WAIT: Duration = Duration::from_secs(5);
const LAST_WAIT: Duration = Duration::from_secs(300);
/// The install units' names (`join::install`).
pub const UNIT_PREFIX: &str = "theseus-self-install-";

/// What the predicate says.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallState {
    Quiet,
    Busy(Installing),
    /// Whether an install is under way is not known yet: why.
    NotKnown(String),
}

/// The start's discovery, as far as it got: `None` not done yet, `Some(Err)` the last try failed, `Some(Ok)` done.
#[derive(Debug)]
pub struct Discovery(pub(crate) std::sync::Mutex<Option<Result<(), String>>>);

impl Default for Discovery {
    /// A daemon starts not knowing. A test's core starts as though its start had looked and found nothing (no start
    /// runs in a test); a test of discovery sets it back.
    fn default() -> Self {
        Self(std::sync::Mutex::new(cfg!(test).then_some(Ok(()))))
    }
}

/// What a reader asks of the machine's units: a test's stand-in, or systemd.
pub trait Units: Send + Sync {
    /// systemd's state of `unit` (`active`, `activating`, `deactivating`, `failed`, `inactive`, …); none when it
    /// cannot be asked.
    fn state(&self, unit: &str) -> Option<String>;
    /// The install units running now whose script is this state directory's, exactly: `<root>/install-<short>.sh`
    /// for a unit `theseus-self-install-<short>`. Why not, when systemd cannot be asked.
    fn running(&self, root: &Path) -> Result<Vec<String>, String>;
}

/// The machine's own: `systemctl --user`, each call a registered child (`children::spawn`), its output drained while
/// it runs, bounded by [`ASK`].
pub struct Systemd;

/// A test's stand-in for the `systemctl` program (a script): the adapter itself runs, on what it prints.
#[cfg(test)]
pub(crate) static SYSTEMCTL: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

/// The program asked: `systemctl`, or a test's stand-in.
fn program() -> std::ffi::OsString {
    #[cfg(test)]
    if let Some(p) = SYSTEMCTL
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        return p.into();
    }
    "systemctl".into()
}

/// `systemctl --user <args>`'s output, or why there is none (it could not start, failed, or took longer than [`ASK`]).
fn systemctl(args: &[&str]) -> Result<Answer, String> {
    let mut c = Command::new(program());
    c.arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = children::spawn(Kind::Owned, || c.spawn(), |c| Some(c.id()))
        .map_err(|e| format!("systemctl could not start: {e}"))?;
    // Drained on a thread of its own, so a full pipe never reads as no answer.
    let out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut o) = out {
            let _ = std::io::Read::read_to_end(&mut o, &mut buf);
        }
        buf
    });
    let until = Instant::now() + ASK;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("systemctl took longer than {} s", ASK.as_secs()));
            }
            Err(e) => break Err(format!("systemctl could not be waited for: {e}")),
        }
    };
    let text = String::from_utf8_lossy(&reader.join().unwrap_or_default())
        .trim()
        .to_string();
    status
        .map(|s| (s.success(), text))
        .map(|(ok, text)| Answer { ok, text })
}

/// What `systemctl` said: whether it succeeded, and what it printed. `is-active` prints a state whatever its exit
/// status; `list-units` and `show` count only when they succeed.
struct Answer {
    ok: bool,
    text: String,
}

impl Answer {
    /// Its text, when it succeeded; else why not.
    fn succeeded(self, what: &str) -> Result<String, String> {
        match self.ok {
            true => Ok(self.text),
            false => Err(format!("systemctl {what} failed")),
        }
    }
}

/// The tokens of `ExecStart` as `systemctl show -p ExecStart --value` prints them: each `argv[]=` list.
fn argv_of(exec: &str) -> Vec<&str> {
    exec.split(';')
        .filter_map(|part| part.trim().strip_prefix("argv[]="))
        .flat_map(str::split_whitespace)
        .collect()
}

impl Units for Systemd {
    fn state(&self, unit: &str) -> Option<String> {
        // `is-active` prints the state whatever its exit code; an empty answer is no answer.
        systemctl(&["is-active", unit])
            .ok()
            .map(|a| a.text)
            .filter(|s| !s.is_empty())
    }

    fn running(&self, root: &Path) -> Result<Vec<String>, String> {
        let pattern = format!("{UNIT_PREFIX}*");
        let list = systemctl(&[
            "list-units",
            "--plain",
            "--no-legend",
            "--state=active,activating,deactivating",
            &pattern,
        ])?
        .succeeded("list-units")?;
        let units: Vec<String> = list
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .map(|u| u.trim_end_matches(".service").to_string())
            .filter(|u| u.starts_with(UNIT_PREFIX))
            .collect();
        // The bound stays; past it the unit not looked at could be this loop's, so the answer is incomplete.
        if units.len() > MOST_UNITS {
            return Err(format!(
                "{} install units run, more than the {MOST_UNITS} looked at: not all could be looked at",
                units.len()
            ));
        }
        let mut out = Vec::new();
        for unit in units {
            let short = unit.trim_start_matches(UNIT_PREFIX);
            let script = root.join(format!("install-{short}.sh"));
            let exec =
                systemctl(&["show", "-p", "ExecStart", "--value", &unit])?.succeeded("show")?;
            if argv_of(&exec).iter().any(|a| Path::new(a) == script) {
                out.push(unit);
            }
        }
        Ok(out)
    }
}

/// Whether systemd's `state` is a unit not yet ended (`deactivating` still runs its processes).
fn running_state(state: &str) -> bool {
    matches!(
        state,
        "active" | "activating" | "deactivating" | "reloading" | "refreshing"
    )
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

    /// The commit this daemon runs (a test's stand-in, else the build's stamp).
    fn running_commit(pipeline: &super::Shared) -> Option<String> {
        #[cfg(test)]
        if let Some(c) = pipeline
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Some(c);
        }
        #[cfg(not(test))]
        let _ = pipeline;
        crate::build().commit
    }

    /// Whether the loop is installing: the module's doc.
    pub(crate) fn self_install_state(&self) -> InstallState {
        if let Some(p) = self
            .root_pending()
            .filter(|p| matches!(p.stage, Stage::Restarting | Stage::Started))
        {
            return InstallState::Busy(Installing {
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
            .clone();
        if let Some(i) = mine {
            let Some(unit) = i.unit.clone() else {
                // Its build (or the root helper's work), in this daemon: under way until its phase says otherwise.
                return InstallState::Busy(i);
            };
            match self.unit_state(&unit) {
                Some(s) if !running_state(&s) => self.install_ended(&i, &unit, &s),
                // Running, or systemd cannot be asked: under way still.
                _ => return InstallState::Busy(i),
            }
        }
        match self
            .rsi
            .pipeline
            .discovered
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            Some(Ok(())) => InstallState::Quiet,
            None => InstallState::NotKnown(
                "whether an install of this loop still runs is not known yet: the start has not finished looking"
                    .into(),
            ),
            Some(Err(why)) => InstallState::NotKnown(format!(
                "whether an install of this loop still runs is not known yet: {why}"
            )),
        }
    }

    /// The loop's install's unit has ended: no install is under way. When this daemon runs the install's commit the
    /// install worked (this daemon is its restart) and nothing more is said; otherwise a row says it did not finish.
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
        let worked = Self::running_commit(&self.rsi.pipeline)
            .is_some_and(|c| !c.is_empty() && (c.starts_with(&i.merge) || i.merge.starts_with(&c)));
        if worked {
            tracing::info!(%unit, merge = %i.merge, "self: the install's unit ended, and this daemon runs its build");
            return;
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
            "its unit {unit} is no longer running (systemd says {state}) and this daemon does not run the build it \
             installs: the install did not finish"
        ));
        self.rec(None).record(&Ended(m));
        tracing::warn!(%unit, %state, merge = %i.merge, "self: the install's unit ended without a restart");
    }

    /// One try of the start's discovery: an install unit of this state directory still running, named by a join of
    /// this loop, is the loop's install under way again; one no join of this loop names is reported and not adopted.
    /// Whether the try succeeded (systemd answered), whatever it found.
    pub fn discover_installing(&self) -> bool {
        let set = |r: Result<(), String>| {
            *self
                .rsi
                .pipeline
                .discovered
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(r);
        };
        if self.cfg.self_improve.repo.is_none() {
            set(Ok(()));
            return true;
        }
        let root = self.cfg.state_dir().join("self");
        let units = match self.units().running(&root) {
            Ok(u) => u,
            Err(why) => {
                tracing::info!(%why, "self: whether an install still runs is not known yet: asked again later");
                set(Err(format!(
                    "systemd could not be asked ({why}); it is asked again"
                )));
                return false;
            }
        };
        // The joins must be read to tell this loop's unit from a stranger's: an index still being built, or a ledger
        // that cannot be read, leaves discovery not done.
        let joined = match crate::rsi::join::rows_of(&self.store, &["self.joined"]) {
            Ok(Some(rows)) => rows,
            Ok(None) => {
                set(Err(
                    "the ledger's index is still being built, so the loop's joins cannot be read yet; it is asked \
                     again"
                        .into(),
                ));
                return false;
            }
            Err(e) => {
                set(Err(format!(
                    "the loop's joins could not be read ({e:#}); it is asked again"
                )));
                return false;
            }
        };
        for unit in units {
            let short = unit.trim_start_matches(UNIT_PREFIX).to_string();
            let Some(row) = joined.iter().rev().find(|r| {
                r.data["merge"]
                    .as_str()
                    .is_some_and(|m| !short.is_empty() && m.starts_with(&short))
            }) else {
                tracing::warn!(%unit,
                    "self: an install unit of this state directory runs, and no join of this loop names it: not adopted");
                continue;
            };
            let s = |k: &str| row.data[k].as_str().unwrap_or_default().to_string();
            let i = Installing {
                item: s("item"),
                title: String::new(),
                branch: s("branch"),
                merge: s("merge"),
                since_ms: row.at_unix_ms,
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
            break;
        }
        set(Ok(()));
        true
    }

    /// The start's discovery, in the background: tried until it succeeds, at most [`TRIES`] times, each wait twice
    /// the last. Until it succeeds the loop is "not known", never quiet.
    pub fn discover_installing_after_serving(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut wait = FIRST_WAIT;
            for _ in 0..TRIES {
                let Some(c) = core.upgrade() else { return };
                let done = tokio::task::spawn_blocking(move || c.discover_installing())
                    .await
                    .unwrap_or(false);
                if done {
                    return;
                }
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(LAST_WAIT);
            }
            // The tries are spent: say what is true now.
            let Some(c) = core.upgrade() else { return };
            let mut d = c
                .rsi
                .pipeline
                .discovered
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(Err(why)) = d.as_mut() {
                *why = format!(
                    "{}; after {TRIES} tries it is asked again only at the next start",
                    why.trim_end_matches("; it is asked again")
                );
            }
        });
    }

    /// Why the loop does not start an install now: one is under way, or whether one is is not known yet.
    pub(crate) fn install_blocked(&self) -> Option<String> {
        match self.self_install_state() {
            InstallState::Quiet => None,
            InstallState::Busy(i) => Some(format!("another install is under way: {}", said(&i))),
            InstallState::NotKnown(why) => Some(why),
        }
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

    /// The root helper's build, staging and helper run are under way (`join::root_install`).
    pub(crate) fn root_install_busy(&self, merge: &str, branch: &str, what: &str) {
        *self
            .rsi
            .pipeline
            .installing
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Installing {
            item: String::new(),
            title: what.into(),
            branch: branch.into(),
            merge: merge.into(),
            since_ms: theseus_protocol::now_unix_ms(),
            unit: None,
            root_helper: true,
        });
    }

    /// The root helper's work ended: refused, failed, waiting on the owner, or handed to its record's stages, which the
    /// predicate reads from the store.
    pub(crate) fn root_install_idle(&self) {
        let mut held = self
            .rsi
            .pipeline
            .installing
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if held.as_ref().is_some_and(|i| i.root_helper) {
            *held = None;
        }
    }
}

/// An install's line for a reader: how it stands.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit is this state directory's only when its `ExecStart` names this script exactly: a longer path that
    /// holds it, or another state's, does not match.
    #[test]
    fn only_the_exact_script_argument_matches() {
        let script = Path::new("/state/self/install-72af5c3fd1ff.sh");
        let of = |exec: &str| argv_of(exec).iter().any(|a| Path::new(a) == script);
        assert!(of(
            "{ path=/bin/bash ; argv[]=/bin/bash /state/self/install-72af5c3fd1ff.sh ; ignore_errors=no }"
        ));
        assert!(!of(
            "{ path=/bin/bash ; argv[]=/bin/bash /other/state/self/install-72af5c3fd1ff.sh ; ignore_errors=no }"
        ));
        assert!(!of(
            "{ path=/bin/bash ; argv[]=/bin/bash /state/self/install-72af5c3fd1ff.sh.old ; ignore_errors=no }"
        ));
        assert!(!of(""));
    }
}
