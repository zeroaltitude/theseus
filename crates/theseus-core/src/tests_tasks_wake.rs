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
    let (loaded, req) = woken(Some(DM)).await;
    let tokens = req.census().tokens(crate::catalog::TokenRates::CLAUDE);
    eprintln!("TABLE dm wake: {loaded:?} {tokens}");
    assert!(offered(&req, "task_create"), "present, and not deferred");
    assert!(req.tools.iter().any(|t| t["name"] == "task_create"));
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

/// The same safety property one turn later: a person's turn after a wake turn is a person's turn
/// again. The wake's recompile recorded the tasks family; the next appended turn must drop it, so
/// it recompiles without it (`Deferral::dropped`) and carries what a person's turn always did.
#[tokio::test]
async fn a_persons_turn_after_a_wake_costs_what_it_always_did() {
    let r = rig_deferring(|_| {}, waking);
    let sid = session(&r.core, Some(DM));
    run(&r.core, &sid, Some("WAKE me")).await;
    r.until("the wake's turn", 30, |r| r.requests().len() >= 3)
        .await;
    let wake = r.requests().pop().unwrap();
    assert!(
        !hidden(&wake, "task_create"),
        "the wake's turn has the tasks"
    );
    run(&r.core, &sid, Some("hello")).await;
    let (loaded, _, req) = first(&r);
    eprintln!("TABLE dm person after a wake: {loaded:?}");
    assert!(hidden(&req, "task_create"));
    assert_eq!(loaded, DM_PERSON, "{loaded:?}");
    // The whole request also holds the earlier turns' messages, so its token count is not
    // `DM_PERSON_TOKENS`; the tools are what the wake could have left behind, and they must be
    // exactly a first person turn's.
    let (_, _, fresh) = table(Some(DM), Some("hello")).await;
    assert_eq!(req.tools, fresh.tools);
}

/// Whether the request carries the tool, loaded: present, and not deferred.
fn offered(req: &ProviderRequest, name: &str) -> bool {
    req.tools.iter().any(|t| t["name"] == name) && !hidden(req, name)
}

/// The tasks ride past the place's budget by their own size on a turn with no person's input, as
/// an admitted family does: a family the ledger learned for the place stays loaded on the wake's
/// turn, where the tasks would otherwise be held to the budget and evict it.
#[tokio::test]
async fn a_wake_turn_loads_the_tasks_and_evicts_no_learned_family() {
    let r = rig_deferring(|_| {}, waking);
    let at = theseus_protocol::now_unix_ms() - 3 * 86_400_000;
    for i in 0..12 {
        r.core.runner.tools.defer.apply_used(
            at + i,
            &json!({"place": "dm", "loaded": ["files", "commands"], "used": ["routines"],
            "searched": [], "unused": []}),
        );
    }
    let sid = session(&r.core, Some(DM));
    run(&r.core, &sid, Some("WAKE me")).await;
    let person = r.requests().pop().unwrap();
    assert!(
        offered(&person, "routine_create"),
        "the ledger's family loads"
    );
    assert!(!offered(&person, "task_create"));
    r.until("the wake's turn", 30, |r| r.requests().len() >= 3)
        .await;
    let wake = r.requests().pop().unwrap();
    assert!(
        offered(&wake, "task_create"),
        "the wake is offered the tasks"
    );
    assert!(
        offered(&wake, "routine_create"),
        "the tasks evicted the learned family"
    );
}

/// A task rig whose parent starts a task named `CHILD` (asleep for `start_after`, when given),
/// and whose child answers at once; its tools are deferred as a daemon's are.
fn task_rig(start_after: Option<&str>) -> crate::tests_tasks::Rig {
    use crate::tests_tasks as tt;
    let after = start_after.map(str::to_string);
    tt::rig_deferring(
        move |req| {
            if tt::first_user(req).contains("CHILD") {
                return Scripted::text("Drafted it.");
            }
            if tt::answers_a_call(req) {
                return Scripted::text("Started.");
            }
            if tt::last_user(req).starts_with("START") {
                let mut input = json!({"brief": "CHILD: draft the release notes",
                                       "budget_usd": 1.5, "arrangement": tt::arranged()});
                if let Some(a) = &after {
                    input["start_after"] = json!(a);
                }
                return Scripted::tools("Starting.", &[("t_task", "task_create", input)]);
            }
            Scripted::text("ok")
        },
        |_| {},
    )
}

/// The requests the model got from the task's own session.
fn childs(r: &crate::tests_tasks::Rig) -> Vec<ProviderRequest> {
    let all = r.model.requests.lock().unwrap();
    all.iter()
        .filter(|q| crate::tests_tasks::first_user(q).contains("CHILD"))
        .cloned()
        .collect()
}

fn names(q: &ProviderRequest) -> Vec<Value> {
    q.tools.iter().map(|t| t["name"].clone()).collect()
}

/// The class, not the session kind (1/2): the turn the driver runs in a task's session has no
/// person's input, so it is offered the tasks.
#[tokio::test]
async fn a_task_sessions_driver_turn_is_offered_the_tasks() {
    use crate::tests_tasks as tt;
    let r = task_rig(None);
    let parent = tt::parent_session(&r.core);
    tt::turn(&r.core, &parent, tt::ASK).await;
    let task = tt::only_task(&r.core, &parent);
    tt::until("the child's report", || {
        tt::exec(&r.core, &task.id).state == theseus_kernel::ExecState::Complete
    })
    .await;
    let driver = childs(&r).into_iter().next().expect("the driver's turn");
    assert!(
        offered(&driver, "task_create"),
        "a driver-run turn is offered the tasks: {:?}",
        names(&driver)
    );
}

/// The class, not the session kind (2/2): a person's typed turn in a task's session is a person's
/// and is offered no more than it was. A read of the session's kind (`tc.task`) would give this
/// turn and the driver's the same answer.
#[tokio::test]
async fn a_task_sessions_typed_turn_is_a_persons() {
    use crate::tests_tasks as tt;
    // 60s, not 2s: the rig runs on a real clock, so a loaded host can wake the task before the
    // person's message arrives and the turn under test stops being the session's first
    // (the join-fix review's finding 5).
    let r = task_rig(Some("60s"));
    let parent = tt::parent_session(&r.core);
    tt::turn(&r.core, &parent, tt::ASK).await;
    // The task is asleep until it is due, so a person's message is its session's first turn.
    let task = tt::only_task(&r.core, &parent);
    assert!(childs(&r).is_empty(), "no turn before it is due");
    tt::turn(&r.core, &task.session_id, "CHILD: and the changelog too").await;
    let typed = childs(&r).pop().expect("the typed turn ran");
    assert!(
        !offered(&typed, "task_create"),
        "a typed turn is not offered the tasks: {:?}",
        names(&typed)
    );
}

const DM_PERSON: [&str; 5] = ["commands", "files", "memory", "reminders", "web"];
const DM_PERSON_TOKENS: u64 = 9908;
const CLI_PERSON: [&str; 3] = ["commands", "files", "memory"];
const CLI_PERSON_TOKENS: u64 = 7624;

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
