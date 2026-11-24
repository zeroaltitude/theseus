//! Who am I talking to through real turns (issue 44; `view::select::people`, `view::sources_people`): the
//! people's front holds its bytes from one turn to the next, across a minute and across a change of speaker;
//! the speaker is named beside the message; a guest's recent lines elsewhere keep to their bound as the
//! request lays them, whatever their name.

use serde_json::json;
use theseus_ontology::{Category, CategoryId, Origin, Record};

use crate::exchange::{self, Exchange, Opener, OwnerMark, Peer, Root};
use crate::node::{Body, Node};
use crate::provider::ProviderRequest;
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{asked, texts, turn_as, view_rig};
use crate::view::select::RECENT_TOKENS;
use crate::view::sources_people::NAME_IN_WRAPPER_BYTES;
use crate::view::text::tokens;
use crate::view::wire_text::BESIDE_HEAD;

/// A guest's Discord id: their held record is `person:<id>`, which holds `discord:<id>`.
const GUEST: &str = "4401";

fn guest() -> String {
    format!("discord:{GUEST}")
}

/// Hold a person as the owner's records would: `local` (`person:<local>`) named `name`.
fn hold(c: &crate::Core, local: &str, name: &str) {
    hold_by(c, local, name, &[]);
}

/// [`hold`], the record holding `handles` too.
fn hold_by(c: &crate::Core, local: &str, name: &str, handles: &[&str]) {
    let mut held = Category::new(CategoryId::new("person", local).unwrap(), name, "test");
    held.handles = handles.iter().map(|h| h.to_string()).collect();
    (c.runner.ontology)
        .write(
            &c.store,
            vec![Record::Category(held)],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
}

/// An exchange `person` opened with `words` in `place` of session `sid` at `at_ms`, marked as the chat
/// service's (not the owner's), on the place's and the person's timelines: its turn id.
fn said_by(
    store: &crate::store::Store,
    (sid, place, private): (&str, &str, bool),
    person: &str,
    at_ms: u64,
    words: &str,
) -> String {
    let turn = crate::new_id("trn");
    let ask = Node::user(sid, Some(&turn), "op", words);
    let mut reply = Node::user(sid, Some(&turn), "op", "");
    reply.body = serde_json::from_value::<Body>(json!({
        "kind": "assistant_message", "model": "big", "provider": "fake",
        "blocks": [{"type": "text", "text": "Noted."}],
    }))
    .unwrap();
    let ex = Exchange {
        turn_id: turn.clone(),
        session_id: sid.into(),
        place: place.into(),
        private,
        opened_by: Opener::Message,
        person: Some(person.into()),
        start_ms: at_ms,
        end_ms: Some(at_ms + 1),
        ask: Some(ask.id.clone()),
        reply: Some(reply.id.clone()),
        messages: vec![ask.id.clone()],
        stop: Some("no_tool_calls".into()),
        rebuilt: false,
    };
    let mark = OwnerMark {
        node: ask.id.clone(),
        in_person: false,
        owner_handle: Some(false),
        peer: Peer::Transport,
        agent: None,
        surface: "discord".into(),
        place: place.into(),
        arrived_session: sid.into(),
        ran_session: sid.into(),
        author: Some(person.into()),
        at_ms,
    };
    let mut recs = vec![
        ask.record().unwrap(),
        reply.record().unwrap(),
        exchange::exchange_record(&ex).unwrap(),
        exchange::mark_record(&mark).unwrap(),
    ];
    recs.extend(exchange::touch(Root::Place(place), &turn, sid, at_ms));
    recs.extend(exchange::touch(Root::Person(person), &turn, sid, at_ms));
    store.append(&recs).unwrap();
    turn
}

/// A session of its own, for exchanges elsewhere.
fn far_session(store: &crate::store::Store) -> String {
    let r = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

/// The text blocks of the request's first user message: the front.
fn front(q: &ProviderRequest) -> Vec<String> {
    (q.messages[0]["content"].as_array().unwrap().iter())
        .filter_map(|b| b["text"].as_str().map(str::to_string))
        .collect()
}

/// The front's people: every block of it that names a person present, in order (the block beside the
/// message, in the first message of a first turn, is not the front).
fn people(q: &ProviderRequest) -> Vec<String> {
    let out: Vec<String> = (front(q).into_iter())
        .filter(|b| !b.starts_with(BESIDE_HEAD))
        .filter(|b| b.contains("In this conversation: ") || b.contains("Who is speaking: "))
        .collect();
    assert!(!out.is_empty(), "{}", texts(q));
    out
}

/// The block beside the message.
fn beside(q: &ProviderRequest) -> String {
    let all = q.messages.iter().flat_map(|m| {
        (m["content"].as_array().into_iter().flatten())
            .filter_map(|b| b["text"].as_str().map(str::to_string))
            .collect::<Vec<_>>()
    });
    let found: Vec<String> = all.filter(|t| t.starts_with(BESIDE_HEAD)).collect();
    assert_eq!(found.len(), 1, "{}", texts(q));
    found.into_iter().next().unwrap()
}

/// A turn of `sid` opened by a message the chat service says `person` wrote (not the owner).
async fn turn_by(
    core: &std::sync::Arc<crate::Core>,
    sid: &str,
    input: &str,
    person: &str,
) -> theseus_protocol::TurnSubmitResult {
    use crate::session::SessionRecord;
    let mut mark = crate::view_gate::test_mark(&core.runner, sid, false);
    mark.peer = Peer::Transport;
    mark.agent = None;
    mark.owner_handle = Some(false);
    mark.surface = "discord".into();
    mark.author = Some(person.into());
    let rec = (core.store.get_session::<SessionRecord>(sid).unwrap()).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

/// KEY: the people's front is the owner's filing, headed with no time: two turns a minute and more apart,
/// with the same records, lay it in the same bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_peoples_front_holds_its_bytes_across_a_minute() {
    let r = view_rig();
    let c = &r.core;
    hold(c, &crate::tests_places::OWNER.to_string(), "Ada");
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: first light", true).await;
    theseus_protocol::time::move_clock_to(theseus_protocol::now_unix_ms() + 125_000);
    turn_as(c, &desk, "desk: second light", true).await;
    theseus_protocol::time::reset_clock();
    let a = people(&asked(&r, "desk: first light").pop().unwrap());
    let b = people(&asked(&r, "desk: second light").pop().unwrap());
    assert_eq!(a, b);
}

/// KEY (the owner's word, 2026-11-24): with the same people present, a change of speaker leaves the front's
/// bytes as they were; the one line beside the message names who is speaking.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_of_speaker_leaves_the_front_as_it_was() {
    let r = view_rig();
    let c = &r.core;
    hold(c, &crate::tests_places::OWNER.to_string(), "Ada");
    hold(c, GUEST, "Wren Hallam");
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: morning one", true).await;
    turn_by(c, &desk, "desk: morning two", &guest()).await;
    turn_as(c, &desk, "desk: morning three", true).await;
    let two = asked(&r, "desk: morning two").pop().unwrap();
    let three = asked(&r, "desk: morning three").pop().unwrap();
    let (two_front, three_front) = (people(&two), people(&three));
    assert!(
        two_front
            .iter()
            .any(|b| b.contains("In this conversation: Wren Hallam")),
        "{two_front:#?}"
    );
    assert!(
        two_front
            .iter()
            .any(|b| b.contains("In this conversation: Ada")),
        "{two_front:#?}"
    );
    assert_eq!(two_front, three_front);
    assert!(
        beside(&two).contains("Who is speaking: Wren Hallam"),
        "{}",
        beside(&two)
    );
    assert!(
        beside(&three).contains("Who is speaking: Ada"),
        "{}",
        beside(&three)
    );
    for q in [&two, &three] {
        assert!(
            !people(q).iter().any(|b| b.contains("Who is speaking")),
            "{:#?}",
            front(q)
        );
    }
}

/// The recent lines elsewhere of the guest present, as the owner's next turn lays them beside the message:
/// each line's text, from its own heading to its handle.
async fn recent_as_laid(name: &str) -> Vec<String> {
    let r = view_rig();
    let c = &r.core;
    hold(c, GUEST, name);
    let desk = session_in(c, None, &r.root);
    let first = turn_as(c, &desk, "desk: who else?", true).await;
    let here = (exchange::exchange(&c.store, &first.turn_id).unwrap())
        .unwrap()
        .place;
    let now = theseus_protocol::now_unix_ms();
    said_by(
        &c.store,
        (&desk, &here, true),
        &guest(),
        now,
        "Morning, all.",
    );
    let far = far_session(&c.store);
    let words =
        "Can you check the tide table for Thursday, and the ferry's times after it? ".repeat(3);
    let elsewhere: Vec<String> = (1..=5u64)
        .map(|k| {
            let place = format!("discord:channel:50{k}");
            said_by(
                &c.store,
                (&far, &place, false),
                &guest(),
                now - k * 60_000,
                &words,
            )
        })
        .collect();
    turn_as(c, &desk, "desk: and now?", true).await;
    let b = beside(&asked(&r, "desk: and now?").pop().unwrap());
    let mut lines = Vec::new();
    for t in &elsewhere {
        let Some(end) = b.find(&format!("(open {t})")) else {
            continue;
        };
        let end = end + format!("(open {t})").len();
        let start = (b[..end].rfind("\n\noutside text from "))
            .unwrap_or_else(|| panic!("{t}'s line is not headed with its words: {b}"))
            + 2;
        lines.push(b[start..end].to_string());
    }
    assert!(!lines.is_empty(), "{b}");
    lines
}

/// KEY: every recent line, its two headings, both wrappers and the name in them counted as the request lays
/// them, keeps within the lines' bound, each line whole; an ordinary name, and one that grows under escaping.
/// The name in the wrapper is whole when it is ordinary, and within its own bound once escaped when it grows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_recent_lines_keep_their_bound_as_the_request_lays_them() {
    for name in ["Wren Hallam".to_string(), "[".repeat(100)] {
        let lines = recent_as_laid(&name).await;
        let spent: u64 = lines.iter().map(|l| tokens(&format!("{l}\n\n"))).sum();
        assert!(spent <= RECENT_TOKENS, "{spent} tokens: {lines:#?}");
        for l in &lines {
            assert!(l.starts_with("outside text from \""), "{l}");
            assert!(l.contains("[end of outside text, fence "), "{l}");
            let rest = &l["outside text from ".len()..];
            let quoted = &rest[..rest.find("; not the person's words").unwrap()];
            assert!(quoted.len() > 2, "{l}");
            assert!(quoted.len() - 2 <= NAME_IN_WRAPPER_BYTES, "{quoted}");
            if name == "Wren Hallam" {
                assert_eq!(quoted, "\"Wren Hallam\"");
            }
        }
    }
}

