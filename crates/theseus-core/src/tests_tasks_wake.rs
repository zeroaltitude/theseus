//! The task tools load when Theseus noticed the work (theseus-o1h3f.3): a turn with no person's
//! input (a wake, a report, a routine's run) is offered the `tasks` family in a DM and at the CLI,
//! and a person's turn is exactly as it was (the owner's 13,290-token rule of 2026-10-14).

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::TurnSubmitResult;

use crate::bus::EventSink;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_places::{answers_a_call, last_user, rig_deferring, rows, session, Rig};
use crate::turn::TurnRequest;
use crate::Core;

const DM: &str = "dm:271828182845904523";

fn rig() -> Rig {
    rig_deferring(|_| {}, |_| Scripted::text("Hello."))
}

/// One turn: a person's when `input` is given, else a continuation (a wake's).
async fn run(core: &Arc<Core>, sid: &str, input: Option<&str>) -> TurnSubmitResult {
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
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            surface: None,
        })
        .await
        .unwrap()
}

fn hidden(req: &ProviderRequest, name: &str) -> bool {
    req.tools
        .iter()
        .find(|t| t["name"] == name)
        .is_some_and(|t| t["defer_loading"] == true)
}

/// The families the turn loaded, and the request's tool tokens.
fn first(r: &Rig) -> (Vec<String>, u64, ProviderRequest) {
    let req = r.requests().pop().expect("a request");
    let used = rows(&r.core, "tools.used").pop().expect("a row");
    let loaded = used["loaded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v: &Value| v.as_str().unwrap().to_string())
        .collect();
    let tokens = req.census().tokens(crate::catalog::TokenRates::CLAUDE);
    (loaded, tokens, req)
}

async fn table(place: Option<&str>, input: Option<&str>) -> (Vec<String>, u64, ProviderRequest) {
    let r = rig();
    let sid = session(&r.core, place);
    run(&r.core, &sid, input).await;
    first(&r)
}

fn has_tasks(f: &[String]) -> bool {
    f.iter().any(|x| x == "tasks")
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

/// The request of the turn the wake started, in a fresh session at `place`.
async fn woken(place: Option<&str>) -> (Vec<String>, ProviderRequest) {
    let r = rig_deferring(|_| {}, waking);
    let sid = session(&r.core, place);
    run(&r.core, &sid, Some("WAKE me")).await;
    r.until("the wake's turn", 30, |r| r.requests().len() >= 3)
        .await;
    let req = r.requests().pop().unwrap();
    let used = rows(&r.core, "tools.used").pop().unwrap();
    let loaded = used["loaded"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    (loaded, req)
}

/// A wake turn sees the task tools in a DM.
#[tokio::test]
async fn a_turn_with_no_input_is_offered_the_task_tools() {
    // A placeless (CLI) session's wake is a reminder the driver posts, not a turn, so the
    // CLI is not driven here: the read is the same `Turn::continuation` in either place.
    for (name, place) in [("dm", Some(DM))] {
        let (loaded, req) = woken(place).await;
        let tokens = req.census().tokens(crate::catalog::TokenRates::CLAUDE);
        eprintln!("TABLE {name} wake: {loaded:?} {tokens}");
        assert!(!hidden(&req, "task_create"), "{name}");
        assert!(
            req.tools.iter().any(|t| t["name"] == "task_create"),
            "{name}"
        );
    }
}

/// The safety property: a person's turn that does not ask for a task is as it was, by family and
/// by the request's token count.
#[tokio::test]
async fn a_persons_turn_loads_what_it_always_did() {
    let (dm, dm_tokens, req) = table(Some(DM), Some("hello")).await;
    eprintln!("TABLE dm person: {dm:?} {dm_tokens}");
    assert!(hidden(&req, "task_create"));
    assert_eq!(dm, DM_PERSON, "{dm:?}");
    assert_eq!(dm_tokens, DM_PERSON_TOKENS);
    let (cli, cli_tokens, req) = table(None, Some("hello")).await;
    eprintln!("TABLE cli person: {cli:?} {cli_tokens}");
    assert!(hidden(&req, "task_create"));
    assert_eq!(cli, CLI_PERSON, "{cli:?}");
    assert_eq!(cli_tokens, CLI_PERSON_TOKENS);
}

const DM_PERSON: [&str; 5] = ["commands", "files", "memory", "reminders", "web"];
const DM_PERSON_TOKENS: u64 = 9756;
const CLI_PERSON: [&str; 3] = ["commands", "files", "memory"];
const CLI_PERSON_TOKENS: u64 = 7472;

/// A person's message that asks for a task still loads the family by the ask.
#[tokio::test]
async fn a_persons_ask_for_a_task_still_loads_it() {
    let (loaded, _, req) = table(
        Some(DM),
        Some("please start a new task for the release notes"),
    )
    .await;
    assert!(has_tasks(&loaded), "{loaded:?}");
    assert!(!hidden(&req, "task_create"));
}

/// The words: the refusal carries the sentence. The tool's description does not: the owner's
/// shape reads the channel's first request at 7,889 of 7,900 tokens, and the sentence costs 35.
#[test]
fn the_arrangements_refusal_accepts_work_theseus_noticed() {
    const S: &str =
        "For work you noticed yourself, quote the tool result or your own line that shows it.";
    assert!(crate::arrangement::REFUSAL.contains(S));
    assert!(crate::arrangement::REFUSAL.starts_with("Promotion needs an arrangement:"));
}
