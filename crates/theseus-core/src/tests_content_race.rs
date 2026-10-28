//! Two items of one session, judged in two batches: the mark must reach both
//! (theseus-8qgwo). The rig is `tests_content_late`'s -- the **real** judge
//! against the fake Jev, with `content.v3` live -- because the defects here
//! live in `judge::content`'s own landing, which the floor's stand-in judge
//! (`content_wait::judging`) replaces entirely and so cannot show.
//!
//! What this file is for. theseus-sh1n3 made the blocked set and the submitted
//! set one set, so every item that enters a session is offered for judgment.
//! It did not make a *landing* reach the mark in time. Cell 8's
//! `c8c-deepseek-high-k1` then failed **39 of 89** trials at the data gate:
//!
//! | 25 | `holds an item that has not been judged yet` | the defect |
//! | 8  | `carries a 'secret' label` | the gate working |
//! | 5  | `carries a 'confidential' label` | the gate working |
//! | 1  | `No model cleared …` | the fallback hole |
//!
//! All 25 share one shape: the model emitted a **parallel tool-call group**
//! (2 × 22, 3 × 2, 11 × 1), the first result was judged and a later one never
//! was. Across the whole cell, 1,181 of 1,387 tool-call loops were single-call
//! and **not one of those ever failed** — 83 of them ran inside the 25 failing
//! trials themselves and survived. Every one of the 25 waited **30.007 to
//! 30.014 s**, one uninterrupted `[content_based_security] wait_ms`, and in every one every
//! judgment that exists reads `outcome: answered`. **Nothing failed. A verdict
//! was simply never asked for.**
//!
//! Three defects, in the order they matter:
//!
//! 1. **The landing deadlock**, which is what cell 8 died of. `land` reads
//!    `content_awaited` **once** and then parks in `Turns::between` waiting for
//!    a quiet moment, on the drain task's own path — so the park stops the
//!    whole session's queue. A group's first batch lands while the turn is
//!    still running the rest of the group, so nothing is waiting yet and it
//!    parks; the group's later results are never taken; the turn reaches the
//!    floor and waits its whole `wait_ms` on a notify that cannot fire. The
//!    landing waits for the turn and the turn waits for the landing. A
//!    single-call loop cannot reach it, which is the fingerprint.
//!    Tested by `a_group_wider_than_the_coalesce_window_does_not_deadlock_its_own_landing`.
//! 2. **The sweep stands down on a stuck mark.** `ask_unsubmitted` leaves a
//!    node an earlier batch could not judge to `with_worklist`, which rides a
//!    **new batch**; mid-turn there is none to ride. A latent wedge of the
//!    same family, not cell 8's — no batch failed there.
//!    Tested by `a_failed_content_call_mid_turn_is_asked_again_rather_than_wedging`.
//! 3. **The floor's fallback does not read the fallback's origin.** A turn
//!    refused for its content moves to the `[model]` default whatever that
//!    model is; where the default is itself restricted — a pinned arm, which
//!    is how every bench cell runs — "go to Claude" lands back on the same
//!    model and the chokepoint fails the turn with `nothing was sent`. That is
//!    the 1 `No model cleared` trial, and it is why the other 25 died hard.
//!    Tested by `a_restricted_default_is_not_a_fallback_on_a_later_loop`.
//!
//! The two whole-group tests below are **regression cover, not evidence**:
//! their results land inside one `COALESCE` window, so there is no second
//! batch to strand and they pass on the unfixed tree.

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
        // content.v3 live (the judge rigs turn it off), the rest as they are.
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
/// (`69b373e8`, which carries sh1n3) a group's later result could go unjudged
/// for the rest of its turn, which is enough to refuse every request of that
/// turn to a model of restricted origin.
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
    settles(&r, &sid, "a parallel tool-call group of three").await;
}

