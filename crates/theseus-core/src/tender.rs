//! The index tender (M6 §2.2; roadmap row 51): the daemon's long-lived child,
//! `theseus-index serve`, installed beside `theseusd`. It follows the store's
//! WAL read-only into `<state>/index`, and answers on `<state>/index/sock`,
//! with the core its only client.
//!
//! - **After serving**, never on the start path: the socket daemon's
//!   `after_serving` runs [`IndexTender::run`], which starts a tender
//!   [`START_AFTER`] later, once the start's aftermath has settled, and takes
//!   over at once a tender an exec kept. A `--stdio` daemon runs none.
//! - **One per index directory.** The tender takes `<state>/index/LOCK`, and a
//!   second exits 3 at once. A restart onto the vault's changed note execs the
//!   daemon in place, with its pid and so its children: `children::relearn`
//!   knows the tender by its command line, and `run` takes it over instead of
//!   starting a second.
//! - **Restarted whenever it exits.** The reaper's sweep reaps it by its pid
//!   and hands its exit to [`IndexTender::exited`]; the next starts after
//!   [`next_backoff`]: 1 s, doubling to 60 s while it keeps failing, and 1 s
//!   again after a run of a minute.
//! - **A stop sends SIGTERM and never waits** (§9). Ingest is idempotent by
//!   node id, and the cursor follows each commit, so a kill costs one batch at
//!   most. The tender exits with the daemon (`--parent`), so a crash leaves
//!   none holding the index's lock.
//! - **The core asks it, bounded**: health's `index` block
//!   ([`IndexTender::health_block`], only of a tender it runs) and
//!   `index.status` ([`IndexTender::health`]), and `index.query`
//!   ([`IndexTender::query`]), one connection per call, each under a deadline.
//!
//! Each start, take-over, failed start, settings change, and exit is a fact
//! (`crate::fact::index`), an `index.tender` ledger row; a stop records none
//! (the daemon's `server.stopping` says it).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use theseus_protocol::index::{
    method, IndexHealth, IndexQueryParams, IndexQueryResult, IndexStatus,
};
use theseus_protocol::{Id, Request, Response, TenderStatus};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::config::IndexConfig;
use crate::fact::index::{
    TenderAdopted, TenderExited, TenderSettingsChanged, TenderStartFailed, TenderStarted,
};
use crate::fact::Fact;
use crate::ledger::LedgerRow;

mod sample;

/// What the tender tends, in the children registry and in health.
pub const NAME: &str = "index";
/// Its binary, installed beside `theseusd`.
pub const BINARY: &str = "theseus-index";
/// Its exit code when another tender holds the index directory.
pub const HELD: i32 = 3;
/// The wait before the first restart, and the longest.
pub const BACKOFF_FIRST: Duration = Duration::from_secs(1);
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// A run this long was healthy: the next exit waits `BACKOFF_FIRST` again.
pub const HEALTHY_RUN: Duration = Duration::from_secs(60);
/// How long after serving a fresh start waits (a tender an exec kept is taken
/// over at once). The daemon's start and its aftermath settle first, so the
/// tender's own start (its process, the index's open, its lock and socket, its
/// first commits, and its `started` row) shares no journal commit with them,
/// and a daemon stopped, swapped, or killed within it starts none. The
/// lifecycle bench's daemons each live a few milliseconds: with the tender
/// started at once, each stop, kill, and swap met one starting.
pub const START_AFTER: Duration = Duration::from_secs(2);
/// How long `health` waits for the tender's `index.status`. A tender answers
/// in well under a millisecond, but its status reads the model's state, which
/// an embedding batch may hold: then health says what it last heard.
pub const HEALTH_DEADLINE: Duration = Duration::from_millis(100);
/// How long `index.status`, an explicit question, waits.
pub const STATUS_DEADLINE: Duration = Duration::from_secs(2);
/// How long it waits for an `index.query`, besides the query's own `wait_ms`.
pub const QUERY_DEADLINE: Duration = Duration::from_secs(10);

