//! The injection tier's reach into the recall manifest, driven by `work.v1` (theseus-h66v4.11).
//! route.v8's tier tests set a turn's tier through the class `mode`, which route.v10 retires;
//! `tests_tier_work` restored the route row, the prefix and the admission through `work = coding`
//! at the confirm bar, and asserts the route row and the `why` line, not the recall the model was
//! sent. Here the same turn's recall manifest and the request's size are read, so a pack that
//! reads the right tier on the route row and then sizes the recall by another is caught.
//!
//! - a turn `work.v1` made deep gets the deep pack in the recall manifest and the footer's count;
//! - the request sizes follow the tier, and not by the coding hint alone.
//!
//! The rig is `tests_injection_tiers`'s (recall live over twenty harbour notes), with route.v10's
//! shape acting (`tests_act_open`).

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::TurnSubmitResult;

use crate::tests_act_open::act_open;
use crate::tests_injection_tiers::tiered;
use crate::tests_route::{decided, turn, Rig};
use crate::tests_route_open::says;

fn work(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "work",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

/// `tiered`, with route.v10 acting: its verdict lands before the pack reads it.
fn tiered_open(jev: &FakeJev, n: usize, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let r = tiered(jev, n, true, tweak);
    act_open(&r);
    r
}

/// The turn's one live recall row.
fn recalled(r: &Rig, res: &TurnSubmitResult) -> RecallManifest {
    let rows = crate::tests_recall::recalls(&r.core, &res.session_id);
    assert_eq!(rows.len(), 1, "one recall row");
    rows.into_iter().next().unwrap()
}

const ASK: &str = "Which moorings on the outer wall were checked?";

/// The key test of the tier's reach: a turn `work.v1` made deep (`work = coding` at the confirm
/// bar, its verdict landed before the pack) gets the deep recall pack, 8,000 tokens and 16 items
/// of the twenty notes that fit, in the recall manifest and the footer's count, and the route row
/// says so. An ordinary turn (`work = conversation`) gets 3,000 tokens and 8 items, from the same
/// twenty notes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deep_turn_gets_the_deep_recall_pack_in_the_manifest() {
    let mut seen = Vec::new();
    for (label, tokens, items, ceiling) in [
        ("coding", 8_000_u64, 16_usize, 3.0),
        ("conversation", 3_000, 8, 2.0),
    ] {
        let jev = FakeJev::start().unwrap();
        says(&jev, "sonnet", "high", 0.95);
        work(&jev, label, 0.95);
        let r = tiered_open(&jev, 1, |_| {});
        let res = turn(&r.core, None, ASK, None).await;
        let m = recalled(&r, &res);
        assert_eq!(
            (m.budget_tokens, m.admitted.len(), m.outcome.as_str()),
            (tokens, items, "ran"),
            "work = {label}"
        );
        assert_eq!(
            res.recalled, items as u32,
            "work = {label}: the footer counts what the model saw"
        );
        let i = decided(&r.core.store).pop().unwrap()["injection"].clone();
        assert_eq!(
            (
                &i["tier"],
                &i["fallback"],
                &i["recall_tokens"],
                &i["recall_items"],
                &i["admit_ceiling"]
            ),
            (
                &if label == "coding" {
                    "deep"
                } else {
                    "ordinary"
                }
                .into(),
                &Value::Null,
                &tokens.into(),
                &items.into(),
                &ceiling.into()
            ),
            "work = {label}: {i}"
        );
        seen.push(m.admitted.len());
    }
    assert!(seen[0] > seen[1], "deep admits more: {seen:?}");
}

/// The request sizes the tiers make on this rig, before (1,500 tokens and 6 items for every
/// prompt) and after (`out` is before ordinary, before deep, after ordinary, after deep). A deep
/// turn's request carries the coding hint beside its recall, so before, deep is larger than
/// ordinary by the hint alone; after, it is larger by the hint and 5,000 more tokens of recall.
/// The tiers move the sizes the way the pack says, and not by the hint: printed for the report.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_sizes_follow_the_tier() {
    let mut out = Vec::new();
    for (label, before) in [("before", true), ("after", false)] {
        for work_label in ["conversation", "coding"] {
            let jev = FakeJev::start().unwrap();
            says(&jev, "sonnet", "high", 0.95);
            work(&jev, work_label, 0.95);
            let r = tiered_open(&jev, 1, |c| {
                if before {
                    (c.memory.recall_budget_tokens, c.memory.recall_max_items) = (1_500, 6);
                    (c.memory.deep_recall_tokens, c.memory.deep_recall_items) = (1_500, 6);
                    c.tools.defer.admit_ceiling_deep = 2.0;
                }
            });
            turn(&r.core, None, ASK, None).await;
            let c = crate::tests_judge::kinds(&r.core.store, "context.compiled");
            out.push((label, work_label, c[0].data["est_tokens"].as_i64().unwrap()));
        }
    }
    eprintln!("request sizes (est_tokens): {out:?}");
    let tokens = |i: usize| out[i].2;
    let hint = tokens(1) - tokens(0);
    assert!(
        hint > 0,
        "the coding hint is the whole of the before difference: {out:?}"
    );
    assert!(
        tokens(2) > tokens(0),
        "ordinary's 3,000 tokens and 8 items beat 1,500 and 6: {out:?}"
    );
    assert!(
        tokens(3) - tokens(2) > hint,
        "deep's recall beats ordinary's by more than the coding hint: {out:?}"
    );
    assert!(tokens(3) > tokens(1), "deep's 8,000 beat 1,500: {out:?}");
}

