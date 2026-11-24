//! `profile.run`'s notes from its review (issue 28), played through the whole core: a place whose
//! ceiling has no `task`, an unpriced model, a stopped asking turn, a cancelled handed task, the
//! card that asks for the work, and the handed task equal to a `task.create`'s from the same
//! conversation. The first passes are `tests_profile_run.rs` and `tests_profile_run_more.rs`.

use serde_json::json;
use theseus_protocol::PlaceCeiling;

use super::tests::{rig, rig_with, Rig};
use super::tests_answer::owner_in_dm;
use crate::node::ResultStatus;
use crate::places::BoundPlace;
use crate::policy::Posture;
use crate::profile_tool::RUN;
use crate::session::SessionRecord;

const PLACE: &str = "dm:4100000000000000001";
const REPORT: &str = "Listed the folder: a.txt, b.txt";
const OWNER_SAYS: &str = "please list the folder's files now";
const WORK: &str = "AGENT: list the files of the folder and report them";

fn work(profile: &str) -> serde_json::Value {
    json!({"profile": profile, "work": WORK})
}

fn tasks(r: &Rig, sid: &str) -> Vec<theseus_kernel::Execution> {
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    r.core.kernel.tasks(rec.execution_id.as_deref()).unwrap()
}

fn ran(r: &Rig, m: &str) -> bool {
    r.seen().iter().any(|(model, _)| model == m)
}

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

fn spoken(r: &Rig) -> Vec<serde_json::Value> {
    posts(r)
        .into_iter()
        .filter(|(k, _)| k == "subagent")
        .map(|(_, b)| b)
        .collect()
}

/// The owner's DM, bound private with the bindings file's ceiling on it.
fn ceilinged(r: &Rig, tools: &[&str], floor: Option<&str>) {
    r.core.runner.place_rule.bind(vec![BoundPlace {
        target: format!("discord:{PLACE}"),
        name: "DM".into(),
        private: true,
        guild: None,
        ceiling: Some(PlaceCeiling {
            tools: Some(tools.iter().map(|t| t.to_string()).collect()),
            posture_floor: floor.map(str::to_string),
            ..Default::default()
        }),
        persona: None,
    }]);
}

