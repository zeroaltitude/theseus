//! `profile.run`'s limits played through the whole core (issue 28), the second pass: the floor a
//! task has for one call of its model, a card the agent raises, the day's ceiling, a restart
//! before the agent ends, and each caller the wall refuses. What the first pass proves is in
//! `tests_profile_run.rs`.

use serde_json::json;
use theseus_kernel::Authority;

use super::tests::{rig, rig_with, Rig, OWNER};
use super::tests_answer::owner_in_dm;
use crate::node::ResultStatus;
use crate::policy::Posture;
use crate::session::SessionRecord;

/// The owner's place: a private DM.
const PLACE: &str = "dm:4100000000000000001";
const REPORT: &str = "Listed the folder: a.txt, b.txt";
const WORK: &str = "AGENT: list the files of the folder and report them";

fn work(profile: &str) -> serde_json::Value {
    json!({"profile": profile, "work": WORK})
}

/// The posts of the owner's place, in order: kind and body.
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

fn subagent_posts(r: &Rig) -> Vec<serde_json::Value> {
    posts(r)
        .into_iter()
        .filter(|(k, _)| k == "subagent")
        .map(|(_, b)| b)
        .collect()
}

fn tasks(r: &Rig, sid: &str) -> Vec<theseus_kernel::Execution> {
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    r.core.kernel.tasks(rec.execution_id.as_deref()).unwrap()
}

/// Whether a request was ever made to the model `m`.
fn ran(r: &Rig, m: &str) -> bool {
    r.seen().iter().any(|(model, _)| model == m)
}

/// Wait, on the paused clock, for the owner's place to hold `n` subagent messages.
async fn wait_for_posts(r: &Rig, n: usize) -> Vec<serde_json::Value> {
    for _ in 0..400 {
        let got = subagent_posts(r);
        if got.len() >= n {
            return got;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    subagent_posts(r)
}

/// A conversation at the owner's place whose execution acts for `principal`, as a routine's, the
/// self loop's or a connected client's does.
fn as_principal(r: &Rig, principal: &str) -> String {
    let sid = r.session_at(PLACE);
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let e = r
        .core
        .kernel
        .open_execution(
            &sid,
            rec.kind,
            Authority {
                principal: principal.into(),
                ..Default::default()
            },
            None,
            None,
        )
        .unwrap();
    r.core
        .store
        .update_session(&sid, |rec: &mut SessionRecord| {
            rec.execution_id = Some(e.id.clone());
            Ok(vec![])
        })
        .unwrap();
    sid
}

/// A conversation with so little left that a task's default share of it cannot pay for one call of
/// the asked profile's model is refused up front, in words that say what is left and what one
/// call needs, by the same floor `task.fan_out` keeps; no task is opened and no model is asked.
#[tokio::test]
async fn a_carve_too_small_for_the_asked_model_is_refused_before_a_task_opens() {
    let r = rig_with(|cfg, _| cfg.kernel.spend_limit_usd = 0.2);
    r.agents_report(REPORT, None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("opus"))]).await;
    let (status, text) = r.result(&sid);
    let floor = crate::task_fan_out::call_floor(&r.core, "claude-opus-5-5");
    assert!(
        floor > 50_000,
        "the rig's limit must leave a share under one call's floor: {floor}"
    );
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("one call of opus's model (claude-opus-5-5) needs at least")
            && text.contains(&theseus_kernel::usd(floor)),
        "{text}"
    );
    assert!(text.contains("Nothing was started"), "{text}");
    assert!(tasks(&r, &sid).is_empty(), "no task opened");
    assert!(!ran(&r, "claude-opus-5-5"), "no model was asked");
}

/// A card the handed task raises (an approval for a program) is the owner's alone. The asking turn
/// was offered only what waited when it began, so its model's answer tool cannot say yes to a card
/// that came after; the card keeps waiting, the agent stays parked, and the owner's own press is
/// what lets it go on and report.
#[tokio::test(start_paused = true)]
async fn a_card_the_agent_raises_is_answered_by_the_owners_press_alone() {
    let r = rig_with(|cfg, _| {
        cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
    });
    r.agents_report(REPORT, None);
    r.agents_call("proc_run", json!({"command": "true"}));
    let sid = r.session_at(PLACE);
    r.steps(
        &sid,
        vec![
            ("profile_run", work("haiku")),
            ("chat_answer_question", json!({"answer": "yes"})),
        ],
    )
    .await;
    let (_, handed) = r.result_of(&sid, "p1");
    assert!(
        handed.contains("still working"),
        "the agent waits on its card: {handed}"
    );
    // The asking model tried to say yes for it: refused.
    let (status, said) = r.result_of(&sid, "p2");
    assert_ne!(status, ResultStatus::Ok, "{said}");
    let waiting = r.asked();
    assert_eq!(waiting.len(), 1, "the card still waits: {waiting:?}");
    assert_eq!(waiting[0].tool, "proc.run");
    assert!(
        waiting[0].session_id != sid,
        "the card is the agent's own task's"
    );
    assert!(subagent_posts(&r).is_empty(), "the agent has not reported");
    // The owner's press answers it, and the agent goes on and reports.
    let q = waiting[0].clone();
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
    let said = wait_for_posts(&r, 1).await;
    assert_eq!(said.len(), 1, "{:?}", posts(&r));
    assert_eq!(said[0]["answer"]["text"], json!(REPORT));
    assert!(r.asked().is_empty());
}

