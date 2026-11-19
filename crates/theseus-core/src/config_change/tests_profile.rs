//! `profile.ask` and `profile.hold` played through the whole core (issue 28), on the settings
//! rig: "use the haiku profile to say hello" is a tool call and not a shell command; the asked
//! model is offered no tool and its words come back as the result, with its model named and
//! the call booked as a provider call of the turn's; the turn's reply quotes them; and the
//! hold goes through the verb the command calls, only in a turn the owner's own message
//! started.

use serde_json::json;

use super::tests::{rig, Rig, FAILS};
use crate::chat_acts::Said;
use crate::node::{Body, ResultStatus};
use crate::profile_tool::{ASK, HOLD};

/// What the asked model says.
const HELLO: &str = "Hello from the other model, in my own words.";

/// The text of the turn's last reply.
fn reply(r: &Rig, sid: &str) -> String {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|(_, n)| match n.body {
            Body::AssistantMessage { blocks, .. } => Some(
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// The tools the session called, by name.
fn called(r: &Rig, sid: &str) -> Vec<String> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolCall { tool, .. } => Some(tool),
            _ => None,
        })
        .collect()
}

/// The owner's own typed message started the turn that follows.
fn the_owner_speaks(r: &Rig, sid: &str) {
    r.core.runner.tools.chat_acts.heard.stage(
        sid,
        Said {
            text: "stay on haiku".into(),
            who: crate::test_support::owner_cli(),
            owner_input: true,
        },
    );
}

/// "Use the haiku profile to say hello": the turn calls the tool, not a shell; the asked model
/// is given the one question and no tool; its words, its model and its provider are the result;
/// and the turn's reply quotes them.
#[tokio::test]
async fn use_a_profile_to_say_hello_is_a_tool_and_its_words_come_back() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session();
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say hello."}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.starts_with("haiku (model claude-haiku-5-5, from anthropic) answered:"),
        "{text}"
    );
    assert!(text.contains(HELLO), "{text}");
    assert!(text.contains("not yours"), "{text}");
    assert!(reply(&r, &sid).contains(HELLO), "{}", reply(&r, &sid));
    assert_eq!(called(&r, &sid), [ASK], "no shell");
    assert!(r.asked().is_empty(), "told, not asked: {:?}", r.asked());
    let asked = r.asked_plain();
    assert_eq!(asked.len(), 1, "one call");
    let q = &asked[0];
    assert!(q.tools.is_empty(), "no tool is offered to the asked model");
    assert_eq!(q.model, "claude-haiku-5-5");
    assert!(q.max_tokens <= crate::profile_tool::ANSWER_MAX_TOKENS);
    assert_eq!(q.messages.len(), 1, "one question and nothing else");
    assert!(q.messages[0].to_string().contains("Say hello."));
}

/// The call is a provider call of this turn's: planned and settled as an action of its
/// execution, named for what it is, with its cost booked to the turn.
#[tokio::test]
async fn the_asked_call_is_booked_as_a_provider_call_of_the_turn() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session();
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "opus", "question": "Say hello."}),
    )
    .await;
    let provider_calls: Vec<theseus_kernel::Action> = r
        .rows("action.planned")
        .iter()
        .filter(|d| d["tool"] == json!(theseus_kernel::PROVIDER_TOOL))
        .map(|d| {
            let cid = d["correlation_id"].as_str().unwrap();
            r.core.kernel.action(cid).unwrap().unwrap()
        })
        .collect();
    // The turn's own two calls (the one that chose the tool, the one that answered) and the asked one.
    assert_eq!(provider_calls.len(), 3, "{provider_calls:?}");
    let rows = r.rows("profile.asked");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row["profile"], json!("opus"));
    assert_eq!(row["model"], json!("claude-opus-5-5"));
    assert_eq!(row["outcome"], json!("answered"));
    assert!(
        !row.to_string().contains(HELLO),
        "no words in the row: {row}"
    );
    let a = r
        .core
        .kernel
        .action(row["correlation_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Succeeded);
    assert_eq!(a.tool, theseus_kernel::PROVIDER_TOOL);
    assert_eq!(a.resource.as_deref(), Some("anthropic"));
    assert!(
        provider_calls
            .iter()
            .any(|c| c.correlation_id == a.correlation_id),
        "it is one of the turn's provider calls"
    );
    assert_eq!(
        a.execution_id, provider_calls[0].execution_id,
        "the asked call is the asking turn's execution's"
    );
}

/// A profile that is not configured is told in the words the floor already has, and no model is called.
#[tokio::test]
async fn an_unknown_profile_is_told_and_nothing_is_called() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session();
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "nope", "question": "Say hello."}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("`nope` is not a configured profile"),
        "{text}"
    );
    assert!(r.asked_plain().is_empty());
}

/// A provider that cannot answer is told in its own words, the call settles as failed at no
/// cost, and its row says so.
#[tokio::test]
async fn a_provider_that_cannot_answer_is_told_and_settled_as_failed() {
    let r = rig();
    r.answers_plain(FAILS);
    let sid = r.session();
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say hello."}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("haiku could not answer (overloaded)"),
        "{text}"
    );
    assert!(text.contains("the lighthouse is busy"), "{text}");
    let rows = r.rows("profile.asked");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["outcome"].as_str(), rows[0]["class"].as_str()),
        (Some("failed"), Some("overloaded"))
    );
    let a = r
        .core
        .kernel
        .action(rows[0]["correlation_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Failed);
}

/// A profile whose provider is not configured is told in the provider layer's own words.
#[tokio::test]
async fn a_profile_on_a_provider_that_is_not_configured_is_told_so() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session();
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "glm", "question": "Say hello."}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("unknown provider"), "{text}");
    assert!(r.asked_plain().is_empty());
}