/// The wait before the next start, after an exit: [`BACKOFF_FIRST`] after the
/// first exit or a healthy run, and otherwise twice the last wait, at most
/// [`BACKOFF_MAX`].
pub fn next_backoff(last: Option<Duration>, ran: Duration) -> Duration {
    match last {
        Some(w) if ran < HEALTHY_RUN => (w * 2).clamp(BACKOFF_FIRST, BACKOFF_MAX),
        _ => BACKOFF_FIRST,
    }
}

/// The index directory beside a store's: `store` → `index`, `store-stdio` →
/// `index-stdio`, as the spool's is (`rpc::spool_dir`).
pub fn index_dir(store_dir: &Path) -> PathBuf {
    let name = store_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "store".into());
    store_dir.with_file_name(name.replacen("store", "index", 1))
}

/// The tender's binary beside this process's own: `theseusd`'s directory, as
/// the install recipe lays them out (and `target/<profile>/` in a build). By
/// that path only, never PATH: the daemon runs the tender of its own install.
pub fn beside_this_binary() -> io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(BINARY))
}

/// What the supervisor does to the world, so that its tests run on tokio's
/// paused clock with no process.
pub trait Os: Send + Sync + 'static {
    /// The live tender this daemon already has: the one an exec kept, which
    /// `children::relearn` found.
    fn running(&self) -> Option<u32>;
    /// Start a tender, registered so the reaper's sweep reports its exit.
    fn spawn(&self, program: &Path, args: &[OsString]) -> io::Result<u32>;
    /// SIGTERM, never waited for.
    fn terminate(&self, pid: u32);
    /// A process's arguments after its binary, when they can be read.
    fn args_of(&self, pid: u32) -> Option<Vec<OsString>>;
}

/// The real one: the children registry, and signals.
pub struct ChildrenOs;

/// What the tender's environment keeps of the daemon's: none of its secrets
/// (the vault's token among them), only its home and a log filter.
const KEEP_ENV: [&str; 2] = ["HOME", "RUST_LOG"];

impl Os for ChildrenOs {
    fn running(&self) -> Option<u32> {
        theseus_kernel::children::tender(NAME)
    }