/// **The cell-8 deadlock itself** (theseus-8qgwo, the review's B1): a turn
/// pinned to a model of restricted origin runs a parallel tool-call group
/// whose results are written **further apart than `COALESCE`**, so the group
/// needs two batches. The first batch's `land` runs while the turn is still
/// executing the rest of the group — no turn is waiting yet — so it parks in
/// `Turns::between` on the drain task's own path and the second result is
/// never taken from the queue. The turn then waits its whole `wait_ms` on a
/// notify that cannot fire. The landing waits for the turn to end and the
/// turn waits for the landing.
///
/// Three knobs make it, and getting any of them wrong hides it:
/// - **Jev must be fast.** A slow Jev is the one thing that *prevents* this:
///   it delays the first landing until after the floor has begun waiting, so
///   `awaited` is already true and the landing writes at once. An earlier
///   version of this test set a 300 ms latency and passed on the broken tree.
/// - **The results must straddle `COALESCE`** (50 ms). Two `echo`s land in
///   one batch, there is no second batch to strand, and it passes.
/// - **The profile must be restricted**, or the floor never waits at all.
///
/// On the unfixed tree the mark stops short of the second result with
/// `unjudged` empty — nothing failed; a verdict was simply never asked for.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_group_wider_than_the_coalesce_window_does_not_deadlock_its_own_landing() {
    use crate::tests_route::{rig_parts, try_turn_attached as route_turn};
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    let pair = Scripted::tools(
        "",
        &[
            ("t1", "proc_run", json!({"argv": ["bash", "-c", "echo p1"]})),
            (
                "t2",
                "proc_run",
                // Six times `COALESCE`, so this result misses the first
                // batch and needs a second the parked drain cannot take.
                json!({"argv": ["bash", "-c", "sleep 0.3; echo p2"]}),
            ),
        ],
    );
    let mine = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![
        pair,
        Scripted::text("Both ran."),
    ]));
    let r = rig_parts(
        Some(&jev),
        2,
        |c| {
            crate::tests_route::restricted_placements(c);
            // content.v3 live: the judge rigs turn it off.
            c.judge.packs.remove(crate::judge::content::PACK);
            c.judge.total_secs = 10;
            // Generous, so a move to the default means the verdict never came
            // rather than that the fake Jev was slow.
            c.content_based_security.wait_ms = 5_000;
            c.tools.proc_sync_secs = 5;
            c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
            c.model.retries.transient = 0;
            c.policy.tools.insert("proc.run".into(), Posture::Open);
        },
        {
            let mine = mine.clone();
            move |p| {
                p.providers.insert("zai".into(), mine);
            }
        },
    );
    let one = route_turn(&r.core, None, "run both of those", Some("glm"), vec![])
        .await
        .expect("the turn was refused outright");
    let sid = one.session_id.clone();
    // The discriminating assertion. The landing deadlock shows here: the floor
    // cannot settle inside `wait_ms`, so the turn leaves the model it was
    // pinned to. `settles_on` below is a true invariant but does not
    // discriminate — the park ends when the turn ends, so the verdict lands
    // late either way.
    assert_eq!(
        one.profile, "glm",
        "the turn left its pinned restricted model: {:?}",
        one.route
    );
    assert_eq!(
        crate::tests_dm_rig::blocked(&r.core),
        0,
        "the chokepoint refused a request the floor should have settled"
    );
    settles_on(&r.core, &sid, "a pinned restricted turn's parallel group").await;
}

/// **The wedge itself** (theseus-8qgwo): a content batch that fails *mid-turn*
/// puts its node on `ContentFlags::unjudged`, and from that moment nothing
/// will ever ask for it again while the turn lasts -- so a turn pinned to a
/// model of restricted origin waits its whole `[content_based_security] wait_ms` and is then
/// refused, however healthy the judge has since become.
///
/// **This is not what cell 8 died of** — no content call failed there (every
/// judgment reads `answered`). It is a latent wedge of the same family, found
/// while chasing that one, and it is real: a shed or a refused reservation
/// mid-turn costs the rest of the turn.
///
/// Why nothing asks again. `ask_unsubmitted` stands down when the first item
/// past the mark is on the worklist, leaving the re-ask to `with_worklist`
/// (the review of 2026-10-27, B3). But `with_worklist` rides a **new batch**,
/// and between a tool's result and the next model call of the same turn there
/// is no new item to queue, so no batch is ever made for it to ride. The
/// session is off every model of restricted origin for the rest of that turn,
/// and until something else queues an item.
///
/// Cell 8's profile sets `[content_based_security] wait_ms = 30000`, so a turn in this state
/// burns a full thirty seconds waiting on a verdict nothing asked for.
///
/// The rig holds the result's own content call until the judge's own
/// `total_secs` gives up on it, then lets later calls through. A build that
/// re-asks the worklist finishes the turn on the restricted model; a build
/// that leaves it to a batch that never comes does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_content_call_mid_turn_is_asked_again_rather_than_wedging() {
    use crate::tests_route::{rig_parts, try_turn_attached as route_turn};
    const MARK: &str = "wedgemarker";
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    // Only the call whose state carries the result's own text waits; the
    // route verdict and every other pack answer at once.
    jev.hold_when(MARK);
    let one_call = Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            json!({"argv": ["bash", "-c", format!("echo {MARK}")]}),
        )],
    );
    let mine = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![
        one_call,
        Scripted::text("Ran it."),
    ]));
    let r = rig_parts(
        Some(&jev),
        2,
        |c| {
            crate::tests_route::restricted_placements(c);
            c.judge.packs.remove(crate::judge::content::PACK);
            // The held call gives up here; the floor has far longer to notice
            // and ask again, as cell 8's profile does.
            c.judge.total_secs = 3;
            c.content_based_security.wait_ms = 30_000;
            c.tools.proc_sync_secs = 5;
            c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
            c.model.retries.transient = 0;
            c.policy.tools.insert("proc.run".into(), Posture::Open);
        },
        {
            let mine = mine.clone();
            move |p| {
                p.providers.insert("zai".into(), mine);
            }
        },
    );
    // The judge recovers while the floor is still waiting: every call held
    // from here on answers, so a re-ask succeeds and only a build that never
    // re-asks is still stuck.
    let releasing = {
        let jev = jev.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            for _ in 0..100 {
                jev.release();
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
    };
    let one = route_turn(&r.core, None, "run that one", Some("glm"), vec![])
        .await
        .expect("the turn was refused outright");
    releasing.abort();
    assert_eq!(
        one.profile, "glm",
        "the turn left its pinned restricted model for an item nothing re-asked: {:?}",
        one.route
    );
    assert_eq!(
        crate::tests_dm_rig::blocked(&r.core),
        0,
        "the chokepoint refused a request whose item the floor could have re-asked"
    );
    settles_on(&r.core, &one.session_id, "a failed content call mid-turn").await;
}

