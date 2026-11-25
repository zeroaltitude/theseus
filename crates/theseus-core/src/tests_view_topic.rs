//! The topic block over a real store (issue 48; `view::select::beside`, `view::sources_names`): a topic the
//! judge filed one of the last exchanges here under brings its card into the next turn's view, in slot H,
//! and the block is held to `[context] topic_max_tokens`.

use theseus_ontology::{Category, CategoryId, Origin, Record};

use crate::exchange::{self, Root};
use crate::tests_places::{rig_with, Rig};
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{asked, texts, turn_as};
use crate::view::budget::DropWhy;
use crate::view::select::Band;
use crate::view::{Slot, Space};

const FIRST: &str = "the harbour log is late again";
const NEXT: &str = "and what comes next?";

/// `n` of the owner's topics, each filed under the first exchange here as `names::topics` files it (a touch at
/// the exchange's start), then the next turn: the rig, and that turn's id.
async fn filed(n: usize, tweak: impl FnOnce(&mut crate::config::Config)) -> (Rig, String) {
    let r = rig_with(tweak);
    let c = &r.core;
    let ids: Vec<CategoryId> = (0..n)
        .map(|i| CategoryId::parse(&format!("topic:harbour-{i:02}")).unwrap())
        .collect();
    let cats = (ids.iter().enumerate())
        .map(|(i, id)| Record::Category(Category::new(id.clone(), format!("Harbour {i}"), "owner")))
        .collect();
    (c.runner.ontology)
        .write(&c.store, cats, Origin::Operator, |_| Ok(vec![]))
        .unwrap();
    let sid = session_in(c, None, &r.root);
    let first = turn_as(c, &sid, FIRST, true).await;
    let ex = (exchange::exchange(&c.store, &first.turn_id).unwrap()).expect("the first exchange");
    let touches: Vec<_> = (ids.iter())
        .filter_map(|id| {
            exchange::touch(Root::Thing(id.as_str()), &first.turn_id, &sid, ex.start_ms)
        })
        .collect();
    c.store.append(&touches).unwrap();
    let next = turn_as(c, &sid, NEXT, true).await;
    (r, next.turn_id)
}

/// The topic cards the turn's views showed (in slot H, by its band) and dropped over the budget.
fn topic_cards(r: &Rig, turn: &str) -> (Vec<(Band, Slot, String)>, usize) {
    let records = crate::view::record::of_turn(&r.core.store, turn).unwrap();
    assert!(!records.is_empty(), "the turn was a view turn");
    let topic = |s: &Space| match s {
        Space::Card { thing, .. } if thing.starts_with("topic:") => Some(thing.clone()),
        _ => None,
    };
    let shown = (records.iter().flat_map(|v| &v.shown))
        .filter_map(|s| topic(&s.piece.range.space).map(|t| (s.piece.band, s.piece.slot, t)))
        .collect();
    let dropped = (records.iter().flat_map(|v| &v.dropped))
        .filter(|d| d.why == DropWhy::OverBudget && topic(&d.range.space).is_some())
        .count();
    (shown, dropped)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_topic_the_judge_filed_the_last_exchange_under_brings_its_card() {
    let (r, next) = filed(1, |_| {}).await;
    let all = texts(&asked(&r, NEXT).pop().expect("the next turn's request"));
    assert!(
        all.contains("Harbour 0 (topic:harbour-00)"),
        "the filed topic's card is beside the message: {all}"
    );
    let (shown, dropped) = topic_cards(&r, &next);
    assert!(
        shown
            .iter()
            .all(|s| *s == (Band::InPlay, Slot::Called, "topic:harbour-00".into())),
        "{shown:?}"
    );
    assert!(!shown.is_empty());
    assert_eq!(dropped, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_topic_block_is_held_to_the_cap_its_setting_names() {
    let (r, next) = filed(40, |_| {}).await;
    let (shown, dropped) = topic_cards(&r, &next);
    assert!(!shown.is_empty());
    assert_eq!(dropped, 0, "forty cards fit the default cap");
    let (r, next) = filed(40, |c| c.context.topic_max_tokens = Some(1_000)).await;
    let (_, dropped) = topic_cards(&r, &next);
    assert!(dropped > 0, "at 1000 tokens the block is cut");
    let records = crate::view::record::of_turn(&r.core.store, &next).unwrap();
    for v in &records {
        let block: u64 = (v.shown.iter())
            .filter(|s| s.piece.slot == Slot::Called)
            .map(|s| s.tokens)
            .sum();
        assert!(block <= 1_000, "the block within its cap: {block}");
    }
}