/// The agent's calls are the day's: what a hand-over spends is booked in the day's total.
#[tokio::test]
async fn what_a_hand_over_spends_is_counted_in_the_days_total() {
    let r = rig();
    r.agents_report(REPORT, None);
    let sid = r.session_at(PLACE);
    let c = r.core.kernel.day_ceiling().clone();
    let before = c.today(c.now()).spent;
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let ts = tasks(&r, &sid);
    assert!(
        ts[0].budget.spent_micros > 0,
        "the agent's calls were priced"
    );
    let spent = c.today(c.now()).spent;
    assert!(
        spent - before >= ts[0].budget.spent_micros,
        "the day counts the agent's {} of its {}",
        ts[0].budget.spent_micros,
        spent - before
    );
}

/// A hand-over that would cross the day's ceiling is refused as a task's own call is: the agent's
/// first call is not made, the day's row says it was a task's, the owner's place hears the agent
/// did not finish (as the agent's own message), and no model of the profile was asked. Under the
/// ceiling's stop mode that is the ceiling; under notify, the default, the runaway line.
#[tokio::test(start_paused = true)]
async fn a_hand_over_that_crosses_the_days_ceiling_is_refused() {
    use crate::config::DailyCeilingMode::{Notify, Stop};
    for mode in [Stop, Notify] {
        let r = rig_with(move |cfg, _| {
            cfg.kernel.daily_spend_ceiling_usd = 100.0;
            cfg.kernel.daily_spend_ceiling_mode = mode;
        });
        r.agents_report(REPORT, None);
        let sid = r.session_at(PLACE);
        let c = r.core.kernel.day_ceiling().clone();
        r.when_the_turn_asks_the_model(move || {
            // The day's spend passes the line between the turn's first call and the agent's.
            c.book(c.now(), theseus_kernel::usd_to_micros(5_000.0));
        });
        let _ = r
            .try_steps(&sid, vec![("profile_run", work("haiku"))])
            .await;
        assert!(
            !ran(&r, "claude-haiku-5-5"),
            "{mode:?}: the agent's call was never made"
        );
        let rows = r.rows("spend.ceiling");
        assert!(
            rows.iter().any(|d| d["what"] == json!("task")),
            "{mode:?}: the refusal is a task's: {rows:?}"
        );
        let said = wait_for_posts(&r, 1).await;
        assert_eq!(said.len(), 1, "{mode:?}: {:?}", posts(&r));
        let text = said[0]["answer"]["text"].as_str().unwrap();
        assert!(
            text.starts_with("The agent did not finish"),
            "{mode:?}: {text}"
        );
    }
}

