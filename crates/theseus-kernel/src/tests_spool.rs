//! One sync per completion, and the start that finishes a rename a crash cut
//! short (theseus-yxiv). A wrapper stand-in, this test binary run again, is
//! stopped right after its completion's sync or its rename and killed there
//! with SIGKILL; the next start recovers that completion once, and a
//! completion whose action is settled already writes nothing.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::kernel::*;
use crate::spool::Spool;
use crate::tests::*;
use crate::types::*;

/// Where the stand-in writes, which job, and where it stops.
const DIR: &str = "THESEUS_TEST_SPOOL_DIR";
const JOB: &str = "THESEUS_TEST_SPOOL_JOB";
const AT: &str = "THESEUS_TEST_SPOOL_AT";

/// The wrapper's stand-in for the kill test: run again by it, it writes the
/// job's completion as a wrapper does, stops itself with SIGSTOP after the
/// sync (`synced`) or after the rename (`renamed`), and waits there for the
/// SIGKILL. Run on its own, with no environment, it does nothing.
#[test]
fn a_wrapper_stand_in_for_the_kill_test() {
    let (Ok(dir), Ok(job), Ok(at)) = (std::env::var(DIR), std::env::var(JOB), std::env::var(AT))
    else {
        return;
    };
    let spool = Spool::open(Path::new(&dir)).unwrap();
    let synced = spool
        .write_synced(&completion(&job, Outcome::Succeeded, None))
        .unwrap();
    let _held = match at.as_str() {
        "renamed" => {
            synced.publish().unwrap();
            None
        }
        _ => Some(synced),
    };
    loop {
        // SAFETY: a signal to this process itself.
        unsafe { libc::raise(libc::SIGSTOP) };
    }
}

/// Run the stand-in for `job`, wait until it has stopped at `at`, and kill it
/// with SIGKILL there.
fn kill_wrapper_at(spool: &Spool, job: &str, at: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "tests_spool::a_wrapper_stand_in_for_the_kill_test",
            "--exact",
            "--nocapture",
        ])
        .env(DIR, spool.dir())
        .env(JOB, job)
        .env(AT, at)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    // SAFETY: waits for our own child to stop; it is not reaped while stopped.
    let got = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED) };
    assert_eq!(got, pid, "{}", std::io::Error::last_os_error());
    assert!(
        libc::WIFSTOPPED(status),
        "the stand-in ended before it reached {at}: status {status:#x}"
    );
    // SAFETY: our own stopped child.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    child.wait().unwrap();
}

fn tmp_path(w: &World, job: &str) -> std::path::PathBuf {
    w.spool.dir().join(format!("{job}.json.tmp"))
}

/// An execution waiting on one dispatched job.
fn waiting_on_a_job(w: &World) -> (Execution, Action) {
    let (_, e, g) = running(w);
    let a = dispatched(w, &g, "proc.run", 0);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![a.correlation_id.clone()],
                },
            },
        )
        .unwrap();
    (e, a)
}

fn frames(w: &World) -> u64 {
    w.kernel.store().stats().unwrap().frames_appended
}

/// A wrapper killed after its completion's sync and before its rename leaves
/// the completion whole under its tmp name. The next start renames it into
/// place and settles the job, once: the start after writes nothing for it.
#[test]
fn a_wrapper_killed_before_its_rename_is_recovered_once_at_the_start() {
    let w = world();
    let (e, a) = waiting_on_a_job(&w);
    kill_wrapper_at(&w.spool, &a.correlation_id, "synced");
    assert!(tmp_path(&w, &a.correlation_id).exists());
    assert!(!w.spool.has_completion(&a.correlation_id));

    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!((rep.spool_recovered, rep.spool_drained), (1, 1));
    let a2 = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(a2.state, ActionState::Succeeded);
    assert_eq!(a2.completions_seen, 1);
    let e2 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e2.state, ExecState::Queued, "the job's turn wakes");
    assert_eq!(e2.queued_results, vec![a.correlation_id.clone()]);
    assert!(!tmp_path(&w, &a.correlation_id).exists());
    assert!(!w.spool.has_completion(&a.correlation_id));

    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!((rep.spool_recovered, rep.spool_drained), (0, 0));
    assert_eq!(frames(&w), 1, "the start's own rows, and nothing else");
    let a3 = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(a3.completions_seen, 1);
}

/// A wrapper killed after its rename, before anything after it (its pid
/// file's removal, its poke): the start drains the completion as always.
#[test]
fn a_wrapper_killed_after_its_rename_is_drained_at_the_start() {
    let w = world();
    let (_, a) = waiting_on_a_job(&w);
    kill_wrapper_at(&w.spool, &a.correlation_id, "renamed");
    assert!(!tmp_path(&w, &a.correlation_id).exists());
    assert!(w.spool.has_completion(&a.correlation_id));

    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!((rep.spool_recovered, rep.spool_drained), (0, 1));
    let a2 = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(a2.state, ActionState::Succeeded);
    assert!(!w.spool.has_completion(&a.correlation_id));
}

/// The case one sync makes possible: the daemon settled a job, and a machine
/// crash then lost the completion's rename (or its file's removal). The start
/// still finds the completion, and takes it as the no-op it is: no frame, no
/// `completion.duplicate` row, the action as it was.
#[test]
fn a_recovered_completion_whose_job_is_settled_writes_nothing() {
    let w = world();
    let (e, a) = waiting_on_a_job(&w);
    let c = completion(&a.correlation_id, Outcome::Succeeded, None);
    w.kernel.take_completion_with(&c, vec![]).unwrap();
    // Its writer gone, its file whole, its rename lost.
    drop(w.spool.write_synced(&c).unwrap());
    let before = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(before.state, ActionState::Succeeded);

    let (w, rep) = crash(w, KernelConfig::default());
    assert_eq!((rep.spool_recovered, rep.spool_drained), (1, 1));
    assert_eq!(frames(&w), 1, "the start's own rows, and nothing else");
    let after = w.kernel.action(&a.correlation_id).unwrap().unwrap();
    assert_eq!(after, before);
    assert!(rows(&w, &e.session_id, "completion.duplicate").is_empty());
    assert!(!tmp_path(&w, &a.correlation_id).exists());
    assert!(!w.spool.has_completion(&a.correlation_id));
}
