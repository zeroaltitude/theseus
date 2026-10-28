//! Health's waiting split, the retired term (theseus-g00vv, theseus-tx1vj):
//! `waiting_split` tells the retired sessions among the executions at rest on
//! input by counting the session records marked retired, capped at the
//! executions waiting on input. It is an approximation, and these tests pin
//! it as it is today: a count of records, not of what waits. Whether it
//! should be an exact count (the retired sessions' executions read) is the
//! owner's call; changing it changes these tests on purpose.

use crate::approval::Answerer;
use crate::test_support::turn;
use crate::tests_unlabel::{core_in, session};
use crate::{session::SessionRecord, Core};
use std::path::Path;
use std::sync::Arc;
use theseus_protocol::notice::WaitingSplit;

/// A core, and `n` sessions that each took one turn, so each one's execution
/// waits on input.
async fn at_rest(dir: &Path, n: usize) -> (Arc<Core>, Vec<String>) {
    let core = core_in(dir);
    let mut sids = Vec::new();
    for i in 0..n {
        let sid = session(&core);
        turn(&core, &sid, &format!("hello {i}")).await;
        sids.push(sid);
    }
    (core, sids)
}

fn split(core: &Core) -> WaitingSplit {
    core.kernel_status().waiting_split
}

fn owner() -> Answerer {
    crate::test_support::owner_cli()
}

async fn end_execution(core: &Core, sid: &str) {
    let exec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap()
        .execution_id
        .unwrap();
    core.cancel_execution(&exec, "test").await.unwrap();
}

fn says(at_rest: u64, retired: u64) -> WaitingSplit {
    WaitingSplit {
        at_rest,
        retired,
        work: 0,
        time: 0,
    }
}

/// Nothing retired: every execution at rest on input is a conversation at
/// rest, and the retired figure is 0.
#[tokio::test]
async fn with_nothing_retired_every_session_at_rest_is_at_rest() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _) = at_rest(dir.path(), 3).await;
    assert_eq!(split(&core), says(3, 0));
}

/// A session the owner retired by hand, its execution still at rest on
/// input, moves from "at rest" to "retired"; reopened, it moves back.
#[tokio::test]
async fn a_retired_session_at_rest_reads_retired_and_reopened_reads_at_rest() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sids) = at_rest(dir.path(), 3).await;
    core.retire_session(&sids[0], &owner()).unwrap();
    assert_eq!(split(&core), says(2, 1));
    core.retire_session(&sids[1], &owner()).unwrap();
    assert_eq!(split(&core), says(1, 2));
    core.reopen_session(&sids[0], &owner()).unwrap();
    assert_eq!(split(&core), says(2, 1));
}

/// The approximation: the count is of retired session records, ended ones
/// included, and capped at the executions waiting on input. Two retired
/// sessions whose executions ended wait on nothing, yet one live session at
/// rest reads as retired (and none at rest), because the kernel knows nothing
/// of retirement and the cap is all the figure has to go by.
#[tokio::test]
async fn ended_retired_sessions_count_up_to_the_cap_and_a_live_one_reads_retired() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sids) = at_rest(dir.path(), 3).await;
    for sid in &sids[1..] {
        end_execution(&core, sid).await;
        core.retire_session(sid, &owner()).unwrap();
    }
    // One execution waits on input: the live, unretired session.
    assert_eq!(
        split(&core),
        says(0, 1),
        "two ended retired records, capped at the one execution at rest"
    );
    // The record of that one was never retired: the figure is wrong by
    // exactly the ended ones it counted.
    let live: SessionRecord = core.store.get_session(&sids[0]).unwrap().unwrap();
    assert!(live.retired.is_none());
}
