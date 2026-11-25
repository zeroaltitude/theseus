//! What has been said since, beside the message (issue 44; `copies::since`, `view::wire_text`): a line of
//! another place's exchange, his own or a person's present, is laid with the mark its message was given, as
//! the window lays it; the mark holds its bytes from one turn to the next; an erased message lays no words.

use theseus_protocol::retract::{FactRetractParams, FactRetractResult};

use crate::exchange;
use crate::tests_places::session;
use crate::tests_view_compile::{asked, texts, turn_as, view_rig};
use crate::tests_view_people::{beside, far_session, guest, hold, said_by, GUEST};

/// Why the message is retracted.
const WHY: &str = "the ferry leaves at five, not four";
/// The words said in another place.
const WORDS: &str = "far: the ferry leaves at four";

fn retract(c: &crate::Core, node: &str) -> FactRetractResult {
    c.fact_retract(
        &FactRetractParams {
            node: node.into(),
            why: WHY.into(),
            session_id: None,
        },
        "cli",
    )
    .unwrap()
}

/// The line a message retracted by `out` is laid with, after its words (`copies::since`).
fn mark(out: &FactRetractResult) -> String {
    format!(
        "[Since this was said: it was marked wrong on {}: {WHY}. The words above are as they were said.]",
        crate::judge::spend::local_day(out.at_ms)
    )
}

/// The message that opened exchange `turn`.
fn ask_of(store: &crate::store::Store, turn: &str) -> String {
    (exchange::exchange(store, turn).unwrap())
        .and_then(|e| e.ask)
        .unwrap()
}

/// His exchange at another place saying [`WORDS`], then a conversation at the desk: the desk's session and
/// the other exchange's turn.
async fn his_words_elsewhere(c: &std::sync::Arc<crate::Core>) -> (String, String) {
    let far = session(c, Some("local:/far"));
    let t = turn_as(c, &far, WORDS, true).await;
    (session(c, None), t.turn_id)
}

/// KEY: his line in another place, laid beside the message, carries the mark its message was given since,
/// right after his words, as the window lays it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_elsewhere_line_of_a_retracted_message_carries_the_mark() {
    let r = view_rig();
    let c = &r.core;
    let (desk, far) = his_words_elsewhere(c).await;
    let out = retract(c, &ask_of(&c.store, &far));
    turn_as(c, &desk, "desk: what did I say?", true).await;
    let b = beside(&asked(&r, "desk: what did I say?").pop().unwrap());
    assert!(b.contains(&format!("{WORDS}\n{}", mark(&out))), "{b}");
}

/// KEY: a recent line elsewhere of a person present, laid beside the message, carries the mark its message
/// was given since, right after their words.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persons_recent_line_of_a_retracted_message_carries_the_mark() {
    let r = view_rig();
    let c = &r.core;
    hold(c, GUEST, "Wren Hallam");
    let desk = session(c, None);
    let first = turn_as(c, &desk, "desk: who else?", true).await;
    let here = (exchange::exchange(&c.store, &first.turn_id).unwrap())
        .unwrap()
        .place;
    let now = theseus_protocol::now_unix_ms();
    said_by(
        &c.store,
        (&desk, &here, true),
        &guest(),
        now - 120_000,
        "Morning, all.",
    );
    let far = far_session(&c.store);
    let t = said_by(
        &c.store,
        (&far, "discord:channel:509", false),
        &guest(),
        now - 60_000,
        WORDS,
    );
    let out = retract(c, &ask_of(&c.store, &t));
    turn_as(c, &desk, "desk: and now?", true).await;
    let b = beside(&asked(&r, "desk: and now?").pop().unwrap());
    assert!(b.contains(&format!("(open {t})")), "{b}");
    assert!(b.contains(&format!("{WORDS}\n{}", mark(&out))), "{b}");
}

/// An erased message elsewhere lays none of its words anywhere in the request; its exchange is still in the
/// view's record, which says it was chosen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_erased_message_elsewhere_lays_no_words_and_stays_in_the_record() {
    let r = view_rig();
    let c = &r.core;
    let (desk, far) = his_words_elsewhere(c).await;
    let (_, ask) = c.store.get_node(&ask_of(&c.store, &far)).unwrap().unwrap();
    (c.store)
        .append(&[ask
            .erased(theseus_protocol::now_unix_ms(), "a test")
            .record()
            .unwrap()])
        .unwrap();
    let t = turn_as(c, &desk, "desk: what did I say?", true).await;
    let q = asked(&r, "desk: what did I say?").pop().unwrap();
    assert!(!texts(&q).contains("the ferry leaves at four"), "{}", texts(&q));
    let records = crate::view::record::of_turn(&c.store, &t.turn_id).unwrap();
    assert!(
        (records.iter())
            .flat_map(|v| &v.shown)
            .any(|s| s.piece.range.first.id == far),
        "{records:#?}"
    );
}

/// KEY: the mark depends only on what has been said: two turns with nothing said between them about the
/// message lay the block beside the message in the same bytes, the mark in both.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_turns_with_nothing_said_between_lay_the_same_beside_bytes() {
    let r = view_rig();
    let c = &r.core;
    let (desk, far) = his_words_elsewhere(c).await;
    let out = retract(c, &ask_of(&c.store, &far));
    turn_as(c, &desk, "desk: first look", true).await;
    turn_as(c, &desk, "desk: second look", true).await;
    let one = beside(&asked(&r, "desk: first look").pop().unwrap());
    let two = beside(&asked(&r, "desk: second look").pop().unwrap());
    assert!(one.contains(&mark(&out)), "{one}");
    assert_eq!(one, two);
}
