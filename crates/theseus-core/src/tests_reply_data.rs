//! Theseus's replies in an open run (theseus-a1wvq.15): in the owner's DM an
//! open run includes a reply Jev judges `open` at the act band, so the next
//! open message still carries the run from its first message. Every other
//! outcome ends the run. The rig is `tests_open_request`'s, with the first
//! message a `chat` (not a detour), so the live profile answers it and a
//! reply is written for the question to be asked about.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_judge::band::Band;
use theseus_judge::fake::FakeJev;

use crate::data::reply::{lands, ReplyAsk};
use crate::data::{DataState, OpenRun, Taint};
use crate::tests_judge::kinds;
use crate::tests_open_request::{
    answer, choice, dm, jev, record, rig_with, say, text, OPEN_A, OPEN_B,
};
use crate::tests_route::mode;

/// The text of the live profile's first reply in `texts`.
const REPLY: &str = "Done: answer 0.";

/// Jev: `open_a` and `open_b` are open and `trivial`, but the first is a
/// `chat`, and the reply to it is answered `answer`.
fn scripted(answer: Option<(&str, f64)>) -> FakeJev {
    let jev = jev(&[OPEN_A, OPEN_B]);
    mode(&jev, "trivial", 0.95);
    jev.script_when(
        &format!("\"message\":\"{OPEN_A}\""),
        None,
        "mode",
        choice("chat", 0.95),
    );
    let reply = format!("\"message\":\"{REPLY}\"");
    match answer {
        Some((option, confidence)) => {
            jev.script_when(&reply, None, "data", choice(option, confidence))
        }
        None => jev.hold_when(&reply),
    }
    jev
}

fn rig(
    jev: &FakeJev,
) -> (
    crate::tests_route::Rig,
    Arc<crate::provider::FakeProvider>,
    tempfile::TempDir,
) {
    rig_with(jev, None, |c| c.judge.total_secs = 30)
}

fn rows(core: &crate::Core) -> Vec<Value> {
    kinds(&core.store, "data.reply")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// The `data.reply` rows, once `n` are written (or give up after 10 s).
async fn until_rows(core: &crate::Core, n: usize) -> Vec<Value> {
    for _ in 0..200 {
        let r = rows(core);
        if r.len() >= n {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    rows(core)
}

/// The two messages and their reply: what restricted `zai` was sent on the
/// second, as one string.
async fn second_message_after(jev: &FakeJev, rows_wanted: usize) -> (String, Vec<Value>, String) {
    let (r, zai, _dir) = rig(jev);
    let sid = dm(&r.core);
    let first = say(&r.core, &sid, OPEN_A, None).await;
    assert_ne!(first.profile, "dsflash", "{:?}", first.route);
    let rows = until_rows(&r.core, rows_wanted).await;
    say(&r.core, &sid, OPEN_B, None).await;
    let sent = zai.requests();
    assert_eq!(
        sent.len(),
        1,
        "the second message went to dsflash: {:?}",
        rows
    );
    (text(&sent[0]), rows, sid)
}

/// The key test. A message judged open is answered by the live profile; Jev
/// judges the reply open at the act band; the next open message reaches
/// dsflash with the run from the first message, the reply in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dm_reply_judged_open_extends_the_run() {
    let jev = scripted(Some(("open", 0.95)));
    let (body, rows, _) = second_message_after(&jev, 1).await;
    assert_eq!(rows[0]["extended"], true, "{rows:?}");
    assert_eq!(rows[0]["choice"], "open");
    for part in [OPEN_A, REPLY, OPEN_B] {
        assert!(body.contains(part), "{part:?} missing: {body}");
    }
}

/// The reply is not judged before the next message arrives: the run restarts
/// at the second message, and nothing of the first reaches dsflash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_with_no_answer_ends_the_run() {
    let jev = scripted(None);
    let (body, rows, _) = second_message_after(&jev, 0).await;
    assert!(rows.is_empty(), "no answer, no row yet: {rows:?}");
    assert!(body.contains(OPEN_B));
    for part in [OPEN_A, REPLY] {
        assert!(!body.contains(part), "{part:?} rode past the reply: {body}");
    }
    jev.release();
}

/// Private, unclear, and open below the act band each end the run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_judged_private_or_below_act_ends_the_run() {
    for (option, confidence) in [("private", 0.95), ("unclear", 0.95), ("open", 0.7)] {
        let jev = scripted(Some((option, confidence)));
        let (body, rows, _) = second_message_after(&jev, 1).await;
        assert_eq!(
            rows[0]["extended"], false,
            "{option} {confidence}: {rows:?}"
        );
        for part in [OPEN_A, REPLY] {
            assert!(
                !body.contains(part),
                "{option} {confidence}: {part:?} rode on: {body}"
            );
        }
    }
}