/// KEY: a line found under another handle of a held person's record, one they never spoke by here, carries
/// their name as the request lays it beside the message; words by a handle no record holds are still
/// "another person"'s.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_line_under_another_handle_of_theirs_carries_their_name() {
    const ALIAS: &str = "slack:U4401";
    let r = view_rig();
    let c = &r.core;
    hold_by(c, GUEST, "Wren Hallam", &[ALIAS]);
    let desk = session_in(c, None, &r.root);
    let first = turn_as(c, &desk, "desk: who else?", true).await;
    let here = (exchange::exchange(&c.store, &first.turn_id).unwrap())
        .unwrap()
        .place;
    let now = theseus_protocol::now_unix_ms();
    let in_desk = (desk.as_str(), here.as_str(), true);
    said_by(&c.store, in_desk, &guest(), now - 120_000, "Morning, all.");
    said_by(
        &c.store,
        in_desk,
        "discord:zed",
        now - 90_000,
        "Tide turning now.",
    );
    let far = far_session(&c.store);
    let t = said_by(
        &c.store,
        (&far, "discord:channel:509", false),
        ALIAS,
        now - 60_000,
        "Ferry at four?",
    );
    turn_as(c, &desk, "desk: and now?", true).await;
    let q = asked(&r, "desk: and now?").pop().unwrap();
    let b = beside(&q);
    let end = (b.find(&format!("(open {t})"))).unwrap_or_else(|| panic!("{t} is not laid: {b}"));
    let start = (b[..end].rfind("\n\noutside text from "))
        .unwrap_or_else(|| panic!("{t}'s line is not headed with its words: {b}"))
        + 2;
    let line = &b[start..end];
    assert!(
        line.starts_with("outside text from \"Wren Hallam\""),
        "{line}"
    );
    assert!(!line.contains("another person"), "{line}");

    let all: Vec<String> = (q.messages.iter())
        .flat_map(|m| {
            (m["content"].as_array().into_iter().flatten())
                .filter_map(|b| b["text"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect();
    let all = all.join("\n");
    let at = (all.find("\nTide turning now.\n")).unwrap_or_else(|| panic!("{}", texts(&q)));
    let wrapper = &all[all[..at].rfind("[outside text from ").unwrap()..at];
    assert!(wrapper.contains("\"another person\""), "{wrapper}");
}
