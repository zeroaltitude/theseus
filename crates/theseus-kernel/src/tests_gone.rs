//! Jobs whose wrappers went while no daemon ran (theseus-vej5): at the first
//! beat after serving, each job startup found dispatched is probed once. A
//! gone wrapper's job is unknown, a finished one's completion is taken, and
//! a live wrapper, a lingering one, and an action with no evidence of a job
//! are left as they are, all in one frame.

use std::process::{Child, Command, Stdio};

use crate::gone::{at_start, AtStart, WRAPPER_GONE};
use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

/// A live process whose command line is job `id`'s wrapper's
/// (`<sh> job-wrapper --correlation-id <id>`), as `job_in_cmdline` reads it:
/// `sh` runs a script named `job-wrapper`, so the script's name is the
/// second word. Killed and reaped when dropped.
struct Fake(Child);

impl Fake {
    fn wrapper(dir: &std::path::Path, id: &str) -> Fake {
        // `sleep` is not the script's last command, so `sh` never execs it,
        // and the process keeps its command line. Written once: a rewrite
        // truncates it under an `sh` that has not read it yet, which then
        // finds it empty and exits.
        let script = dir.join("job-wrapper");
        if !script.exists() {
            std::fs::write(&script, "sleep 60\n:\n").unwrap();
        }
        Fake::spawn(
            Command::new("sh")
                .args(["job-wrapper", "--correlation-id", id, "--", "sleep"])
                .current_dir(dir),
        )
    }

    /// A live process that is no job's wrapper: a pid reused by something
    /// else.
    fn other() -> Fake {
        Fake::spawn(Command::new("sleep").arg("60"))
    }

