//! The turn's end, and a late result's wake, in one frame (theseus-6qwr; v1.1
//! V3: every queue writes its row, in its frame). A background result that
//! landed while the turn ran, which its model has not read (`rewake`), queues
//! the execution for the driver's next turn. That wake was a frame of its own
//! after the end, and between the two every surface read the execution
//! waiting on input, which is settled: a `session.wait` until settled
//! returned there, before the late result was read (theseus-jj9f's second
//! shape).

use super::*;

/// End the turn held by `guard` with `end` and `extra` (`end_turn_with`), and
/// when `rewake`, wake its execution for the late result in the same frame.
/// Returns the execution as the frame left it, and whether the wake was
/// staged.
///
/// The stop is read inside the frame, under its lock: a `/stop` that landed
/// after the turn's own read still keeps the execution parked on input (W1),
/// as `end_turn` parks it. The wake is a nested transaction whose error is
/// logged, not returned, so a wake that can't happen (an execution a cancel
/// ended during the turn) takes back only itself, never the end.
pub(super) fn end_and_wake(
    kernel: &Kernel,
    guard: TurnGuard,
    end: TurnEnd,
    rewake: bool,
    extra: impl FnOnce(&Execution) -> Result<Vec<NewRecord>>,
) -> Result<(Execution, bool)> {
    let id = guard.execution_id.clone();
    kernel.frame(&[&id], |k| {
        let stopped = k.execution(&id)?.is_some_and(|e| e.stopped.is_some());
        let ended = k.end_turn_with(guard, end, extra)?;
        if !rewake || stopped {
            return Ok((ended, false));
        }
        match k.frame(&[&id], |k| k.wake(&id, "late_result")) {
            Ok(woken) => Ok((woken, true)),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), execution_id = %id, "a late result's wake was not written with the turn's end");
                Ok((ended, false))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FakeProvider;
    use theseus_protocol::SessionKind;

    fn core() -> (Arc<crate::Core>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::example();
        cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let core = crate::Core::build(crate::rpc::Parts::for_tests(
            cfg,
            Arc::new(FakeProvider::scripted(vec![])),
            store,
        ))
        .unwrap();
        (core, dir)
    }

    /// A held turn on a new execution.
    fn held(core: &crate::Core) -> TurnGuard {
        let e = core
            .kernel
            .open_execution(
                "ses_endstep",
                SessionKind::Conversation,
                Authority {
                    principal: "test".into(),
                    delegated_by: None,
                    ceilings: BTreeMap::new(),
                },
                None,
                None,
            )
            .unwrap();
        core.kernel.admit_input(&e.id).unwrap()
    }

    fn kinds_of(core: &crate::Core, exec: &str) -> Vec<(String, Option<String>)> {
        core.store
            .ledger_tail::<crate::ledger::LedgerRow>(100)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.data["execution_id"] == exec)
            .map(|(_, r)| (r.kind, r.data["why"].as_str().map(str::to_string)))
            .collect()
    }

    fn wait_input() -> TurnEnd {
        TurnEnd::Wait { wake: Wake::Input }
    }

    /// The end parks on input and the late result queues it: one frame, the
    /// end's row and then the queue's, why `late_result`.
    #[test]
    fn the_end_and_the_late_results_wake_are_one_frame() {
        let (core, _dir) = core();
        let g = held(&core);
        let id = g.execution_id.clone();
        let before = core.store.stats().unwrap().frames_appended;
        let (e, woke) = end_and_wake(&core.kernel, g, wait_input(), true, |_| Ok(vec![])).unwrap();
        assert_eq!(core.store.stats().unwrap().frames_appended - before, 1);
        assert!(woke);
        assert_eq!((e.state, e.resume_pending), (ExecState::Queued, true));
        let rows = kinds_of(&core, &id);
        assert_eq!(
            rows[rows.len() - 2..],
            [
                ("execution.waiting".to_string(), None),
                ("execution.queued".to_string(), Some("late_result".into())),
            ],
            "{rows:?}"
        );
    }

    /// A stop that landed during the turn, read inside the end's frame, keeps
    /// it parked on input: nothing queues it.
    #[test]
    fn a_stopped_turn_queues_nothing() {
        let (core, _dir) = core();
        let g = held(&core);
        let id = g.execution_id.clone();
        core.kernel.stop_execution(&id, "test").unwrap();
        let (e, woke) = end_and_wake(&core.kernel, g, wait_input(), true, |_| Ok(vec![])).unwrap();
        assert!(!woke);
        assert_eq!((e.state, e.wake), (ExecState::Waiting, Some(Wake::Input)));
        let rows = kinds_of(&core, &id);
        assert_eq!(
            rows.last().unwrap(),
            &("execution.waiting".to_string(), Some("stopped".into())),
            "{rows:?}"
        );
    }

    /// A cancel that landed during the turn ended the execution: its end
    /// writes nothing, and the wake that can't happen fails alone. The end
    /// is still `Ok`, with the cancelled execution, so the turn answers the
    /// calls the cancel cut (theseus-0o8).
    #[test]
    fn a_wake_that_cant_happen_takes_back_nothing() {
        let (core, _dir) = core();
        let g = held(&core);
        let id = g.execution_id.clone();
        core.kernel.cancel_execution(&id, "test").unwrap();
        let before = core.store.stats().unwrap().frames_appended;
        let (e, woke) = end_and_wake(&core.kernel, g, wait_input(), true, |_| Ok(vec![]))
            .expect("the end stands, whatever the wake");
        assert!(!woke);
        assert_eq!(e.state, ExecState::Cancelled);
        assert_eq!(core.store.stats().unwrap().frames_appended, before);
    }
}
