//! `onto.v1` (theseus-xzxqv): its pack, its state and options, and a scripted
//! judgment's three bands.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::band::Band;
use crate::breaker::BreakerConfig;
use crate::builders::{
    prepare, InboundInput, Input, OntoInput, OntoItem, OntoKind, OntoTable, ONTO_CANDIDATES,
};
use crate::client::{ClientConfig, JevClient, Question, StaticKey};
use crate::fake::FakeJev;
use crate::judge::{Ask, DecisionPoint, JevJudge, Judge, MemorySink, Mode, Recording};
use crate::pack::{by_name, Source};
use crate::{price, state::NoScrub, Urgency};

fn input() -> InboundInput {
    serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap()
}

fn item(id: &str, name: &str, description: &str) -> OntoItem {
    OntoItem {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        session: false,
        thread: false,
        used_ms: 0,
    }
}

fn kind(name: &str, pool: Vec<OntoItem>) -> OntoKind {
    OntoKind {
        kind: name.into(),
        meaning: format!("what a {name} is"),
        pool,
        mined_from: Vec::new(),
    }
}

fn ids(q: &Question) -> Vec<String> {
    let Question::Choice { options, .. } = q else {
        panic!("a choice")
    };
    options.iter().map(|o| o.id.clone()).collect()
}

fn with_onto(kinds: Vec<OntoKind>) -> InboundInput {
    let mut i = input();
    i.onto = Some(OntoInput {
        thread_title: Some("Lantern roll-out".into()),
        project_title: None,
        kinds,
        table: vec![OntoTable {
            name: "topic".into(),
            meaning: "a subject of work".into(),
        }],
        suggested: Vec::new(),
    });
    i
}

fn onto_state(i: InboundInput) -> (Value, crate::builders::Prepared) {
    let p = by_name("onto.v1").unwrap();
    let prepared = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    (prepared.state.value(), prepared)
}

#[test]
fn the_pack_loads_with_the_designs_bands_and_the_shared_state() {
    let p = by_name("onto.v1").unwrap();
    assert_eq!(p.jev_model, "jev-1.13.0");
    assert_eq!(
        p.state_cap_tokens,
        by_name("classify.v3").unwrap().state_cap_tokens
    );
    for q in &p.questions {
        assert_eq!(
            (q.thresholds.act, q.thresholds.confirm),
            (0.80, 0.55),
            "{}",
            q.id
        );
    }
    let ids: Vec<&str> = p.questions.iter().map(|q| q.id.as_str()).collect();
    for want in [
        "topic",
        "project",
        "event",
        "resource",
        "capability",
        "expertise",
        "topic_name",
        "expertise_name",
        "still_topic",
        "still_expertise",
        "kind_of_thing",
        "personal",
        "elicits",
    ] {
        assert!(ids.contains(&want), "{want}");
    }
}

/// A state with no ontology fields is the one before, and with them it is
/// that plus the fields: `thread`, `memberships` and `mined_names`.
#[test]
fn the_state_with_ontology_is_todays_plus_its_fields() {
    let mut plain = input();
    plain.message = "ok, then do that".into();
    let today = prepare(
        &by_name("classify.v3").unwrap(),
        &Input::Inbound(plain),
        &NoScrub,
    )
    .unwrap()
    .state
    .value();
    let mut held = item("topic:lantern", "Lantern", "the roll-out");
    held.session = true;
    let mut i = with_onto(vec![kind("topic", vec![held])]);
    i.message = "ok, then do that".into();
    let (state, _) = onto_state(i);
    let (today, state) = (today.as_object().unwrap(), state.as_object().unwrap());
    for (k, v) in today {
        assert_eq!(state.get(k), Some(v), "{k} is as it was");
    }
    let added: Vec<&String> = state.keys().filter(|k| !today.contains_key(*k)).collect();
    assert_eq!(added, ["memberships", "mined_names", "thread"]);
    assert_eq!(
        state["mined_names"],
        json!(["Lantern roll-out"]),
        "the thread's title"
    );
    assert_eq!(state["memberships"], json!({"topic": ["Lantern"]}));
}

#[test]
fn a_kind_is_asked_when_it_is_listed_and_has_new_even_with_no_category() {
    let mut i = with_onto(vec![kind("topic", vec![])]);
    i.message = "ok, then do that".into();
    i.onto.as_mut().unwrap().thread_title = None;
    let (_, prepared) = onto_state(i);
    let p = by_name("onto.v1").unwrap();
    let asked = p.ask(&prepared.dynamic);
    let names: Vec<&str> = asked.iter().map(|a| a.id.as_str()).collect();
    assert!(names.contains(&"topic"), "{names:?}");
    assert!(!names.contains(&"project"), "a kind left out is not asked");
    assert!(
        !names.contains(&"topic_name"),
        "no mined name, no name question"
    );
    let q = asked.iter().find(|a| a.id == "topic").unwrap();
    assert_eq!(ids(&q.question), ["new", "none"]);
    assert!(names.contains(&"kind_of_thing"));
}

