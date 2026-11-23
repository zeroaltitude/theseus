//! A call's result answers ITS call (issue 44): the request a turn sends pairs each result with a call of its
//! own exchange, the earliest unanswered one with that id standing before it, and never places a result
//! twice. A provider may number its calls per response (`call_0`, `call_1`, …), so two exchanges laid in one
//! request, or two responses of one turn, can carry the same id. A call with no result of its own is closed
//! with the harness's stub, never with another call's output: a call parked on an approval must not read as
//! having run.

use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::node::Body;
use crate::places::BoundPlace;
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, last_user, rig_scripted, session, turn, OWNER};
use crate::Core;

/// Two channels bound shared: a post from one into the other asks first (`glide`), a post into its own
/// place runs.
const HALL: u64 = 161_803_398_874_989_484;
const PIER: u64 = 141_421_356_237_309_504;

/// The id every call of these scripts carries, as a provider that numbers its calls per response would.
const SAME: &str = "call_0";

/// The harness's stub for a call with no result (`compiler::render::repaired`).
const STUB: &str = "No result was recorded for this call";

/// A waiting call's result when the person writes instead of answering (`toolrun::resume`).
const SUPERSEDED: &str = "instead of confirming this call";

fn bind(core: &Core) {
    let channel = |id: u64, name: &str| BoundPlace {
        target: format!("discord:channel:{id}"),
        name: name.into(),
        private: false,
        ..Default::default()
    };
    core.bind_places(vec![
        channel(HALL, "#hall"),
        channel(PIER, "#pier"),
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @owner".into(),
            ..Default::default()
        },
    ]);
}

/// `POST <place> :: <text>` posts with the one id; a call's result ends the turn.
fn post_script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    // What was asked is the message's last line (a heading may come before the person's words).
    let said = last_user(req);
    match said
        .lines()
        .last()
        .unwrap_or_default()
        .strip_prefix("POST ")
    {
        Some(rest) => {
            let (to, text) = rest.split_once(" :: ").unwrap();
            Scripted::tools(
                "Posting.",
                &[(SAME, "channel_post", json!({"to": to, "text": text}))],
            )
        }
        None => Scripted::text("Hello."),
    }
}