    fn spawn(&self, program: &Path, args: &[OsString]) -> io::Result<u32> {
        use std::process::{Command, Stdio};
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env_clear()
            .envs(
                KEEP_ENV
                    .iter()
                    .filter_map(|k| Some((k, std::env::var_os(k)?))),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        // Its log goes where the daemon's goes: stderr, inherited.
        let child = theseus_kernel::children::spawn(
            theseus_kernel::children::Kind::Tender(NAME),
            || cmd.spawn(),
            |c| Some(c.id()),
        )?;
        // The sweep reaps it by its pid; std's `Child` never waits on drop.
        Ok(child.id())
    }

    fn terminate(&self, pid: u32) {
        // SAFETY: plain integers. The pid is this daemon's own registered
        // child, which only this daemon reaps, so it is not yet reused.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    fn args_of(&self, pid: u32) -> Option<Vec<OsString>> {
        use std::os::unix::ffi::OsStrExt;
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let mut args: Vec<OsString> = cmdline
            .split(|&b| b == 0)
            .skip(1)
            .map(|a| std::ffi::OsStr::from_bytes(a).to_os_string())
            .collect();
        // The command line ends with its last argument's NUL.
        if args.last().is_some_and(|a| a.is_empty()) {
            args.pop();
        }
        Some(args)
    }
}

/// Where the supervisor's facts go: each one's row (`crate::fact::index`,
/// all `index.tender`), for the core's ledger.
pub type Ledger = Arc<dyn Fn(LedgerRow) + Send + Sync>;

enum Event {
    Exited {
        pid: u32,
        status: Option<ExitStatus>,
    },
    Stop,
}

/// The exit of tender `pid` (its status, when the reaper had one), or None at
/// the stop. An exit of an older tender is history.
async fn exit_of(
    inbox: &mut mpsc::UnboundedReceiver<Event>,
    pid: u32,
) -> Option<Option<ExitStatus>> {
    loop {
        match inbox.recv().await {
            None | Some(Event::Stop) => return None,
            Some(Event::Exited { pid: p, status }) if p == pid => return Some(status),
            Some(Event::Exited { .. }) => {}
        }
    }
}

/// The tender as supervised: what health's `children` and `index` show.
struct Board {
    state: &'static str,
    pid: Option<u32>,
    adopted: bool,
    started: Option<Instant>,
    started_at_ms: u64,
    restarts: u64,
    last_exit: Option<String>,
    last_exit_ms: u64,
    next_start_ms: Option<u64>,
    backoff: Option<Duration>,
    why: Option<String>,
    stopping: bool,
}

/// The index tender's supervisor: one per core.
pub struct IndexTender {
    cfg: IndexConfig,
    store_dir: PathBuf,
    dir: PathBuf,
    /// The binary to run; `None`: the one beside this process's own, looked
    /// for once serving (`beside_this_binary`).
    program: Option<PathBuf>,
    /// The binary `run` found.
    binary: std::sync::OnceLock<PathBuf>,
    os: Arc<dyn Os>,
    /// How long a fresh start waits once `run` begins: [`START_AFTER`].
    start_after: Duration,
    /// Set once the core is built: it writes each `index.tender` row.
    ledger: std::sync::OnceLock<Ledger>,
    board: Mutex<Board>,
    /// The tender's last answer to `index.status`, and when (unix ms).
    last: Mutex<Option<(IndexStatus, u64)>>,
    events: mpsc::UnboundedSender<Event>,
    inbox: Mutex<Option<mpsc::UnboundedReceiver<Event>>>,
}

impl IndexTender {
    /// A supervisor for the tender of the store at `store_dir`, running
    /// `program`, or (`None`) the tender's binary beside this process's own.
    /// Nothing starts, and nothing is looked for, until [`IndexTender::run`].
    pub fn new(
        cfg: IndexConfig,
        store_dir: &Path,
        program: Option<PathBuf>,
        os: Arc<dyn Os>,
    ) -> Self {
        let (events, inbox) = mpsc::unbounded_channel();
        let state = if cfg.enabled { "pending" } else { "off" };
        Self {
            dir: index_dir(store_dir),
            store_dir: store_dir.to_path_buf(),
            cfg,
            program,
            binary: std::sync::OnceLock::new(),
            os,
            start_after: START_AFTER,
            ledger: std::sync::OnceLock::new(),
            board: Mutex::new(Board {
                state,
                pid: None,
                adopted: false,
                started: None,
                started_at_ms: 0,
                restarts: 0,
                last_exit: None,
                last_exit_ms: 0,
                next_start_ms: None,
                backoff: None,
                why: None,
                stopping: false,
            }),
            last: Mutex::new(None),
            events,
            inbox: Mutex::new(Some(inbox)),
        }
    }

    /// The same, with a fresh start waiting `wait` instead of
    /// [`START_AFTER`] (tests: zero).
    pub fn with_start_after(mut self, wait: Duration) -> Self {
        self.start_after = wait;
        self
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    /// The index's directory, `<state>/index`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The tender's socket.
    pub fn socket(&self) -> PathBuf {
        self.dir.join("sock")
    }

    /// Where each fact's `index.tender` row goes: the core's ledger. Set
    /// once.
    pub fn set_ledger(&self, ledger: Ledger) {
        let _ = self.ledger.set(ledger);
    }

    /// Record one of the tender's facts: its row, the only channel it has.
    fn record<F: Fact>(&self, f: &F) {
        if let (Some(kind), Some(l)) = (F::KIND, self.ledger.get()) {
            l(LedgerRow::new(kind, None, None, f.row()));
        }
    }

    fn board(&self) -> MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The tender's command line after its binary.
    pub fn args(&self) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec![
            "serve".into(),
            "--store".into(),
            self.store_dir.clone().into(),
            "--index".into(),
            self.dir.clone().into(),
            "--parent".into(),
            std::process::id().to_string().into(),
            "--weights-dir".into(),
            crate::config::expand(&self.cfg.weights_dir).into(),
            "--threads".into(),
            self.cfg.threads.max(1).to_string().into(),
            "--idle-unload-mins".into(),
        ];
        a.push(self.cfg.idle_unload_mins.max(0.0).to_string().into());
        a
    }