/// The small-talk bar, in shadow (theseus-fpm2): a greeting that `effort.v1` calls `low` has its
/// manifest record how many admitted notes a bar would drop, and still attaches them; a real
/// question, same rig and notes, drops nothing. Both halves assert the positive count, so a
/// recall that is simply switched off passes neither. Caveat: `effort` is scripted from a
/// `FakeJev`, so this proves the plumbing from a `low` verdict, not that `effort.v1` calls a real
/// greeting `low`; a green here is not a working bar.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_greeting_records_what_a_bar_would_drop_and_a_question_drops_nothing() {
    for (message, level, greeting) in [
        ("Hello there, good morning!", "low", true),
        (ASK, "high", false),
    ] {
        let jev = FakeJev::start().unwrap();
        says(&jev, "sonnet", level, 0.95);
        work(&jev, "conversation", 0.95);
        let r = tiered_open(&jev, 1, |_| {});
        let res = turn(&r.core, None, message, None).await;
        let m = recalled(&r, &res);
        assert!(!m.admitted.is_empty(), "{message}: notes were admitted");
        assert_eq!(
            res.recalled as usize,
            m.admitted.len(),
            "{message}: shadow, so the notes still ride the request"
        );
        let want = greeting.then_some(m.admitted.len() as u64);
        assert_eq!(m.would_drop, want, "{message}");
    }
}

/// A greeting `effort.v1` calls `low` whose recall the session's cap paused: the notes were
/// admitted, never attached (`recalled == 0`), so no bar could have "dropped" them, and the
/// shadow field stays absent. A paused turn must not inflate the count the shadow exists to read
/// (theseus-fpm2). Turn one fills the cap; turn two, a greeting, is paused. The positive state is
/// asserted too: notes admitted, `outcome == "paused"`, nothing attached.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paused_greeting_records_no_would_drop() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "low", 0.95);
    work(&jev, "conversation", 0.95);
    let r = tiered_open(&jev, 1, |c| {
        c.memory.recall_budget_tokens = 200;
        c.memory.recall_max_items = 1;
        c.memory.deep_recall_tokens = 200;
        c.memory.deep_recall_items = 1;
        c.memory.session_recall_cap_tokens = 200;
    });
    let first = turn(&r.core, None, "Hello there, good morning!", None).await;
    assert!(first.recalled > 0, "turn one fills the cap");
    let res = turn(
        &r.core,
        Some(&first.session_id),
        "Hello again, good evening!",
        None,
    )
    .await;
    let rows = crate::tests_recall::recalls(&r.core, &res.session_id);
    let m = rows.last().unwrap();
    assert_eq!(m.outcome, "paused", "{m:?}");
    assert!(!m.admitted.is_empty(), "notes were admitted");
    assert_eq!(res.recalled, 0, "none were attached");
    assert_eq!(m.would_drop, None);
}

/// The verdict lands after the pack (theseus-fpm2's `Late` hole) but inside the route's own wait:
/// the pack fixed an ordinary budget for want of it, and a greeting `effort.v1` calls `low` still
/// records what a bar would drop, so the shadow's sample is not missing the late turns that wait
/// catches (here 150 ms into a 2 s wait). The route row says `verdict late` (the proof it was late
/// at the pack), the route's own wait still catching it before the first compile, and the same
/// late turn called `high` records nothing. A verdict that misses that wait is not this test's:
/// `a_verdict_that_misses_the_routes_wait_records_no_would_drop` pins it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_low_verdict_still_records_what_a_bar_would_drop() {
    for (level, greeting) in [("low", true), ("high", false)] {
        let jev = FakeJev::start().unwrap();
        says(&jev, "sonnet", level, 0.95);
        work(&jev, "conversation", 0.95);
        jev.set_mode(theseus_judge::fake::FakeMode::Held);
        let r = crate::tests_injection_tiers::tiered(&jev, 1, false, |c| {
            c.routing.max_wait_ms = 2_000;
            c.judge.total_secs = 30;
        });
        act_open(&r);
        let late = jev.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            late.release();
        });
        let res = turn(&r.core, None, "Hello there, good morning!", None).await;
        let m = recalled(&r, &res);
        assert!(!m.admitted.is_empty(), "{level}: notes were admitted");
        let row = decided(&r.core.store).pop().unwrap();
        assert_eq!(
            row["injection"]["fallback"], "verdict late",
            "{level}: {row}"
        );
        let want = greeting.then_some(m.admitted.len() as u64);
        assert_eq!(m.would_drop, want, "{level}");
    }
}

