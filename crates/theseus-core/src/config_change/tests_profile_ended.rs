//! What a turn that ends badly does with the answer another profile gave it (issue 28), and what
//! the owner's yes leaves behind: the answer was produced and paid for, so it is posted for a turn
//! that then failed, marked as such; a card held past its life does nothing and says so; an untaken
//! yes does not outlive the turn that should have taken it.

use serde_json::json;

use super::tests::{rig, Rig};
use super::tests_answer::owner_in_dm;
use crate::chat_acts::Said;
use crate::content::Flag;
use crate::node::ResultStatus;
use crate::profile_tool::HOLD;

const HELLO: &str = "Hello from the other model, before the turn failed.";
const PLACE: &str = "dm:4100000000000000001";

fn posts(r: &Rig) -> Vec<(String, serde_json::Value)> {
    r.core
        .outbox
        .open_for(&format!("discord:{PLACE}"))
        .iter()
        .map(|a| {
            (
                crate::outbox::kind_of(a).to_string(),
                crate::outbox::body_of(a).clone(),
            )
        })
        .collect()
}

/// A session at the owner's DM that has read text from outside, so both tools ask first.
fn steered(r: &Rig) -> String {
    let sid = r.session_at(PLACE);
    crate::test_support::label_session(&r.core, &sid, Flag::Injection);
    sid
}

fn the_owner_speaks(r: &Rig, sid: &str) {
    r.core.runner.tools.chat_acts.heard.stage(
        sid,
        std::time::Instant::now(),
        Said {
            text: "stay on haiku".into(),
            who: crate::test_support::owner_cli(),
            owner_input: true,
        },
    );
}

/// A turn that asks a profile, gets its answer, and then faults on its own next call: the result
/// said Theseus shows the answer, so it does, once, marked as the answer of a turn that failed;
/// nothing is kept in memory after it.
#[tokio::test]
async fn a_turn_that_fails_after_an_ask_still_shows_the_answer_marked_as_failed() {
    let r = rig();
    r.answers_plain(HELLO);
    r.the_turn_fails_after_a_result();
    let sid = r.session_at(PLACE);
    // The turn's own provider call fails after the tool result: the turn ends in an error.
    let ended = r
        .try_turn_with(
            "profile_ask",
            &sid,
            json!({"profile": "haiku", "question": "Say hello."}),
        )
        .await;
    assert!(ended.is_err(), "the turn failed: {ended:?}");
    let said: Vec<(String, serde_json::Value)> = posts(&r)
        .into_iter()
        .filter(|(k, _)| k == "subagent")
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0].1["answer"]["text"], json!(HELLO));
    assert_eq!(said[0].1["turn_ended"], json!("failed"));
    assert_eq!(r.core.runner.tools.profile.held_answers(), 0);
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("shows it to the owner"), "{text}");
}

/// A held card past its life does nothing and says so: the question stays (it is told about, never
/// declined), the hold is not done, one quiet notice is counted for the life, and the owner's yes
/// afterwards still acts.
#[tokio::test]
async fn a_hold_card_held_past_its_life_does_nothing_and_says_so_and_still_takes_a_yes() {
    let r = rig();
    let sid = steered(&r);
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = r.asked().remove(0);
    assert_eq!(q.tool, HOLD);
    let ttl_ms = r.core.cfg.kernel.confirm_ttl_secs * 1000;
    let late = r.core.kernel.now_ms() + 3 * ttl_ms;
    assert_eq!(
        r.core.expire_questions(late),
        0,
        "a call's question is held, not expired"
    );
    assert_eq!(r.asked().len(), 1, "the card still waits");
    assert_eq!(r.core.runner.sticky_profile(&sid), None, "nothing was held");
    assert!(r.rows("session.profile_set").is_empty());
    let told: Option<serde_json::Value> = r
        .core
        .store
        .get_meta(&format!("question.noticed.{}", q.correlation_id))
        .unwrap();
    assert!(told.is_some(), "the wait was told, once, for its life");
    // The yes after the wait is still the owner's word for that hold.
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    r.try_continue(&sid).await.unwrap();
    assert_eq!(r.core.runner.sticky_profile(&sid).as_deref(), Some("haiku"));
}

