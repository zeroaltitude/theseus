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

/// A second `Late` hole, closed SEPARATE (theseus-ectzg, B4 L79; was pinned open as theseus-fpm2):
/// a late verdict decides a LATER message's `would_drop` too. Message one is a greeting whose
/// `low` verdict misses the route's wait; it lands for the session's next message
/// (`route_step.rs`'s `read_verdict`, `Got::Late` with a stashed verdict, which comes back with no
/// `reason`). Message two's own verdict is late too, so its pack fixes a `Late` budget, and the
/// carried `low` passes `raise_low_effort`'s filter: message two's manifest still records
/// `would_drop` from message ONE's verdict, though nothing of its own said `low`. The routing
/// carry is deliberate (theseus-6n5j); the owner's answer (2026-11-10 22:31:37 MST, "your
/// recommendation" to the recommendation SEPARATE) is that this sample tells a carried verdict
/// from the turn's own rather than drop it: `would_drop` stays, and `would_drop_carried` says
/// which. What makes this fail: any edit that stops `raise_low_effort`/`injection_read` computing
/// `v.turn != t.tc.turn_id` and stamping it through to the manifest, which is the edit to undo,
/// with this test and the doc changed to match. Lateness is by order (`Held`, `release`,
/// `until_late`), never by the hour.
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
    // Message two's row: its own wait was missed (`late`), and the verdict it reads is message
    // one's, carried (`effort_reason: carried`, `reason: verdict`: no sign of lateness).
    assert_eq!(
        (&row["late"], &row["reason"], &row["effort_reason"]),
        (&true.into(), &"verdict".into(), &"carried".into()),
        "message two reads message one's verdict: {row}"
    );
    assert_eq!(
        m.would_drop,
        Some(m.admitted.len() as u64),
        "the carried verdict of message one marks message two"
    );
    assert_eq!(
        m.would_drop_carried,
        Some(true),
        "message two's mark is flagged as message one's carried verdict, not its own: {m:?}"
    );
    jev.release();
}

/// The sibling of the carried case above, same rig: message one's OWN turn (never anybody
/// else's) records `would_drop_carried: Some(false)`, so a reader can tell a false negative (never
/// flagged) from an absent field (no `would_drop` at all, [`a_greeting_records_what_a_bar_would_drop_and_a_question_drops_nothing`]).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_own_would_drop_is_not_marked_carried() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "low", 0.95);
    work(&jev, "conversation", 0.95);
    let r = tiered_open(&jev, 1, |_| {});
    let res = turn(&r.core, None, "Hello there, good morning!", None).await;
    let m = recalled(&r, &res);
    assert!(!m.admitted.is_empty(), "notes were admitted: {m:?}");
    assert_eq!(m.would_drop, Some(m.admitted.len() as u64), "{m:?}");
    assert_eq!(
        m.would_drop_carried,
        Some(false),
        "this turn's own in-time verdict said `low`; nothing was carried: {m:?}"
    );
    jev.release();
}

/// `would_drop` is absent when nothing was admitted, too (`mark_would_drop`'s third conjunct):
/// the same `low` greeting is driven twice, once with the harbour notes admitted (`would_drop`
/// is the count, above zero) and once with an index that answers nothing
/// (`admitted` is empty, a recall row is still written, and `would_drop` is absent). The
/// admitted-notes twin is what stops the absent case passing for want of a manifest. A reader
/// counting low-effort turns from `recall.ran` rows misses the empty ones; the field's doc says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_greeting_that_admitted_nothing_records_no_would_drop() {
    for (silent, admits) in [(false, true), (true, false)] {
        let jev = FakeJev::start().unwrap();
        says(&jev, "sonnet", "low", 0.95);
        work(&jev, "conversation", 0.95);
        let r = tiered_open(&jev, 1, |_| {});
        if silent {
            let none = crate::tests_recall::index_of(&r.core, vec![]);
            r.core.runner.memory.set_ask(none);
        }
        let res = turn(&r.core, None, "Hello there, good morning!", None).await;
        let m = recalled(&r, &res);
        assert_eq!(m.admitted.is_empty(), !admits, "silent {silent}: {m:?}");
        let want = admits.then_some(m.admitted.len() as u64);
        assert_eq!(m.would_drop, want, "silent {silent}");
    }
}

/// A greeting the shadow bar would have dropped notes for.
const GREETING: &str = "Hello there, good morning!";

