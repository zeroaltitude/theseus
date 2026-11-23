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
use theseus_protocol::SubagentAnswer;

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
        std::time::Instant::now(),
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
    assert!(text.contains("need not repeat the words"), "{text}");
    // Where the owner sees it is not the model's to say.
    for place in ["above", "below", "earlier", "later", "when this turn ends"] {
        assert!(!text.contains(place), "{place}: {text}");
    }
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
        std::time::Instant::now(),
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
        ["profile_ask", "profile_hold", "profile_run"],
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

/// The owner's place: a private DM.
const PLACE: &str = "dm:4100000000000000001";

/// The posts a place was written, in order: their kind and their body.
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

/// The subagent's answer is a message of its own, posted to the place the turn's reply goes to and
/// in order BEFORE that reply; its model part is the profile and model that answered, never the
/// asking turn's, and it says it is a subagent's and who asked.
#[tokio::test]
async fn the_answer_is_posted_as_a_message_of_its_own_before_the_reply() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session_at(PLACE);
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say hello."}),
    )
    .await;
    let posts = posts(&r);
    let kinds: Vec<&str> = posts.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["subagent", "reply"], "{posts:?}");
    let said: SubagentAnswer = serde_json::from_value(posts[0].1["answer"].clone()).unwrap();
    assert_eq!(said.text, HELLO);
    assert_eq!(
        (said.profile.as_str(), said.model.as_str()),
        ("haiku", "claude-haiku-5-5")
    );
    assert_eq!(said.asked_by, "sonnet", "the turn's own profile asked");
    let footer = said.footer();
    assert!(
        footer.starts_with("haiku · claude-haiku-5-5 · subagent answer, asked by sonnet"),
        "{footer}"
    );
    assert!(
        !footer.contains("claude-sonnet-5-5"),
        "never the asking turn's model: {footer}"
    );
    // The reply that follows is the asking turn's own, on its own model.
    assert_eq!(posts[1].1["result"]["model"], json!("claude-sonnet-5-5"));
    assert_eq!(posts[0].1["turn_id"], posts[1].1["turn_id"]);
}

/// A turn that asks two profiles posts two messages, in the order they were asked, each with its
/// own model, and each once.
#[tokio::test]
async fn two_profiles_asked_in_one_turn_post_two_messages_in_order() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session_at(PLACE);
    r.steps(
        &sid,
        vec![
            (
                "profile_ask",
                json!({"profile": "haiku", "question": "One."}),
            ),
            (
                "profile_ask",
                json!({"profile": "opus", "question": "Two."}),
            ),
        ],
    )
    .await;
    let posts = posts(&r);
    let kinds: Vec<&str> = posts.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds, ["subagent", "subagent", "reply"], "{posts:?}");
    let models: Vec<&str> = posts[..2]
        .iter()
        .map(|(_, b)| b["answer"]["model"].as_str().unwrap())
        .collect();
    assert_eq!(models, ["claude-haiku-5-5", "claude-opus-5-5"]);
    assert_ne!(posts[0].1["answer"]["call"], posts[1].1["answer"]["call"]);
}

/// A turn that was stopped while its profile was being asked still shows the answer it was given
/// (it was produced and paid for, and its live message is already in the place): the post is its
/// one message, and its footer says the turn that asked was stopped.
#[tokio::test]
async fn a_stopped_turn_posts_its_subagent_message_marked_as_stopped() {
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
    let said: Vec<serde_json::Value> = posts(&r)
        .into_iter()
        .filter(|(k, _)| k == "subagent")
        .map(|(_, b)| b)
        .collect();
    assert_eq!(
        said.iter()
            .map(|b| b["turn_ended"].clone())
            .collect::<Vec<_>>(),
        [json!("stopped")],
        "{said:?}"
    );
}

/// A session that posts nowhere (the CLI's) stages no post; its answer is in the result node,
/// which every client draws from.
#[tokio::test]
async fn a_session_with_no_place_posts_nothing_and_its_node_holds_the_answer() {
    let r = rig();
    r.answers_plain(HELLO);
    let sid = r.session();
    r.turn_with(
        "profile_ask",
        &sid,
        json!({"profile": "haiku", "question": "Say hello."}),
    )
    .await;
    let node = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| n)
        .find(
            |n| matches!(&n.body, Body::ToolResult { meta, .. } if meta.get("subagent").is_some()),
        )
        .expect("the result node is marked as a subagent's");
    let Body::ToolResult { meta, .. } = &node.body else {
        unreachable!()
    };
    let said = SubagentAnswer::from_meta(meta).unwrap();
    assert_eq!(
        (said.text.as_str(), said.model.as_str()),
        (HELLO, "claude-haiku-5-5")
    );
    assert_eq!(said.asked_by, "sonnet");
    assert!(said.cost_usd.is_some() && said.elapsed_ms < 60_000);
    // The live event carries the same answer, for the terminal, the terminal UI and the cockpit.
    use crate::fact::Fact as _;
    let event = crate::fact::tool::ToolEnded {
        session_id: &sid,
        turn_id: node.turn_id.as_deref().unwrap_or_default(),
        node: &node,
    }
    .event();
    let Some(theseus_protocol::Event::ToolEnded(t)) = event else {
        panic!("a tool.ended event")
    };
    assert_eq!(t.subagent, Some(said));
}
