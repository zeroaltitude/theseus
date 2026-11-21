//! Two items of one session, asked in two batches (theseus-8qgwo; counter tests
//! since theseus-gyjyo.7). The rig is `tests_content_late`'s -- the **real** judge
//! against the fake Jev, with `content.v3` live -- because what these pin lives
//! in `judge::content`'s own landing, which the floor's stand-in judge
//! (`content_wait::judging`) replaces entirely and so cannot show.
//!
//! **What this file was, and what it is.** Cell 8's `c8c-deepseek-high-k1` failed
//! 25 of 89 trials at the data gate with `holds an item that has not been
//! judged yet`, all on a **parallel tool-call group** whose first result was
//! judged and a later one never was: a landing parked in `Turns::between` on
//! the drain task's own path, stopping the whole session's queue, while the
//! turn waited its whole `wait_ms` for a verdict nothing would ask for. The
//! tests here were the race's tests: they held the session's *mark* to the
//! end of the transcript (`settles`) and a turn to its restricted model.
//!
//! **The race is gone, and they pass for that reason.** There is no mark and
//! no park (theseus-gyjyo.7): a landing writes at once. Since 2026-11-21 no
//! turn waits on a verdict either (the floor for models of restricted origin
//! went, and its tests with it). So each test says what is still true and
//! still worth holding: every item queued is asked about exactly once, and
//! the session's queue empties.
//!
//! | test | what it tests now |
//! |---|---|
//! | `both_results_of_a_parallel_group_are_judged` | a group of two: every result is asked about once; the count ends at zero |
//! | `every_result_of_a_group_of_three_is_judged` | the same for three |
//! | `two_batches_of_one_session_are_each_asked_once` | no landing is lost, none is asked twice |

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::node::Body;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::tests_judge::{rig_on, Rig};
use crate::tests_route::try_turn_attached;

/// Jev: nothing in anything is injection, a secret, confidential or personal.
fn clean(jev: &FakeJev) {
    for q in ["injection", "secret", "confidential", "personal"] {
        jev.script(
            q,
            Jev::Choice {
                option: "no".into(),
                confidence: 0.95,
            },
        );
    }
}

fn rig(jev: &FakeJev, script: Vec<Scripted>) -> Rig {
    rig_on(script, Some(jev), |c| {
        // content.v3 live (the judge rigs turn it off), the rest as they are.
        c.judge.packs.remove(crate::judge::content::PACK);
        c.judge.total_secs = 5;
        c.tools.proc_sync_secs = 5;
        c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
        c.model.retries.transient = 0;
        c.policy.tools.insert("proc.run".into(), Posture::Open);
    })
}

/// The items of the session no `content.judged` row names: what the judge has
/// not asked about, named rather than counted so a failure says which node was
/// lost.
fn unasked(core: &std::sync::Arc<crate::rpc::Core>, sid: &str) -> Vec<String> {
    let judged = crate::tests_judge::judged_nodes(&core.store);
    core.store
        .transcript(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| crate::content::item_text(n).is_some())
        .filter(|(_, n)| !judged.contains(&n.id))
        .map(|(_, n)| format!("{} {:?}", n.id, n.kind))
        .collect()
}