/// Neither the turn nor its send waits for the reply's judgment: Jev holds
/// the answer, and the turn has returned, its reply written, with no row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_reply_question_never_delays_a_turn() {
    let jev = scripted(None);
    let (r, _zai, _dir) = rig(&jev);
    let sid = dm(&r.core);
    let done =
        tokio::time::timeout(Duration::from_secs(10), say(&r.core, &sid, OPEN_A, None)).await;
    let res = done.expect("the turn waited for the reply's judgment");
    assert!(res.output.contains(REPLY), "{}", res.output);
    assert!(rows(&r.core).is_empty());
    jev.release();
}

/// A session with no open run to continue asks nothing: a private first
/// message is not admitted, so the reply to it is never queued.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_without_a_run_asks_nothing() {
    let jev = scripted(Some(("open", 0.95)));
    let (r, _zai, _dir) = rig(&jev);
    let sid = dm(&r.core);
    say(&r.core, &sid, crate::tests_open_request::PRIVATE, None).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(rows(&r.core).is_empty(), "{:?}", rows(&r.core));
    let asked = jev
        .seen()
        .iter()
        .any(|s| s.body.to_string().contains(REPLY));
    assert!(!asked, "Jev was asked about the reply");
}

fn ask(expect: Option<OpenRun>) -> ReplyAsk {
    ReplyAsk {
        session: "ses_x".into(),
        node: "nod_reply".into(),
        text: "a reply".into(),
        base: OpenRun {
            from: "nod_first".into(),
            through: "nod_input".into(),
        },
        expect,
        names: Default::default(),
    }
}

fn run(from: &str, through: &str) -> OpenRun {
    OpenRun {
        from: from.into(),
        through: through.into(),
    }
}

/// What may extend a run, and nothing else, as a table: only `open` at the
/// act band, with the stored run unchanged and no taint.
#[test]
fn only_an_open_act_answer_onto_the_same_run_lands() {
    let stored = DataState::new(None, Some(run("nod_first", "nod_input")), None);
    let a = ask(stored.run.clone());
    let got = lands(Some(&stored), &a, Some(&answer("open", Band::Act)));
    assert_eq!(got, Some(run("nod_first", "nod_reply")));
    for (choice, band) in [
        ("private", Band::Act),
        ("unclear", Band::Act),
        ("open", Band::Confirm),
        ("open", Band::Escalate),
    ] {
        assert_eq!(
            lands(Some(&stored), &a, Some(&answer(choice, band))),
            None,
            "{choice} {band:?}"
        );
    }
    assert_eq!(lands(Some(&stored), &a, None), None, "no answer");
    let tainted = DataState::new(
        None,
        stored.run,
        Some(Taint {
            why: crate::data::DataWhy::VerdictPrivate,
            node: None,
            at_ms: 0,
        }),
    );
    assert_eq!(
        lands(Some(&tainted), &a, Some(&answer("open", Band::Act))),
        None,
        "tainted"
    );
}

/// The run moved between the ask and the answer: the write leaves it alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_that_moved_is_not_overwritten() {
    let jev = scripted(Some(("open", 0.95)));
    let (r, _zai, _dir) = rig(&jev);
    let sid = dm(&r.core);
    let moved = run("nod_other", "nod_later");
    r.core
        .store
        .update_session(&sid, |rec| {
            rec.data = Some(Box::new(DataState::new(None, Some(moved.clone()), None)));
            Ok(vec![])
        })
        .unwrap();
    let mut a = ask(Some(run("nod_first", "nod_input")));
    a.session = sid.clone();
    r.core
        .runner
        .judge
        .land_reply(&a, Some(&answer("open", Band::Act)), Some(3));
    assert_eq!(record(&r.core, &sid).data.unwrap().run, Some(moved));
    let rows = rows(&r.core);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["extended"], false);
    // The same answer onto the run it was asked about does land.
    let mut b = ask(Some(run("nod_other", "nod_later")));
    b.session = sid.clone();
    r.core
        .runner
        .judge
        .land_reply(&b, Some(&answer("open", Band::Act)), None);
    assert_eq!(
        record(&r.core, &sid).data.unwrap().run,
        Some(run("nod_first", "nod_reply"))
    );
}
