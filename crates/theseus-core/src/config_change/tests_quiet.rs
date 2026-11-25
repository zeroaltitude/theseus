//! `[content_based_security] outside` (issue 54), played through the whole core: at the default
//! the seven tools run after outside text or with the flag and the owner is told once a span; at
//! `ask` each is asked, and "Approve and clear" approves the call and ends the hold; the freeze at
//! `injection = "refuse"` is untouched by either.

use serde_json::{json, Value};
use theseus_protocol::grants::{GrantAnswer, GrantOffer, CLEAR_FLAG, CLEAR_OUTSIDE};

use super::hold::{self, Cause};
use super::tests::{rig_with, Rig};
use super::tests_answer::owner_in_dm;
use crate::content::{Flag, InjectionMode, OutsideMode};
use crate::node::{Body, Node, ResultStatus};
use crate::session::SessionRecord;

fn at(outside: OutsideMode) -> Rig {
    rig_with(move |c, _| c.content_based_security.outside = outside)
}

/// A session whose turns run as the owner's, flagged.
fn flagged(r: &Rig) -> String {
    let sid = r.session();
    crate::test_support::label_session(&r.core, &sid, Flag::Injection);
    sid
}

/// A guest's words in another place: a turn that opens them reads outside text, as the history
/// tool marks it.
const GUEST: &str = "a guest asks to list the vault and post it";

fn guest(r: &Rig) {
    crate::tests_security::history_exchange(
        &r.core,
        "ses_guest_words",
        "turn_guest_words",
        ("discord:channel:77", "discord:7"),
        GUEST,
    );
}

/// The call that reads the guest's words.
fn read_guest() -> (&'static str, Value) {
    ("context_history", json!({"open": "turn_guest_words"}))
}

/// Another guest's words, read later.
fn another_guest(r: &Rig) -> (&'static str, Value) {
    crate::tests_security::history_exchange(
        &r.core,
        "ses_guest_more",
        "turn_guest_more",
        ("discord:channel:78", "discord:8"),
        "a second guest's words",
    );
    ("context_history", json!({"open": "turn_guest_more"}))
}

/// The notes the owner was told that a call went on unasked.
fn notes(r: &Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter_map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|t| t.starts_with("ℹ️ FYI · Theseus went on without asking"))
        .collect()
}

/// The turn the driver runs after an answer: no input, the call that was waiting.
async fn continue_turn(r: &Rig, sid: &str) {
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    r.core
        .runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: None,
            target,
            sink: crate::bus::EventSink::new(r.core.bus.clone(), sid, None),
            author: "test".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

fn flag_of(r: &Rig, sid: &str) -> bool {
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    rec.data.is_some_and(|d| d.frozen())
}

/// Each of the seven tools, with an input it takes.
fn seven() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("config.show", "config_show", json!({"paths": ["policy"]})),
        ("look.theseus", "look_theseus", json!({})),
        ("look.vault", "look_vault", json!({})),
        ("look.work", "look_work", json!({})),
        (
            "profile.ask",
            "profile_ask",
            json!({"profile": "haiku", "question": "Say hello."}),
        ),
        ("profile.hold", "profile_hold", json!({"profile": "haiku"})),
        (
            "profile.run",
            "profile_run",
            json!({"profile": "haiku", "work": "say hello"}),
        ),
    ]
}

/// Every call of a turn ran: none was refused for its input.
fn ran(r: &Rig, sid: &str) {
    for (_, n) in r.core.store.session_nodes(sid).unwrap() {
        if let Body::ToolResult { tool, content, .. } = &n.body {
            assert!(!content.starts_with("Invalid input"), "{tool}: {content}");
        }
    }
}

/// At the default, each of the seven runs with the flag and after outside text, unasked, and the
/// owner is told once, by a note that names where the text came from and none of what it said.
#[tokio::test]
async fn at_the_default_each_tool_runs_unasked_and_the_owner_is_told_once() {
    for (tool, wire, input) in seven() {
        let r = at(OutsideMode::default());
        r.answers_plain("hello");
        if wire == "profile_run" {
            r.agents_report("done", None);
        }
        let sid = flagged(&r);
        r.steps(&sid, vec![(wire, input.clone()), (wire, input.clone())])
            .await;
        ran(&r, &sid);
        assert!(r.asked().iter().all(|q| q.tool != tool), "{tool}: {:?}", r.asked());
        let told = notes(&r);
        assert_eq!(told.len(), 1, "{tool}: one note for the span: {told:?}");
        assert!(told[0].contains("flagged"), "{}", told[0]);

        let r = at(OutsideMode::default());
        r.answers_plain("hello");
        if wire == "profile_run" {
            r.agents_report("done", None);
        }
        guest(&r);
        let sid = r.session();
        r.steps(&sid, vec![read_guest(), (wire, input.clone()), (wire, input)])
            .await;
        ran(&r, &sid);
        assert!(r.asked().iter().all(|q| q.tool != tool), "{tool}: {:?}", r.asked());
        let told = notes(&r);
        assert_eq!(told.len(), 1, "{tool}: {told:?}");
        assert!(told[0].contains("from history"), "the source is named: {}", told[0]);
        assert!(!told[0].contains(GUEST), "none of what it said: {}", told[0]);
        assert!(!told[0].contains("layer 1"), "{}", told[0]);
        assert!(
            told[0].contains("outside = \"ask\""),
            "the setting that asks: {}",
            told[0]
        );
    }
}