    /// Supervise until [`IndexTender::stop`]: take over the tender an exec
    /// kept, or start one, then start the next after each exit, once its
    /// backoff has passed. Runs once; a second call returns at once.
    pub async fn run(self: Arc<Self>) {
        let Some(mut inbox) = self
            .inbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return;
        };
        if self.board().stopping {
            return;
        }
        if !self.cfg.enabled {
            // A restart onto a note that turned `[index]` off: the tender the
            // last image started goes too.
            if let Some(pid) = self.os.running() {
                tracing::info!(
                    pid,
                    "index: [index] is off; stopping the tender the last image started"
                );
                self.os.terminate(pid);
            }
            return;
        }
        let Some(program) = self.program() else {
            return;
        };
        let mut kept = self.os.running();
        if kept.is_none() && !self.start_after.is_zero() && !self.first_wait(&mut inbox).await {
            return;
        }
        loop {
            let pid = match kept.take() {
                Some(pid) => self.take_over(pid),
                None => match self.start(&program) {
                    Ok(pid) => pid,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        self.absent(format!(
                            "{} is gone ({e}): install it beside theseusd, and restart the daemon",
                            program.display()
                        ));
                        return;
                    }
                    Err(e) => {
                        let wait = self.failed(&format!("it would not start: {e}"));
                        if !self.wait(&mut inbox, wait).await {
                            return;
                        }
                        continue;
                    }
                },
            };
            let Some(status) = exit_of(&mut inbox, pid).await else {
                return;
            };
            let wait = self.ended(pid, status);
            if !self.wait(&mut inbox, wait).await {
                return;
            }
        }
    }

    /// The tender's binary: `program`, or the `theseus-index` beside this
    /// daemon's. None, and the board says `absent`, when it is not there.
    fn program(&self) -> Option<PathBuf> {
        let program = match self.program.clone() {
            Some(p) => p,
            None => match beside_this_binary() {
                Ok(p) if p.is_file() => p,
                looked => {
                    let at = looked.map_or_else(|e| format!("({e})"), |p| p.display().to_string());
                    self.absent(format!(
                        "no {BINARY} beside theseusd, at {at}: install it there, as the install \
                         recipe does, and restart the daemon"
                    ));
                    return None;
                }
            },
        };
        let _ = self.binary.set(program.clone());
        Some(program)
    }

    /// No tender can run: its binary is not installed (`why`).
    fn absent(&self, why: String) {
        tracing::warn!(why = %why, "index: no tender");
        let mut b = self.board();
        b.state = "absent";
        b.why = Some(why);
    }

    /// A fresh start waits for the daemon's own start to settle
    /// (`START_AFTER`). False when the daemon stops first.
    async fn first_wait(&self, inbox: &mut mpsc::UnboundedReceiver<Event>) -> bool {
        {
            let mut b = self.board();
            b.next_start_ms =
                Some(theseus_protocol::now_unix_ms() + self.start_after.as_millis() as u64);
            b.why = Some(format!(
                "it starts {:.0} s after the daemon serves",
                self.start_after.as_secs_f64()
            ));
        }
        self.wait(inbox, self.start_after).await
    }

    /// Take over the tender an exec kept. A restart onto a note that changed
    /// `[index]` (its model's files, its threads, its unload): the tender the
    /// last image started has the old settings, so it goes, and the next
    /// starts with the new ones.
    fn take_over(&self, pid: u32) -> u32 {
        self.adopted(pid);
        if self.os.args_of(pid).is_some_and(|a| a != self.args()) {
            tracing::info!(pid, "index: [index] changed; restarting the tender");
            self.record(&TenderSettingsChanged { pid });
            self.os.terminate(pid);
        }
        pid
    }

    /// Sleep `wait` on the runtime's timer, unless the daemon stops first:
    /// false then.
    async fn wait(&self, inbox: &mut mpsc::UnboundedReceiver<Event>, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(until) => return !self.board().stopping,
                ev = inbox.recv() => match ev {
                    None | Some(Event::Stop) => return false,
                    Some(Event::Exited { .. }) => {}
                },
            }
        }
    }

    fn start(&self, program: &Path) -> io::Result<u32> {
        let mut b = self.board();
        if b.stopping {
            return Err(io::Error::other("the daemon is stopping"));
        }
        let pid = self.os.spawn(program, &self.args())?;
        let first = b.started_at_ms == 0 && !b.adopted;
        if !first {
            b.restarts += 1;
        }
        b.state = "running";
        b.adopted = false;
        b.pid = Some(pid);
        b.started = Some(Instant::now());
        b.started_at_ms = theseus_protocol::now_unix_ms();
        b.next_start_ms = None;
        b.why = None;
        let restarts = b.restarts;
        drop(b);
        tracing::info!(pid, restarts, "index: the tender started");
        self.record(&TenderStarted {
            pid,
            restarts,
            binary: program,
        });
        Ok(pid)
    }

    fn adopted(&self, pid: u32) {
        let mut b = self.board();
        b.state = "running";
        b.pid = Some(pid);
        b.adopted = true;
        b.started = Some(Instant::now());
        b.started_at_ms = theseus_protocol::now_unix_ms();
        drop(b);
        tracing::info!(
            pid,
            "index: took over the tender this daemon's last image started"
        );
        self.record(&TenderAdopted { pid });
    }

    /// The tender `pid` ended: its backoff.
    fn ended(&self, pid: u32, status: Option<ExitStatus>) -> Duration {
        let how = match status {
            Some(s) => exit_words(s),
            None => "gone".to_string(),
        };
        let mut b = self.board();
        let ran = b.started.map(|t| t.elapsed()).unwrap_or_default();
        let wait = next_backoff(b.backoff, ran);
        b.backoff = Some(wait);
        b.pid = None;
        b.started = None;
        b.state = "backoff";
        b.last_exit = Some(how.clone());
        b.last_exit_ms = theseus_protocol::now_unix_ms();
        b.next_start_ms = Some(b.last_exit_ms + wait.as_millis() as u64);
        b.why = Some(format!(
            "it exited ({how}) after {:.1} s; it starts again in {:.0} s",
            ran.as_secs_f64(),
            wait.as_secs_f64()
        ));
        drop(b);
        tracing::warn!(pid, how = %how, ran_ms = ran.as_millis() as u64, wait_ms = wait.as_millis() as u64,
                       "index: the tender exited; starting another after its backoff");
        self.record(&TenderExited {
            pid,
            how: &how,
            ran,
            backoff: wait,
        });
        wait
    }

    /// A start that failed: its backoff.
    fn failed(&self, why: &str) -> Duration {
        let mut b = self.board();
        let wait = next_backoff(b.backoff, Duration::ZERO);
        b.backoff = Some(wait);
        b.state = "backoff";
        b.next_start_ms = Some(theseus_protocol::now_unix_ms() + wait.as_millis() as u64);
        b.why = Some(format!(
            "{why}; trying again in {:.0} s",
            wait.as_secs_f64()
        ));
        drop(b);
        tracing::warn!(why = %why, wait_ms = wait.as_millis() as u64, "index: the tender did not start");
        self.record(&TenderStartFailed { why, backoff: wait });
        wait
    }

    /// The reaper's sweep reaped the tender `pid` (`status`: how it ended, or
    /// `None` when it was not this process's to reap). Its supervisor starts
    /// the next.
    pub fn exited(&self, pid: u32, status: Option<ExitStatus>) {
        let _ = self.events.send(Event::Exited { pid, status });
    }

    /// The daemon stops (§9: shutdown waits on nothing): nothing starts after
    /// this, and the tender gets SIGTERM, never waited for. `keep`: a restart
    /// onto the vault's changed note, whose new image takes the tender over,
    /// so it gets no signal. It writes no row: the daemon's own
    /// `server.stopping` says it stopped, and a frame here would put an fsync
    /// on the stop's path, which the runtime's end waits for.
    pub fn stop(&self, keep: bool) {
        let mut b = self.board();
        if b.stopping || !self.cfg.enabled {
            return;
        }
        b.stopping = true;
        let pid = b.pid;
        if b.state == "running" || b.state == "backoff" || b.state == "pending" {
            b.state = "stopped";
            b.why = Some(if keep {
                "the daemon restarts in place, and its next image takes the tender over".into()
            } else {
                "the daemon is stopping: the tender was sent SIGTERM".into()
            });
        }
        drop(b);
        let _ = self.events.send(Event::Stop);
        if let (Some(pid), false) = (pid, keep) {
            self.os.terminate(pid);
        }
    }

    /// The tender as supervised: health's `children.tenders`. `None` while
    /// `[index]` is off.
    pub fn status(&self) -> Option<TenderStatus> {
        if !self.cfg.enabled {
            return None;
        }
        let b = self.board();
        Some(TenderStatus {
            name: NAME.into(),
            state: b.state.into(),
            pid: b.pid,
            adopted: b.adopted,
            started_at_ms: b.started_at_ms,
            restarts: b.restarts,
            last_exit: b.last_exit.clone(),
            last_exit_ms: b.last_exit_ms,
            next_start_ms: b.next_start_ms,
            backoff_ms: b.backoff.map(|w| w.as_millis() as u64).unwrap_or(0),
            why: b.why.clone(),
            binary: self
                .binary
                .get()
                .or(self.program.as_ref())
                .map(|p| p.display().to_string()),
        })
    }

    /// Health's `index` block: the tender's own `index.status`, asked under
    /// `deadline` ([`HEALTH_DEADLINE`] for `health`, [`STATUS_DEADLINE`] for
    /// `index.status`). A running tender that does not answer in time is
    /// shown by its last answer, and says how old it is; one that is not
    /// running is `down`, and why.
    pub async fn health(&self, deadline: Duration) -> IndexHealth {
        let Some(tender) = self.status() else {
            return IndexHealth {
                state: "off".into(),
                why: Some("[index] enabled = false".into()),
                ..IndexHealth::default()
            };
        };
        let asked =
            call::<IndexStatus>(&self.socket(), method::STATUS, Value::Null, deadline).await;
        let last = &mut *self.last.lock().unwrap_or_else(PoisonError::into_inner);
        match asked {
            Ok(s) => {
                *last = Some((s.clone(), theseus_protocol::now_unix_ms()));
                IndexHealth {
                    state: s.state.clone(),
                    why: None,
                    tender: Some(tender),
                    status: Some(s),
                }
            }
            Err(e) => match last.as_ref().filter(|_| tender.state == "running") {
                Some((s, at)) => IndexHealth {
                    state: s.state.clone(),
                    why: Some(format!(
                        "its socket did not answer ({e}): its status as of {:.1} s ago",
                        theseus_protocol::now_unix_ms().saturating_sub(*at) as f64 / 1000.0
                    )),
                    tender: Some(tender),
                    status: Some(s.clone()),
                },
                None => IndexHealth {
                    state: "down".into(),
                    why: Some(down_why(&tender, Some(&e))),
                    tender: Some(tender),
                    status: None,
                },
            },
        }
    }

    /// Health's `index` block for `health`: [`IndexTender::health`] under
    /// [`HEALTH_DEADLINE`], but only of a tender its supervisor runs. Before
    /// one starts, between its runs, or with none installed, the block is what
    /// the supervisor knows, and no socket is asked: so a start's first answer
    /// never waits on one (a stale socket, or a predecessor's still dying).
    /// The wait after serving ([`START_AFTER`]) is `starting`.
    pub async fn health_block(&self) -> IndexHealth {
        match self.status() {
            Some(t) if t.state != "running" => IndexHealth {
                state: if t.state == "pending" && t.next_start_ms.is_some() {
                    "starting"
                } else {
                    "down"
                }
                .into(),
                why: Some(down_why(&t, None)),
                tender: Some(t),
                status: None,
            },
            _ => self.health(HEALTH_DEADLINE).await,
        }
    }

    /// `index.query`, forwarded to the tender as it came, under
    /// [`QUERY_DEADLINE`] and the query's own `wait_ms`. An error is a
    /// protocol code and its message: `DISABLED` while `[index]` is off, the
    /// tender's own code when it refused the query, and `INTERNAL` when no
    /// tender answered, saying why.
    pub async fn query(&self, p: &IndexQueryParams) -> Result<IndexQueryResult, (i64, String)> {
        use theseus_protocol::error_code;
        let Some(tender) = self.status() else {
            return Err((
                error_code::DISABLED,
                "the index is off: [index] enabled = false in the config".into(),
            ));
        };
        let deadline = QUERY_DEADLINE + Duration::from_millis(p.wait_ms.min(60_000));
        match call(&self.socket(), method::QUERY, p, deadline).await {
            Ok(r) => Ok(r),
            Err(CallError::Answered { code, message }) => Err((
                code,
                format!("the index tender refused the query: {message}"),
            )),
            Err(e) => Err((
                error_code::INTERNAL,
                format!(
                    "the index tender did not answer: {}",
                    down_why(&tender, Some(&e))
                ),
            )),
        }
    }
}