async fn wait_for_spoken(r: &Rig, n: usize) -> Vec<serde_json::Value> {
    for _ in 0..400 {
        let got = spoken(r);
        if got.len() >= n {
            return got;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    spoken(r)
}

/// A place whose ceiling offers the `profile` family and not `task` does not let a task start, so
/// the hand-over (which opens one) is refused there, in words, and nothing opens.
#[tokio::test]
async fn a_place_whose_ceiling_has_no_task_refuses_the_hand_over() {
    let r = rig();
    r.agents_report(REPORT, None);
    ceilinged(&r, &["profile"], None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("profile.run opens a task, and this place does not allow one")
            && text.contains("task.create is not offered in DM"),
        "{text}"
    );
    assert!(tasks(&r, &sid).is_empty());
    assert!(!ran(&r, "claude-haiku-5-5"));
}

/// A hand-over and a `task.create` from the same conversation, in two daemons set alike, give a task
/// that is the same one: the same tools offered (and only the families the place's ceiling lists),
/// the same limit, the same authority, labels and working directory, the same place and ceiling.
#[tokio::test]
async fn the_handed_task_is_equal_to_a_task_create_from_the_same_conversation() {
    let families = ["task", "profile", "fs", "proc", "text", "tools"];
    let made = |hand: bool| async move {
        let r = rig_with(|cfg, _| cfg.kernel.spend_limit_usd = 8.0);
        r.agents_report(REPORT, None);
        ceilinged(&r, &families, Some("notify"));
        r.says(OWNER_SAYS);
        let sid = r.session_at(PLACE);
        let call = match hand {
            true => ("profile_run", work("sonnet")),
            false => (
                "task_create",
                json!({"brief": WORK, "fidelity_ack": true, "arrangement":
                    {"pieces": [{"quote": OWNER_SAYS, "role": "objective"}]}}),
            ),
        };
        r.steps(&sid, vec![call]).await;
        // The task takes its turn, as the driver gives it.
        let mut got = vec![];
        for _ in 0..200 {
            got = r.agent_tools();
            if !got.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let mut opened = tasks(&r, &sid);
        assert!(
            !opened.is_empty(),
            "hand-over {hand}: no task opened: {:?}",
            r.result(&sid)
        );
        let t = opened.remove(0);
        let rec: SessionRecord = r.core.store.get_session(&t.session_id).unwrap().unwrap();
        (r, t, rec, got)
    };
    let (rb, handed, hrec, htools) = made(true).await;
    let (ra, created, crec, ctools) = made(false).await;
    let (mut h, mut c) = (htools[0].clone(), ctools[0].clone());
    h.sort();
    c.sort();
    assert!(!h.is_empty(), "the agent was offered tools");
    assert_eq!(h, c, "the same tools offered to both tasks");
    // Only what the place's ceiling lists.
    for (r, names) in [(&rb, &h), (&ra, &c)] {
        for wire in names {
            let tool = r.core.runner.tools.registry.by_wire(wire).expect(wire);
            let family = crate::ceiling::family(tool.name());
            assert!(families.contains(&family), "{wire} is of {family}");
        }
    }
    assert_eq!(
        handed.budget.limit_micros, created.budget.limit_micros,
        "the same limit"
    );
    assert_eq!(handed.authority, created.authority, "the same authority");
    assert_eq!(hrec.data, crec.data, "the same labels");
    assert_eq!(hrec.dir, crec.dir, "the same working directory");
    assert_eq!(hrec.kind, crec.kind);
    let (hv, cv) = (
        rb.core.runner.view_of(&handed.session_id),
        ra.core.runner.view_of(&created.session_id),
    );
    assert_eq!(hv.class, cv.class, "the same place class");
    assert_eq!(
        hv.ceiling, cv.ceiling,
        "the same ceiling: the handed task has the place's, as any task does"
    );
    assert!(hv.ceiling.is_some_and(|c| c.floor == Some(Posture::Notify)));
}

/// `profile.ask` refuses a model with no price in the catalog, and so does `profile.run`, in the same
/// words: a call cannot be reserved for without one. Nothing opens and no model is asked.
#[tokio::test]
async fn an_unpriced_model_is_refused_as_profile_ask_refuses_it() {
    let r = rig_with(|cfg, _| cfg.unprice_haiku());
    r.agents_report(REPORT, None);
    r.answers_plain("never");
    let sid = r.session_at(PLACE);
    r.steps(
        &sid,
        vec![
            (
                "profile_ask",
                json!({"profile": "haiku", "question": "Hi."}),
            ),
            ("profile_run", work("haiku")),
        ],
    )
    .await;
    let (_, ask) = r.result_of(&sid, "p1");
    let (status, run) = r.result_of(&sid, "p2");
    assert_eq!(status, ResultStatus::Error, "{run}");
    let said = "claude-unpriced-9 has no price in the catalog, so it is not asked";
    assert!(ask.contains(said), "{ask}");
    assert!(run.contains(said), "{run}");
    assert!(tasks(&r, &sid).is_empty());
    assert!(!ran(&r, "claude-unpriced-9"));
}

/// A stop of the asking turn leaves the agent working, as it leaves any task: the stop's answer and
/// the session's note say so and name the task's own cancel, which then stops it.
#[tokio::test(start_paused = true)]
async fn a_stop_says_the_agent_it_handed_work_to_is_still_working() {
    let r = rig();
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    r.agents_report(REPORT, Some(gate));
    let sid = r.session_at(PLACE);
    let core = r.core.clone();
    let stopper = async {
        let mut exec = None;
        for _ in 0..400 {
            if let Some(t) = tasks(&r, &sid).first() {
                exec = t.parent.clone();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let exec = exec.expect("the agent's task opened");
        // Let the agent take its turn and wait on its call.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        core.stop_execution(&exec, "tester").await.unwrap()
    };
    let (_, stop) = tokio::join!(
        r.try_steps(&sid, vec![("profile_run", work("haiku"))]),
        stopper
    );
    assert!(stop.stopped, "{stop:?}");
    assert_eq!(stop.tasks_running, 1, "{stop:?}");
    assert_eq!(stop.agents_running.len(), 1, "{stop:?}");
    let a = &stop.agents_running[0];
    assert_eq!(
        (a.profile.as_str(), a.model.as_str()),
        ("haiku", "claude-haiku-5-5")
    );
    let t = tasks(&r, &sid).remove(0);
    assert_eq!(a.task, crate::task::short(&t.session_id));
    // What the surfaces say, from the one wording.
    let said = theseus_protocol::StopAgent::words(&stop.agents_running, "/cancel");
    assert!(
        said.contains("is still working") && said.contains(&format!("/cancel {}", a.task)),
        "{said}"
    );
    // The agent went on; its own cancel stops it.
    assert!(!tasks(&r, &sid)[0].state.is_terminal());
    r.core.cancel_execution(&t.id, "tester").await.unwrap();
    assert!(tasks(&r, &sid)[0].state.is_terminal());
}

/// A cancelled handed task tells the conversation once, as the subagent's message marked cancelled,
/// and is not left with only the task channel's report.
#[tokio::test(start_paused = true)]
async fn a_cancelled_handed_task_tells_the_conversation_once() {
    let r = rig();
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    r.agents_report(REPORT, Some(gate));
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let t = tasks(&r, &sid).remove(0);
    assert!(spoken(&r).is_empty(), "nothing yet");
    r.core.cancel_execution(&t.id, "tester").await.unwrap();
    // Cancelled twice is no second word.
    let _ = r.core.cancel_execution(&t.id, "tester").await;
    let said = wait_for_spoken(&r, 1).await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(spoken(&r).len(), 1, "once: {:?}", posts(&r));
    let a = &said[0]["answer"];
    assert_eq!(a["outcome"], json!("cancelled"));
    assert!(
        a["text"]
            .as_str()
            .unwrap()
            .starts_with("The agent was cancelled"),
        "{a}"
    );
    let answer: theseus_protocol::SubagentAnswer = serde_json::from_value(a.clone()).unwrap();
    assert!(
        answer
            .footer()
            .contains("subagent answer, asked by sonnet · cancelled"),
        "{}",
        answer.footer()
    );
    // The task's own report post is not a second message in the same place.
    let kinds: Vec<String> = posts(&r).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        kinds.iter().filter(|k| *k == "report").count(),
        0,
        "one message stands for both: {kinds:?}"
    );
}

/// Asked first, the card names the profile and as much of the work as a card allows, in words and not
/// clipped JSON; `task.create`'s card says what the task is to do the same way.
#[tokio::test]
async fn the_approval_card_says_what_the_work_is_in_words() {
    let r = rig_with(|cfg, _| {
        cfg.policy.tools.insert(RUN.into(), Posture::Approve);
    });
    let sid = r.session_at(PLACE);
    let long = format!(
        "AGENT: list the folder's files, then {}",
        "check each name ".repeat(40)
    );
    r.steps(
        &sid,
        vec![("profile_run", json!({"profile": "haiku", "work": long}))],
    )
    .await;
    let q = r.asked().remove(0);
    let what = q
        .plain
        .as_ref()
        .expect("an approval has plain words")
        .what
        .clone();
    assert!(
        what.starts_with(
            "hand the profile `haiku` this work to do with tools: AGENT: list the folder's files"
        ),
        "{what}"
    );
    assert!(!what.contains('{') && !what.contains("\"work\""), "{what}");
    assert!(what.chars().count() > 150, "much of the work shows: {what}");
    assert!(what.ends_with('…'), "a long one is cut at a word: {what}");
    // The yes still works.
    r.core
        .confirm_action(&q.correlation_id, true, None, owner_in_dm())
        .unwrap();
}

/// `task.create`'s card, the same few lines.
#[tokio::test]
async fn a_task_creates_card_says_what_the_task_is_to_do() {
    let r = rig_with(|cfg, _| {
        cfg.policy
            .tools
            .insert("task.create".into(), Posture::Approve);
    });
    r.says(OWNER_SAYS);
    let sid = r.session_at(PLACE);
    r.steps(
        &sid,
        vec![(
            "task_create",
            json!({"brief": "List the files of the\nfolder and report them.", "fidelity_ack": true,
                "arrangement": {"pieces": [{"quote": OWNER_SAYS, "role": "objective"}]}}),
        )],
    )
    .await;
    let q = r.asked().remove(0);
    let what = q.plain.as_ref().unwrap().what.clone();
    assert_eq!(
        what,
        "start a background task: List the files of the folder and report them."
    );
}

/// The late message names the asking conversation and turn, not only the task's own session: that is
/// how a direct message that hosts several thread sessions draws it with its thread's tag. The post is
/// still the task's (its call, its key), so a retry posts it once.
#[tokio::test]
async fn the_report_post_names_the_asking_conversation_and_turn() {
    let r = rig();
    r.agents_report(REPORT, None);
    let sid = r.session_at(PLACE);
    r.steps(&sid, vec![("profile_run", work("haiku"))]).await;
    let said = wait_for_spoken(&r, 1).await;
    assert_eq!(said.len(), 1, "{:?}", posts(&r));
    assert_eq!(
        said[0]["session_id"],
        json!(sid),
        "the asking conversation's"
    );
    let turn = said[0]["turn_id"].as_str().unwrap();
    assert!(turn.starts_with("turn_"), "the asking turn's: {turn:?}");
    let t = tasks(&r, &sid).remove(0);
    assert_eq!(
        said[0]["answer"]["call"],
        json!(t.session_id),
        "keyed to the task"
    );
}

/// The session's narrative lines so far, as one run of text.
fn narrated(rx: &mut tokio::sync::mpsc::UnboundedReceiver<theseus_protocol::Message>) -> String {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|m| format!("{m:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A stop that goes on to cancel the tasks says in the session's note that the agent was cancelled with
/// them, and never that it is still working or that the task goes on; a plain stop's note says the agent
/// goes on, as its answer does.
#[tokio::test(start_paused = true)]
async fn the_sessions_note_of_a_stop_that_cancels_the_agent_does_not_say_it_is_working() {
    for all in [false, true] {
        let r = rig_with(|_, _| {});
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(
            r.core.narrator.watch("note", tx).is_some(),
            "narration is on"
        );
        let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
        r.agents_report(REPORT, Some(gate));
        let sid = r.session_at(PLACE);
        let core = r.core.clone();
        let stopper = async {
            let mut exec = None;
            for _ in 0..400 {
                if let Some(t) = tasks(&r, &sid).first() {
                    exec = t.parent.clone();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            let exec = exec.expect("the agent's task opened");
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            match all {
                true => core.stop_execution_all(&exec, "tester").await.unwrap(),
                false => core.stop_execution(&exec, "tester").await.unwrap(),
            }
        };
        let (_, stop) = tokio::join!(
            r.try_steps(&sid, vec![("profile_run", work("haiku"))]),
            stopper
        );
        let note = narrated(&mut rx);
        match all {
            true => {
                assert!(stop.agents_running.is_empty(), "{stop:?}");
                // The words the note wrote before this branch said "(1 task and 0 wakes go on)".
                assert!(
                    !note.contains("still working")
                        && !note.contains("1 task and")
                        && !note.contains("wakes go on"),
                    "{note}"
                );
                assert!(
                    note.contains("Stop all by tester: 1 task cancelled"),
                    "{note}"
                );
            }
            false => assert!(note.contains("is still working: `theseus cancel"), "{note}"),
        }
    }
}

/// A stop-all whose cancel fails leaves the task going, and the note says so: the note of the stop was
/// written before the cancels and says none goes on, so a line after them corrects it (the answer's
/// own count is right too). The call ends with the cancel's error, after the line.
#[tokio::test(start_paused = true)]
async fn a_stop_all_whose_cancel_fails_says_the_task_goes_on() {
    let r = rig_with(|_, _| {});
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        r.core.narrator.watch("note", tx).is_some(),
        "narration is on"
    );
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(0));
    r.agents_report(REPORT, Some(gate));
    let sid = r.session_at(PLACE);
    let core = r.core.clone();
    let stopper = async {
        let mut found = None;
        for _ in 0..400 {
            if let Some(t) = tasks(&r, &sid).first() {
                found = Some((t.parent.clone().unwrap(), t.session_id.clone()));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let (exec, task_session) = found.expect("the agent's task opened");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // The task's record cannot be read, so its cancel fails and the task goes on.
        r.core
            .store
            .append(&[theseus_store::NewRecord::json(
                theseus_store::kinds::SESSION,
                Some(&task_session),
                &json!("not a session"),
            )
            .unwrap()])
            .unwrap();
        core.stop_execution_all(&exec, "tester").await
    };
    let (_, stopped) = tokio::join!(
        r.try_steps(&sid, vec![("profile_run", work("haiku"))]),
        stopper
    );
    assert!(stopped.is_err(), "the cancel's error ends the call");
    let note = narrated(&mut rx);
    assert!(
        note.contains("Stop all by tester: 1 task could not be cancelled and go on."),
        "{note}"
    );
    assert!(
        note.contains("is still working: `theseus cancel"),
        "the agent is named: {note}"
    );
    assert!(
        !note.contains("task cancelled with the conversation's stop"),
        "no claim that it was cancelled: {note}"
    );
}
