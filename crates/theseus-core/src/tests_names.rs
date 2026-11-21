//! Recognition (issue 44, `crate::names`), through a whole core and the fake
//! Jev: the topics `onto.v1` chose touch the message's exchange.

use theseus_judge::fake::FakeJev;
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};

use crate::exchange::{self, Root};
use crate::tests_onto::{bound, rig, rig_with, Rig};

const SAID: &str = "let's plan the Lantern Ledger migration";

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn operator_writes(r: &Rig, records: Vec<Record>) {
    r.core
        .runner
        .ontology
        .write(&r.core.store, records, Origin::Operator, |_| Ok(vec![]))
        .unwrap();
}

fn ledger_category(r: &Rig) {
    operator_writes(
        r,
        vec![Record::Category(Category::new(
            id("topic:ledger"),
            "Ledger",
            "ada",
        ))],
    );
}

/// A turn in `sid` and the sink's frame written: the turn's id.
async fn said(r: &Rig, sid: &str) -> String {
    let done = crate::tests_route::turn(&r.core, Some(sid), SAID, None).await;
    let judge = r.core.runner.judge.clone();
    tokio::task::spawn_blocking(move || judge.flush_sink())
        .await
        .unwrap();
    done.turn_id
}

fn timeline(r: &Rig, category: &str) -> Vec<exchange::Touch> {
    crate::names::topics::timeline(&r.core.store, &id(category), 10).unwrap()
}

/// KEY: a listed topic chosen at the act band touches the exchange of the
/// message it was asked for, at the exchange's own time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_topic_chosen_at_the_act_band_touches_the_messages_exchange() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_category(&r);
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    let ex = exchange::exchange(&r.core.store, &turn)
        .unwrap()
        .expect("the turn's exchange");
    let [t] = timeline(&r, "topic:ledger").try_into().unwrap_or_else(|v| {
        panic!("one touch: {v:?}");
    });
    assert_eq!(
        (t.exchange.as_str(), t.session_id.as_str(), t.at_ms),
        (turn.as_str(), ex.session_id.as_str(), ex.start_ms)
    );
}

/// A topic the session already holds writes no filing, and its exchange is
/// touched all the same: the exchange was about it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_topic_the_session_already_holds_is_touched_all_the_same() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_category(&r);
    let sid = bound(&r.core);
    operator_writes(
        &r,
        vec![Record::Members(MemberList {
            session: sid.clone(),
            kind: "topic".into(),
            members: vec![Membership::operator(id("topic:ledger"), 1_000)],
        })],
    );
    let turn = said(&r, &sid).await;
    let touched: Vec<String> = timeline(&r, "topic:ledger")
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(touched, vec![turn]);
}

/// A topic minted at the act band starts its timeline with the message that
/// minted it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_minted_topic_starts_its_timeline_with_the_message_that_minted_it() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.9);
    jev.script_onto("topic_name", "n1", 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let minted = o.member_list(&sid, "topic").expect("filed").members[0]
        .category
        .clone();
    let touched = crate::names::topics::timeline(&r.core.store, &minted, 10).unwrap();
    assert_eq!(touched.len(), 1, "{touched:?}");
    assert_eq!(touched[0].exchange, turn);
}

/// Below the act band, and with the ledger off, no topic is touched.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn below_the_act_band_or_with_the_ledger_off_no_topic_is_touched() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.7);
    let r = rig(jev);
    ledger_category(&r);
    let sid = bound(&r.core);
    said(&r, &sid).await;
    assert!(timeline(&r, "topic:ledger").is_empty(), "the confirm band");

    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig_with(jev, Vec::new(), |c| c.context.ledger = Some(false));
    ledger_category(&r);
    let sid = bound(&r.core);
    said(&r, &sid).await;
    assert!(timeline(&r, "topic:ledger").is_empty(), "the ledger off");
    let place = exchange::newest(&r.core.store, Root::Place("discord:dm:42"), 5).unwrap();
    assert!(place.is_empty(), "no exchange either");
}