/// The hole the test above does not cover, pinned on purpose (theseus-fpm2's `Late` hole): a
/// greeting whose `low` verdict misses the route's own wait (`routing.max_wait_ms`; here the judge
/// is held past it) is `Got::Late` in `route_step.rs`'s `read_verdict`, its verdict lands for the
/// session's next message, and this turn records no `would_drop`. The shadow's sample is short by
/// every low-effort turn the wait missed; `RecallManifest::would_drop`'s doc says so. What closes
/// it: marking such a turn when its verdict arrives (a design change to the route step, not made).
/// What makes this fail: any edit that records `would_drop` for a turn like this one, which is
/// then the edit to make, with that doc changed to match.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verdict_that_misses_the_routes_wait_records_no_would_drop() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "low", 0.95);
    work(&jev, "conversation", 0.95);
    jev.set_mode(theseus_judge::fake::FakeMode::Held);
    let r = crate::tests_injection_tiers::tiered(&jev, 1, false, |c| {
        c.routing.max_wait_ms = 100;
        c.judge.total_secs = 30;
    });
    act_open(&r);
    let res = turn(&r.core, None, "Hello there, good morning!", None).await;
    let m = recalled(&r, &res);
    assert!(!m.admitted.is_empty(), "notes were admitted");
    let row = decided(&r.core.store).pop().unwrap();
    assert_eq!(row["injection"]["fallback"], "verdict late", "{row}");
    assert_eq!(
        (&row["late"], &row["reason"], &row["answered_ms"]),
        (&true.into(), &"late".into(), &Value::Null),
        "the route's own wait ended with no verdict: {row}"
    );
    assert_eq!(
        m.would_drop, None,
        "a verdict that missed the wait marks nothing"
    );
    jev.release();
}

/// A second `Late` hole, pinned on purpose (theseus-fpm2): a late verdict decides a LATER message's
/// `would_drop`. Message one is a greeting whose `low` verdict misses the route's wait; it lands
/// for the session's next message (`route_step.rs`'s `read_verdict`, `Got::Late` with a stashed
/// verdict, which comes back with no `reason`). Message two's own verdict is late too, so its pack
/// fixes a `Late` budget, and the carried `low` passes `raise_low_effort`'s filter: message two's
/// manifest records `would_drop` from message ONE's verdict, though nothing of its own said `low`.
/// The routing carry is deliberate (theseus-6n5j); nobody decided it should also decide this
/// sample, so a count over `would_drop` rows mixes the two (the field's doc says so). What closes
/// it: telling a carried verdict from the turn's own at that filter (a design change to the route
/// step, not made; the owner decides first). What makes this fail: any edit that stops a carried
/// verdict marking the next message, which is then the edit to make, with this test and the doc
/// changed to match. Lateness is by order (`Held`, `release`, `until_late`), never by the hour.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_verdict_marks_the_next_messages_would_drop() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "low", 0.95);
    work(&jev, "conversation", 0.95);
    jev.set_mode(theseus_judge::fake::FakeMode::Held);
    let r = crate::tests_injection_tiers::tiered(&jev, 1, false, |c| {
        c.routing.max_wait_ms = 100;
        c.judge.total_secs = 30;
    });
    act_open(&r);
    let one = turn(&r.core, None, "Hello there, good morning!", None).await;
    jev.release();
    crate::tests_route::until_late(&r.core, &one.session_id).await;
    let two = turn(
        &r.core,
        Some(&one.session_id),
        "Hello again, good evening!",
        None,
    )
    .await;
    let rows = crate::tests_recall::recalls(&r.core, &two.session_id);
    let m = rows.last().unwrap();
    assert!(!m.admitted.is_empty(), "notes were admitted: {m:?}");
    let d = decided(&r.core.store);
    let row = &d[1];
    assert_eq!(row["injection"]["fallback"], "verdict late", "{row}");
    assert_eq!(
        (&row["late"], &row["reason"]),
        (&true.into(), &"late".into()),
        "message two's own verdict missed the wait too: {row}"
    );
    assert_eq!(
        m.would_drop,
        Some(m.admitted.len() as u64),
        "the carried verdict of message one marks message two"
    );
    jev.release();
}