/// The daemon stops after the task opened and before the agent ran, and starts again on the same
/// store: the late report still speaks in the conversation, once, from the model that ran.
#[tokio::test(start_paused = true)]
async fn a_restart_before_the_agent_ends_still_speaks_in_the_conversation_once() {
    let r = rig();
    // No driver: the agent has not taken a turn when the daemon stops.
    r.agents_say(REPORT, None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let (_, text) = r.result(&sid);
    assert!(text.contains("still working"), "{text}");
    assert!(subagent_posts(&r).is_empty());
    let r = r.restart(|_, _| {});
    assert!(!ran(&r, "claude-haiku-5-5"), "nothing ran yet");
    r.drive();
    let said = wait_for_posts(&r, 1).await;
    assert_eq!(said.len(), 1, "{:?}", posts(&r));
    assert_eq!(said[0]["answer"]["text"], json!(REPORT));
    assert_eq!(said[0]["answer"]["model"], json!("claude-haiku-5-5"));
    assert_eq!(said[0]["answer"]["asked_by"], json!("sonnet"));
    // Give the driver time to do anything else it would: still one message.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(subagent_posts(&r).len(), 1);
}

/// The message a hand-over's end stages is the one a question's answer is: the same kind and the
/// same fields, so every place that draws the one draws the other (the icon, the tables, the footer).
#[tokio::test]
async fn a_hand_overs_report_is_the_post_a_questions_answer_is() {
    let r = rig();
    r.agents_report(REPORT, None);
    r.answers_plain("A question's answer.");
    let sid = r.session_at(PLACE);
    r.steps(
        &sid,
        vec![
            ("profile_run", work("haiku")),
            (
                "profile_ask",
                json!({"profile": "opus", "question": "Say hello."}),
            ),
        ],
    )
    .await;
    let said = subagent_posts(&r);
    assert_eq!(said.len(), 2, "{:?}", posts(&r));
    let keys = |b: &serde_json::Value| {
        let mut k: Vec<String> = b.as_object().unwrap().keys().cloned().collect();
        k.sort();
        let mut a: Vec<String> = b["answer"].as_object().unwrap().keys().cloned().collect();
        a.sort();
        (k, a)
    };
    assert_eq!(keys(&said[0]), keys(&said[1]), "{said:?}");
    // The result node carries the model that ran, for `theseus why`.
    let (_, _) = r.result_of(&sid, "p1");
    let node = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult { meta, .. } if meta.get("handed").is_some() => {
                Some(meta["handed"].clone())
            }
            _ => None,
        })
        .expect("the result names who was handed the work");
    assert_eq!(node["profile"], json!("haiku"));
    assert_eq!(node["model"], json!("claude-haiku-5-5"));
}

/// Another person's direct message and a channel the guild writes in are shared places: refused in
/// the tool's own words, and nothing is started.
#[tokio::test]
async fn a_shared_place_and_a_guild_participant_are_refused_and_start_nothing() {
    use crate::places::BoundPlace;
    let r = rig();
    r.agents_report(REPORT, None);
    let shared = r.session_at(&format!("dm:{}", OWNER + 1));
    r.steps(&shared, vec![("profile_run", work("haiku"))]).await;
    let (status, text) = r.result(&shared);
    assert_ne!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("profile.run is not offered in a shared place"),
        "{text}"
    );
    // A channel bound private is still the guild's: whoever it lets in writes there.
    r.core.runner.place_rule.bind(vec![BoundPlace {
        target: "discord:channel:314159265358979323".into(),
        name: "#lab".into(),
        private: true,
        ..Default::default()
    }]);
    let guild = r.session_at("channel:314159265358979323");
    r.steps(&guild, vec![("profile_run", work("haiku"))]).await;
    let (status, text) = r.result(&guild);
    assert_ne!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("profile.run") && text.contains("shared"),
        "{text}"
    );
    for s in [&shared, &guild] {
        assert!(tasks(&r, s).is_empty());
    }
    assert!(!ran(&r, "claude-haiku-5-5"));
}

/// The first tool result of a session, when it has one.
fn first_result(r: &Rig, sid: &str) -> Option<(ResultStatus, String)> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            crate::node::Body::ToolResult {
                status, content, ..
            } => Some((status, content)),
            _ => None,
        })
}

/// A conversation whose execution acts for a routine, the self-improvement loop, a connected client
/// or a job never starts an agent: where the turn runs at all, the tool's wall refuses it in the
/// tool's own words; where the turn is refused before it can call anything, that is the refusal.
/// Either way nothing opens and no model of the profile is asked.
#[tokio::test]
async fn a_routine_the_self_loop_a_client_and_a_job_are_refused() {
    let r = rig();
    r.agents_report(REPORT, None);
    let mut played = vec![];
    for principal in ["routine", "self", "mcp", "job"] {
        let sid = as_principal(&r, principal);
        match r
            .try_steps(&sid, vec![("profile_run", work("haiku"))])
            .await
        {
            Ok(_) => match first_result(&r, &sid) {
                Some((status, text)) => {
                    assert_eq!(status, ResultStatus::Error, "{principal}: {text}");
                    assert!(
                        text.starts_with("profile.run is only for the owner's own conversation"),
                        "{principal}: {text}"
                    );
                    played.push(principal);
                }
                None => eprintln!("{principal}: the turn ran and made no call"),
            },
            Err(e) => eprintln!("{principal}: the turn itself was refused: {e:#}"),
        }
        assert!(tasks(&r, &sid).is_empty(), "{principal}");
    }
    eprintln!("PLAYED to the tool's wall: {played:?}");
    assert!(!ran(&r, "claude-haiku-5-5"));
    assert!(
        !played.is_empty(),
        "at least one principal reaches the wall in this rig"
    );
}
