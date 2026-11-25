//! `profile.ask` and `profile.hold` after the owner's yes (issue 28): when a turn has read
//! outside text each is asked first, and what the owner's yes runs is the continuation's call.
//! The answer is still posted to the conversation, and the hold still acts, on that yes and no
//! other word; a no, a stop, another's answer or an agent's process does nothing and says so.

use serde_json::json;
use theseus_protocol::ConfirmRequest;

use super::tests::{rig_with, Rig};
use super::tests_answer::owner_in_dm;
use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::chat_acts::Said;
use crate::content::Flag;
use crate::node::{Body, ResultStatus};
use crate::profile_tool::{ASK, HOLD, RUN};
use crate::session::SessionRecord;
use crate::turn::TurnRequest;

/// These cards are the hold's at `[content_based_security] outside = "ask"`: at the default the
/// calls run and the owner is told (`tests_quiet`).
fn rig() -> Rig {
    rig_with(|c, _| c.content_based_security.outside = crate::content::OutsideMode::Ask)
}

/// What the asked model says.
const HELLO: &str = "Hello from the other model, after the yes.";
/// The owner's place: a private DM.
const PLACE: &str = "dm:4100000000000000001";

/// A session at the owner's DM that has read text from outside, so both tools ask first.
fn steered(r: &Rig) -> String {
    let sid = r.session_at(PLACE);
    crate::test_support::label_session(&r.core, &sid, Flag::Injection);
    sid
}

/// The owner's own typed message started the turn that follows.
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

/// The turn the driver runs after an answer: no input, the call that was waiting.
async fn continue_turn(r: &Rig, sid: &str) {
    try_continue(r, sid).await.unwrap();
}

/// [`continue_turn`], for a case where the driver may have no turn to run (the call still waits,
/// or a stop ended the execution).
async fn try_continue(r: &Rig, sid: &str) -> anyhow::Result<()> {
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    r.core
        .runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: None,
            target,
            sink: EventSink::new(r.core.bus.clone(), sid, None),
            author: "test".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .map(|_| ())
}

/// The one card the turn asked, for `tool`.
fn card(r: &Rig, tool: &str) -> ConfirmRequest {
    let mut asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].tool, tool);
    asks.remove(0)
}

/// The kinds of the posts the place was written, in order.
fn kinds(r: &Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(&format!("discord:{PLACE}"))
        .iter()
        .map(|a| crate::outbox::kind_of(a).to_string())
        .collect()
}

/// The result of the turn's first call.
fn result_of_first(r: &Rig, sid: &str) -> (ResultStatus, String) {
    r.result(sid)
}

/// Asked first, then the owner's yes: the question is asked in the continuation, and the answer
/// is a message of its own, posted once, before that turn's reply.
#[tokio::test]
async fn rv_an_asked_first_profile_answer_is_posted_after_the_yes() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = steered(&r);
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say hello."}),
    )
    .await;
    let q = card(&r, ASK);
    assert!(
        r.asked_plain().is_empty(),
        "nothing was asked before the yes"
    );
    let done = r
        .core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    assert!(done.approved && done.resumes);
    continue_turn(&r, &sid).await;
    assert_eq!(r.asked_plain().len(), 1, "asked once, after the yes");
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains(HELLO), "{text}");
    let kinds = kinds(&r);
    assert_eq!(
        kinds.iter().filter(|k| *k == "subagent").count(),
        1,
        "the answer is posted, once: {kinds:?}"
    );
    let at = kinds.iter().position(|k| k == "subagent").unwrap();
    assert_eq!(
        kinds.last().map(String::as_str),
        Some("reply"),
        "the turn's reply is the last post: {kinds:?}"
    );
    assert!(at < kinds.len() - 1, "before the reply: {kinds:?}");
    assert_eq!(
        r.core.runner.tools.profile.held_answers(),
        0,
        "nothing is kept past the turn"
    );
}

