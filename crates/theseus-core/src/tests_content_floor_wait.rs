//! **A cn turn's own message, judged inside the floor's wait** (theseus-oc9n4).
//!
//! The row's observation, on a scratch daemon with `[content] wait_ms = 30000`
//! and eight cn sessions: five of the eight spent their whole `wait_ms`,
//! `route.decided` read `cn=unjudged`, the turn fell to Claude, and the
//! verdict landed **0.5 s after the turn ended** — twice, on turns of
//! different lengths. Three landed in 0.24 s and ran on `glm`. The floor's
//! wait was dead weight on the five that lost.
//!
//! The rig is `tests_content_race`'s: the **real** judge against the fake
//! Jev, with `content.v3` live, driving real turns through the route step, so
//! what these hold is `judge::content`'s own queue and the floor's wait
//! against each other and not a stand-in for either.
//!
//! | test | what it holds |
//! |---|---|
//! | `one_cn_turns_own_message_is_judged_inside_the_floors_wait` | the single-session control: one turn, its own message judged well inside `wait_ms` |
//! | `eight_concurrent_cn_turns_each_judge_their_own_message_inside_the_floors_wait` | the row's own shape: eight at once, none spends its whole `wait_ms` |

use std::time::{Duration, Instant};

use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::policy::Posture;
use crate::tests_route::{rig_parts, try_turn_attached as route_turn, Rig};

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

/// The floor's wait in these tests. Long enough that a turn which spends it
/// all is unmistakable in the elapsed time, short enough that a red test
/// fails in seconds.
const WAIT: Duration = Duration::from_millis(4_000);

/// The rig: `content.v3` live, `glm` the cn placement for the trivial class,
/// and the floor's wait at [`WAIT`]. `turns` scripted replies on each
/// provider.
fn rig(jev: &FakeJev, turns: usize) -> Rig {
    rig_parts(
        Some(jev),
        turns * 2,
        |c| {
            crate::tests_route::restricted_placements(c);
            // content.v3 live: the judge rigs turn it off.
            c.judge.packs.remove(crate::judge::content::PACK);
            c.judge.total_secs = 10;
            c.content_based_security.wait_ms = WAIT.as_millis() as u64;
            c.model.retries.transient = 0;
            c.policy.tools.insert("proc.run".into(), Posture::Open);
        },
        |_| {},
    )
}

/// What one cn turn cost and where it ran.
struct Ran {
    took: Duration,
    profile: String,
    session: String,
}

/// One cn turn pinned to `glm`, timed.
async fn cn_turn(core: &std::sync::Arc<crate::rpc::Core>, input: &str) -> Ran {
    let t0 = Instant::now();
    let one = route_turn(core, None, input, Some("glm"), vec![])
        .await
        .expect("the turn was refused outright");
    Ran {
        took: t0.elapsed(),
        profile: one.profile.clone(),
        session: one.session_id.clone(),
    }
}

/// How many of the session's items no `content.judged` row names.
fn unjudged_items(core: &std::sync::Arc<crate::rpc::Core>, sid: &str) -> usize {
    let judged = crate::tests_judge::judged_nodes(&core.store);
    core.store
        .transcript(sid)
        .unwrap()
        .iter()
        .filter(|(_, n)| crate::content::item_text(n).is_some())
        .filter(|(_, n)| !judged.contains(&n.id))
        .count()
}

/// **The single-session control.** One cn turn pinned to `glm`: its own
/// operator message is queued at the top of the turn (`content_input`), and
/// the floor's wait at the route step finds the count already at zero, or
/// waits only as long as one fake Jev call takes. The turn runs on `glm` and
/// nothing like [`WAIT`] is spent.
///
/// This is the control for the test below: it says the floor and the queue
/// work against each other correctly when nothing else is in flight, so a
/// red there is contention and not the wiring.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_cn_turns_own_message_is_judged_inside_the_floors_wait() {
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    let r = rig(&jev, 2);
    let ran = cn_turn(&r.core, "say hello back").await;
    assert_eq!(ran.profile, "glm", "one cn turn ran on its pinned model");
    assert!(
        ran.took < WAIT,
        "the one turn spent its whole wait_ms ({:?} of {WAIT:?}): its own message's verdict did not land inside the floor's wait",
        ran.took
    );
    assert_eq!(crate::tests_dm_rig::blocked(&r.core), 0);
    assert_eq!(
        unjudged_items(&r.core, &ran.session),
        0,
        "its own message was judged"
    );
}

