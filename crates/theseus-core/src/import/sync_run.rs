//! The sync on demand (theseus-ievo3, S29-1): **the request runs it.**
//!
//! The first build of this half recorded that a sync was wanted and left a
//! two-minute systemd timer to notice. The owner read that and rejected its
//! premise, 2026-11-13 14:01:14 MST (Discord message 1570906412190540072):
//!
//! > *"I still don't understand. Why is there a polling timer? I'm asking in a
//! > theseus thread to just re-import from external sessions, right? Can't it
//! > just execute it? Like I iimagine there's a once a day thing that just
//! > executes a script, or a tool, or a process. How about when the request
//! > comes in we just execute it"*
//!
//! So `import.sync.request` starts the run itself, and the timer is gone.
//! What makes that safe is not a check on the way in; it is that **there is
//! nothing to check**:
//!
//! - **The command is a constant.** [`ARGV`] is an array of string literals
//!   and [`command`] **takes no arguments at all**, so there is no parameter
//!   for a model's or a message's string to arrive in. "Narrow by
//!   construction" is the owner's own framing of this half, and this is where
//!   it is held. The two guards are [`the argv is a constant of literals`] and
//!   [`the command builder takes no arguments`] in `tests_sync_run`.
//! - **The binary is the daemon's own**, beside this process's image and never
//!   from `PATH` (`crate::tender::beside_this_binary`'s rule, for the same
//!   reason: the daemon runs the program of its own install).
//! - **The environment is cleared** and rebuilt from [`KEEP_ENV`], four names,
//!   taken from the daemon's own environment. Nothing a caller said is in it.
//!   The working directory is left as the daemon's own and is never set from a
//!   caller.
//! - **It is a registered child** (`theseus_kernel::children::spawn`,
//!   `Kind::Owned`), so the children census still holds and
//!   `tests_children_complete`'s walk sees it registered.
//! - **One run at a time** ([`OnDemand::claim`]). A second request while one
//!   runs is answered `already running`: it starts nothing, queues nothing,
//!   and does not quietly report success.
//! - **The deadline is the scheduled run's**, not a new one:
//!   [`DEADLINE_MS`] is `SYNC_HOURLY_TIMEOUT` out of `scripts/user-service.sh`,
//!   which bounds the import-only unit -- the unit that runs *the same
//!   command*. `the deadline is the scheduled import only run's own bound`
//!   reads the script and fails if the two ever drift apart.
//! - **The spend cap is the scheduled run's**, by taking the same road: the
//!   argv carries `--no-people`, so no model call and no `[people]
//!   sweep_usd_per_day` spend happens on this path at all. The people step
//!   stays nightly, exactly as it did under the timer.

use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The one command's arguments after the binary, whole. A constant of string
/// literals: this is the argv of every on-demand run there will ever be.
///
/// `--no-people` is not an optimisation. It is the spend cap: the people step
/// is the only part of a sync that calls a model, so a run without it cannot
/// spend, however often a request arrives. The sessions it touches wait in the
/// daemon's `people_todo` for the nightly full run, under that run's own cap.
pub const ARGV: [&str; 3] = ["import", "sync", "--no-people"];

/// The program, beside this daemon's own image: the `theseus` of this install,
/// never whatever `PATH` finds.
pub const BINARY: &str = "theseus";

/// The names the child keeps of the daemon's own environment, and the whole of
/// its environment besides. `PATH` is here because the exporter the run starts
/// is `python3` and systemd's own `PATH` may not find it (theseus-7kuy2, the
/// same reason the unit carried a `PATH` line); `THESEUS_SOCKET` so the child
/// reaches *this* daemon and not another.
pub const KEEP_ENV: [&str; 4] = ["HOME", "PATH", "RUST_LOG", "THESEUS_SOCKET"];

/// How long a run may take before it is killed.
///
/// This is the scheduled import-only run's own bound, reused and not
/// re-invented: `scripts/user-service.sh`'s `SYNC_HOURLY_TIMEOUT=45min`, which
/// is the `TimeoutStartSec=` of the unit that runs the same
/// `import sync --no-people`. Under the timer design systemd held this bound;
/// with the daemon starting the run, the daemon holds it.
pub const DEADLINE_MS: u64 = 45 * 60_000;

/// What one run came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    /// The child's pid, 0 when it never started.
    pub pid: u32,
    /// Its exit code, when it had one.
    pub code: Option<i32>,
    /// It outlived [`DEADLINE_MS`] and was killed.
    pub timed_out: bool,
    /// Why it never started, or could not be waited for.
    pub error: Option<String>,
}

impl Ran {
    /// A run that started, finished inside the deadline, and exited 0.
    pub fn ok(&self) -> bool {
        self.error.is_none() && !self.timed_out && self.code == Some(0)
    }