/// A yes that no call took is dropped when the turn that arrived after it ends, and a yes kept
/// while a turn ran is left for the next one.
#[test]
fn an_untaken_yes_is_dropped_with_the_turn_that_should_have_taken_it() {
    let board = crate::profile_tool::Board::default();
    let who = crate::test_support::owner_cli();
    let before = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(3));
    board.keep_yes("act_a", "ses_a", "exe_a", &who);
    board.keep_yes("act_b", "ses_b", "exe_b", &who);
    // Their confirms are bound.
    board.bind_yes("act_a");
    board.bind_yes("act_b");
    std::thread::sleep(std::time::Duration::from_millis(3));
    let after = std::time::Instant::now();
    // A turn that arrived before the yes was kept (the yes came while it ran): the yes stays.
    board.drop_untaken("ses_a", before);
    assert_eq!(board.held_yes(), 2);
    // A turn that ended before its resume ran (it erred first) takes nothing and drops nothing:
    // the next turn takes the yes.
    board.sweep("ses_a");
    assert_eq!(board.held_yes(), 2);
    // A turn that arrived after it has resumed: its session's yes goes, another session's stays.
    board.drop_untaken("ses_a", after);
    assert_eq!(board.held_yes(), 1);
    assert!(board.take_yes("act_a").is_none());
    assert!(board.take_yes("act_b").is_some());
    // A yes whose confirm is not bound yet (the owner's answer is mid-write) is not dropped by a
    // turn that arrived after it: that turn may not have seen the confirm.
    board.keep_yes("act_e", "ses_e", "exe_e", &who);
    std::thread::sleep(std::time::Duration::from_millis(3));
    board.drop_untaken("ses_e", std::time::Instant::now());
    assert_eq!(board.held_yes(), 1, "unbound: kept");
    board.bind_yes("act_e");
    board.drop_untaken("ses_e", std::time::Instant::now());
    assert!(
        board.take_yes("act_e").is_none(),
        "bound and untaken: dropped"
    );
    // A stop of an execution drops what was kept for it.
    board.keep_yes("act_c", "ses_c", "exe_c", &who);
    board.keep_yes("act_d", "ses_c", "exe_d", &who);
    board.drop_yes_of("exe_c");
    assert!(board.take_yes("act_c").is_none());
    assert!(board.take_yes("act_d").is_some());
}

/// The same through a played turn: the owner says yes, the execution is stopped, and the
/// continuation that ends takes nothing: nothing is kept.
#[tokio::test]
async fn a_yes_the_continuation_never_took_is_not_kept() {
    let r = rig();
    let sid = steered(&r);
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = r.asked().remove(0);
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    assert_eq!(r.core.runner.tools.profile.held_yes(), 1);
    r.core.runner.stop_landed(&q.execution_id, "tester");
    r.core
        .kernel
        .stop_execution(&q.execution_id, "tester")
        .unwrap();
    let _ = r.try_continue(&sid).await;
    assert_eq!(r.core.runner.tools.profile.held_yes(), 0);
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
}

/// A stop notice that landed in the window before a turn that went on to end well marks nothing on
/// its answer.
#[tokio::test]
async fn a_stop_notice_before_a_turn_that_ends_well_marks_nothing() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session_at(PLACE);
    let warm = r.run_turn(&sid).await;
    let exec = warm.execution_id.clone().expect("an execution");
    let core = r.core.clone();
    // The notice lands during the call; the kernel's stop does not: the turn goes on and ends well.
    r.when_the_profile_is_asked(move || {
        core.runner.stop_landed(&exec, "tester");
    });
    let ended = r
        .try_turn_with(
            "profile_ask",
            &sid,
            json!({"profile": "haiku", "question": "Say hello."}),
        )
        .await
        .unwrap();
    assert_ne!(ended.stop_reason, "stopped");
    let said: Vec<serde_json::Value> = posts(&r)
        .into_iter()
        .filter(|(k, _)| k == "subagent")
        .map(|(_, b)| b)
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].get("turn_ended").is_none(), "{said:?}");
}

