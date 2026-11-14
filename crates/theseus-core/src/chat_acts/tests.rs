//! The pure half of the chat acts: what is OFFERED and what a description
//! says. The whole-core half — the tools called by a scripted model through a
//! real turn — is `tests_chat_acts.rs`.

use super::*;

/// The three are held out of every kept catalog, and nothing else is.
#[test]
fn the_three_are_conditional_and_no_other_tool_is() {
    for n in NAMES {
        assert!(conditional(n), "{n}");
        assert!(is_chat_act_wire(&theseus_tools::wire_name(n)), "{n}");
    }
    for n in [
        "fs.write",
        "task.create",
        "owner_ask",
        "import.sync.request",
    ] {
        assert!(!conditional(n), "{n}");
    }
}

/// A message is bound to the turn it started and to no other: the structural
/// rule, measured. The first turn claims it; a later turn gets nothing, and
/// the staged message is dropped so it cannot be claimed later either.
#[test]
fn a_message_is_bound_to_the_turn_it_started_and_to_no_other() {
    let heard = Heard::default();
    let said = Said {
        text: "run the original".into(),
        who: Answerer::from("owner"),
        owner_input: true,
    };
    heard.stage("ses_1", said);
    assert!(
        heard.bound("ses_1", "turn_1").is_none(),
        "unclaimed: a tool call in a turn the offer step never ran for finds nothing"
    );
    assert_eq!(
        heard.claim("ses_1", "turn_1").map(|s| s.text).as_deref(),
        Some("run the original")
    );
    assert!(
        heard.bound("ses_1", "turn_1").is_some(),
        "bound to its own turn"
    );
    assert!(
        heard.bound("ses_1", "turn_2").is_none(),
        "not to another turn"
    );
    assert!(
        heard.claim("ses_1", "turn_2").is_none(),
        "a later turn claims nothing"
    );
    assert_eq!(
        heard.len(),
        0,
        "and the staged message is gone, not left for a third turn"
    );
    assert!(heard.bound("ses_1", "turn_1").is_none());
}

/// The answer tool's description names each waiting question and its answers;
/// with one waiting it says the field may be omitted.
#[test]
fn the_answer_description_names_what_is_waiting() {
    let q = |id: &str, tool: &str| theseus_protocol::ConfirmRequest {
        correlation_id: id.into(),
        session_id: "ses_1".into(),
        execution_id: "exe_1".into(),
        tool: tool.into(),
        reason: "because".into(),
        by: "test".into(),
        ..Default::default()
    };
    let one = tools::answer_description(&[q("cor_aaaabbbb", "fs.write")]);
    assert!(one.contains("One question is waiting"), "{one}");
    assert!(one.contains("fs.write"), "{one}");
    assert!(one.contains("`yes` or `no`"), "{one}");
    assert!(one.contains("may be omitted"), "{one}");

    let two = tools::answer_description(&[
        q("cor_aaaabbbb", "fs.write"),
        q("cor_ccccdddd", crate::ask::OWNER_ASK),
    ]);
    assert!(two.contains("2 questions are waiting"), "{two}");
    assert!(two.contains("ask them instead of guessing"), "{two}");
    assert!(
        two.contains("by its number from 1"),
        "the ask's own answers: {two}"
    );
}

/// The allow tool's description states the held call and whose decision it is.
#[test]
fn the_allow_description_states_the_held_call_and_whose_decision_it_is() {
    let h = |id: &str| theseus_protocol::work::undo::HeldCall {
        id: id.into(),
        session_id: "ses_1".into(),
        turn_id: None,
        tool: "fs.write".into(),
        input: serde_json::json!({"path": "/w/notes.txt"}),
        summary: "fs.write notes.txt".into(),
        judgment: "jdg_1".into(),
        shape: Some("move_aside".into()),
        held_ms: 1,
        allowed: None,
        ran_ms: None,
    };
    let one = tools::allow_description(&[h("hld_1a2b3c4d")]);
    assert!(one.contains("the operator's decision, not yours"), "{one}");
    assert!(one.contains("hld_1a2b3c4d"), "{one}");
    assert!(one.contains("One call is held"), "{one}");
    let two = tools::allow_description(&[h("hld_1a2b3c4d"), h("hld_9z8y7x6w")]);
    assert!(two.contains("2 calls are held"), "{two}");
    assert!(two.contains("ask them which instead of choosing"), "{two}");
}
