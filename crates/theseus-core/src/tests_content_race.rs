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
//! no park (theseus-gyjyo.7): a landing writes at once, the floor waits on a
//! counter, and a turn goes on its labels whatever lands. So each test is
//! rewritten to say what is still true and still worth holding, **the
//! counter**: the asks open for a session count every item queued and come
//! back to zero, each item is asked about exactly once, and a failed ask is
//! asked again inside its own open ask rather than after it.
//!
//! | test | what it tests now |
//! |---|---|
//! | `both_results_of_a_parallel_group_are_judged` | a group of two: every result is asked about once; the count ends at zero |
//! | `every_result_of_a_group_of_three_is_judged` | the same for three |
//! | `a_group_wider_than_the_coalesce_window_ends_the_count_at_zero` | two batches; both asked once; the turn did not wait out `wait_ms` |
//! | `a_content_call_that_fails_mid_turn_is_asked_again_inside_the_wait` | the first ask fails, the second answers: judged, no `unjudged` row, the turn waited for it |
//! | `a_content_call_that_fails_twice_leaves_the_session_on_its_model` | both fail: one `unjudged` row, the turn goes on glm |
//! | `a_restricted_default_is_not_a_fallback_on_a_later_loop` | unchanged: a label on a later loop, with a restricted default |
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
        let open = core.runner.verdicts.outstanding(sid);
        if left.is_empty() && open == 0 {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{what}: asks never landed ({open} still open): {left:#?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Wait until the session's open count is back to zero: for an item whose ask
/// failed, which no `content.judged` row will ever name.
async fn count_ends(core: &std::sync::Arc<crate::rpc::Core>, sid: &str, what: &str) {
    let t0 = Instant::now();
    while core.runner.verdicts.outstanding(sid) > 0 {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{what}: the count never came down"
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

/// **The cell-8 shape, as a counter test** (theseus-8qgwo, theseus-gyjyo.7): a
/// turn pinned to a model of restricted origin runs a parallel tool-call group
/// whose results are written **further apart than `COALESCE`**, so the group
/// needs two batches. The first batch lands while the turn is still running
/// the rest of the group, with nothing waiting yet. On the build cell 8 ran
/// that landing parked on the drain task's own path, stopped the session's
/// queue, and the turn waited its whole `wait_ms` for a verdict nothing would
/// ask for.
///
/// **That cannot happen now and the profile assertion below is true for that
/// reason** (a turn goes on its labels at `wait_ms` whatever the judge does).
/// What bites is the count: both results were asked about, once each, the
/// session's open count is zero, and the turn **did not wait out `wait_ms`**:
/// a count that never came down would have held it for the full five seconds.
///
/// Jev must be fast and the results must straddle `COALESCE` (50 ms), or there
/// is no second batch; the profile must be restricted, or the floor never
/// waits at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_group_wider_than_the_coalesce_window_ends_the_count_at_zero() {
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
                // Six times `COALESCE`, so this result misses the first batch.
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
    let t0 = Instant::now();
    let one = route_turn(&r.core, None, "run both of those", Some("glm"), vec![])
        .await
        .expect("the turn was refused outright");
    let took = t0.elapsed();
    let sid = one.session_id.clone();
    assert_eq!(one.profile, "glm", "{:?}", one.route);
    assert_eq!(crate::tests_dm_rig::blocked(&r.core), 0);
    assert!(
        took < Duration::from_millis(4_500),
        "the turn waited out its wait_ms ({took:?}): the count did not come down"
    );
    settles(&r.core, &sid, "a pinned restricted turn's parallel group").await;
    let both = results_core(&r.core, &sid);
    assert_eq!(both.len(), 2, "both results were written");
    for id in both {
        assert_eq!(asked_times(&r.core, &id), 1, "{id} was asked about once");
    }
}

/// The rig for a restricted turn whose one tool result is held at Jev until
/// `release`: the result's own content call (the only one whose state carries
/// `MARK`) is the one that fails.
fn held_rig(
    jev: &FakeJev,
    total_secs: u64,
    mark: &str,
) -> (
    crate::tests_route::Rig,
    std::sync::Arc<crate::provider::FakeProvider>,
) {
    use crate::tests_route::rig_parts;
    let one_call = Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            json!({"argv": ["bash", "-c", format!("echo {mark}")]}),
        )],
    );
    let mine = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![
        one_call,
        Scripted::text("Ran it."),
    ]));
    let r = rig_parts(
        Some(jev),
        2,
        |c| {
            crate::tests_route::restricted_placements(c);
            c.judge.packs.remove(crate::judge::content::PACK);
            // The held call gives up here.
            c.judge.total_secs = total_secs;
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
    (r, mine)
}

/// **The wedge, as a retry test** (theseus-8qgwo, theseus-gyjyo.7): a content
/// call that fails *mid-turn*. It used to put its node on a worklist nothing
/// read again while the turn lasted, so a turn pinned to a model of restricted
/// origin waited its whole `wait_ms` and was refused. Now the failed ask is
/// asked **once more inside the same open ask**, so the count stays up across
/// the retry and the turn waits for the second answer.
///
/// Jev holds the result's call until the judge's own `total_secs` gives up on
/// it (3 s), and answers from then on: the first ask fails, the second (after
/// `REASK_PAUSE`) is released and answers. The result is judged, **no
/// `content.unjudged` row is written**, and the turn ran on the restricted
/// model having waited for it (more than the first ask's 3 s).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_content_call_that_fails_mid_turn_is_asked_again_inside_the_wait() {
    use crate::tests_route::try_turn_attached as route_turn;
    const MARK: &str = "wedgemarker";
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    jev.hold_when(MARK);
    let (r, _) = held_rig(&jev, 3, MARK);
    // The judge recovers once its first ask has given up (3 s): every call
    // held from here on answers.
    let releasing = {
        let jev = jev.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(3_100)).await;
            for _ in 0..100 {
                jev.release();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    };
    let t0 = Instant::now();
    let one = route_turn(&r.core, None, "run that one", Some("glm"), vec![])
        .await
        .expect("the turn was refused outright");
    let took = t0.elapsed();
    releasing.abort();
    assert_eq!(one.profile, "glm", "{:?}", one.route);
    assert_eq!(crate::tests_dm_rig::blocked(&r.core), 0);
    assert!(
        took >= Duration::from_secs(3),
        "the turn waited for the first ask to fail and the second to answer: {took:?}"
    );
    settles(&r.core, &one.session_id, "a failed content call mid-turn").await;
    let result = results_core(&r.core, &one.session_id);
    assert_eq!(result.len(), 1);
    assert_eq!(asked_times(&r.core, &result[0]), 1, "judged, on the retry");
    let unjudged = crate::tests_judge::kinds(&r.core.store, "content.unjudged");
    assert!(unjudged.is_empty(), "the retry answered: {unjudged:?}");
}

