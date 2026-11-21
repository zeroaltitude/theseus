//! Recognition (issue 44, `crate::names`), through a whole core and the fake
//! Jev: the topics `onto.v1` chose touch the message's exchange.

use std::sync::Arc;

use theseus_judge::fake::FakeJev;
use theseus_ontology::{Category, CategoryId, Guidance, MemberList, Membership, Origin, Record};
use theseus_protocol::index::{IndexNeighboursParams, IndexNeighboursResult};

use crate::exchange::{self, Root};
use crate::memory_pass::{IndexFuture, PassIndex};
use crate::tender::TenderMiss;
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

/// An index that names one numbered item, as the tender's extractor would.
struct Items;

impl PassIndex for Items {
    fn neighbours(&self, _: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult> {
        Box::pin(async { Err(TenderMiss::Down("a test's index".into())) })
    }

    fn entities(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        Box::pin(async move { Ok(vec![Vec::new(); texts.len()]) })
    }

    fn items(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        let named = |t: &String| {
            t.contains("issue 33")
                .then(|| "item:issue-33".to_string())
                .into_iter()
                .collect()
        };
        let out = texts.iter().map(named).collect();
        Box::pin(async move { Ok(out) })
    }

    fn mode(&self) -> IndexFuture<String> {
        Box::pin(async { Ok("bm25_only".into()) })
    }
}

/// KEY: a numbered item said in a session that serves a project touches
/// the exchange scoped by that project; in one that serves none it is an
/// unscoped candidate, and never the scoped thing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_numbered_item_is_touched_scoped_by_the_places_project_or_unscoped() {
    let r = rig(FakeJev::start().unwrap());
    r.core.runner.pass.set_index(Arc::new(Items));
    let sid = bound(&r.core);
    r.core
        .store
        .update_session(&sid, |s| {
            s.project = Some("prj_lantern".into());
            Ok(vec![])
        })
        .unwrap();
    let said = |sid: String| {
        let core = r.core.clone();
        async move {
            crate::tests_route::turn(&core, Some(&sid), "look at issue 33 again", None)
                .await
                .turn_id
        }
    };
    let turn = said(sid).await;
    let touched = |term: &str| -> Vec<String> {
        exchange::newest(&r.core.store, Root::Thing(term), 5)
            .unwrap()
            .into_iter()
            .map(|t| t.exchange)
            .collect()
    };
    assert_eq!(touched("item:project:prj_lantern#issue-33"), vec![turn]);
    assert!(touched("item:issue-33").is_empty(), "scoped, not bare");

    let other =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&other.session_id, &other).unwrap();
    let turn = said(other.session_id.clone()).await;
    assert_eq!(
        touched("item:issue-33"),
        vec![turn],
        "an unscoped candidate"
    );
}

/// What a card's caller admits: everything, or no exchange.
struct Admit(bool);

impl crate::names::card::Admits for Admit {
    fn fact(&self, _: &theseus_protocol::StandingRecord) -> bool {
        true
    }

    fn exchange(&self, _: &crate::exchange::Exchange) -> bool {
        self.0
    }
}

/// KEY: the name table finds the topic by its name, and its card, read
/// once, holds the owner's guidance and the exchange the judge filed under
/// it, with the words that opened it; an exchange the caller's gate refuses
/// is not on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_topics_card_holds_its_guidance_and_the_exchanges_its_gate_admits() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_category(&r);
    operator_writes(
        &r,
        vec![Record::Guidance(Guidance::new(
            id("topic:ledger"),
            "Keep the ledger's own words.",
            1,
            "ada",
        ))],
    );
    let sid = bound(&r.core);
    let turn = said(&r, &sid).await;
    let table = crate::names::table::build(&r.core).unwrap();
    let found = table.find("how is the ledger");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (found[0].how, found[0].things.as_slice()),
        (
            crate::names::table::How::Unique,
            &["topic:ledger".to_string()][..]
        )
    );
    let card = crate::names::card::card(&r.core, &table, "topic:ledger", &Admit(true))
        .unwrap()
        .expect("a card");
    assert_eq!(card.title, "Topic: Ledger (topic:ledger)");
    assert_eq!(card.exchanges, vec![turn]);
    assert!(
        card.lines
            .contains(&"Guidance: Keep the ledger's own words.".to_string()),
        "{:#?}",
        card.lines
    );
    assert!(
        card.lines
            .iter()
            .any(|l| l.ends_with(&format!("\"{SAID}\""))),
        "{:#?}",
        card.lines
    );
    let refused = crate::names::card::card(&r.core, &table, "topic:ledger", &Admit(false))
        .unwrap()
        .unwrap();
    assert!(refused.exchanges.is_empty());
    assert!(
        crate::names::card::card(&r.core, &table, "topic:none", &Admit(true))
            .unwrap()
            .is_none()
    );
}

/// A gate that refuses one standing record.
struct Hides(&'static str);

impl crate::names::card::Admits for Hides {
    fn fact(&self, r: &theseus_protocol::StandingRecord) -> bool {
        r.id != self.0
    }

    fn exchange(&self, _: &crate::exchange::Exchange) -> bool {
        true
    }
}

fn fact(id: &str, text: &str, ends: Option<&str>) -> theseus_protocol::StandingRecord {
    theseus_protocol::StandingRecord {
        id: id.into(),
        kind: theseus_protocol::StandingKind::Fact,
        text: text.into(),
        scope: theseus_protocol::StandingScope::Place("thread:kiln-fire".into()),
        since_ms: 1_000,
        until_ms: None,
        source: "cli".into(),
        ends: ends.map(str::to_string),
    }
}

/// A line of work is found by its title, and its card's facts are its
/// place's standing records as they stand now: a replaced one is dropped,
/// and one the caller's gate refuses is never read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_line_of_works_card_holds_its_current_facts_as_its_gate_admits_them() {
    let r = rig(FakeJev::start().unwrap());
    let who = crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    };
    r.core
        .thread_new(
            &theseus_protocol::threads::ThreadNewParams {
                slug: "kiln-fire".into(),
                title: Some("Kiln Fire".into()),
                ..Default::default()
            },
            who,
        )
        .unwrap();
    let facts = vec![
        fact("std_a", "The kiln runs at night.", None),
        fact("std_b", "The kiln runs at dawn.", Some("std_a")),
        fact("std_hidden", "Not for this view.", None),
    ];
    r.core
        .runner
        .standing
        .write(&r.core.store, |_| Ok(facts), |_, _| Ok(vec![]))
        .unwrap();
    let table = crate::names::table::build(&r.core).unwrap();
    let found = table.find("back to the kiln fire");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].things, ["thread:kiln-fire"]);
    let card = crate::names::card::card(&r.core, &table, "thread:kiln-fire", &Hides("std_hidden"))
        .unwrap()
        .expect("a card");
    assert_eq!(card.title, "Line of work: Kiln Fire (thread:kiln-fire)");
    assert_eq!(card.facts, ["std_b"]);
    assert!(
        card.lines
            .contains(&"Fact: The kiln runs at dawn.".to_string()),
        "{:#?}",
        card.lines
    );
}
