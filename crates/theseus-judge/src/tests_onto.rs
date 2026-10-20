//! `onto.v1` (theseus-xzxqv): its pack, its state and options, and a scripted
//! judgment's three bands.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::band::Band;
use crate::breaker::BreakerConfig;
use crate::builders::{
    prepare, InboundInput, Input, OntoInput, OntoItem, OntoKind, OntoTable, OntoText,
    ONTO_CANDIDATES,
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
    i.onto = Some(Box::new(OntoInput {
        thread_title: Some("Lantern roll-out".into()),
        project_title: None,
        kinds,
        table: vec![OntoTable {
            name: "topic".into(),
            meaning: "a subject of work".into(),
        }],
        suggested: Vec::new(),
        ..OntoInput::default()
    }));
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

// ------------------------------------------------- pictures and attachments

fn picture(name: &str, w: u32, h: u32) -> crate::pic::Picture {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        // Noise, so the picture's bytes are large however it is encoded.
        let n =
            (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)).wrapping_mul(2_654_435_761);
        image::Rgb([(n >> 8) as u8, (n >> 16) as u8, (n >> 24) as u8])
    });
    let mut raw = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut raw), image::ImageFormat::Png)
        .unwrap();
    crate::pic::Picture {
        name: name.into(),
        shown: crate::pic::prepare(&raw),
    }
}

fn with_pictures(shared: bool, pictures: Vec<crate::pic::Picture>) -> InboundInput {
    let mut i = with_onto(vec![
        kind("topic", vec![item("topic:a", "Alpha", "")]),
        kind("project", vec![item("project:a", "Alpha", "")]),
    ]);
    let o = i.onto.as_mut().unwrap();
    o.pictures = pictures;
    o.shared = shared;
    i
}

fn asked_ids(p: &crate::pack::Pack, d: &crate::pack::Dynamic) -> Vec<String> {
    p.ask(d).into_iter().map(|a| a.id).collect()
}

/// A picture is in the state once, by reference, and the asset travels
/// beside it; the image questions are asked and the others are as before.
#[test]
fn a_picture_is_referenced_once_and_its_questions_are_asked() {
    let pic = picture("shot.png", 640, 480);
    let id = pic.shown.as_ref().unwrap().id();
    let (state, prepared) = onto_state(with_pictures(false, vec![pic]));
    assert_eq!(state["images"], json!([{"name": "shot.png", "image": id}]));
    assert_eq!(prepared.dynamic.assets.len(), 1);
    assert_eq!(prepared.dynamic.assets[0].id, id);
    assert_eq!(
        prepared.state.json.matches(&id).count(),
        1,
        "each reference is billed again"
    );
    assert!(!prepared.state.json.contains("data_base64"));
    let p = by_name("onto.v1").unwrap();
    let ids = asked_ids(&p, &prepared.dynamic);
    for q in [
        "image_subject",
        "shows_people",
        "shows_screen_or_document",
        "shows_identifiers",
        "topic",
        "personal",
    ] {
        assert!(ids.contains(&q.to_string()), "{q}");
    }
}

/// Without a picture none of the image questions is asked and the state has
/// none of the new fields.
#[test]
fn no_picture_no_image_questions() {
    let (state, prepared) = onto_state(with_pictures(false, Vec::new()));
    for f in ["images", "skipped_images", "attachments"] {
        assert!(state.get(f).is_none(), "{f}");
    }
    assert!(prepared.dynamic.assets.is_empty());
    let p = by_name("onto.v1").unwrap();
    let ids = asked_ids(&p, &prepared.dynamic);
    for q in [
        "image_subject",
        "shows_people",
        "shows_screen_or_document",
        "shows_identifiers",
    ] {
        assert!(!ids.contains(&q.to_string()), "{q}");
    }
}

/// A picture that was skipped is in the state with its reason, and sends no
/// asset: the record says what was left out.
#[test]
fn a_skipped_picture_is_a_row_in_the_state() {
    let big = crate::pic::Picture {
        name: "scan.tiff".into(),
        shown: crate::pic::prepare(&vec![0u8; 25 << 20]),
    };
    let (state, prepared) = onto_state(with_pictures(false, vec![big]));
    assert_eq!(
        state["skipped_images"],
        json!([{"name": "scan.tiff", "why": "skipped: 25 MB is over the 20 MB limit"}])
    );
    assert!(state.get("images").is_none());
    assert!(prepared.dynamic.assets.is_empty());
    let p = by_name("onto.v1").unwrap();
    assert!(!asked_ids(&p, &prepared.dynamic).contains(&"image_subject".to_string()));
}