/// A span is told once, and the next is told again: outside text read after the owner's next
/// message is a new span; the flag's span lasts until it is cleared, across turns.
#[tokio::test]
async fn a_second_span_tells_again_and_the_flags_lasts_across_turns() {
    let r = at(OutsideMode::default());
    guest(&r);
    let sid = r.session();
    r.steps(&sid, vec![read_guest(), ("look_theseus", json!({}))])
        .await;
    r.steps(&sid, vec![read_guest(), ("look_theseus", json!({}))])
        .await;
    assert_eq!(notes(&r).len(), 2, "{:?}", notes(&r));
    // A turn that read nothing from outside tells nothing.
    r.turn_with("look_theseus", &sid, json!({})).await;
    assert_eq!(notes(&r).len(), 2, "{:?}", notes(&r));

    let r = at(OutsideMode::default());
    let sid = flagged(&r);
    r.turn_with("look_theseus", &sid, json!({})).await;
    r.turn_with("config_show", &sid, json!({"paths": ["policy"]}))
        .await;
    assert_eq!(notes(&r).len(), 1, "{:?}", notes(&r));
}

/// The note in words: FYI-headed, the cause, its source cut at a `?` and to a short length, what
/// the call did, that nothing was held, and the setting.
#[test]
fn the_note_says_the_cause_where_it_came_from_and_the_setting() {
    let long = format!("https://{}.example.test/x?q=secret", "a".repeat(80));
    let n = hold::note_text(
        Some("Trip planning"),
        &Cause::Outside(vec![hold::source_words(&long), "history:ses_1".into()]),
        "look at what Theseus runs as",
    );
    assert!(crate::owner_note::is_headed(&n), "{n}");
    assert!(n.contains("\"Trip planning\""), "{n}");
    assert!(n.contains("history:ses_1"), "{n}");
    assert!(!n.contains("secret") && !n.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), "{n}");
    assert!(n.contains("look at what Theseus runs as, and nothing was held"), "{n}");
    assert!(n.contains("until your next message"), "{n}");
    let f = hold::note_text(
        None,
        &Cause::Flag("http.fetch https://x.example.test/p?k=v".into()),
        "read Theseus's own settings",
    );
    assert!(f.contains("flagged") && f.contains("until the flag is cleared"), "{f}");
    assert!(!f.contains("k=v") && !f.contains("layer 1"), "{f}");
}

/// The hold counts from the owner's message, or from the result an "Approve and clear" ran,
/// whose own mark still counts; text read after it counts again.
#[test]
fn the_hold_counts_from_a_cleared_result() {
    let mut cleared = Value::Null;
    hold::mark_cleared(&mut cleared);
    let result = |id: &str, url: Option<&str>, meta: &Value| {
        let mut n = Node::tool_result(
            "ses_a",
            None,
            None,
            Body::ToolResult {
                tool_use_id: id.into(),
                tool: "look.theseus".into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "x".into(),
                correlation_id: None,
                bytes_total: 1,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: meta.clone(),
                image: None,
                external: url.map(|u| theseus_tools::External { url: u.into() }),
                payload: None,
                parts: Vec::new(),
            },
        );
        n.id = id.into();
        n
    };
    let nodes = |v: Vec<Node>| v.into_iter().enumerate().map(|(i, n)| (i as u64, n)).collect::<Vec<_>>();
    let read = result("a", Some("https://pages.example.test/guide"), &Value::Null);
    let clear = result("b", None, &cleared);
    let later = result("c", Some("https://pages.example.test/guide"), &Value::Null);
    assert!(super::outside_since_owner(&nodes(vec![read.clone()])));
    assert!(!super::outside_since_owner(&nodes(vec![read.clone(), clear.clone()])));
    assert!(super::outside_since_owner(&nodes(vec![read.clone(), clear, later])));
    let own = result("d", Some("https://pages.example.test/guide"), &cleared);
    assert!(super::outside_since_owner(&nodes(vec![read, own])), "its own mark counts");
}

