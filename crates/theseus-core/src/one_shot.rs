//! A daemon spawned for one run (theseus-mqxk): `theseus --spawn ask` starts
//! `theseusd --stdio --one-shot SECS`, and the daemon ends with that run, at
//! most SECS after its turn. theseusd says so before anything is served
//! (`OneShot::set`). Then a turn's result names what it left for later
//! (`TurnSubmitResult.later`, `turn/later_step.rs`), which the CLI follows
//! while its bound lasts; the turn's connection hears its session's later
//! turns (`rpc::methods::turn_submit`); and `wake.at` says when a wake comes
//! after the run ends, so the model can wait another way. A socket daemon, and
//! a `--stdio` one started without the flag, change in nothing.

use std::sync::OnceLock;

/// The run's bound, once theseusd has said the daemon is one run's.
#[derive(Debug, Default)]
pub struct OneShot {
    follow_ms: OnceLock<u64>,
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

    /// Whether a wake due at `due_ms`, set at `now_ms`, comes after the run
    /// has ended: never on a daemon that is not one run's.
    pub fn ends_before(&self, now_ms: u64, due_ms: u64) -> bool {
        self.follow_ms()
            .is_some_and(|f| due_ms > now_ms.saturating_add(f))
    }
}

impl OneShot {
    /// `wake.at`'s answer for `w` when it is due after the run ends: it is
    /// set, and will not fire in this run, so the model can wait inside it
    /// instead. None when it comes within the run, or on any other daemon.
    pub fn wake_after_run(
        &self,
        now_ms: u64,
        w: &theseus_kernel::PendingWake,
        short: &str,
        when: &str,
    ) -> Option<String> {
        let follow = self.follow_ms()?;
        self.ends_before(now_ms, w.due_at_ms).then(|| {
            format!(
                "Set wake {short} for {when}, and its id is {}, but it will not fire in this run: \
                 this daemon was spawned for one run (`theseus --spawn ask`), which follows this \
                 conversation for at most {} after its turn and then stops, so nothing is here \
                 when the wake is due. To wait for something, wait inside this run instead: a \
                 blocking proc.run (a `sleep`, or a loop that checks and sleeps) whose wait stays \
                 within the run's bound.",
                w.id,
                crate::wake::span(follow)
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_one_run_daemon_ends_before_a_wake() {
        let socket = OneShot::default();
        assert!(!socket.ends_before(1_000, u64::MAX));
        let run = OneShot::default();
        run.set(30_000);
        assert!(!run.ends_before(1_000, 31_000), "due at the bound");
        assert!(run.ends_before(1_000, 31_001), "due past it");
        run.set(5);
        assert_eq!(run.follow_ms(), Some(30_000), "said once");
    }
}
