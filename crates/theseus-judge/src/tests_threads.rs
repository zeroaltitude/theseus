//! `classify.v2`'s `thread` question (theseus-rha2): one option per live
//! thread beside `new_thread` and `home`, asked only when the place has
//! threads, on the same state and so the same request as `role.v1`; and
//! `classify.v1` unchanged, on a stored inbound state that holds no threads.

use crate::builders::{prepare, InboundInput, Input, ThreadInput, LIVE_THREADS};
use crate::client::Question;
use crate::pack::{by_name, Source};
use crate::state::NoScrub;

fn input() -> InboundInput {
    serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap()
}

fn thread(id: &str, title: &str) -> ThreadInput {
    ThreadInput {
        id: id.into(),
        title: title.into(),
        goal: format!("Finish the {title} work.\nSecond line, left out."),
        entities: vec!["repo-tides".into(), "#412".into(), "src/moon.rs".into()],
    }
}

fn ids(q: &Question) -> Vec<String> {
    let Question::Choice { options, .. } = q else {
        panic!("a choice")
    };
    options.iter().map(|o| o.id.clone()).collect()
}

/// With live threads, the question has one option for each, then
/// `new_thread` and `home`, and Jev reads each thread's title, goal's first
/// line and entities.
#[test]
fn classify_v2_offers_each_live_thread_beside_new_and_home() {
    let p = by_name("classify.v2").unwrap();
    let mut i = input();
    i.live_threads = vec![
        thread("pr-round", "pr round"),
        thread("security-fix", "security fix"),
    ];
    let prepared = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    let asked = p.ask(&prepared.dynamic);
    let q = asked.iter().find(|a| a.id == "thread").expect("asked");
    assert_eq!(
        ids(&q.question),
        ["pr-round", "security-fix", "new_thread", "home"]
    );
    let Question::Choice { options, .. } = &q.question else {
        panic!()
    };
    let first = options[0].means.as_deref().unwrap();
    assert!(first.contains("\u{ab}pr round\u{bb}"), "{first}");
    assert!(first.contains("Finish the pr round work."), "{first}");
    assert!(!first.contains("Second line"), "{first}");
    assert!(first.contains("repo-tides, #412, src/moon.rs"), "{first}");
    assert_eq!(prepared.dynamic.items(Source::Threads).len(), 2);
    // The state lists them, so role.v1 and route.v4, on the same state,
    // ride the same request.
    assert!(prepared.state.value()["live_threads"].is_array());
}

/// No threads: the question is not asked, the state is byte for byte the
/// one `classify.v1` has, and v2's other questions are v1's, word for word.
#[test]
fn classify_v2_without_threads_is_classify_v1() {
    let (v1, v2) = (
        by_name("classify.v1").unwrap(),
        by_name("classify.v2").unwrap(),
    );
    let i = input();
    assert!(i.live_threads.is_empty());
    let a = prepare(&v1, &Input::Inbound(i.clone()), &NoScrub).unwrap();
    let b = prepare(&v2, &Input::Inbound(i), &NoScrub).unwrap();
    assert_eq!(a.state.json, b.state.json);
    assert!(b.state.value().get("live_threads").is_none());
    let (qa, qb) = (v1.ask(&a.dynamic), v2.ask(&b.dynamic));
    assert!(qb.iter().all(|q| q.id != "thread"));
    assert_eq!(qa.len(), qb.len());
    for (x, y) in qa.iter().zip(&qb) {
        assert_eq!((&x.id, &x.question), (&y.id, &y.question));
        assert_eq!(x.thresholds, y.thresholds);
    }
}

/// A stored inbound state from before threads decodes, and the roles and the
/// route pack read it as they did.
#[test]
fn an_input_stored_before_threads_still_decodes() {
    let text = include_str!("../fixtures/inputs/inbound.json");
    assert!(!text.contains("live_threads"));
    let i: InboundInput = serde_json::from_str(text).unwrap();
    assert!(i.live_threads.is_empty());
    let back = serde_json::to_string(&i).unwrap();
    assert!(
        !back.contains("live_threads"),
        "an empty list is not written"
    );
}

/// The question offers at most fifty threads, and says how many it left out.
#[test]
fn the_thread_question_offers_fifty_at_most() {
    let p = by_name("classify.v2").unwrap();
    let mut i = input();
    i.live_threads = (0..80)
        .map(|n| thread(&format!("t{n}"), &format!("t {n}")))
        .collect();
    let prepared = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    let q = p
        .ask(&prepared.dynamic)
        .into_iter()
        .find(|a| a.id == "thread")
        .unwrap();
    assert_eq!(ids(&q.question).len(), LIVE_THREADS + 2);
    assert!(prepared.state.tokens <= p.state_cap_tokens);
}

/// `classify.v3` is `classify.v2` and exactly one question, `offer_task`, which
/// decides nothing (theseus-bh3t.5): every other question, its wording and its
/// bars, is v2's word for word, with and without live threads.
#[test]
fn classify_v3_is_classify_v2_and_an_offer_question() {
    let (v2, v3) = (
        by_name("classify.v2").unwrap(),
        by_name("classify.v3").unwrap(),
    );
    assert_eq!((v3.id.as_str(), v3.version), ("classify", 3));
    assert_eq!(v2.rollback, v3.rollback);
    assert_eq!(v2.point, v3.point);
    assert_eq!(v2.builder, v3.builder);
    for threads in [vec![], vec![thread("t1", "Moon"), thread("t2", "Tides")]] {
        let mut i = input();
        i.live_threads = threads;
        let a = prepare(&v2, &Input::Inbound(i.clone()), &NoScrub).unwrap();
        let b = prepare(&v3, &Input::Inbound(i), &NoScrub).unwrap();
        assert_eq!(a.state.json, b.state.json);
        let (qa, qb) = (v2.ask(&a.dynamic), v3.ask(&b.dynamic));
        let extra: Vec<&str> = qb
            .iter()
            .filter(|q| !qa.iter().any(|x| x.id == q.id))
            .map(|q| q.id.as_str())
            .collect();
        assert_eq!(extra, ["offer_task"]);
        assert_eq!(qa.len() + 1, qb.len());
        for x in &qa {
            let y = qb.iter().find(|y| y.id == x.id).unwrap();
            assert_eq!((&x.question, x.thresholds), (&y.question, y.thresholds));
        }
        let offer = qb.iter().find(|q| q.id == "offer_task").unwrap();
        assert!(
            !v3.questions
                .iter()
                .find(|q| q.id == "offer_task")
                .unwrap()
                .decides
        );
        assert_eq!(
            (offer.thresholds.act, offer.thresholds.confirm),
            (0.90, 0.60)
        );
    }
}