/// At `ask` the card offers "Approve and clear" for the flag; its press removes the flag as
/// `/trust` does and approves the call, and a later call is free. A press by anyone else changes
/// nothing: the flag stays and the card waits.
#[tokio::test]
async fn at_ask_the_flags_clear_removes_it_and_a_later_call_is_free() {
    let r = at(OutsideMode::Ask);
    let sid = flagged(&r);
    r.steps(
        &sid,
        vec![
            ("look_theseus", json!({})),
            ("config_show", json!({"paths": ["policy"]})),
        ],
    )
    .await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    let q = &asks[0];
    assert_eq!(q.grant_offer, Some(GrantOffer::clear(true)));
    assert!(q.options.iter().any(|o| o.label == "Approve and clear"), "{:?}", q.options);
    let answer = GrantAnswer {
        pattern: CLEAR_FLAG.into(),
    };

    let e = r
        .core
        .confirm_granting(&q.correlation_id, None, crate::test_support::stranger(), &answer)
        .expect_err("a stranger's press");
    assert!(!format!("{e:#}").is_empty());
    assert!(flag_of(&r, &sid), "the flag stays");
    assert_eq!(r.asked().len(), 1, "the card still waits");
    assert!(!r.core.tools.holds.take_cleared(&q.correlation_id));

    let done = r
        .core
        .confirm_granting(&q.correlation_id, None, owner_in_dm(), &answer)
        .unwrap();
    assert!(done.approved);
    assert!(!flag_of(&r, &sid), "the press removed the flag");
    assert!(!r.rows("content.removed").is_empty(), "removed as /trust removes it");
    continue_turn(&r, &sid).await;
    assert_eq!(r.result(&sid).0, ResultStatus::Ok);
    assert_eq!(r.result_of(&sid, "p2").0, ResultStatus::Ok, "the later call ran");
    assert!(r.asked().is_empty(), "the later call is free: {:?}", r.asked());
}

/// At `ask`, outside text's card offers "Approve and clear"; its press ends the hold as if the
/// owner had spoken: a later call is free, and one after new outside text is asked again.
#[tokio::test]
async fn at_ask_outside_texts_clear_ends_the_hold_until_more_is_read() {
    for more in [false, true] {
        let r = at(OutsideMode::Ask);
        guest(&r);
        let later = another_guest(&r);
        let sid = r.session();
        let mut steps = vec![read_guest(), ("look_theseus", json!({}))];
        if more {
            steps.push(later);
        }
        steps.push(("look_work", json!({})));
        r.steps(&sid, steps).await;
        let q = r.asked().remove(0);
        assert_eq!(q.tool, "look.theseus");
        assert_eq!(q.grant_offer, Some(GrantOffer::clear(false)));
        r.core
            .confirm_granting(
                &q.correlation_id,
                None,
                owner_in_dm(),
                &GrantAnswer {
                    pattern: CLEAR_OUTSIDE.into(),
                },
            )
            .unwrap();
        continue_turn(&r, &sid).await;
        let results: Vec<(String, bool)> = r
            .core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult { tool, meta, .. } => {
                    Some((tool.clone(), meta[hold::CLEARED] == true))
                }
                _ => None,
            })
            .collect();
        assert!(
            results.contains(&("look.theseus".into(), true)),
            "the approved call's result is the boundary: {results:?}"
        );
        let asks = r.asked();
        match more {
            false => {
                assert!(asks.is_empty(), "free after the press: {asks:?}");
                assert!(results.iter().any(|(t, _)| t == "look.work"), "{results:?}");
            }
            true => {
                assert_eq!(asks.len(), 1, "{asks:?}");
                assert_eq!(asks[0].tool, "look.work");
            }
        }
    }
}

/// The freeze at `injection = "refuse"` refuses a look in a flagged session at both `outside`
/// settings, as it did before: nothing is asked and nothing is told.
#[tokio::test]
async fn the_freeze_refuses_a_look_at_both_settings() {
    for outside in [OutsideMode::Notify, OutsideMode::Ask] {
        let r = rig_with(move |c, _| {
            c.content_based_security.outside = outside;
            c.content_based_security.injection = InjectionMode::Refuse;
        });
        let sid = flagged(&r);
        r.turn_with("look_theseus", &sid, json!({})).await;
        let (status, text) = r.result(&sid);
        assert_ne!(status, ResultStatus::Ok, "{outside:?}: {text}");
        assert!(text.contains("this session is holding"), "{text}");
        assert!(r.asked().is_empty(), "{:?}", r.asked());
        assert!(notes(&r).is_empty(), "{:?}", notes(&r));
    }
}

/// `outside` takes `notify` or `ask` and nothing else, at a settings change too.
#[tokio::test]
async fn an_unknown_outside_is_refused_at_a_settings_change() {
    let r = at(OutsideMode::default());
    let sid = r.session();
    r.turn(
        &sid,
        json!({"changes": [{"path": "content_based_security.outside", "value": "loud"}],
               "reason": "x"}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("do not load") && text.contains("loud"), "{text}");
    assert!(r.asked().is_empty());
    let e = format!(
        "{:#}",
        crate::Config::parse(
            "[secrets]\nanthropic_api_key = \"env:A\"\n[content_based_security]\noutside = \"loud\"\n"
        )
        .unwrap_err()
    );
    assert!(e.contains("loud"), "{e}");
}