/// The result does not say the answer was shown when the profile had no words to show.
#[tokio::test]
async fn an_answer_with_no_words_is_not_said_to_have_been_shown() {
    let r = rig();
    r.answers_plain("   ");
    let sid = r.session_at(PLACE);
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say nothing."}),
    )
    .await;
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("[no words"), "{text}");
    assert!(
        !text.contains("shown it") && !text.contains("shows it"),
        "{text}"
    );
    assert!(
        !kinds(&r).contains(&"subagent".to_string()),
        "{:?}",
        kinds(&r)
    );
}

/// A turn that was stopped, or that ends in a fault, keeps no answer in memory.
#[tokio::test]
async fn a_stopped_turns_answer_is_dropped_not_kept_until_restart() {
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
    assert_eq!(r.asked_plain().len(), 1, "the call was made");
    assert_eq!(r.core.runner.tools.profile.held_answers(), 0);
}

/// Asked first, then the owner's yes: the hold acts in the continuation, on the profile the card
/// named, for this conversation alone, and its row names who pressed.
#[tokio::test]
async fn rv_an_asked_first_hold_acts_after_the_yes() {
    let r = rig();
    let sid = steered(&r);
    let other = r.session();
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = card(&r, HOLD);
    assert_eq!(
        r.core.runner.sticky_profile(&sid),
        None,
        "nothing before the yes"
    );
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    continue_turn(&r, &sid).await;
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("next turns run on haiku"), "{text}");
    assert_eq!(r.core.runner.sticky_profile(&sid).as_deref(), Some("haiku"));
    assert_eq!(
        r.core.runner.sticky_profile(&other),
        None,
        "this conversation only"
    );
    let sets = r.rows("session.profile_set");
    assert_eq!(sets.len(), 1, "once: {sets:?}");
    assert_eq!(sets[0]["to"], json!("haiku"));
    assert_eq!(sets[0]["by"], owner_in_dm().label.as_str());
    // The yes was one hold's: it is spent.
    assert!(r
        .core
        .runner
        .tools
        .profile
        .take_yes(&q.correlation_id)
        .is_none());
    // The yes made the continuation no offer: it is still offered nothing to read the owner's
    // word from, as every other act reads it.
    let turn = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult { .. } => n.turn_id,
            _ => None,
        })
        .unwrap();
    assert!(r.core.runner.tools.chat_acts.offered(&sid, &turn).is_none());
}

/// A no does nothing, and says so.
#[tokio::test]
async fn a_no_on_the_hold_card_holds_nothing() {
    let r = rig();
    let sid = steered(&r);
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = card(&r, HOLD);
    r.core
        .confirm_action(&q.correlation_id, false, None, owner_in_dm())
        .unwrap();
    continue_turn(&r, &sid).await;
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Declined, "{text}");
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
    assert!(r.rows("session.profile_set").is_empty());
}

/// A stop between the yes and the call does nothing.
#[tokio::test]
async fn a_stop_after_the_yes_holds_nothing() {
    let r = rig();
    let sid = steered(&r);
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = card(&r, HOLD);
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    r.core.runner.stop_landed(&q.execution_id, "tester");
    r.core
        .kernel
        .stop_execution(&q.execution_id, "tester")
        .unwrap();
    let _ = try_continue(&r, &sid).await;
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
    assert!(r.rows("session.profile_set").is_empty());
}

/// A card answered by anyone but the owner, by an agent's process, or by the model's own tool
/// from words, is not answered: it keeps waiting and nothing is held.
#[tokio::test]
async fn a_hold_card_answered_by_anyone_else_is_not_answered() {
    let r = rig();
    let sid = steered(&r);
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = card(&r, HOLD);
    let agent = Answerer {
        label: "agent:claude".into(),
        surface: Surface::Cli,
        discord: None,
        agent: Some("claude".into()),
    };
    for who in [crate::test_support::stranger(), agent] {
        let label = who.label.clone();
        let e = r
            .core
            .confirm_action(&q.correlation_id, true, None, who)
            .expect_err(&label);
        assert!(!format!("{e:#}").is_empty(), "{label}");
    }
    // The model's own answer tool speaks for no one.
    let e = r
        .core
        .answer_plain_act(&q, "yes", None, owner_in_dm())
        .unwrap_err();
    assert!(e.contains("own button"), "{e}");
    assert_eq!(r.asked().len(), 1, "the card still waits");
    assert!(r
        .core
        .runner
        .tools
        .profile
        .take_yes(&q.correlation_id)
        .is_none());
    // The card still waits, so the driver has no continuation to run.
    let _ = try_continue(&r, &sid).await;
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
    assert!(r.rows("session.profile_set").is_empty());
}