    /// Why it did not work, worded, or none.
    pub fn why(&self) -> Option<String> {
        match (&self.error, self.timed_out, self.code) {
            (Some(e), _, _) => Some(e.clone()),
            (None, true, _) => Some(format!(
                "it ran past {} minutes and was stopped",
                DEADLINE_MS / 60_000
            )),
            (None, false, Some(0)) => None,
            (None, false, Some(c)) => Some(format!("it exited {c}")),
            (None, false, None) => Some("it was killed by a signal".into()),
        }
    }
}

/// The run in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flight {
    /// When it started, unix ms.
    pub at_ms: u64,
}

/// What a request got: a run of its own, or the one already going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// This request started a run, at this time.
    Started(u64),
    /// One was already running, since this time. Nothing was started.
    AlreadyRunning(u64),
}

/// The future one run is.
pub type Fut = Pin<Box<dyn Future<Output = Ran> + Send>>;

/// What runs the one fixed command.
///
/// **It takes no arguments.** That is not a convention here, it is the whole
/// of "no argument comes from the model or from the message" (S29-1): a
/// `Fn()` has no parameter, so no caller's string can reach a run by any path
/// through this type. Production is [`run`]; a test installs its own.
pub type Runner = Arc<dyn Fn() -> Fut + Send + Sync>;

/// The daemon's on-demand sync: one run at a time, and what runs it.
pub struct OnDemand {
    /// The run in flight, when there is one. The claim and the start are both
    /// under this lock, so two requests cannot both find it free.
    flight: Mutex<Option<Flight>>,
    runner: Mutex<Runner>,
    /// Runs started since this daemon did, for health and for the guard that
    /// a refused second request starts nothing.
    started: AtomicU64,
}

impl Default for OnDemand {
    fn default() -> Self {
        Self::new(default_runner())
    }
}

impl std::fmt::Debug for OnDemand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnDemand")
            .field("flight", &self.flight.lock().ok().and_then(|g| *g))
            .field("started", &self.started.load(Ordering::Relaxed))
            .finish()
    }
}

impl OnDemand {
    pub fn new(runner: Runner) -> Self {
        Self {
            flight: Mutex::new(None),
            runner: Mutex::new(runner),
            started: AtomicU64::new(0),
        }
    }

    /// Install the runner (a test's stand-in for the child).
    pub fn set_runner(&self, r: Runner) {
        *self.runner.lock().unwrap_or_else(|e| e.into_inner()) = r;
    }

    /// Runs started since the daemon did.
    pub fn started(&self) -> u64 {
        self.started.load(Ordering::Relaxed)
    }

    /// The run in flight, when there is one.
    pub fn flight(&self) -> Option<Flight> {
        *self.flight.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Take the one seat, or say who has it. The whole decision is under the
    /// lock, so of two requests that arrive together exactly one gets
    /// [`Asked::Started`] and the other [`Asked::AlreadyRunning`].
    pub fn claim(&self, now_ms: u64) -> Asked {
        let mut g = self.flight.lock().unwrap_or_else(|e| e.into_inner());
        match *g {
            Some(f) => Asked::AlreadyRunning(f.at_ms),
            None => {
                *g = Some(Flight { at_ms: now_ms });
                self.started.fetch_add(1, Ordering::Relaxed);
                Asked::Started(now_ms)
            }
        }
    }

    /// Give the seat back. Called whatever the run came to, including a run
    /// that never started: a seat a failure kept would wedge the feature
    /// until the daemon restarted.
    pub fn release(&self) {
        *self.flight.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The run itself, with the seat given back after it however it ends.
    pub async fn run_claimed(&self) -> Ran {
        let runner = self
            .runner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let ran = runner().await;
        self.release();
        ran
    }
}

/// Production's runner.
#[cfg(not(test))]
fn default_runner() -> Runner {
    Arc::new(|| Box::pin(run()))
}

/// A test build's runner: a unit test must not fork. It reports a run that
/// did nothing; a test that cares about the run installs its own with
/// [`OnDemand::set_runner`]. The production path is not left unread for
/// that -- [`command`] is built and inspected whole by
/// `the command is the daemons own binary with a fixed argv and environment`.
#[cfg(test)]
fn default_runner() -> Runner {
    Arc::new(|| {
        Box::pin(async {
            Ran {
                pid: 0,
                code: Some(0),
                ..Ran::default()
            }
        })
    })
}

/// The `theseus` of this install: beside this process's own image, never
/// `PATH`. The daemon runs the binary of its own install, as the index
/// tender's supervisor does (`crate::tender::beside_this_binary`).
pub fn binary() -> io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(BINARY))
}

/// **The one command. It takes no arguments.**
///
/// There is no parameter here for a model's or a message's string to arrive
/// in, and nothing inside reads one: the program is this install's own, the
/// argv is [`ARGV`], and the environment is cleared and rebuilt from
/// [`KEEP_ENV`] out of the daemon's own. That signature is the proof, and
/// `the command builder takes no arguments` holds it against the source so
/// a later parameter cannot be added quietly.
pub fn command() -> io::Result<tokio::process::Command> {
    let mut cmd = tokio::process::Command::new(binary()?);
    cmd.args(ARGV)
        .env_clear()
        .envs(
            KEEP_ENV
                .iter()
                .filter_map(|k| Some((*k, std::env::var_os(k)?))),
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true);
    Ok(cmd)
}

