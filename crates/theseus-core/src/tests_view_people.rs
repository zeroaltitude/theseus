//! Who am I talking to through real turns (issue 44; `view::select::people`, `view::sources_people`): the
//! people's front holds its bytes from one turn to the next, across a minute.

use theseus_ontology::{Category, CategoryId, Origin, Record};

use crate::provider::ProviderRequest;
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{asked, texts, turn_as, view_rig};
use crate::view::wire_text::BESIDE_HEAD;

/// Hold a person as the owner's records would: `local` (`person:<local>`) named `name`.
fn hold(c: &crate::Core, local: &str, name: &str) {
    let held = Category::new(CategoryId::new("person", local).unwrap(), name, "test");
    (c.runner.ontology)
        .write(
            &c.store,
            vec![Record::Category(held)],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
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