/// A text attachment's first 2,000 characters are the state's, no more.
#[test]
fn a_text_attachment_keeps_its_first_2000_characters() {
    let mut i = with_pictures(false, Vec::new());
    let text: String = (0..5000)
        .map(|n| char::from(b'a' + (n % 26) as u8))
        .collect();
    i.onto.as_mut().unwrap().texts = vec![OntoText {
        name: "notes.md".into(),
        text: text.clone(),
    }];
    let (state, _) = onto_state(i);
    let excerpt = state["attachments"][0]["excerpt"].as_str().unwrap();
    assert!(excerpt.starts_with(&text[..1990]));
    assert!(!excerpt.contains(&text[..2100]));
    assert!(excerpt.chars().count() <= 2001);
    assert_eq!(state["attachments"][0]["name"], "notes.md");
}

/// A picture from a shared place gets only `image_subject` and the privacy
/// questions, and no candidate, membership, title or name of any session.
#[test]
fn a_shared_places_picture_gets_no_candidates() {
    let (state, prepared) = onto_state(with_pictures(true, vec![picture("shot.png", 640, 480)]));
    for f in ["thread", "project", "memberships", "mined_names"] {
        assert!(state.get(f).is_none(), "{f} was sent from a shared place");
    }
    assert!(state.get("images").is_some());
    let p = by_name("onto.v1").unwrap();
    let asked = p.ask(&prepared.dynamic);
    let ids: Vec<&str> = asked.iter().map(|a| a.id.as_str()).collect();
    for q in [
        "image_subject",
        "shows_people",
        "shows_screen_or_document",
        "shows_identifiers",
        "personal_applies",
        "elicits",
    ] {
        assert!(ids.contains(&q), "{q}");
    }
    for q in [
        "topic",
        "project",
        "event",
        "resource",
        "capability",
        "expertise",
        "kind_of_thing",
    ] {
        assert!(!ids.contains(&q), "{q} was asked in a shared place");
    }
    assert!(
        !ids.iter()
            .any(|q| q.ends_with("_name") || q.starts_with("still_")),
        "{ids:?}"
    );
    // No candidate text reaches any option.
    let wire = serde_json::to_string(&asked.iter().map(|a| a.question.clone()).collect::<Vec<_>>())
        .unwrap();
    assert!(!wire.contains("Alpha"));
    assert!(!prepared.state.json.contains("Alpha"));
    assert!(!prepared.state.json.contains("Lantern"));
    // And the same input from a private place is asked as before.
    let (_, private) = onto_state(with_pictures(false, vec![picture("shot.png", 640, 480)]));
    assert!(asked_ids(&p, &private.dynamic).contains(&"topic".to_string()));
}

fn request_for(prepared: &crate::builders::Prepared) -> crate::client::Request {
    let p = by_name("onto.v1").unwrap();
    let questions = p
        .ask(&prepared.dynamic)
        .into_iter()
        .map(|a| (a.id, a.question))
        .collect();
    crate::client::Request::new(&prepared.state.value(), &p.jev_model, questions)
        .with_assets(prepared.dynamic.assets.clone())
}

/// A request with a picture sends the header and the `assets` map, every
/// asset referenced and inside Jev's limits (the fake refuses what Jev
/// refuses); a request without sends neither.
#[tokio::test]
async fn a_request_with_a_picture_sends_the_header_and_its_assets() {
    let fake = FakeJev::start().unwrap();
    let judge = judge_for(&fake);
    let pack = by_name("onto.v1").unwrap();
    let (_, prepared) = onto_state(with_pictures(
        false,
        vec![picture("a.png", 640, 480), picture("tiny.png", 50, 40)],
    ));
    let js = judge
        .judge(DecisionPoint {
            asks: vec![Ask::new(pack.clone(), &prepared, Mode::Live, json!({}))],
            urgency: Urgency::Shadow,
        })
        .await;
    assert!(
        js[0].answer("image_subject").is_some(),
        "{:?}",
        js[0].outcome
    );
    let seen = fake.seen();
    assert!(seen[0].images_header);
    let assets = seen[0].body["assets"].as_object().unwrap();
    assert_eq!(assets.len(), 2);
    for (id, a) in assets {
        assert_eq!(a["type"], "image");
        assert!(["png", "jpeg"].contains(&a["format"].as_str().unwrap()));
        assert!(seen[0].body["state"].to_string().contains(id.as_str()));
    }
    assert!(!js[0].call.as_ref().unwrap().images_dropped);
    // No picture: no header, no `assets` key.
    let (_, plain) = onto_state(with_pictures(false, Vec::new()));
    judge
        .judge(DecisionPoint {
            asks: vec![Ask::new(pack, &plain, Mode::Live, json!({}))],
            urgency: Urgency::Shadow,
        })
        .await;
    let seen = fake.seen();
    assert!(!seen[1].images_header);
    assert!(seen[1].body.get("assets").is_none());
}

