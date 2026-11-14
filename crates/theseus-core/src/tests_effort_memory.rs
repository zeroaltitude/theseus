//! The effort question is told how the session's previous turn ran (theseus-8euum), by the owner's
//! word of 2026-11-14 13:54, "tell the small model that the last turn failed and what effort it ran
//! at" (and no forced floor: nothing here changes the answer): the state `effort.v1` is asked with,
//! through the whole core, carries `previous_turn` after a turn that failed, and is as it always was
//! for a session's first message and for the facts nothing carries.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::SessionKind;

use crate::provider::{FakeProvider, ProviderError, Scripted};
use crate::session::SessionRecord;
use crate::tests_route::{mode, rig_parts, try_turn_attached, Rig};

const ASK: &str = "Tidy the notes file.";

/// A rig whose route reads every message as routine work at `low` effort, on a provider that answers
/// with `script`; the fake Jev comes back to read what it was asked.
fn rig(script: Vec<Scripted>) -> (Rig, FakeJev) {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    jev.script(
        "effort",
        Jev::Choice {
            option: "low".into(),
            confidence: 0.95,
        },
    );
    let claude = Arc::new(FakeProvider::scripted(script));
    let rig = rig_parts(
        Some(&jev),
        4,
        |_| {},
        move |p| {
            p.providers.insert("anthropic".into(), claude);
        },
    );
    (rig, jev)
}

fn session(rig: &Rig) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    rig.core.store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

async fn say(rig: &Rig, sid: &str, text: &str) -> bool {
    try_turn_attached(&rig.core, Some(sid), text, None, Vec::new())
        .await
        .is_ok()
}

/// The state of each request the effort pack was asked with, oldest first.
fn effort_states(jev: &FakeJev) -> Vec<Value> {
    jev.seen()
        .into_iter()
        .map(|s| s.body)
        .filter(|b| {
            b["questions"]
                .as_object()
                .is_some_and(|q| q.contains_key("effort.v1/effort"))
        })
        .map(|b| b["state"].clone())
        .collect()
}

/// A first message has no previous turn; a turn that failed (a 400 ended it) is told to the next
/// message's effort question as `{effort: low, failed: true}`; and the question's answer is what Jev
/// scripted, `low`, whatever the turn before did: nothing is raised.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_effort_question_is_told_a_failed_turn_and_its_effort() {
    let (rig, jev) = rig(vec![
        Scripted::Fail(ProviderError::InvalidRequest {
            status: 400,
            message: "invalid_request_error: the request was refused".into(),
        }),
        Scripted::text("Done."),
    ]);
    let sid = session(&rig);
    assert!(!say(&rig, &sid, ASK).await, "the first turn failed");
    assert!(say(&rig, &sid, "Try once more.").await);
    let states = effort_states(&jev);
    assert_eq!(states.len(), 2, "{states:?}");
    assert!(states[0].get("previous_turn").is_none(), "{}", states[0]);
    assert_eq!(
        states[1]["previous_turn"],
        json!({"effort": "low", "failed": true}),
        "{}",
        states[1]
    );
    // Told, not forced: the second turn ran at what Jev answered.
    let ran: Vec<Value> = crate::tests_route::decided(&rig.core.store)
        .iter()
        .map(|d| d["effort_ran"].clone())
        .collect();
    assert_eq!(ran, [json!("low"), json!("low")]);
}

/// A turn that finished is told as not failed, and one that a spending limit ended is not a failed
/// attempt either: more effort would fix nothing and cost more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_turn_and_a_refusal_no_effort_could_address_are_not_told_as_failed() {
    let (rig, jev) = rig(vec![Scripted::text("Tidied."), Scripted::text("Done.")]);
    let sid = session(&rig);
    assert!(say(&rig, &sid, ASK).await);
    assert!(say(&rig, &sid, "Try once more.").await);
    let states = effort_states(&jev);
    assert_eq!(
        states[1]["previous_turn"],
        json!({"effort": "low", "failed": false})
    );
    let (rig, jev) = rig(vec![
        Scripted::Fail(ProviderError::Auth {
            status: 401,
            message: "authentication_error: invalid x-api-key".into(),
        }),
        Scripted::text("Done."),
    ]);
    let sid = session(&rig);
    assert!(!say(&rig, &sid, ASK).await);
    assert!(say(&rig, &sid, "Try once more.").await);
    let states = effort_states(&jev);
    assert_eq!(
        states[1]["previous_turn"],
        json!({"effort": "low", "failed": false}),
        "a refused credential is no failed attempt: {}",
        states[1]
    );
}