/// **The fallback is not a model the floor refuses** (theseus-8qgwo). Where
/// the `[model]` default is itself of restricted origin — every bench cell,
/// and anyone who pins one model — a turn the floor keeps off its own profile
/// used to be "moved to Claude" onto that same model, and the chokepoint
/// failed it with `nothing was sent`. That is cell 8's `git-multibranch`
/// trial, and it is why the other 23 died hard instead of degrading.
///
/// The floor decides, and says so: the turn ends with the floor's words and
/// the chokepoint is never reached.
///
/// It must be a **later** loop, which is why the turn runs a tool first. The
/// first loop's move (`floor_unrouted` through `data_floor`) already reads
/// the fallback's origin, because `route_profiles` marks a profile the floor
/// refuses `unusable`; `cn_move`, the loop after a tool's result, did not,
/// and that is the loop every one of cell 8's trials died on. A test that
/// skips the tool call passes on the unfixed build.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restricted_default_is_not_a_fallback_on_a_later_loop() {
    use crate::tests_route::{rig_parts, try_turn_attached as route_turn};
    const MARK: &str = "fallbackmarker";
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    // The result's own content call finds a secret in it (and nothing else
    // does), so the later loop reaches `cn_move` with the session labelled --
    // the state in which the floor needs somewhere to go. (It was a call that
    // never answered until theseus-gyjyo.6: an unanswered ask is no refusal
    // now, only a label is.)
    jev.script_when(
        MARK,
        None,
        "secret",
        Jev::Choice {
            option: "yes".into(),
            confidence: 0.95,
        },
    );
    let one_call = Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            json!({"argv": ["bash", "-c", format!("echo {MARK}")]}),
        )],
    );
    let mine = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![
        one_call,
        Scripted::text("Ran it."),
    ]));
    let r = rig_parts(
        Some(&jev),
        2,
        |c| {
            crate::tests_route::restricted_placements(c);
            // `[model]`'s default is the restricted model itself, as a pinned
            // bench arm's is.
            c.copy_profile("default", "glm");
            c.judge.packs.remove(crate::judge::content::PACK);
            c.judge.total_secs = 2;
            c.content_based_security.wait_ms = 5_000;
            c.tools.proc_sync_secs = 5;
            c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
            c.model.retries.transient = 0;
            c.policy.tools.insert("proc.run".into(), Posture::Open);
        },
        {
            let mine = mine.clone();
            move |p| {
                p.providers.insert("zai".into(), mine);
            }
        },
    );
    let out = route_turn(&r.core, None, "run that one", Some("glm"), vec![]).await;
    assert_eq!(
        crate::tests_dm_rig::blocked(&r.core),
        0,
        "the chokepoint refused what the floor should have: the fallback was \
         a model of restricted origin and the later loop never read its origin"
    );
    let said = match out {
        Ok(ok) => format!("the turn ran on `{}`: {:?}", ok.profile, ok.route),
        Err(e) => format!("{e:#}"),
    };
    assert!(
        said.contains("no model cleared") || said.contains("may not use one"),
        "the floor did not say why it had nowhere to go: {said}"
    );
}

/// `settles`, against a `tests_route` core rather than a `tests_judge` rig.
async fn settles_on(core: &std::sync::Arc<crate::rpc::Core>, sid: &str, what: &str) {
    let t0 = Instant::now();
    loop {
        let nodes = core.store.transcript(sid).unwrap();
        let rec: Option<SessionRecord> = core.store.get_session(sid).unwrap();
        let f = rec.and_then(|r| r.data.map(|b| *b)).unwrap_or_default();
        let through = f
            .judged_through
            .as_deref()
            .and_then(|id| nodes.iter().position(|(_, n)| n.id == id));
        let from = through.map_or(0, |at| at + 1);
        let left: Vec<String> = nodes[from..]
            .iter()
            .filter(|(_, n)| crate::content_wait::is_item(n))
            .map(|(_, n)| format!("{} {:?}", n.id, n.kind))
            .collect();
        if left.is_empty() {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(25),
            "{what}: never judged, so the session is off every model of \
             restricted origin for good: {left:#?}\nmark: {:?}\nworklist: {:?}",
            f.judged_through,
            f.unjudged,
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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