/// **The row's own shape** (theseus-oc9n4): eight cn sessions turning at
/// once, every turn pinned to `glm`, each with its own operator message to
/// judge. Every one of them must have its verdict inside the floor's wait:
/// none spends its whole `wait_ms`, and every one runs on `glm`.
///
/// Eight is the number the live check saw, and it is also the judge client's
/// default `[judge] max_in_flight`, so eight turns at once is exactly the
/// load at which the content asks and the turns' own asks contend for the
/// in-flight permits.
///
/// A failure prints every session's elapsed time, so a red says how many lost
/// and by how much rather than only that one did.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn eight_concurrent_cn_turns_each_judge_their_own_message_inside_the_floors_wait() {
    const N: usize = 8;
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    // A fake Jev that answers instantly hides every queue; a tenth of a
    // second a call is the order a real one answers in.
    jev.set_latency(Duration::from_millis(100), Duration::from_millis(100));
    let r = rig(&jev, N * 2);
    let mut turns = Vec::new();
    for i in 0..N {
        let core = r.core.clone();
        turns.push(tokio::spawn(async move {
            cn_turn(&core, &format!("say hello back, number {i}")).await
        }));
    }
    let mut ran = Vec::new();
    for t in turns {
        ran.push(t.await.expect("a cn turn's task panicked"));
    }
    let table: Vec<String> = ran
        .iter()
        .map(|x| format!("{} {:?} on {}", x.session, x.took, x.profile))
        .collect();
    let lost: Vec<&Ran> = ran.iter().filter(|x| x.took >= WAIT).collect();
    assert!(
        lost.is_empty(),
        "{} of {N} cn turns spent their whole wait_ms ({WAIT:?}): their own messages' verdicts \
         did not land inside the floor's wait.\n{}",
        lost.len(),
        table.join("\n")
    );
    let off: Vec<&Ran> = ran.iter().filter(|x| x.profile != "glm").collect();
    assert!(
        off.is_empty(),
        "{} of {N} cn turns fell off their pinned model.\n{}",
        off.len(),
        table.join("\n")
    );
    assert_eq!(crate::tests_dm_rig::blocked(&r.core), 0);
}

/// Every `content.v3` call's timing, as its `judge.call` row recorded it:
/// `(queued_ms, http_ms, total_ms)`. `queued_ms` is the time the call spent
/// waiting for one of the judge client's in-flight permits
/// (`[judge] max_in_flight`), which is the number that says whether a verdict
/// was late because Jev was slow or because the call never got to be made.
fn content_call_timings(core: &std::sync::Arc<crate::rpc::Core>) -> Vec<(u64, u64, u64)> {
    core.store
        .scope_after("judge:content", 0)
        .unwrap()
        .into_iter()
        .filter_map(|r| r.decode::<crate::ledger::LedgerRow>().ok())
        .filter_map(|r| {
            let t = r.data.get("timing")?;
            let n = |k: &str| t.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
            Some((n("queued_ms"), n("http_ms"), n("total_ms")))
        })
        .collect()
}

/// **The mechanism, measured** (theseus-oc9n4). The row's hypothesis was that
/// the landing defers past the floor's wait. It does not: `judge::content`'s
/// `land` writes at once, and the between-turns park it used to do was taken
/// out by theseus-gyjyo.7. What is left, and what this holds, is the **queue
/// in front of the call**: every judge call takes one of the client's
/// `[judge] max_in_flight` permits, a content call takes one like any other,
/// and a turn's own calls are in that queue too. So a content ask queued at
/// the top of a turn can be served only once the turns around it stop asking
/// — which is why the live check saw the verdict land a fixed half-second
/// after the turn **ended**, on turns of quite different lengths, rather than
/// a fixed time after it was asked.
///
/// The rig puts six cn sessions against two permits with a Jev that answers
/// in 400 ms, so the queue in front of a content call is longer than the
/// floor's wait. A red prints each call's `(queued_ms, http_ms, total_ms)`:
/// a large `queued_ms` beside a small `http_ms` is the permit queue and
/// nothing else.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn a_cn_turns_own_message_does_not_lose_the_floors_wait_to_the_permit_queue() {
    const N: usize = 6;
    const SHORT: Duration = Duration::from_millis(1_500);
    let jev = FakeJev::start().unwrap();
    clean(&jev);
    crate::tests_route::mode(&jev, "trivial", 0.95);
    jev.set_latency(Duration::from_millis(400), Duration::from_millis(400));
    let r = rig_parts(
        Some(&jev),
        N * 2,
        |c| {
            crate::tests_route::restricted_placements(c);
            c.judge.packs.remove(crate::judge::content::PACK);
            c.judge.total_secs = 30;
            // Two permits for six sessions: the contention the live check had
            // with eight sessions and the default eight.
            c.judge.max_in_flight = 2;
            c.content_based_security.wait_ms = SHORT.as_millis() as u64;
            c.model.retries.transient = 0;
        },
        |_| {},
    );
    let mut turns = Vec::new();
    for i in 0..N {
        let core = r.core.clone();
        turns.push(tokio::spawn(async move {
            cn_turn(&core, &format!("say hello back, number {i}")).await
        }));
    }
    let mut ran = Vec::new();
    for t in turns {
        ran.push(t.await.expect("a cn turn's task panicked"));
    }
    let timings = content_call_timings(&r.core);
    let worst = timings.iter().map(|t| t.0).max().unwrap_or(0);
    let table: Vec<String> = ran
        .iter()
        .map(|x| format!("  {:?} on {}", x.took, x.profile))
        .collect();
    let lost: Vec<&Ran> = ran.iter().filter(|x| x.took >= SHORT).collect();
    assert!(
        lost.is_empty(),
        "{} of {N} cn turns spent their whole wait_ms ({SHORT:?}).\nturns:\n{}\n\
         content.v3 calls (queued_ms, http_ms, total_ms): {timings:?}\n\
         worst queued_ms {worst}: a large one beside a small http_ms is the \
         in-flight permit queue, not a slow Jev.",
        lost.len(),
        table.join("\n")
    );
}
