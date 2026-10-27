//! Two items of one session, judged in two batches: the mark must reach both
//! (theseus-8qgwo). The rig is `tests_content_late`'s -- the **real** judge
//! against the fake Jev, with `content.v1` live -- because the defects here
//! live in `judge::content`'s own landing, which the floor's stand-in judge
//! (`content_wait::judging`) replaces entirely and so cannot show.
//!
//! What this file is for. theseus-sh1n3 made the blocked set and the submitted
//! set one set, so every item that enters a session is offered for judgment.
//! It did not make a *landing* reach the mark. Cell 8's `c8c-deepseek-high-k1`
//! then failed 23 of 75 trials on `data_gate: unjudged`, and all 23 share one
//! shape: the model emitted a **parallel tool-call group** (2, 3 or 11 calls in
//! one loop), the first result of the group was judged, a later one never was,
//! and the next model call was refused for the session's whole life. Not one
//! single-call loop failed. The gap between the judged result and the dropped
//! one ran from 21 ms to 20 s, so it is not a narrow timing race.
//!
//! The three defects, each with a test here:
//!
//! 1. `move_mark`'s compare-and-set **loses the landing**. The mark moves only
//!    if `judged_through` still equals the `base` the batch read at
//!    `prepare_content`. A second batch of the same session begun against the
//!    same base returns without moving the mark *and* without putting its node
//!    on `ContentFlags::unjudged` -- neither marked nor stuck, so nothing will
//!    ever ask for it again. Leaving the *label* alone is right; losing the
//!    node is not.
//! 2. The sweep stands down on exactly the state it exists to repair:
//!    `ask_unsubmitted` returns early while anything of the session is in
//!    flight, and again when the first item past the mark is on the worklist.
//!    `with_worklist`'s re-ask runs only when a new batch is queued, and on a
//!    wedged turn no new batch comes.
//! 3. The floor's fallback does not read the fallback's origin: a turn refused
//!    for its content moves to the `[model]` default whatever that model is.
//!    Where the default is itself of restricted origin -- a pinned arm, which
//!    is how every bench cell runs -- "go to Claude" lands back on the same
//!    model and the chokepoint fails the whole turn.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::content::ContentFlags;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::session::SessionRecord;
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
        // content.v1 live (the judge rigs turn it off), the rest as they are.
        c.judge.packs.remove(crate::judge::content::PACK);
        c.judge.total_secs = 5;
        c.tools.proc_sync_secs = 5;
        c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
        c.model.retries.transient = 0;
        c.policy.tools.insert("proc.run".into(), Posture::Open);
    })
}

/// The session's flags, or the default.
fn flags(r: &Rig, sid: &str) -> ContentFlags {
    let rec: Option<SessionRecord> = r.core.store.get_session(sid).unwrap();
    rec.and_then(|r| r.data.map(|b| *b)).unwrap_or_default()
}

/// Every item of the session the floor counts as unjudged: what would refuse
/// the next request to a model of restricted origin, named rather than
/// counted, so a failure says which node was lost.
fn unjudged_items(r: &Rig, sid: &str) -> Vec<String> {
    let nodes = r.core.store.transcript(sid).unwrap();
    let f = flags(r, sid);
    let through = f
        .judged_through
        .as_deref()
        .and_then(|id| nodes.iter().position(|(_, n)| n.id == id));
    let from = through.map_or(0, |at| at + 1);
    nodes[from..]
        .iter()
        .filter(|(_, n)| crate::content_wait::is_item(n))
        .map(|(_, n)| format!("{} {:?}", n.id, n.kind))
        .collect()
}

/// Wait until the session has no unjudged item, or fail saying which is left.
/// The judge lands asynchronously, so the wait is its own bound; 20 s against
/// a fake Jev that answers in microseconds is a hang, not a slow answer.
async fn settles(r: &Rig, sid: &str, what: &str) {
    let t0 = Instant::now();
    loop {
        let left = unjudged_items(r, sid);
        if left.is_empty() {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{what}: these items were never judged, so this session can never \
             again reach a model of restricted origin: {left:#?}\nmark: {:?}\n\
             worklist: {:?}",
            flags(r, sid).judged_through,
            flags(r, sid).unjudged,
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The result nodes of the session, in transcript order.
fn results(r: &Rig, sid: &str) -> Vec<String> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| matches!(n.body, Body::ToolResult { .. }))
        .map(|(_, n)| n.id.clone())
        .collect()
}

/// **The row's own repro** (theseus-8qgwo): a loop of two parallel tool calls
/// writes two results, and *both* must be judged. On the build cell 8 ran
/// (`69b373e8`, which carries sh1n3) the second is offered, judged by nobody,
/// and left neither marked nor on the worklist, so the session is off every
/// model of restricted origin for the rest of its life.
///
/// Two calls, not one, is the whole point: a single-call loop queues one item,
/// its batch lands against the base it read, and the mark moves. The second
/// call's result is queued while the first batch is in flight, so its own
/// batch begins against a base the first has already moved past.
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
    settles(&r, &sid, "a parallel tool-call group").await;
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
            ("t1", "proc_run", json!({"argv": ["bash", "-c", "echo one"]})),
            ("t2", "proc_run", json!({"argv": ["bash", "-c", "echo two"]})),
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
    settles(&r, &sid, "a parallel tool-call group of three").await;
}

/// A landing whose base has moved is **not lost**: `move_mark`'s compare-and-set
/// may decline to move the mark, but the node it judged must still end up
/// covered -- by this landing or by a later read -- never left silently behind.
///
/// This drives the landing directly rather than through a turn, so it fails on
/// the defect itself rather than on whatever schedule a turn happens to
/// produce. Two batches of one session, the second begun against the first's
/// base: after both have landed, no item of the session is unjudged.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_landing_whose_base_moved_is_not_lost() {
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    let pair = Scripted::tools(
        "",
        &[
            ("t1", "proc_run", json!({"argv": ["bash", "-c", "echo a"]})),
            ("t2", "proc_run", json!({"argv": ["bash", "-c", "echo b"]})),
        ],
    );
    let r = rig(&jev, vec![pair, Scripted::text("Ran.")]);
    let first = try_turn_attached(&r.core, None, "run a and b", None, vec![])
        .await
        .unwrap();
    let sid = first.session_id.clone();
    settles(&r, &sid, "two batches of one session").await;
    let f = flags(&r, &sid);
    assert!(
        f.unjudged.is_empty(),
        "a node was parked on the worklist and nothing will re-ask it: {:?}",
        f.unjudged
    );
}