impl IndexTender {
    /// One of the memory pass's calls (M6 step 31a: `index.neighbours`,
    /// `index.entities`), to a tender that runs, under [`QUERY_DEADLINE`].
    /// `Err(None)`: no tender runs, or none answered, and why; `Err(Some)`:
    /// it answered with an error.
    pub async fn ask<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, TenderMiss> {
        let Some(tender) = self.status() else {
            return Err(TenderMiss::Down(
                "the index is off: [index] enabled = false".into(),
            ));
        };
        if tender.state != "running" {
            return Err(TenderMiss::Down(down_why(&tender, None)));
        }
        match call(&self.socket(), method, params, QUERY_DEADLINE).await {
            Ok(r) => Ok(r),
            Err(CallError::Answered { message, .. }) => Err(TenderMiss::Refused(message)),
            Err(e) => Err(TenderMiss::Down(down_why(&tender, Some(&e)))),
        }
    }
}

/// Why the tender gave no answer to one of the memory pass's calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TenderMiss {
    /// No tender runs, or none answered.
    Down(String),
    /// It answered with an error (a node it has no vector for yet).
    Refused(String),
}

/// Why no tender answered, in words: what its supervisor knows, else what the
/// call met (`None`: none was made).
fn down_why(t: &TenderStatus, call: Option<&CallError>) -> String {
    let met = || call.map_or_else(|| "no tender runs".to_string(), |c| c.to_string());
    match t.state.as_str() {
        "running" => format!("its socket did not answer ({})", met()),
        "pending" => t
            .why
            .clone()
            .unwrap_or_else(|| "it starts once the daemon serves".into()),
        _ => t.why.clone().unwrap_or_else(met),
    }
}