/// Whether a request carries a recalled note, looked for wherever it renders
/// (the preamble `recall::render` writes heads every pack).
fn carries_a_note(q: &crate::provider::ProviderRequest) -> bool {
    serde_json::to_string(&q.messages)
        .unwrap_or_default()
        .contains("[Recalled by the harness:")
}

/// The turn's `Recall` nodes, which only a request that carried one writes.
fn recall_nodes(r: &Rig, sid: &str) -> usize {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| matches!(n.body, crate::node::Body::Recall { .. }))
        .count()
}

/// **The small-talk bar firing** (theseus-fpm2, theseus-ectzg's B4): with `[memory]
/// small_talk_bar` on, a turn whose peeked verdict calls the reply `low` effort does not carry
/// the notes its recall admitted — the symptom the row names (off-topic notes attached to a
/// greeting and narrated back) is that they were carried. Three rows, one rig, the same twenty
/// harbour notes:
///
/// - **bar on, `low`**: the request carries no note, the reply counts none, no `Recall` node is
///   written, and the `recall.withheld` row says `why = small_talk` with the count it held back;
/// - **bar on, `high`** (a real question): the notes are carried unchanged, so the bar is not a
///   blunt instrument that strips every turn;
/// - **bar off, `low`** (the default): the notes are carried, which is main's behaviour and what
///   pins the key's default.
///
/// Every row asserts that recall ran and admitted notes (`admitted` above zero), so a recall
/// that is simply switched off, or an index that answers nothing, passes none of them — and
/// every row asserts `would_drop` is the shadow count it always was, so the series the shadow
/// built reads the same number with the bar on. Caveat, as the shadow test above: `effort` is
/// scripted from a `FakeJev`, so this proves the plumbing from a `low` verdict to the
/// withholding, not that `effort.v1` calls a real greeting `low`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_small_talk_bar_withholds_a_greetings_notes_and_carries_a_questions() {
    for (bar, level, message, carried) in [
        (true, "low", GREETING, false),
        (true, "high", ASK, true),
        (false, "low", GREETING, true),
    ] {
        let case = format!("bar {bar}, effort {level}");
        let jev = FakeJev::start().unwrap();
        says(&jev, "sonnet", level, 0.95);
        work(&jev, "conversation", 0.95);
        let r = tiered_open(&jev, 1, |c| c.memory.small_talk_bar = bar);
        let res = turn(&r.core, None, message, None).await;
        let m = recalled(&r, &res);
        let admitted = m.admitted.len();
        assert!(admitted > 0, "{case}: the pack admitted notes");
        let q = r.claude.requests().pop().expect("a request");
        assert_eq!(
            carries_a_note(&q),
            carried,
            "{case}: the request carries the notes"
        );
        let withheld =
            crate::test_support::recall_rows(&r.core, &res.session_id, "recall.withheld");
        if carried {
            assert_eq!(
                res.recalled as usize, admitted,
                "{case}: the footer counts them"
            );
            assert_eq!(
                recall_nodes(&r, &res.session_id),
                1,
                "{case}: one Recall node"
            );
            assert!(withheld.is_empty(), "{case}: nothing was withheld");
        } else {
            assert_eq!(res.recalled, 0, "{case}: the footer counts none");
            assert_eq!(
                recall_nodes(&r, &res.session_id),
                0,
                "{case}: no Recall node the request never carried"
            );
            assert_eq!(withheld.len(), 1, "{case}: one withholding row");
            assert_eq!(withheld[0].data["why"], "small_talk", "{case}");
            assert_eq!(
                withheld[0].data["recall_id"],
                m.recall_id.as_str(),
                "{case}: joined to the admission it dropped"
            );
            assert_eq!(
                withheld[0].data["admitted"], admitted,
                "{case}: the notes the request did not carry"
            );
        }
        // The shadow count is what it was, bar or no bar: `would_drop` is still
        // "what a bar WOULD have dropped", so a series counted over it is unbroken.
        let want = (level == "low").then_some(admitted as u64);
        assert_eq!(m.would_drop, want, "{case}: the shadow count");
    }
}

