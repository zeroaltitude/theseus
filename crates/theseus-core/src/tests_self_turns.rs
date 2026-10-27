//! The RSI pipeline's own turns are offered the task tools (theseus-lmyvl). They carry an input
//! (the objective), so they are not continuations, but the work they see is work Theseus noticed
//! itself: a turn whose author starts `self:` is read as noticed, as a wake's turn is
//! (theseus-o1h3f.3), and is never read as a person's turn that drops what the loosening loaded.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::TurnSubmitResult;

use crate::bus::EventSink;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_places::{answers_a_call, last_user, rig_deferring, rows, session, Rig};
use crate::turn::TurnRequest;
use crate::Core;

const DM: &str = "dm:271828182845904523";

/// The three turns the pipeline runs, by the author each passes (`rsi/backlog/tender.rs`,
/// `rsi/mint/run.rs`, `rsi/join/review.rs`).
const PIPELINE: [&str; 3] = ["self:backlog", "self:mint", "self:review"];

fn rig() -> Rig {
    rig_deferring(|_| {}, |_| Scripted::text("Hello."))
}

/// One turn with `input`, by `author`.
async fn run(core: &Arc<Core>, sid: &str, input: Option<&str>, author: &str) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            answer_at: None,
            session: rec,
            input: input.map(Into::into),
            target,
            sink,
            author: author.into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            surface: None,
        })
        .await
        .unwrap()
}

/// Whether the request carries the tool, loaded: present, and not deferred.
fn offered(req: &ProviderRequest, name: &str) -> bool {
    req.tools.iter().any(|t| t["name"] == name)
        && !req
            .tools
            .iter()
            .find(|t| t["name"] == name)
            .is_some_and(|t| t["defer_loading"] == true)
}

/// Whether the tasks family is among the families the turn loaded.
fn loaded_tasks(r: &Rig) -> bool {
    let used = rows(&r.core, "tools.used").pop().expect("a row");
    used["loaded"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "tasks")
}

/// One input-carrying turn by `author`, in a fresh DM session: whether the tasks were offered.
async fn offered_to(author: &str) -> (bool, bool) {
    let r = rig();
    let sid = session(&r.core, Some(DM));
    run(&r.core, &sid, Some("work the objective"), author).await;
    let req = r.requests().pop().expect("a request");
    (offered(&req, "task_create"), loaded_tasks(&r))
}

#[tokio::test]
async fn the_backlog_tenders_turn_is_offered_the_task_tools() {
    assert_eq!(offered_to(PIPELINE[0]).await, (true, true));
}

#[tokio::test]
async fn the_mints_turn_is_offered_the_task_tools() {
    assert_eq!(offered_to(PIPELINE[1]).await, (true, true));
}

#[tokio::test]
async fn the_join_reviews_turn_is_offered_the_task_tools() {
    assert_eq!(offered_to(PIPELINE[2]).await, (true, true));
}

/// A prefix test, not a list: a fourth pipeline turn is covered by construction.
#[tokio::test]
async fn any_author_that_starts_self_is_offered_the_task_tools() {
    assert_eq!(offered_to("self:a-later-stage").await, (true, true));
}

/// What is not a pipeline turn is a person's: no `self:` prefix, no loosening.
#[tokio::test]
async fn an_author_that_only_contains_self_is_a_persons_turn() {
    for author in ["cli", "myself:backlog", "self", "discord:self:1"] {
        assert_eq!(offered_to(author).await, (false, false), "{author}");
    }
}

/// A model that sets a one-second wake when asked (`WAKE`), and otherwise answers.
fn waking(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Set.");
    }
    if last_user(req).starts_with("WAKE") {
        return Scripted::tools(
            "Setting it.",
            &[(
                "w1",
                "wake_at",
                json!({"after": "1s", "note": "look at the build again"}),
            )],
        );
    }
    Scripted::text("Noted.")
}

/// The converse of the loosening (`Deferral::dropped`): a person's turn after a wake drops the
/// tasks the wake's compilation recorded. A pipeline turn is not a person's, so it must not drop
/// them: its request keeps the tasks, loaded, as the wake's turn had them.
#[tokio::test]
async fn a_self_turn_after_a_wake_does_not_drop_the_tasks() {
    for author in PIPELINE {
        let r = rig_deferring(|_| {}, waking);
        let sid = session(&r.core, Some(DM));
        run(&r.core, &sid, Some("WAKE me"), "cli").await;
        r.until("the wake's turn", 30, |r| r.requests().len() >= 3)
            .await;
        let wake = r.requests().pop().unwrap();
        assert!(offered(&wake, "task_create"), "{author}: the wake");
        run(&r.core, &sid, Some("work the objective"), author).await;
        let req = r.requests().pop().unwrap();
        assert!(offered(&req, "task_create"), "{author}: after it");
        assert!(loaded_tasks(&r), "{author}: loaded");
    }
}
