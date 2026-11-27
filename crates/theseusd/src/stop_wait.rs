//! The runtime's end at a stop and at a restart in place (issue 63). A runtime's drop waits for every thread of
//! its blocking pool, with no bound, and a thread there can be waiting for a run of the daemon's own in L1 (a self
//! build's gate, `theseus_kernel::own_runs`): such a run held a restart for as long as it ran. So the runtime ends
//! here instead:
//!
//! - once its tasks are gone (nothing is left to read what a run cut now returns, so a cut gate is never taken for
//!   a red one), or `ASK_ANYWAY` after the end began, every own run is asked to end: SIGTERM to its job, the
//!   wrapper's `STOP_GRACE`, then the kill of the job's pid namespace. A job of the daemon's own runs in its own
//!   namespace, so nothing else is signalled: never the daemon's process group, which the job's init shares;
//! - a wait past `FIRST_SAY` says what it waits for, and again every `SAY_EVERY`;
//! - the pool gets at most `BOUND`, then the stop goes on, and says what was left.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use theseus_kernel::own_runs::{self, Run};

/// The longest the runtime's blocking pool is waited for. An own run ends within `STOP_GRACE` and the kill's wait
/// once asked; anything else there is a short unit of work.
pub const BOUND: Duration = Duration::from_secs(60);

/// When the watch's times fall.
#[derive(Debug, Clone, Copy)]
struct Timing {
    /// How often it looks.
    look: Duration,
    /// When it asks the own runs to end though tasks are still alive.
    ask_anyway: Duration,
    /// When it first says what the stop waits for.
    first_say: Duration,
    /// How often it says so again.
    say_every: Duration,
}

const REAL: Timing = Timing {
    look: Duration::from_millis(10),
    ask_anyway: Duration::from_secs(2),
    first_say: Duration::from_secs(5),
    say_every: Duration::from_secs(30),
};

/// What a waiting stop names: each own run still going, else the pool's other work.
fn waiting_for(runs: &[Run]) -> String {
    if runs.is_empty() {
        return "work on the runtime's blocking threads".to_string();
    }
    runs.iter().map(Run::words).collect::<Vec<_>>().join(", ")
}

/// End `rt` as a stop does: its tasks dropped, its own runs ended, its pool waited for at most `BOUND`.
pub fn end_runtime(rt: tokio::runtime::Runtime) {
    let handle = rt.handle().clone();
    let watch = Watch::start(
        REAL,
        move || handle.metrics().num_alive_tasks(),
        own_runs::running,
        || own_runs::end_all(theseus_kernel::job::STOP_GRACE),
        |waited, what| {
            tracing::warn!(
                waited_s = waited.as_secs(),
                waiting_for = %what,
                "stop: still waiting for what is running"
            );
        },
    );
    let t0 = Instant::now();
    rt.shutdown_timeout(BOUND);
    watch.done();
    if t0.elapsed() >= BOUND {
        tracing::warn!(
            bound_s = BOUND.as_secs(),
            left = %waiting_for(&own_runs::running()),
            "stop: the runtime's work did not end within its bound; the stop goes on without it"
        );
    }
}

/// The watch beside a runtime's end, on a thread of its own.
struct Watch {
    done: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    fn start(
        t: Timing,
        alive: impl Fn() -> usize + Send + 'static,
        running: impl Fn() -> Vec<Run> + Send + 'static,
        end: impl Fn() + Send + 'static,
        say: impl Fn(Duration, String) + Send + 'static,
    ) -> Self {
        let (done, rx) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("stop-wait".into())
            .spawn(move || {
                let t0 = Instant::now();
                let mut asked = false;
                let mut next_say = t.first_say;
                loop {
                    // The runtime has ended (or the watch was dropped): nothing is waited for any more.
                    if !matches!(rx.recv_timeout(t.look), Err(RecvTimeoutError::Timeout)) {
                        return;
                    }
                    let waited = t0.elapsed();
                    if !asked && (alive() == 0 || waited >= t.ask_anyway) {
                        asked = true;
                        end();
                    }
                    if waited >= next_say {
                        say(waited, waiting_for(&running()));
                        next_say += t.say_every;
                    }
                }
            })
            .ok();
        Self { done, thread }
    }