/// The bar does not fire when the peek found no verdict, which is the safe direction
/// (`tier.rs`'s "is ordinary when it has not"). The bar is on and the message is the same
/// greeting, but the judge is held past the route's own wait, so the budget the pack fixed read
/// nothing (`fallback = "verdict late"`, `reason = "late"`, no answer) and `low_effort` is
/// false: the notes are carried, the reply counts them, and no `recall.withheld` row is written.
///
/// The positive assertions are what stop this passing for the wrong reason: the pack admitted
/// notes, and the route row proves the late case was reached rather than an on-time `high`
/// verdict. Lateness is by order (`Held`, `release`), never by the hour.
///
/// It also pins the asymmetry with the shadow count: this turn records no `would_drop` either
/// (`a_verdict_that_misses_the_routes_wait_records_no_would_drop`), so the bar's sample and the
/// shadow's agree here. Where they do not is the `Late` hole in the test below.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_verdict_peeked_means_the_small_talk_bar_does_not_fire() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "low", 0.95);
    work(&jev, "conversation", 0.95);
    jev.set_mode(theseus_judge::fake::FakeMode::Held);
    let r = crate::tests_injection_tiers::tiered(&jev, 1, false, |c| {
        c.memory.small_talk_bar = true;
        c.routing.max_wait_ms = 100;
        c.judge.total_secs = 30;
    });
    act_open(&r);
    let res = turn(&r.core, None, GREETING, None).await;
    let m = recalled(&r, &res);
    assert!(!m.admitted.is_empty(), "the pack admitted notes");
    let row = decided(&r.core.store).pop().unwrap();
    assert_eq!(
        (
            &row["injection"]["fallback"],
            &row["reason"],
            &row["answered_ms"]
        ),
        (&"verdict late".into(), &"late".into(), &Value::Null),
        "the route's own wait ended with no verdict: {row}"
    );
    let q = r.claude.requests().pop().expect("a request");
    assert!(
        carries_a_note(&q),
        "a turn with no verdict carries its notes"
    );
    assert_eq!(
        res.recalled as usize,
        m.admitted.len(),
        "and the footer counts them"
    );
    assert!(
        crate::test_support::recall_rows(&r.core, &res.session_id, "recall.withheld").is_empty(),
        "nothing was withheld"
    );
    jev.release();
}

/// The `Late` hole's own asymmetry, pinned on purpose and not this row's to fix (theseus-fpm2):
/// a `low` verdict that lands after the pack but inside the route's wait marks the shadow
/// `would_drop` (`InjectionBudget::raise_low_effort`, called from `record_route`) — and that
/// call comes after the turn's first compile has already asked the bar, so the request carried
/// the notes. The bar's sample is therefore a subset of the shadow's, not the same set.
///
/// This is the safe direction of the same hole: a turn whose verdict was not there when the
/// request was built is treated as ordinary, as `tier.rs` says. What would close it: marking
/// such a turn before the compile, which means the pack waiting for the verdict, and
/// `tier.rs:15`'s "never a wait" is load-bearing. What makes this fail: any edit that withholds
/// on a late-marked turn, which is then the edit to make, with this test and
/// `MemoryConfig::small_talk_bar`'s doc changed to match.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_low_verdict_marks_the_shadow_and_the_bar_still_carries_the_notes() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "low", 0.95);
    work(&jev, "conversation", 0.95);
    jev.set_mode(theseus_judge::fake::FakeMode::Held);
    let r = crate::tests_injection_tiers::tiered(&jev, 1, false, |c| {
        c.memory.small_talk_bar = true;
        c.routing.max_wait_ms = 2_000;
        c.judge.total_secs = 30;
    });
    act_open(&r);
    let late = jev.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        late.release();
    });
    let res = turn(&r.core, None, GREETING, None).await;
    let m = recalled(&r, &res);
    assert!(!m.admitted.is_empty(), "the pack admitted notes");
    let row = decided(&r.core.store).pop().unwrap();
    assert_eq!(
        row["injection"]["fallback"], "verdict late",
        "the verdict was late at the pack: {row}"
    );
    assert_eq!(
        m.would_drop,
        Some(m.admitted.len() as u64),
        "the shadow marks it from the late verdict"
    );
    let q = r.claude.requests().pop().expect("a request");
    assert!(
        carries_a_note(&q),
        "and the request carried them: the mark came after the compile"
    );
    assert_eq!(
        res.recalled as usize,
        m.admitted.len(),
        "the footer counts them"
    );
}

/// The key's default, stated where a reader can find it: the bar is off, so a build with no
/// `[memory] small_talk_bar` in its config behaves exactly as main did. The companion is the
/// third row of `the_small_talk_bar_withholds_a_greetings_notes_and_carries_a_questions`, which
/// drives a whole turn with the key off and sees the notes carried.
#[test]
fn the_small_talk_bar_is_off_by_default() {
    assert!(!crate::config::memory::MemoryConfig::default().small_talk_bar);
    // `example()` is the template parsed, whose line for the key is commented
    // out as every default's is, so this reads what a fresh install reads.
    assert!(!crate::Config::example().memory.small_talk_bar);
}