/// The blocks of the message right after the assistant message that calls `SAME` with an input whose `key`
/// is `value`, in `req`.
fn after_call<'a>(req: &'a ProviderRequest, key: &str, value: &str) -> &'a [Value] {
    let at = req
        .messages
        .iter()
        .position(|m| {
            m["role"] == "assistant"
                && m["content"].as_array().is_some_and(|c| {
                    c.iter().any(|b| {
                        b["type"] == "tool_use" && b["id"] == SAME && b["input"][key] == value
                    })
                })
        })
        .unwrap_or_else(|| panic!("no call with {key} = {value}: {:?}", req.messages));
    req.messages[at + 1]["content"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// The text of the first `tool_result` block among `blocks`.
fn first_result(blocks: &[Value]) -> String {
    let b = blocks
        .iter()
        .find(|b| b["type"] == "tool_result")
        .unwrap_or_else(|| panic!("no result: {blocks:?}"));
    match &b["content"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The content of the newest result `sid` recorded.
fn recorded(core: &Core, sid: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|(_, n)| match &n.body {
            Body::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("a result")
}

/// **Two exchanges at one place, one id.** A post into its own place runs and is answered; in another session
/// at the place a post with the SAME call id waits on its approval; then a new message. The store holds ONE
/// result with the id; in the new turn's request the answered call carries it exactly once, the parked call
/// carries the harness's stub and not a second copy of it, and the question still waits. This is the paired
/// request / store fixture of a finished exchange's result read as if a waiting exchange had run.
#[tokio::test]
async fn a_call_parked_on_an_approval_is_never_answered_by_another_exchanges_result_with_its_id() {
    let r = rig_scripted(|_| {}, post_script);
    bind(&r.core);
    let hall = format!("channel:{HALL}");
    let answered = session(&r.core, Some(&hall));
    let ran = turn(&r.core, &answered, "POST #hall :: to ourselves").await;
    assert_eq!(
        ran.awaiting_confirm, None,
        "into its own place it runs: {ran:?}"
    );
    let its_result = recorded(&r.core, &answered);
    assert!(!its_result.starts_with(STUB), "{its_result}");

    let parked = session(&r.core, Some(&hall));
    let asks = turn(&r.core, &parked, "POST #pier :: hello pier").await;
    assert!(
        asks.awaiting_confirm.is_some(),
        "between two shared places it asks: {asks:?}"
    );

    let now = session(&r.core, Some(&hall));
    turn(&r.core, &now, "and now?").await;
    let req = r
        .requests()
        .into_iter()
        .find(|q| last_user(q).contains("and now?"))
        .expect("the new turn's request");

    // The store: one result with the id, the answered session's; the parked session holds none.
    let with_id = |sid: &str| {
        (r.core.store.session_nodes(sid).unwrap().into_iter())
            .filter(|(_, n)| {
                matches!(&n.body, Body::ToolResult { tool_use_id, late: false, .. } if tool_use_id == SAME)
            })
            .count()
    };
    assert_eq!(
        (with_id(&answered), with_id(&parked)),
        (1, 0),
        "one result is recorded"
    );

    // The request: that one result is placed once, under its own call; where the parked call's stub
    // stands, a second copy of it must not (pairing by id alone across exchanges put it there).
    // The answered call's result as this request renders it (the renderer may head a result).
    let shown = first_result(after_call(&req, "to", "#hall"));
    assert!(
        !shown.starts_with(STUB),
        "the answered call carries its result: {shown}"
    );
    let held = first_result(after_call(&req, "to", "#pier"));
    assert_ne!(
        held, shown,
        "a second copy of the answered call's result stands where the parked call's stub should"
    );
    let placed = req
        .messages
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result" && b["content"] == shown.as_str())
        .count();
    assert_eq!(placed, 1, "a result is placed once: {:?}", req.messages);
    assert!(
        held.starts_with(STUB),
        "the parked call is closed with the stub, never another call's output: {held}"
    );
    assert!(
        r.core
            .confirm_list()
            .unwrap()
            .iter()
            .any(|q| q.session_id == parked),
        "the parked call's question still waits"
    );
}

/// **One turn, two responses, one id.** The first response calls with an id; after its result the second
/// response calls with the SAME id (a provider numbering per response). In the turn's last request each
/// result stands under its own call, in order.
#[tokio::test]
async fn two_responses_of_one_turn_that_repeat_a_call_id_each_get_their_own_result_in_order() {
    let stage = Arc::new(AtomicUsize::new(0));
    let seen = stage.clone();
    let r = rig_scripted(
        |_| {},
        move |req: &ProviderRequest| {
            if !answers_a_call(req) {
                if last_user(req).starts_with("TWICE") {
                    seen.store(1, SeqCst);
                    return Scripted::tools(
                        "First.",
                        &[(SAME, "proc_run", json!({"argv": ["echo", "alpha-7"]}))],
                    );
                }
                return Scripted::text("Hello.");
            }
            match seen.swap(0, SeqCst) {
                1 => Scripted::tools(
                    "Second.",
                    &[(SAME, "proc_run", json!({"argv": ["echo", "beta-9"]}))],
                ),
                _ => Scripted::text("Done."),
            }
        },
    );
    let cli = session(&r.core, None);
    turn(&r.core, &cli, "TWICE").await;
    assert_eq!(stage.load(SeqCst), 0, "both calls were made");
    // The turn's last request: the one that carries the second call (a writer's request may follow it).
    let last = r
        .requests()
        .into_iter()
        .rev()
        .find(|q| q.messages.iter().any(|m| m.to_string().contains("beta-9")))
        .expect("the turn's last request");
    let argv = |word: &str| json!(["echo", word]);
    let call = |word: &str| {
        let at = last
            .messages
            .iter()
            .position(|m| {
                m["content"].as_array().is_some_and(|c| {
                    c.iter()
                        .any(|b| b["type"] == "tool_use" && b["input"]["argv"] == argv(word))
                })
            })
            .unwrap_or_else(|| panic!("no call of {word}: {:?}", last.messages));
        first_result(
            last.messages[at + 1]["content"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )
    };
    let (first, second) = (call("alpha-7"), call("beta-9"));
    assert!(
        first.contains("alpha-7") && !first.contains("beta-9"),
        "{first}"
    );
    assert!(
        second.contains("beta-9") && !second.contains("alpha-7"),
        "{second}"
    );
}

/// **A continuation answers the call of the turn it continues, in its own session, never another's.** Session A
/// posts and waits on its approval (turn 1); session B at the same place posts with the SAME id and waits too;
/// A's call is approved and runs in a later turn of A, whose result is under that turn. A request that lays all
/// three, A's call, then B's, then A's result, shows A's call with its result and B's call still with the stub:
/// B's parked call does not read as having run, with A's result.
#[tokio::test]
async fn a_resumed_result_answers_its_own_sessions_call_and_never_another_sessions_parked_one() {
    let r = rig_scripted(|_| {}, post_script);
    bind(&r.core);
    let hall = format!("channel:{HALL}");
    let a = session(&r.core, Some(&hall));
    let asks = turn(&r.core, &a, "POST #pier :: from A").await;
    assert!(asks.awaiting_confirm.is_some(), "A asks: {asks:?}");
    let b = session(&r.core, Some(&hall));
    let asks = turn(&r.core, &b, "POST #pier :: from B").await;
    assert!(asks.awaiting_confirm.is_some(), "B asks: {asks:?}");

    let q = (r.core.confirm_list().unwrap().into_iter())
        .find(|q| q.session_id == a)
        .expect("A's question");
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    // A's call ran in a later turn of A, which answered after its result.
    r.until("A's resumed turn", 15, |r| {
        let nodes = r.core.store.session_nodes(&a).unwrap();
        let ran = (nodes.iter()).position(
            |(_, n)| matches!(&n.body, Body::ToolResult { tool_use_id, late: false, .. } if tool_use_id == SAME),
        );
        ran.is_some_and(|i| {
            (nodes[i + 1..].iter()).any(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        })
    })
    .await;

    let now = session(&r.core, Some(&hall));
    turn(&r.core, &now, "and now?").await;
    let req = r
        .requests()
        .into_iter()
        .find(|q| last_user(q).contains("and now?"))
        .expect("the new turn's request");
    let theirs = first_result(after_call(&req, "text", "from B"));
    assert!(
        theirs.starts_with(STUB),
        "B's parked call is closed with the stub, never A's result: {theirs}"
    );
    let ours = first_result(after_call(&req, "text", "from A"));
    assert!(
        !ours.starts_with(STUB),
        "A's resumed call carries its result: {ours}"
    );
    assert!(
        r.core
            .confirm_list()
            .unwrap()
            .iter()
            .any(|q| q.session_id == b),
        "B's question still waits"
    );
}

/// **Two turns of ONE session, one id, each call its own result.** A session posts and waits (turn 1), then
/// posts again with the SAME id: the new message supersedes the first question (`toolrun::resume`: a waiting
/// call is closed when the person writes instead of answering, so two old turns of one session never wait at
/// once), and the second waits. The second is approved and runs in a later turn. A request that lays all of it
/// shows the first call with ITS result (superseded) and the second with its own (run): neither borrows the
/// other's, though both carry one id and both results stand in later turns than their calls.
#[tokio::test]
async fn of_two_calls_of_one_session_with_one_id_each_reads_its_own_result() {
    let r = rig_scripted(|_| {}, post_script);
    bind(&r.core);
    let hall = format!("channel:{HALL}");
    let s = session(&r.core, Some(&hall));
    let asks = turn(&r.core, &s, "POST #pier :: the first").await;
    assert!(asks.awaiting_confirm.is_some(), "it asks: {asks:?}");
    let asks = turn(&r.core, &s, "POST #pier :: the second").await;
    assert!(asks.awaiting_confirm.is_some(), "it asks again: {asks:?}");

    let waiting: Vec<_> = (r.core.confirm_list().unwrap().into_iter())
        .filter(|q| q.session_id == s)
        .collect();
    assert_eq!(waiting.len(), 1, "the first was superseded: {waiting:?}");
    assert_eq!(waiting[0].input["text"], "the second", "{waiting:?}");
    r.core
        .confirm_action(&waiting[0].correlation_id, true, None, "cli")
        .unwrap();
    r.until("the resumed turn", 15, |r| {
        let nodes = r.core.store.session_nodes(&s).unwrap();
        let ran = (nodes.iter()).rposition(
            |(_, n)| matches!(&n.body, Body::ToolResult { tool_use_id, late: false, content, .. } if tool_use_id == SAME && !content.contains(SUPERSEDED)),
        );
        ran.is_some_and(|i| {
            (nodes[i + 1..].iter()).any(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        })
    })
    .await;

    let now = session(&r.core, Some(&hall));
    turn(&r.core, &now, "and now?").await;
    let req = r
        .requests()
        .into_iter()
        .find(|q| last_user(q).contains("and now?"))
        .expect("the new turn's request");
    let first = first_result(after_call(&req, "text", "the first"));
    assert!(
        first.contains(SUPERSEDED),
        "the first call reads its own result: {first}"
    );
    let second = first_result(after_call(&req, "text", "the second"));
    assert!(
        !second.starts_with(STUB) && !second.contains(SUPERSEDED),
        "the second call reads its own result, not the first's: {second}"
    );
}