/// Start the one command as a registered child and wait for it, up to
/// [`DEADLINE_MS`]. Its log goes where the daemon's goes: stderr, inherited.
pub async fn run() -> Ran {
    let mut cmd = match command() {
        Ok(c) => c,
        Err(e) => {
            return Ran {
                error: Some(format!("the sync's binary could not be found: {e}")),
                ..Ran::default()
            }
        }
    };
    // Registered, so the children census still holds: `Kind::Owned` is the
    // kind whose spawner waits for it, which is what happens below.
    let mut child = match theseus_kernel::children::spawn(
        theseus_kernel::children::Kind::Owned,
        || cmd.spawn(),
        tokio::process::Child::id,
    ) {
        Ok(c) => c,
        Err(e) => {
            return Ran {
                error: Some(format!("the sync did not start: {e}")),
                ..Ran::default()
            }
        }
    };
    let pid = child.id().unwrap_or(0);
    let deadline = std::time::Duration::from_millis(DEADLINE_MS);
    match tokio::time::timeout(deadline, child.wait()).await {
        Ok(Ok(st)) => Ran {
            pid,
            code: st.code(),
            ..Ran::default()
        },
        Ok(Err(e)) => Ran {
            pid,
            error: Some(format!("the sync could not be waited for: {e}")),
            ..Ran::default()
        },
        Err(_) => {
            // `kill` reaps it too, so the registry's owned entry is settled.
            let _ = child.kill().await;
            Ran {
                pid,
                timed_out: true,
                ..Ran::default()
            }
        }
    }
}

// --- what the thread is told (S29-1: "it started", and then what it imported)

/// The first of the two lines: the run started.
pub fn started_line() -> String {
    "The OpenClaw sync started. I will say what it imported when it is done.".into()
}

/// A second request while one runs: it is told plainly, and nothing started.
pub fn already_running_line(since_ms: u64, now_ms: u64) -> String {
    format!(
        "An OpenClaw sync is already running ({}); nothing new was started. \
         I will say what it imported when it is done.",
        ago(since_ms, now_ms)
    )
}

/// `started 2 minutes ago`, as a sentence's parenthesis wants it.
fn ago(since_ms: u64, now_ms: u64) -> String {
    let secs = now_ms.saturating_sub(since_ms) / 1_000;
    match secs {
        0..=1 => "started just now".into(),
        // Under three quarters of a minute is said in seconds; from there it
        // rounds to minutes. The hand-over is 45 s and not 90 s so that the
        // rounding can still reach ONE minute: at 90 s it would jump from
        // "89 seconds ago" to "2 minutes ago" and never say "1 minute ago".
        2..=44 => format!("started {secs} seconds ago"),
        _ => {
            let m = (secs + 30) / 60;
            format!("started {m} minute{} ago", if m == 1 { "" } else { "s" })
        }
    }
}

/// The second of the two lines: what the run that just ended imported, from
/// the state it recorded. `before` is the state as it was when the run
/// started, so the counts are this run's and not the last one's.
pub fn finished_line(
    ran: &Ran,
    before: &super::sync::SyncState,
    after: &super::sync::SyncState,
) -> String {
    if let Some(why) = ran.why() {
        return format!("The OpenClaw sync failed: {why}. Nothing was imported.");
    }
    if let Some(e) = &after.error {
        return format!("The OpenClaw sync failed: {e}. Nothing was imported.");
    }
    if after.at_ms == before.at_ms {
        return "The OpenClaw sync finished and recorded no run: nothing was imported.".into();
    }
    let new = after.sessions_new.saturating_sub(before.sessions_new);
    let grown = after
        .sessions_appended
        .saturating_sub(before.sessions_appended);
    let msgs = after.messages.saturating_sub(before.messages);
    if new == 0 && grown == 0 && msgs == 0 {
        return "The OpenClaw sync finished: nothing new to import.".into();
    }
    let mut parts = Vec::new();
    if new > 0 {
        parts.push(format!("{new} new session{}", plural(new)));
    }
    if grown > 0 {
        parts.push(format!("{grown} session{} that grew", plural(grown)));
    }
    parts.push(format!("{msgs} message{}", plural(msgs)));
    let rejected = after.rejected.saturating_sub(before.rejected);
    let tail = if rejected > 0 {
        format!(" {rejected} message{} were rejected.", plural(rejected))
    } else {
        String::new()
    };
    format!(
        "The OpenClaw sync finished: {}. The people step waits for tonight's full run.{tail}",
        parts.join(", ")
    )
}

fn plural(n: u64) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