/// A question that is not one, or is not the tool's to take, never reaches a model.
#[tokio::test]
async fn a_bad_question_never_reaches_a_model() {
    let r = rig();
    r.answers_plain(HELLO);
    for bad in [
        json!({"profile": "haiku", "question": "  "}),
        json!({"profile": "", "question": "hello"}),
        json!({"profile": "haiku", "question": "x".repeat(8_001)}),
        json!({"profile": "haiku", "question": "hello", "tools": ["fs_read"]}),
        json!({"profile": "haiku", "question": "hello", "max_tokens": 99_999}),
    ] {
        let sid = r.session();
        r.turn_with("profile_ask", &sid, bad.clone()).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{bad}: {text}");
    }
    assert!(r.asked_plain().is_empty());
}

/// A task's session may not ask a profile, in the tool's own words, and no model is called.
#[tokio::test]
async fn a_task_may_not_ask_a_profile() {
    let r = rig();
    r.answers_plain(HELLO);
    let parent = r.session();
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id;
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say hello."}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.starts_with("profile.ask is not for a task's session"),
        "{text}"
    );
    assert!(r.asked_plain().is_empty());
}

/// "Stay on haiku": the owner's own message started the turn, so the tool holds this
/// conversation to the profile through `session.profile`'s verb: its row names who, and the
/// next turn runs there. It is let go the same way.
#[tokio::test]
async fn the_owner_holds_the_conversation_to_a_profile_and_lets_it_go() {
    let r = rig();
    let sid = r.session();
    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("next turns run on haiku"), "{text}");
    assert!(text.contains("keeps the model it started on"), "{text}");
    assert_eq!(called(&r, &sid), [HOLD], "no shell");
    assert_eq!(r.core.runner.sticky_profile(&sid).as_deref(), Some("haiku"));
    let sets = r.rows("session.profile_set");
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert_eq!(sets[0]["to"], json!("haiku"));
    assert_eq!(sets[0]["by"], json!("the CLI"));

    the_owner_speaks(&r, &sid);
    r.turn_with("profile_hold", &sid, json!({"release": true}))
        .await;
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
    assert_eq!(r.rows("session.profile_set").len(), 2);
}

/// A hold in a turn no message of the owner's started, or of a profile that is not one, changes nothing.
#[tokio::test]
async fn a_hold_needs_the_owners_own_message_and_a_real_profile() {
    let r = rig();
    let sid = r.session();
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("acts only in a turn the owner's own message started"),
        "{text}"
    );
    assert_eq!(r.core.runner.sticky_profile(&sid), None);

    let said = r.session();
    the_owner_speaks(&r, &said);
    r.turn_with("profile_hold", &said, json!({"profile": "nope"}))
        .await;
    let (status, text) = r.result(&said);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("Nothing was changed"), "{text}");
    assert_eq!(r.core.runner.sticky_profile(&said), None);
    assert!(r.rows("session.profile_set").is_empty());
}

/// A turn started by a message that was not the owner's own typed input (a person in a
/// channel, a job's turn) holds nothing, though a message started it.
#[tokio::test]
async fn a_message_that_is_not_the_owners_own_input_holds_nothing() {
    let r = rig();
    let sid = r.session();
    r.core.runner.tools.chat_acts.heard.stage(
        &sid,
        Said {
            text: "stay on haiku".into(),
            who: crate::test_support::stranger(),
            owner_input: false,
        },
    );
    r.turn_with("profile_hold", &sid, json!({"profile": "haiku"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("acts only in a turn the owner's own message started"),
        "{text}"
    );
    assert_eq!(r.core.runner.sticky_profile(&sid), None);
    assert!(r.rows("session.profile_set").is_empty());
}

/// What the family costs a request, deferred as it is: nothing, measured against the same daemon
/// without the two tools (a deferred tool is counted at a constant and the family's summary is read
/// only in the answer to a search), and what loading it by default would add, which is more than
/// the 400 under which the owner's rule would load it. A bound, not a budget.
#[test]
fn the_profile_family_is_deferred_and_adds_nothing_to_a_request() {
    let (with, _a) = super::tests_look::deferred_core(true);
    let (without, _b) = super::tests_look::deferred_core(false);
    let d = with.tool_defer();
    let dm = d.places.iter().find(|p| p.place == "dm").expect("dm");
    let fam = dm
        .families
        .iter()
        .find(|f| f.family == "profile")
        .expect("the profile family");
    assert_eq!(
        fam.tools,
        ["profile_ask", "profile_hold"],
        "{:?}",
        fam.tools
    );
    assert!(!fam.loaded, "deferred in a direct message: {}", fam.why);
    let added = |place: &str| {
        let (a, b) = (
            with.first_request(place).unwrap(),
            without.first_request(place).unwrap(),
        );
        assert_eq!(
            a.loaded, b.loaded,
            "{place}: the family loads nothing by default"
        );
        a.tokens - b.tokens
    };
    let (dm_added, cli_added) = (added("dm"), added("cli"));
    eprintln!(
        "MEASURED profile family: loads at {} tokens; deferred adds {dm_added} to a dm request, {cli_added} to a cli one",
        fam.tokens
    );
    assert_eq!((dm_added, cli_added), (0, 0));
    assert!(fam.tokens > 400, "{}", fam.tokens);
}
