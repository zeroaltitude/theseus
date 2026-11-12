//! The job cap (theseus-v6bd): how many jobs may be in flight at once, and
//! what a job over the cap is told.
//!
//! **The owner's word, 2026-11-12: the cap REFUSES with a named reason; it
//! does NOT queue.** A job over the cap is not held somewhere nobody is
//! looking, to start later under a turn that has moved on; it is refused,
//! and the refusal names which cap and why, in words the model and every
//! surface can act on. That is the whole decision this module carries, and
//! the reason it is a module rather than a line: the words are the point.
//!
//! **What "in flight" counts.** The spool's pid files
//! (`theseus_kernel::Spool::running`), which is one directory read and is
//! authoritative: a wrapper writes its pid file as it starts and the file is
//! removed at its report. The alternative — an in-memory counter incremented
//! at the launch and decremented at the settle — was weighed and rejected:
//! a job settles by several paths (the drain, the turn's own take, a late
//! result, a cancel, a deadline, a restart's reconcile), and a counter that
//! leaks one decrement refuses every job for the rest of the daemon's life,
//! which is a worse failure than the one the cap exists to stop.
//!
//! **What that count does not see, said plainly.** A job between
//! `launch`'s decision and its wrapper's pid file is in flight and is not
//! counted, so a burst of simultaneous launches can pass the cap by the
//! number of calls inside `launch` at that instant. That number is small by
//! construction — one group of a response's `proc.run` calls is already held
//! to `[tools] parallel_runs` — and the cap's job is to stop a runaway, not
//! to be an instantaneous admission control. A cap that promised the second
//! thing would need a durable reservation per launch, which is a frame per
//! job start.

/// `[tools] max_jobs = 0`: no cap, which is what every daemon did before
/// this row. Held here rather than inline so the one reader and the config's
/// own documentation cannot disagree about which value turns it off.
pub const OFF: usize = 0;

/// Whether `running` jobs is already at `cap`, and so a new one is refused.
/// `cap` of [`OFF`] is no cap and is never at it.
pub fn at_cap(cap: usize, running: usize) -> bool {
    cap != OFF && running >= cap
}

/// Why this job is refused, in the owner's terms: which cap, what it is set
/// to, how many are running, and what to do — never "try again later",
/// which is the queueing the decision rules out.
///
/// It names `[tools] max_jobs` so a reader can find the knob, and it says
/// the job did not run, in the words `not_started.rs` and the disk floor's
/// refusal use, so the model reads it as every other "it did not run".
pub fn refusal(cap: usize, running: usize) -> String {
    format!(
        "{running} jobs are already running and `[tools] max_jobs` is {cap}, so this one was \
         refused rather than queued: it did not run, and nothing of it was started. Nothing is \
         waiting to start it later. Let the running jobs end (`job.read`, `job.wait`) or stop one \
         (`job.stop`) and call again, or ask the operator to raise `[tools] max_jobs`."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boundary, each side of it, and the off switch. The card's own
    /// numbers: with the cap at 200 the 201st is the one refused, which is
    /// 200 running.
    #[test]
    fn the_cap_refuses_at_it_and_never_below_it_and_off_refuses_nothing() {
        assert!(!at_cap(200, 199), "the 200th is inside the cap");
        assert!(at_cap(200, 200), "the 201st is over it");
        assert!(at_cap(200, 1_000), "and so is anything past it");
        assert!(!at_cap(OFF, 0));
        // Off is off whatever is running: every daemon before this row.
        for running in [0, 1, 200, 10_000] {
            assert!(!at_cap(OFF, running), "{running}");
        }
        // A cap of 1 is a cap, not an accident of 0 reading as 1.
        assert!(!at_cap(1, 0) && at_cap(1, 1));
    }

    /// The refusal names the cap, the count, and the knob, and it does NOT
    /// say the job is queued or will run later: the owner's decision is that
    /// it refuses, and the words are the part a reader acts on.
    #[test]
    fn the_refusal_names_which_cap_and_never_promises_a_later_run() {
        let r = refusal(200, 200);
        for want in [
            "200 jobs are already running",
            "[tools] max_jobs",
            "did not run",
        ] {
            assert!(r.contains(want), "{want} missing from {r}");
        }
        // Counted, so a refusal that said none of it could not pass: three
        // of three wanted phrases, and none of the forbidden ones.
        let forbidden = [
            "queued and will",
            "try again later",
            "it will start",
            "waiting in line",
        ];
        let said = forbidden.iter().filter(|f| r.contains(**f)).count();
        assert_eq!(said, 0, "{r}");
        assert!(r.contains("rather than queued"), "{r}");
    }
}