/// The yes the act reads is judged again there: one kept for an agent's process is refused,
/// though the owner's own press was what the confirm took.
#[tokio::test]
async fn an_agents_yes_kept_for_a_hold_is_refused_at_the_act() {
    let r = rig();
    let sid = steered(&r);
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = card(&r, HOLD);
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    let agent = Answerer {
        label: "agent:claude".into(),
        surface: Surface::Cli,
        discord: None,
        agent: Some("claude".into()),
    };
    r.core
        .runner
        .tools
        .profile
        .keep_yes(&q.correlation_id, &sid, &q.execution_id, &agent);
    continue_turn(&r, &sid).await;
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
}

/// The wall is judged again when the approved call runs: a session that became a task between the
/// card and the yes is refused, though the owner pressed.
#[tokio::test]
async fn the_wall_is_checked_again_when_the_approved_hold_runs() {
    let r = rig();
    let sid = steered(&r);
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let q = card(&r, HOLD);
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    let parent = r.session();
    r.core
        .store
        .update_session(&sid, |rec: &mut SessionRecord| {
            rec.task = Some(crate::session::TaskOf {
                parent_session: parent.clone(),
                parent_execution: "exe_x".into(),
                by: "tool_x".into(),
                target: None,
                arrangement: None,
                check: None,
                notify_done: false,
            });
            Ok(vec![])
        })
        .unwrap();
    continue_turn(&r, &sid).await;
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("not for a task"), "{text}");
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
}

/// A hold in a turn with no card and no owner message is refused as it was.
#[tokio::test]
async fn a_hold_with_no_card_and_no_owner_message_is_still_refused() {
    let r = rig();
    let sid = r.session();
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("acts only in a turn the owner's own message started"),
        "{text}"
    );
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
}

/// Handing work to a profile after outside text is asked first too: nothing opens before the
/// yes, and after it the task opens in the continuation, on the profile, with tools, and its
/// report speaks in the conversation once, before that turn's reply.
#[tokio::test]
async fn a_hand_over_asked_first_opens_the_task_after_the_yes_and_its_report_speaks() {
    let r = rig();
    r.agents_report("Listed the folder: a.txt, b.txt", None);
    let sid = steered(&r);
    r.steps(
        &sid,
        vec![(
            "profile_run",
            json!({"profile": "haiku", "work": "AGENT: list the files of the folder"}),
        )],
    )
    .await;
    let q = card(&r, RUN);
    assert!(
        r.seen().iter().all(|(m, _)| m != "claude-haiku-5-5"),
        "nothing ran before the yes: {:?}",
        r.seen()
    );
    let done = r
        .core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    assert!(done.approved && done.resumes);
    continue_turn(&r, &sid).await;
    let (status, text) = result_of_first(&r, &sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("Handed the work to haiku"), "{text}");
    assert!(
        r.seen()
            .iter()
            .any(|(m, tools)| m == "claude-haiku-5-5" && *tools > 0),
        "the agent has tools: {:?}",
        r.seen()
    );
    let kinds = kinds(&r);
    assert_eq!(
        kinds.iter().filter(|k| *k == "subagent").count(),
        1,
        "the report is posted, once: {kinds:?}"
    );
    assert_eq!(kinds.last().map(String::as_str), Some("reply"), "{kinds:?}");
}
