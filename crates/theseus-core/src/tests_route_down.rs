//! A session Jev moved up steps back down (theseus-p8dy; `routing::decide`,
//! `turn::route_step`): after two verdicts in a row whose mode's first
//! profile is the base or cheaper, the first sets a hold toward the base and
//! the second makes the switch, at any size of context; one is not enough,
//! and a dearer verdict between them breaks the row. The rig is
//! `tests_route`'s.

use serde_json::json;
use theseus_judge::fake::FakeJev;

use crate::tests_route::{decided, mode, rig, session, turn};

fn ran(d: &serde_json::Value) -> (String, String, bool) {
    (
        d["profile"].as_str().unwrap().to_string(),
        d["reason"].as_str().unwrap().to_string(),
        d["switch"].as_bool().unwrap(),
    )
}

/// Sophisticated moves the session to Opus; the first `chat` holds, the
/// second goes back to Sonnet (`step_down`), and the move is over.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_chat_verdicts_step_a_raised_session_back_to_its_base() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 4, |_| {});
    let one = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(one.profile, "opus");
    let sid = one.session_id;
    mode(&jev, "chat", 0.95);
    let two = turn(&r.core, Some(&sid), "ok, and the index?", None).await;
    assert_eq!(two.profile, "opus", "one chat is not enough");
    let held = session(&r.core, &sid).routed.unwrap();
    let hold = held.hold.unwrap();
    assert_eq!(
        (hold.mode.as_str(), hold.profile.as_str()),
        ("step_down", "sonnet")
    );
    assert_eq!(held.profile.as_deref(), Some("opus"));
    let three = turn(&r.core, Some(&sid), "and the cache?", None).await;
    assert_eq!(three.profile, "sonnet", "the second agreeing turn");
    assert!(session(&r.core, &sid).routed.is_none(), "the move is over");
    let d = decided(&r.core.store);
    assert_eq!(ran(&d[0]), ("opus".into(), "verdict".into(), true));
    assert_eq!(ran(&d[1]), ("opus".into(), "cache_hold".into(), false));
    assert_eq!(d[1]["mode"], json!("chat"));
    assert_eq!(ran(&d[2]), ("sonnet".into(), "step_down".into(), true));
    // A third chat on the base changes nothing.
    let four = turn(&r.core, Some(&sid), "thanks, and the wal?", None).await;
    assert_eq!(four.profile, "sonnet");
    assert!(session(&r.core, &sid).routed.is_none());
}

/// A dearer verdict between two chats breaks the row, and above
/// `cold_switch_tokens` the hold, not a third turn, governs the step down.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hard_verdict_between_breaks_the_row_and_the_hold_governs_above_the_cold_line() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 6, |c| c.routing.cold_switch_tokens = 1);
    let one = turn(&r.core, None, "Weigh two designs for the log.", None).await;
    assert_eq!(one.profile, "sonnet", "above the cold line the move waits");
    let sid = one.session_id;
    let two = turn(&r.core, Some(&sid), "Weigh them again.", None).await;
    assert_eq!(two.profile, "opus", "the second agreeing turn");
    mode(&jev, "chat", 0.95);
    turn(&r.core, Some(&sid), "ok", None).await;
    mode(&jev, "sophisticated", 0.95);
    let broken = turn(&r.core, Some(&sid), "Weigh a third way.", None).await;
    assert_eq!(broken.profile, "opus");
    assert!(
        session(&r.core, &sid).routed.unwrap().hold.is_none(),
        "the row broke"
    );
    mode(&jev, "chat", 0.95);
    let again = turn(&r.core, Some(&sid), "fine", None).await;
    assert_eq!(
        again.profile, "opus",
        "the first chat after the break holds"
    );
    let down = turn(&r.core, Some(&sid), "fine, go on", None).await;
    assert_eq!(down.profile, "sonnet", "the hold governs: no third turn");
    let d = decided(&r.core.store);
    assert_eq!(
        ran(d.last().unwrap()),
        ("sonnet".into(), "step_down".into(), true)
    );
}

/// The owner's pick is never stepped down: a turn pinned to Opus
/// (`ask -P`) changes nothing in routing, and the next chats run where the
/// pane carries them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_owners_pick_is_not_stepped_down() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 4, |_| {});
    let one = turn(&r.core, None, "hello there", Some("opus")).await;
    assert_eq!(one.profile, "opus");
    let sid = one.session_id;
    assert!(session(&r.core, &sid).routed.is_none());
    for text in ["and now?", "and then?"] {
        let t = turn(&r.core, Some(&sid), text, Some("opus")).await;
        assert_eq!(t.profile, "opus");
    }
    assert!(decided(&r.core.store)
        .iter()
        .all(|d| d["switch"] == json!(false)));
}

/// route.v4's `review` (theseus-p8dy): a verification runs on `opushi` (Opus
/// 5.5 at high effort, from the template), and on `fable` when the list's
/// first profile is not configured (passed over as unusable). The rig pins that
/// older table (`before_the_cn_rubric_tables`); the defaults are the next test's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_review_verdict_runs_on_opushi_and_then_fable() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "review", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let one = turn(&r.core, None, "Check this change before I merge it.", None).await;
    assert_eq!(one.profile, "opushi");
    assert_eq!(one.route.unwrap().mode.as_deref(), Some("review"));
    let jev = FakeJev::start().unwrap();
    mode(&jev, "review", 0.95);
    let r = rig(Some(&jev), 1, |c| {
        c.routing.classes.review.profiles = vec!["nosuch".into(), "fable".into()];
    });
    let one = turn(&r.core, None, "Is this right?", None).await;
    assert_eq!(one.profile, "fable");
}

/// The defaults (theseus-p8dy, then theseus-tqomi's cn rubric §7.4): a
/// verification runs on `opus` (the template's, no effort of its own) and a
/// hard-reasoning turn on `opushi` (Opus 5.5 at high effort), each passing
/// over a profile the config lacks: `review` to `sonnethi`, `sophisticated`
/// to `opus` and then `sonnethi`. The rig's pin to the older tables is undone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_tables_run_sophisticated_on_opushi_and_review_on_opus() {
    for (kind, text, first, then) in [
        (
            "review",
            "Check this change before I merge it.",
            "opus",
            "sonnethi",
        ),
        (
            "sophisticated",
            "Weigh two designs for a crash-safe write-ahead log.",
            "opushi",
            "opus",
        ),
    ] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, kind, 0.95);
        let r = rig(Some(&jev), 1, |c| c.routing.classes = Default::default());
        let one = turn(&r.core, None, text, None).await;
        assert_eq!(one.profile, first, "{kind}");
        assert_eq!(one.route.unwrap().mode.as_deref(), Some(kind));
        let jev = FakeJev::start().unwrap();
        mode(&jev, kind, 0.95);
        let r = rig(Some(&jev), 1, |c| {
            c.routing.classes = Default::default();
            let table = match kind {
                "review" => &mut c.routing.classes.review.profiles,
                _ => &mut c.routing.classes.sophisticated.profiles,
            };
            table.retain(|p| p != first);
            assert_eq!(table.first().map(String::as_str), Some(then), "{kind}");
        });
        let one = turn(&r.core, None, text, None).await;
        assert_eq!(
            one.profile, then,
            "{kind}: the first profile is passed over"
        );
    }
}