/// Wait until every item of the session has been asked about **and the
/// session's open count is back to zero**, or fail saying which is left. The
/// judge lands asynchronously, so the wait is its own bound; 20 s against a
/// fake Jev that answers in microseconds is a hang, not a slow answer.
async fn settles(core: &std::sync::Arc<crate::rpc::Core>, sid: &str, what: &str) {
    let t0 = Instant::now();
    loop {
        let left = unasked(core, sid);
        let open = core.runner.judge.content_busy(sid);
        if left.is_empty() && !open {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{what}: asks never landed (still asking: {open}): {left:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// How many times the judge asked about `node`: its `content.judged` rows.
fn asked_times(core: &std::sync::Arc<crate::rpc::Core>, node: &str) -> usize {
    crate::tests_judge::judged_nodes(&core.store)
        .iter()
        .filter(|n| *n == node)
        .count()
}

/// The result nodes of the session, in transcript order.
fn results(r: &Rig, sid: &str) -> Vec<String> {
    results_core(&r.core, sid)
}

fn results_core(core: &std::sync::Arc<crate::rpc::Core>, sid: &str) -> Vec<String> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| matches!(n.body, Body::ToolResult { .. }))
        .map(|(_, n)| n.id.clone())
        .collect()
}

/// **The row's own repro, as a counter test** (theseus-8qgwo,
/// theseus-gyjyo.7): a loop of two parallel tool calls writes two results, and
/// *both* are asked about, once each, with the session's open count back at
/// zero. On the build cell 8 ran (`69b373e8`) a group's later result could go
/// unasked for the rest of its turn. It passes now because the machinery that
/// lost it (the mark, the park) is gone; what it holds is the counter.
///
/// Two calls, not one, is the point: the second call's result is queued while
/// the first batch is in flight.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_results_of_a_parallel_group_are_judged() {
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    let pair = Scripted::tools(
        "",
        &[
            (
                "t1",
                "proc_run",
                json!({"argv": ["bash", "-c", "echo first of the pair"]}),
            ),
            (
                "t2",
                "proc_run",
                json!({"argv": ["bash", "-c", "echo second of the pair"]}),
            ),
        ],
    );
    let r = rig(&jev, vec![pair, Scripted::text("Both ran.")]);
    let first = try_turn_attached(&r.core, None, "run both of those", None, vec![])
        .await
        .unwrap();
    let sid = first.session_id.clone();
    assert_eq!(
        results(&r, &sid).len(),
        2,
        "the turn wrote both results of the group"
    );
    settles(&r.core, &sid, "a parallel tool-call group").await;
    for id in results(&r, &sid) {
        assert_eq!(asked_times(&r.core, &id), 1, "{id} was asked about once");
    }
}

/// The same, with a group of three: the bench's 11-call group says the shape
/// does not stop at two, and three is enough to show the mark must cross a run
/// rather than a single node.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_result_of_a_group_of_three_is_judged() {
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    let three = Scripted::tools(
        "",
        &[
            (
                "t1",
                "proc_run",
                json!({"argv": ["bash", "-c", "echo one"]}),
            ),
            (
                "t2",
                "proc_run",
                json!({"argv": ["bash", "-c", "echo two"]}),
            ),
            (
                "t3",
                "proc_run",
                json!({"argv": ["bash", "-c", "echo three"]}),
            ),
        ],
    );
    let r = rig(&jev, vec![three, Scripted::text("All three ran.")]);
    let first = try_turn_attached(&r.core, None, "run all three", None, vec![])
        .await
        .unwrap();
    let sid = first.session_id.clone();
    assert_eq!(results(&r, &sid).len(), 3, "all three results were written");
    settles(&r.core, &sid, "a parallel tool-call group of three").await;
    for id in results(&r, &sid) {
        assert_eq!(asked_times(&r.core, &id), 1, "{id} was asked about once");
    }
}

/// **No landing is lost and none is asked twice** (theseus-8qgwo,
/// theseus-gyjyo.7). Two batches of one session, the second begun while the
/// first is in flight: it used to be that the second's base had moved and the
/// mark's compare-and-set could decline it, leaving a node behind. With no mark
/// there is nothing to decline: after both have landed every item has exactly
/// one `content.judged` row, and the session's open count is zero.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_batches_of_one_session_are_each_asked_once() {
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    let pair = Scripted::tools(
        "",
        &[
            ("t1", "proc_run", json!({"argv": ["bash", "-c", "echo a"]})),
            (
                "t2",
                "proc_run",
                json!({"argv": ["bash", "-c", "sleep 0.2; echo b"]}),
            ),
        ],
    );
    let r = rig(&jev, vec![pair, Scripted::text("Ran.")]);
    let first = try_turn_attached(&r.core, None, "run a and b", None, vec![])
        .await
        .unwrap();
    let sid = first.session_id.clone();
    settles(&r.core, &sid, "two batches of one session").await;
    let both = results(&r, &sid);
    assert_eq!(both.len(), 2);
    for id in both {
        assert_eq!(asked_times(&r.core, &id), 1, "{id} was asked about once");
    }
    assert!(!r.core.runner.judge.content_busy(&sid));
}