    /// The runtime has ended: the watch ends too.
    fn done(mut self) {
        let _ = self.done.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A test's plant (a debug build's `THESEUS_TEST_SELF_JOB`): a self build's command, the variable's shell text,
/// started at serving through the gate's own launcher (`Jail::run`, in L1, waited for on the blocking pool), as
/// the self pipeline starts its gate. What it does not hold: the pipeline before the gate (its tree, its lock,
/// may-build). Its result, should one come back to it, is logged and nothing else. A release build has no plant.
pub fn planted_self_job(core: &Arc<theseus_core::Core>, state_dir: &std::path::Path) {
    #[cfg(debug_assertions)]
    if let Ok(body) = std::env::var("THESEUS_TEST_SELF_JOB") {
        use std::ffi::OsString;
        use theseus_core::rsi::jail::{Jail, Job};
        use theseus_core::rsi::tree::{Place, Tree};
        let root = state_dir.join("planted-self-job");
        let jail = Jail::new(core.tools.sandbox.clone());
        tokio::spawn(async move {
            for d in ["repo", "state", "tree", "target"] {
                let _ = std::fs::create_dir_all(root.join(d));
            }
            let tree = Tree {
                slug: "planted".into(),
                branch: "self/planted".into(),
                dir: root.join("tree"),
                target: root.join("target"),
            };
            let place = Place {
                repo: root.join("repo"),
                state: root.join("state"),
                env: Vec::new(),
                jail: None,
                notify: None,
                stop: None,
                owner: None,
            };
            let log = root.join(format!("gate-{}.log", theseus_protocol::now_unix_ms()));
            let job = Job {
                tree: &tree,
                ro: Vec::new(),
                argv: ["sh", "-c", body.as_str()].map(OsString::from).to_vec(),
                env: Vec::new(),
                log: &log,
                limit: Duration::from_secs(3600),
            };
            tracing::info!("planted self job: started");
            let r = jail.run(&place, job).await;
            tracing::info!(result = ?r.map_err(|e| format!("{e:#}")), "planted self job: its result came back");
        });
    }
    #[cfg(not(debug_assertions))]
    let _ = (core, state_dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    const FAST: Timing = Timing {
        look: Duration::from_millis(2),
        ask_anyway: Duration::from_millis(400),
        first_say: Duration::from_millis(100),
        say_every: Duration::from_millis(300),
    };

    fn run(kind: &str, pid: u32) -> Run {
        Run {
            kind: kind.into(),
            pid,
        }
    }

    /// The own runs are asked to end once the runtime's tasks are gone, and not before.
    #[test]
    fn the_runs_are_asked_to_end_once_the_tasks_are_gone() {
        let alive = Arc::new(AtomicUsize::new(3));
        let asked = Arc::new(AtomicBool::new(false));
        let (a, b) = (alive.clone(), asked.clone());
        let w = Watch::start(
            FAST,
            move || a.load(Ordering::SeqCst),
            Vec::new,
            move || b.store(true, Ordering::SeqCst),
            |_, _| {},
        );
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !asked.load(Ordering::SeqCst),
            "asked while tasks were alive"
        );
        alive.store(0, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            asked.load(Ordering::SeqCst),
            "not asked once the tasks were gone"
        );
        w.done();
    }

    /// Tasks that never go do not hold the ask past `ask_anyway`.
    #[test]
    fn the_runs_are_asked_to_end_though_a_task_stays() {
        let asked = Arc::new(AtomicBool::new(false));
        let b = asked.clone();
        let t0 = Instant::now();
        let w = Watch::start(
            FAST,
            || 1,
            Vec::new,
            move || b.store(true, Ordering::SeqCst),
            |_, _| {},
        );
        while !asked.load(Ordering::SeqCst) {
            assert!(t0.elapsed() < Duration::from_secs(5), "never asked");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            t0.elapsed() >= FAST.ask_anyway,
            "asked early: {:?}",
            t0.elapsed()
        );
        w.done();
    }

    /// A wait past `first_say` says what it waits for, the run's kind and short id, and again each `say_every`;
    /// one that ends before says nothing.
    #[test]
    fn a_long_wait_says_what_it_waits_for_and_says_it_again() {
        let said = Arc::new(Mutex::new(Vec::<(Duration, String)>::new()));
        let s = said.clone();
        let w = Watch::start(
            FAST,
            || 0,
            || vec![run("self-build", 4242)],
            || {},
            move |waited, what| s.lock().unwrap().push((waited, what)),
        );
        std::thread::sleep(Duration::from_millis(80));
        assert!(said.lock().unwrap().is_empty(), "said before its time");
        std::thread::sleep(Duration::from_millis(900));
        w.done();
        let said = said.lock().unwrap().clone();
        assert!(said.len() >= 2, "{said:?}");
        assert_eq!(said[0].1, "the self-build run (job 4242)");
        assert!(said[0].0 >= FAST.first_say, "{said:?}");
        assert!(said[1].0 >= FAST.first_say + FAST.say_every, "{said:?}");
        assert!(said.len() <= 4, "said too often: {said:?}");
    }

    /// With no own run left, the wait names the pool's other work.
    #[test]
    fn a_wait_with_no_run_names_the_pool() {
        assert_eq!(waiting_for(&[]), "work on the runtime's blocking threads");
        assert_eq!(
            waiting_for(&[run("self-build", 7), run("watch", 9)]),
            "the self-build run (job 7), the watch run (job 9)"
        );
    }
}