/// Why a call to the tender failed.
#[derive(Debug)]
pub enum CallError {
    /// It answered with an error.
    Answered { code: i64, message: String },
    /// No answer: no socket, a refused or broken connection, an answer that
    /// does not read, or none within the deadline.
    NoAnswer(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Answered { code, message } => write!(f, "{message} ({code})"),
            CallError::NoAnswer(why) => f.write_str(why),
        }
    }
}

/// `exit 1`, `held (exit 3: another tender holds the index)`, `signal 9`.
pub(crate) fn exit_words(s: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (s.code(), s.signal()) {
        (Some(HELD), _) => format!("held (exit {HELD}: another tender holds the index)"),
        (Some(c), _) => format!("exit {c}"),
        (None, Some(sig)) => format!("signal {sig}"),
        _ => format!("{s}"),
    }
}

/// One request on the tender's socket, under `deadline`: connect, ask, and
/// read its one answer.
pub async fn call<T: DeserializeOwned>(
    socket: &Path,
    method: &str,
    params: impl Serialize,
    deadline: Duration,
) -> Result<T, CallError> {
    let no = |e: String| CallError::NoAnswer(e);
    let ask = async {
        let s = tokio::net::UnixStream::connect(socket)
            .await
            .map_err(|e| no(format!("connecting to {}: {e}", socket.display())))?;
        let (r, mut w) = s.into_split();
        let mut line = serde_json::to_vec(&Request::new(Id::Num(1), method, params))
            .map_err(|e| no(e.to_string()))?;
        line.push(b'\n');
        w.write_all(&line).await.map_err(|e| no(e.to_string()))?;
        let mut answer = String::new();
        let read = BufReader::new(r).read_line(&mut answer).await;
        if read.map_err(|e| no(e.to_string()))? == 0 {
            return Err(no("the tender closed the connection".into()));
        }
        let resp: Response = serde_json::from_str(&answer).map_err(|e| no(e.to_string()))?;
        if let Some(e) = resp.error {
            return Err(CallError::Answered {
                code: e.code,
                message: e.message,
            });
        }
        serde_json::from_value(resp.result.unwrap_or_default()).map_err(|e| no(e.to_string()))
    };
    tokio::time::timeout(deadline, ask)
        .await
        .map_err(|_| no(format!("no answer within {} ms", deadline.as_millis())))?
}