/// Without the header the API refuses a picture (the fake keeps the rule),
/// so the header is what the client sends: a raw call over the same request.
#[test]
fn the_request_body_has_assets_only_with_pictures() {
    let (_, prepared) = onto_state(with_pictures(false, vec![picture("a.png", 640, 480)]));
    let req = request_for(&prepared);
    let body: Value = serde_json::from_str(&req.body()).unwrap();
    assert_eq!(body["assets"].as_object().unwrap().len(), 1);
    assert!(req.text_len() < req.body().len());
    let plain = req.without_assets();
    let body: Value = serde_json::from_str(&plain.body()).unwrap();
    assert!(body.get("assets").is_none());
    assert!(body["state"].to_string().contains("image not sent"));
    assert!(!body["state"].to_string().contains(&req.assets[0].id));
}

/// When Jev refuses an image, the call goes again once without them and the
/// text's answers survive; the record says the images were dropped.
#[tokio::test]
async fn an_image_refused_with_a_400_is_retried_once_without_images() {
    let fake = FakeJev::start().unwrap();
    fake.refuse_images(true);
    fake.script_onto("topic", "c1", 0.9);
    let judge = judge_for(&fake);
    let pack = by_name("onto.v1").unwrap();
    let (_, prepared) = onto_state(with_pictures(false, vec![picture("a.png", 640, 480)]));
    let js = judge
        .judge(DecisionPoint {
            asks: vec![Ask::new(pack, &prepared, Mode::Live, json!({}))],
            urgency: Urgency::Shadow,
        })
        .await;
    let topic = js[0].answer("topic").expect("the text's answers survive");
    assert_eq!(topic.band.band, Band::Act);
    assert!(js[0].call.as_ref().unwrap().images_dropped);
    let seen = fake.seen();
    assert_eq!(seen.len(), 2, "one try with images, one without");
    assert!(seen[0].body.get("assets").is_some());
    assert!(seen[1].body.get("assets").is_none());
    assert!(!seen[1].images_header);
}

/// A 400 that is not about an image is not retried.
#[tokio::test]
async fn a_400_that_names_no_image_is_not_retried() {
    let fake = FakeJev::start().unwrap();
    fake.set_mode(crate::fake::FakeMode::Status(400));
    let judge = judge_for(&fake);
    let pack = by_name("onto.v1").unwrap();
    let (_, prepared) = onto_state(with_pictures(false, vec![picture("a.png", 640, 480)]));
    let js = judge
        .judge(DecisionPoint {
            asks: vec![Ask::new(pack, &prepared, Mode::Live, json!({}))],
            urgency: Urgency::Shadow,
        })
        .await;
    assert!(js[0].answer("topic").is_none());
    assert_eq!(fake.seen().len(), 1);
}

/// The reserve of a request with a picture counts the picture as an image's
/// tokens, not its base64 bytes.
#[test]
fn a_pictures_reserve_is_its_tokens_not_its_bytes() {
    let (_, prepared) = onto_state(with_pictures(false, vec![picture("a.png", 1000, 800)]));
    let with = request_for(&prepared);
    let without = with.without_assets();
    let price = crate::price::JevPrice::jev_1_13_0();
    let (a, b) = (
        price.reserve_request(&with),
        price.reserve_request(&without),
    );
    assert!(a > b, "an image costs something");
    let asset_bytes = with.body().len() - without.body().len();
    let bytes_as_text = price.reserve_tokens(asset_bytes, 0, 0) - price.reserve_tokens(0, 0, 0);
    assert!(a - b < bytes_as_text, "not its bytes as text");
}