/// **The same call fails both times** (theseus-gyjyo.7): the owner's trade. The
/// result's ask fails, is asked once more, fails again, and is rowed
/// `content.unjudged` (`failed`); nothing is raised, so the session stays
/// eligible, and the turn pinned to the restricted model **goes** on it, having
/// waited for the two attempts and no longer. The chokepoint is never reached.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_content_call_that_fails_twice_leaves_the_session_on_its_model() {
    use crate::tests_route::try_turn_attached as route_turn;
    const MARK: &str = "failtwicemarker";
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    // Never released: both attempts are held until the judge gives up on them.
    jev.hold_when(MARK);
    let (r, mine) = held_rig(&jev, 2, MARK);
    let t0 = Instant::now();
    let one = route_turn(&r.core, None, "run that one", Some("glm"), vec![])
        .await
        .expect("the turn was refused outright");
    let took = t0.elapsed();
    assert_eq!(one.profile, "glm", "{:?}", one.route);
    assert_eq!(mine.requests().len(), 2, "GLM was sent both loops");
    assert_eq!(crate::tests_dm_rig::blocked(&r.core), 0);
    assert!(
        took >= Duration::from_secs(4),
        "two attempts of 2 s were waited for: {took:?}"
    );
    assert!(
        took < Duration::from_secs(25),
        "and no longer than that, with a wait_ms of 30 s: {took:?}"
    );
    count_ends(&r.core, &one.session_id, "a content call that failed twice").await;
    let unjudged = crate::tests_judge::kinds(&r.core.store, "content.unjudged");
    assert_eq!(unjudged.len(), 1, "{unjudged:?}");
    assert_eq!(unjudged[0].data["why"], "failed");
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
    assert_eq!(r.core.runner.verdicts.outstanding(&sid), 0);
}