/// The candidates: the session's, the thread's, a word match, then the most
/// recent; at most twenty; `new` and `none` last.
#[test]
fn the_picker_orders_the_pool_and_stops_at_twenty() {
    let mut pool: Vec<OntoItem> = (0..30)
        .map(|n| {
            let mut i = item(
                &format!("topic:t{n}"),
                &format!("filler {n}"),
                "nothing here",
            );
            i.used_ms = n;
            i
        })
        .collect();
    pool[3].session = true;
    pool[4].thread = true;
    pool[5].name = "Quarterly migration".into();
    pool[5].used_ms = 0;
    let k = kind("topic", pool);
    let picked = k.pick("what is left of the migration work?");
    let order: Vec<&str> = picked.candidates.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(&order[..3], ["topic:t3", "topic:t4", "topic:t5"]);
    assert_eq!(order[3], "topic:t29", "then the most recent");
    assert_eq!(order.len(), ONTO_CANDIDATES);
    assert_eq!(picked.members.len(), 1);
    let (_, prepared) = onto_state(with_onto(vec![k]));
    let q = by_name("onto.v1").unwrap().ask(&prepared.dynamic);
    let topic = q.iter().find(|a| a.id == "topic").unwrap();
    let got = ids(&topic.question);
    assert_eq!(got.len(), ONTO_CANDIDATES + 2);
    assert_eq!(&got[got.len() - 2..], ["new", "none"]);
    assert_eq!(got[0], "c1");
    assert_eq!(prepared.dynamic.items(Source::TopicMembers).len(), 1);
}

#[test]
fn mined_names_are_options_and_a_membership_is_asked_about() {
    let mut i = with_onto(vec![kind("topic", vec![])]);
    i.message = "see https://github.com/finchworks/lantern-core.git and Lantern Ledger".into();
    let mut held = item("topic:lantern", "Lantern", "");
    held.session = true;
    i.onto.as_mut().unwrap().kinds[0].pool.push(held);
    let (state, prepared) = onto_state(i);
    assert_eq!(
        state["mined_names"],
        json!([
            "Lantern roll-out",
            "lantern-core",
            "github.com",
            "Lantern Ledger"
        ])
    );
    let asked = by_name("onto.v1").unwrap().ask(&prepared.dynamic);
    let ids_asked: Vec<&str> = asked.iter().map(|a| a.id.as_str()).collect();
    assert!(ids_asked.contains(&"topic_name"));
    assert!(ids_asked.contains(&"still_topic.1"));
    let name = asked.iter().find(|a| a.id == "topic_name").unwrap();
    assert_eq!(ids(&name.question), ["n1", "n2", "n3", "n4", "none"]);
}

#[test]
fn the_input_round_trips_through_json() {
    let i = with_onto(vec![kind("topic", vec![item("topic:a", "A", "d")])]);
    let text = serde_json::to_string(&i).unwrap();
    let back: InboundInput = serde_json::from_str(&text).unwrap();
    assert_eq!(back, i);
    assert!(!serde_json::to_string(&input()).unwrap().contains("onto"));
}

fn judge_for(fake: &FakeJev) -> Recording<JevJudge, Arc<MemorySink>> {
    let client = JevClient::new(
        ClientConfig {
            api_base: fake.base(),
            ..ClientConfig::default()
        },
        Arc::new(StaticKey::new(
            "apik-test-0123456789-abcdefghijklmnop".into(),
        )),
    )
    .unwrap();
    Recording::new(
        JevJudge::new(client, price::builtin(), BreakerConfig::default()),
        Arc::new(MemorySink::default()),
    )
}

/// A scripted judgment lands in the three bands: act at 0.9, confirm at 0.7,
/// recorded at 0.4.
#[tokio::test]
async fn a_scripted_judgment_parses_into_the_three_bands() {
    let fake = FakeJev::start().unwrap();
    fake.script_onto("topic", "c1", 0.9);
    fake.script_onto("project", "c1", 0.7);
    fake.script_onto("event", "c1", 0.4);
    let cands = |n: &str| kind(n, vec![item(&format!("{n}:a"), "Alpha", "")]);
    let (_, prepared) = onto_state(with_onto(vec![
        cands("topic"),
        cands("project"),
        cands("event"),
    ]));
    let pack = by_name("onto.v1").unwrap();
    let js = judge_for(&fake)
        .judge(DecisionPoint {
            asks: vec![Ask::new(
                pack,
                &prepared,
                Mode::Live,
                json!({"session": "s1"}),
            )],
            urgency: Urgency::Shadow,
        })
        .await;
    let band = |q: &str| js[0].answer(q).unwrap().band.band;
    assert_eq!(
        [band("topic"), band("project"), band("event")],
        [Band::Act, Band::Confirm, Band::Escalate]
    );
}

/// The golden state of the inbound input with the ontology fields: the
/// fields added to the state `classify.v3` reads, and nothing else.
#[test]
fn the_state_with_ontology_matches_its_golden() {
    let i: InboundInput = serde_json::from_str(&crate::builders::tests::onto_fixture()).unwrap();
    let (state, _) = onto_state(i);
    crate::tests::golden(
        "fixtures/golden/inbound.onto.state.json",
        &serde_json::to_string_pretty(&state).unwrap(),
    );
}

/// Names the naming call suggested come before the mined ones, and the ten
/// is the most either way.
#[test]
fn suggested_names_lead_the_mined_ones() {
    let mut i = with_onto(vec![kind("topic", vec![])]);
    i.message = "about Lantern Ledger".into();
    i.onto.as_mut().unwrap().suggested = vec!["Ledger work".into(), "lantern ledger".into()];
    let names = i.onto.as_ref().unwrap().names(&i.message, None);
    assert_eq!(names, ["Ledger work", "lantern ledger", "Lantern roll-out"]);
}
