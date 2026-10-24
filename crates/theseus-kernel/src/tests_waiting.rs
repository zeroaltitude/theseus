//! Health's waiting executions split by wake (theseus-g00vv): a conversation
//! at rest on input, a task asleep until a time, and a turn that ended
//! waiting on work, counted by terms; `executions_by_state` keeps its keys
//! and values.

use crate::kernel::*;
use crate::tests::*;
use crate::tests_deferred::asleep;
use crate::types::*;
use crate::WaitingByWake;

#[test]
fn waiting_executions_are_counted_by_what_they_wait_on() {
    let w = world();
    // Three conversations: one at rest on input (a turn ended), one waiting
    // on a job's completion, and a parent whose task sleeps until a time.
    let (_, at_rest, g) = running(&w);
    w.kernel
        .end_turn(g, TurnEnd::Wait { wake: Wake::Input })
        .unwrap();
    let (_, on_work, g) = running(&w);
    let act = dispatched(&w, &g, "proc.run", 1_000);
    w.kernel
        .end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: vec![act.correlation_id],
                },
            },
        )
        .unwrap();
    let (_parent, task, _at) = asleep(&w, 30_000, 120_000);
    let st = w.kernel.stats().unwrap();
    assert_eq!(
        st.waiting_by_wake,
        WaitingByWake {
            input: 2,
            time: 1,
            work: 1
        },
        "{:?}",
        st.executions_by_state
    );
    assert_eq!(
        st.executions_by_state["waiting"], 4,
        "the keys are as they were"
    );
    // The label moves with the wake and changes no state.
    assert_eq!(
        w.kernel.execution(&at_rest.id).unwrap().unwrap().wake,
        Some(Wake::Input)
    );
    assert_eq!(
        w.kernel.execution(&task.id).unwrap().unwrap().state,
        ExecState::Waiting
    );
    assert_eq!(
        w.kernel.execution(&on_work.id).unwrap().unwrap().state,
        ExecState::Waiting
    );
}