/// The record of how the turn ended, read where the surfaces read it: the node list's detail.
#[tokio::test]
async fn the_node_list_says_how_the_turn_that_asked_ended() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session_at(PLACE);
    let warm = r.run_turn(&sid).await;
    let exec = warm.execution_id.clone().expect("an execution");
    let core = r.core.clone();
    r.when_the_profile_is_asked(move || {
        core.runner.stop_landed(&exec, "tester");
        core.kernel.stop_execution(&exec, "tester").unwrap();
    });
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say hello."}),
    )
    .await;
    let answer = || {
        r.core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .find(|(_, n)| {
                matches!(&n.body, crate::node::Body::ToolResult { meta, .. } if meta.get("subagent").is_some())
            })
            .map(|(pos, n)| r.core.marked_answer(crate::Core::node_info(pos, &n)))
            .expect("the answer's result node")
    };
    assert_eq!(answer().detail["subagent_ended"], json!("stopped"));
}

/// An answer of a turn that failed goes where the turn's reply would have gone (`answer_at`), marked
/// failed, and is recorded as such; the session's own place gets none of it.
#[tokio::test]
async fn a_failed_turns_answer_goes_where_its_reply_would_have_gone() {
    const ELSEWHERE: &str = "dm:4100000000000000002";
    let r = rig();
    r.answers_plain(HELLO);
    r.the_turn_fails_after_a_result();
    let sid = r.session_at(PLACE);
    r.core.outbox.bind_place(ELSEWHERE, &r.session()).unwrap();
    r.call_next(
        "profile_ask",
        json!({"profile": "haiku", "question": "Say hello."}),
    );
    let ended = r
        .try_turn_answering_at(&sid, &format!("discord:{ELSEWHERE}"))
        .await;
    assert!(ended.is_err(), "{ended:?}");
    let at = |place: &str| -> Vec<serde_json::Value> {
        r.core
            .outbox
            .open_for(&format!("discord:{place}"))
            .iter()
            .filter(|a| crate::outbox::kind_of(a) == "subagent")
            .map(|a| crate::outbox::body_of(a).clone())
            .collect()
    };
    let there = at(ELSEWHERE);
    assert_eq!(there.len(), 1, "{there:?}");
    assert_eq!(there[0]["turn_ended"], json!("failed"));
    assert!(
        at(PLACE).is_empty(),
        "the session's own place: {:?}",
        at(PLACE)
    );
    let call = there[0]["answer"]["call"].as_str().unwrap();
    let ended: Option<String> = r
        .core
        .store
        .get_meta(&crate::profile_tool::ended_key(call))
        .unwrap();
    assert_eq!(ended.as_deref(), Some("failed"));
}

/// A yes kept until its confirm is bound goes with its guard when the guard does: an error, an early
/// return, and a panic unwinding through the frame all drop it, where binding keeps it. (The
/// confirm's frame cannot be made to unwind from a test, so the guard is tested on its own; the
/// played hold tests show the bound yes is taken.)
#[test]
fn a_kept_yes_goes_with_its_guard_unless_bound_even_when_a_panic_unwinds() {
    let board = crate::profile_tool::Board::default();
    let who = crate::test_support::owner_cli();
    // Let go: gone.
    drop(board.keep_yes_until_bound("act_a", "ses_a", "exe_a", &who));
    assert!(board.take_yes("act_a").is_none());
    // A panic unwinds through the frame while the guard is held: gone.
    let fell = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = board.keep_yes_until_bound("act_b", "ses_a", "exe_a", &who);
        assert_eq!(board.held_yes(), 1, "kept while the frame runs");
        panic!("the frame fell over");
    }));
    assert!(fell.is_err());
    assert_eq!(board.held_yes(), 0, "an unwind drops the yes");
    // Bound: it stays, and a panic after the bind leaves it, since the confirm landed.
    let fell = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        board
            .keep_yes_until_bound("act_c", "ses_a", "exe_a", &who)
            .bind();
        panic!("something after the bind fell over");
    }));
    assert!(fell.is_err());
    assert!(board.take_yes("act_c").is_some());
}