    fn spawn(cmd: &mut Command) -> Fake {
        let c = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Fake(c)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A pid no process holds: a child's, once reaped.
fn dead_pid() -> u32 {
    let mut c = Command::new("true").spawn().unwrap();
    let pid = c.id();
    c.wait().unwrap();
    pid
}

/// The ids are those actions', in any order.
fn same(ids: &[&str], actions: &[&Action]) {
    let mut got = ids.to_vec();
    got.sort_unstable();
    let mut want: Vec<&str> = actions.iter().map(|a| a.correlation_id.as_str()).collect();
    want.sort_unstable();
    assert_eq!(got, want);
}

fn frames(w: &World) -> u64 {
    w.kernel.store().stats().unwrap().frames_appended
}

fn state(w: &World, a: &Action) -> ActionState {
    w.kernel.action(&a.correlation_id).unwrap().unwrap().state
}

/// Every case at one start. Each job was dispatched by a process that died;
/// what its wrapper left is set up before the next start's probe:
/// - `gone`: its pid file names a pid no process holds;
/// - `stale`: no pid file, its lingering marker names a gone pid;
/// - `reused`: its pid file names a live process that is not a wrapper;
/// - `other`: its pid file names another job's live wrapper;
/// - `alive`: its pid file names its live wrapper;
/// - `lingering`: no pid file, its marker names its live wrapper;
/// - `finished`: its completion on the spool, written after the start's
///   drain, and a pid file naming a gone pid;
/// - `quiet`: nothing at all (an in-process tool call, say).
///
/// The first four are unknown with the reason, the fifth and sixth stay
/// dispatched, the seventh settles from its completion (its file removed),
/// the last stays dispatched, and the provider call is left to its own
/// mark. One frame for all; a second probe writes nothing.
#[test]
fn a_start_settles_the_jobs_whose_wrappers_went_and_leaves_the_rest() {
    let w = world();
    let (s, _, g) = running(&w);
    let job = |w: &World| dispatched(w, &g, "proc.run", 0);
    let (gone, stale, reused, other) = (job(&w), job(&w), job(&w), job(&w));
    let (alive, lingering, finished, quiet) = (job(&w), job(&w), job(&w), job(&w));
    let call = dispatched(&w, &g, PROVIDER_TOOL, 0);
    std::mem::forget(g); // the process dies holding the turn

    let (w, rep) = crash(w, KernelConfig::default());
    assert!(
        rep.reconcile.marked_unknown.is_empty(),
        "nothing before serving"
    );
    let scripts = tempfile::tempdir().unwrap();
    let mine = Fake::wrapper(scripts.path(), &alive.correlation_id);
    let lingers = Fake::wrapper(scripts.path(), &lingering.correlation_id);
    let theirs = Fake::wrapper(scripts.path(), "act_someone_elses");
    let unrelated = Fake::other();
    let sp = &w.spool;
    sp.write_pid(&gone.correlation_id, dead_pid()).unwrap();
    sp.write_lingering(&stale.correlation_id, dead_pid())
        .unwrap();
    sp.write_pid(&reused.correlation_id, unrelated.pid())
        .unwrap();
    sp.write_pid(&other.correlation_id, theirs.pid()).unwrap();
    sp.write_pid(&alive.correlation_id, mine.pid()).unwrap();
    sp.write_lingering(&lingering.correlation_id, lingers.pid())
        .unwrap();
    sp.write_pid(&finished.correlation_id, dead_pid()).unwrap();
    sp.write(&completion(
        &finished.correlation_id,
        Outcome::Succeeded,
        None,
    ))
    .unwrap();

    let f0 = frames(&w);
    // Each gone job's row, as the core's, rides in the same frame.
    let out = w
        .kernel
        .settle_gone_jobs_with(sp, |a, pid| {
            Ok(vec![w.kernel.ledger(
                theseus_protocol::LedgerKind::JobWrapperGone,
                Some(&a.session_id),
                serde_json::json!({"correlation_id": a.correlation_id, "pid": pid}),
            )?])
        })
        .unwrap();
    assert_eq!(frames(&w), f0 + 1, "one frame for all, the rows in it");
    assert_eq!(rows(&w, &s, "job.wrapper_gone").len(), 4);
    assert_eq!(out.probed, 8, "every job, and not the provider call");
    let marked: Vec<&str> = out
        .gone
        .iter()
        .map(|(a, _)| a.correlation_id.as_str())
        .collect();
    same(&marked, &[&gone, &stale, &reused, &other]);
    assert_eq!(out.finished, vec![finished.correlation_id.clone()]);
    let alive_ids: Vec<&str> = out.alive.iter().map(String::as_str).collect();
    same(&alive_ids, &[&alive, &lingering]);
    assert_eq!(out.unwitnessed, 1);

    for a in [&gone, &stale, &reused, &other] {
        assert_eq!(
            state(&w, a),
            ActionState::OutcomeUnknown,
            "{}",
            a.correlation_id
        );
    }
    for a in [&alive, &lingering, &quiet, &call] {
        assert_eq!(
            state(&w, a),
            ActionState::Dispatched,
            "{}",
            a.correlation_id
        );
    }
    assert_eq!(state(&w, &finished), ActionState::Succeeded);
    assert!(
        !sp.has_completion(&finished.correlation_id),
        "its file is taken"
    );
    let unknown = rows(&w, &s, "action.outcome_unknown");
    assert_eq!(unknown.len(), 4);
    assert!(unknown
        .iter()
        .all(|r| r["producer"] == format!("reconciler:{WRAPPER_GONE}")));
    // The live wrappers were not touched.
    assert!(crate::job::wrapper_alive(mine.pid(), &alive.correlation_id));
    assert!(crate::job::wrapper_alive(
        lingers.pid(),
        &lingering.correlation_id
    ));

    let again = w.kernel.settle_gone_jobs(sp).unwrap();
    assert_eq!(again.probed, 0);
    assert_eq!(frames(&w), f0 + 1, "a second probe writes nothing");
}

/// A start with no dispatched job probes nothing and writes nothing.
#[test]
fn a_start_with_no_job_probes_nothing() {
    let w = world();
    let (w, _) = crash(w, KernelConfig::default());
    let f0 = frames(&w);
    let out = w.kernel.settle_gone_jobs(&w.spool).unwrap();
    assert_eq!((out.probed, frames(&w)), (0, f0));
}

/// A job whose cancel was asked before the process died is the cancel's
/// (the reconciler probes it at every beat), and is never noted here; nor is
/// one past its deadline, which startup's reconcile probed already.
#[test]
fn a_cancelled_or_overdue_job_is_not_the_probes() {
    let w = world();
    let (_, e, g) = running(&w);
    let cancelled = dispatched(&w, &g, "proc.run", 0);
    std::mem::forget(g);
    w.kernel.cancel_execution(&e.id, "zeroaltitude").unwrap();
    let (_, _, g2) = running(&w);
    let overdue = dispatched(&w, &g2, "proc.run", 0);
    std::mem::forget(g2);
    w.clock.advance(120_000);
    let (w, rep) = crash(w, KernelConfig::default());
    assert!(rep
        .reconcile
        .marked_unknown
        .contains(&overdue.correlation_id));
    let out = w.kernel.settle_gone_jobs(&w.spool).unwrap();
    assert_eq!(out.probed, 0);
    assert!(w
        .kernel
        .action(&cancelled.correlation_id)
        .unwrap()
        .unwrap()
        .cancel
        .is_some());
}

/// A wrapper that reports between the probe's two reads: its completion is
/// written before its pid file goes, so the second read finds it, and the
/// job is finished, never gone.
#[test]
fn the_completion_is_read_before_the_wrapper_and_again_after() {
    let w = world();
    let id = "act_reported";
    assert_eq!(at_start(&w.spool, id), AtStart::Unwitnessed);
    w.spool.write_pid(id, dead_pid()).unwrap();
    assert!(matches!(at_start(&w.spool, id), AtStart::Gone { .. }));
    w.spool
        .write(&completion(id, Outcome::Failed, None))
        .unwrap();
    assert!(matches!(at_start(&w.spool, id), AtStart::Finished(c) if c.outcome == Outcome::Failed));
}

/// FAST: the probe's cost at a start with 0, 10, and 200 jobs dispatched,
/// each wrapper gone (its pid file names a pid no process holds), so every
/// one is read, probed, and marked: one frame for all, none with none. The
/// start's own reconcile only notes each job, and is printed beside it.
#[test]
fn the_probes_cost_at_0_10_and_200_jobs() {
    for n in [0usize, 10, 200] {
        let w = world();
        let (_, _, g) = running(&w);
        let jobs: Vec<Action> = (0..n).map(|_| dispatched(&w, &g, "proc.run", 0)).collect();
        std::mem::forget(g);
        let (w, rep) = crash(w, KernelConfig::default());
        let dead = dead_pid();
        for a in &jobs {
            w.spool.write_pid(&a.correlation_id, dead).unwrap();
        }
        let f0 = frames(&w);
        let t = std::time::Instant::now();
        let out = w.kernel.settle_gone_jobs(&w.spool).unwrap();
        let took = t.elapsed();
        assert_eq!((out.probed as usize, out.gone.len()), (n, n));
        assert_eq!(
            frames(&w) - f0,
            u64::from(n > 0),
            "one frame for all, none for none"
        );
        eprintln!(
            "theseus-vej5 probe: {n} jobs gone: {} us ({} us here); the start's reconcile {} us",
            out.elapsed_us,
            took.as_micros(),
            rep.reconcile.elapsed_us
        );
    }
}
