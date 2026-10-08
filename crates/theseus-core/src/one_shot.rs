//! A daemon spawned for one run (theseus-mqxk): `theseus --spawn ask` starts
//! `theseusd --stdio --one-shot SECS`, and the daemon ends with that run, at
//! most SECS after its turn. theseusd says so before anything is served
//! (`OneShot::set`). Then a turn's result names what it left for later
//! (`TurnSubmitResult.later`, `turn/later_step.rs`), which the CLI follows
//! until the run's end; the turn's connection hears its session's later
//! turns (`rpc::methods::turn_submit`); and `wake.at` says when a wake comes
//! after the run ends, so the model can wait another way. A socket daemon, and
//! a `--stdio` one started without the flag, change in nothing.
//!
//! The run ends SECS after the ask's turn ends (`fix_end`, at that turn's
//! end), and the daemon, not the client, says what fires before it: a wake
//! set at `t` fires in the run when it is due by `ends_by(t)`, the run's end
//! once it is fixed and `t` + SECS before then, which is the earliest the
//! run can end. So `wake.at`'s words, written during the ask's turn, and the
//! result's `fires`, which the CLI follows, are one rule, whatever the turn's
//! length.

use std::sync::OnceLock;

/// The run's bound, once theseusd has said the daemon is one run's, and the
/// run's end, once its first turn has ended.
#[derive(Debug, Default)]
pub struct OneShot {
    follow_ms: OnceLock<u64>,
    ends_at_ms: OnceLock<u64>,
}

impl OneShot {
    /// This daemon serves one run, whose client follows it for at most
    /// `follow_ms` after its turn. Said once, before serving.
    pub fn set(&self, follow_ms: u64) {
        let _ = self.follow_ms.set(follow_ms);
    }

    /// The run's bound, on a one-run daemon; None on any other.
    pub fn follow_ms(&self) -> Option<u64> {
        self.follow_ms.get().copied()
    }

    /// The run's end: fixed by its first turn's end at `now_ms` (the ask's
    /// own), and read as it was by every later one. None on any other daemon.
    pub fn fix_end(&self, now_ms: u64) -> Option<u64> {
        let follow = self.follow_ms()?;
        Some(
            *self
                .ends_at_ms
                .get_or_init(|| now_ms.saturating_add(follow)),
        )
    }

    /// The latest a wake set at `set_ms` may be due and still fire in the
    /// run: the run's end once fixed, and before then the earliest it can
    /// be. None on a daemon that is not one run's.
    pub fn ends_by(&self, set_ms: u64) -> Option<u64> {
        let soonest = set_ms.saturating_add(self.follow_ms()?);
        Some(
            self.ends_at_ms
                .get()
                .map_or(soonest, |&end| end.min(soonest)),
        )
    }

    /// Whether `w` fires in the run: on any daemon but one run's, always.
    pub fn fires(&self, w: &theseus_kernel::PendingWake) -> bool {
        self.ends_by(w.set_at_ms)
            .is_none_or(|end| w.due_at_ms <= end)
    }

    /// `wake.at`'s answer for `w` when it does not fire in the run: it is
    /// set, and will not fire here, so the model can wait inside the run
    /// instead. None when it fires, and on any other daemon.
    pub fn wake_after_run(
        &self,
        w: &theseus_kernel::PendingWake,
        short: &str,
        when: &str,
    ) -> Option<String> {
        let follow = self.follow_ms()?;
        (!self.fires(w)).then(|| {
            format!(
                "Set wake {short} for {when}, and its id is {}, but it will not fire in this run: \
                 this daemon was spawned for one run (`theseus --spawn ask`), which follows this \
                 conversation for at most {} after its first turn and then stops, so nothing is \
                 here when the wake is due. To wait for something, wait inside this run instead: \
                 a blocking proc.run (a `sleep`, or a loop that checks and sleeps) whose wait \
                 stays within the run's bound.",
                w.id,
                crate::wake::span(follow)
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wake(set_at_ms: u64, due_at_ms: u64) -> theseus_kernel::PendingWake {
        serde_json::from_value(serde_json::json!({
            "id": "wak_1", "due_at_ms": due_at_ms, "note": "check the lighthouse",
            "set_at_ms": set_at_ms, "by": "act_1",
        }))
        .unwrap()
    }

    #[test]
    fn a_socket_daemons_wakes_all_fire() {
        let socket = OneShot::default();
        assert!(socket.fires(&wake(1_000, u64::MAX)));
        assert_eq!(socket.fix_end(1_000), None);
        assert!(socket
            .wake_after_run(&wake(1_000, u64::MAX), "1", "then")
            .is_none());
    }

    /// One rule before and after the run's end is fixed: a wake set during
    /// the ask's turn is judged by its set time plus the bound (the words
    /// the model read), however long that turn ran on; one set later, by
    /// the run's end.
    #[test]
    fn a_wake_fires_when_due_by_the_end_the_run_had_when_it_was_set() {
        let run = OneShot::default();
        run.set(30_000);
        assert!(run.fires(&wake(1_000, 31_000)), "due at the bound");
        assert!(!run.fires(&wake(1_000, 31_001)), "due past it");
        // The ask's turn ran 5 s past the wake's call: the run ends at 36 s,
        // and the wake set at 1 s still does not fire.
        assert_eq!(run.fix_end(6_000), Some(36_000));
        assert!(!run.fires(&wake(1_000, 31_001)));
        // A later turn's wake: the run's end, not its set time plus 30 s.
        assert!(run.fires(&wake(20_000, 36_000)));
        assert!(!run.fires(&wake(20_000, 36_001)));
        assert_eq!(run.fix_end(50_000), Some(36_000), "fixed once");
        run.set(5);
        assert_eq!(run.follow_ms(), Some(30_000), "said once");
    }
}
