//! The view's sources over a real store (issue 44; `view::sources_names`): what a message names reaches a
//! turn in a place set to `view` as its thing's card, through the turn's own gate. A thing the people
//! present may not see is no name, so it brings nothing and is counted nowhere.

use std::sync::Arc;

use theseus_protocol::index::{IndexNeighboursParams, IndexNeighboursResult};

use crate::config::assembly::{Assembly, AssemblySetting};
use crate::exchange::{self, Root};
use crate::memory_pass::{IndexFuture, PassIndex};
use crate::tender::TenderMiss;
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{asked, kelp, texts, turn_as, view_rig};
use crate::view::select::Band;
use crate::view::Space;

/// An index that names "issue 33" as a numbered item, as the tender's extractor would.
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
            (t.contains("issue 33").then(|| "item:issue-33".to_string()))
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

/// Until the naming task has touched `term`, a bounded wait.
async fn touched(store: &crate::store::Store, term: &str) {
    for _ in 0..50 {
        if !(exchange::newest(store, Root::Thing(term), 5).unwrap()).is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{term} was never touched");
}

/// The pieces of a named thing's card the turn's view chose, showed or dropped, and the residue of the
/// bands that call things.
fn cards_counted(store: &crate::store::Store, turn: &str) -> usize {
    let records = crate::view::record::of_turn(store, turn).unwrap();
    assert!(!records.is_empty(), "the turn was a view turn");
    let card = |s: &Space| matches!(s, Space::Card { .. });
    (records.iter())
        .map(|r| {
            r.shown
                .iter()
                .filter(|s| card(&s.piece.range.space))
                .count()
                + r.dropped.iter().filter(|d| card(&d.range.space)).count()
                + (r.residue.iter())
                    .filter(|x| matches!(x.band, Band::Named | Band::InPlay))
                    .count()
        })
        .sum()
}

/// KEY: a message naming a numbered item brings the item's card beside it: the item, and the words of the
/// exchange it was said in before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_naming_a_numbered_item_brings_its_card() {
    let r = view_rig();
    let c = &r.core;
    c.runner.pass.set_index(Arc::new(Items));
    let sid = session_in(c, None, &r.root);
    turn_as(c, &sid, "the tide gauge: see issue 33 first", true).await;
    touched(&c.store, "item:issue-33").await;
    let t = turn_as(c, &sid, "and where is issue 33 now?", true).await;
    let q = asked(&r, "and where is issue 33 now?").pop().unwrap();
    let all = texts(&q);
    assert!(all.contains("Item: issue-33 (item:issue-33)"), "{all}");
    assert!(
        all.contains("\"the tide gauge: see issue 33 first\""),
        "{all}"
    );
    assert!(cards_counted(&c.store, &t.turn_id) >= 1);
}

/// A rig in `view` with threads on and the thread `kelp`, sealed as `seal` says.
fn kelp_rig(seal: bool) -> crate::tests_places::Rig {
    crate::tests_places::rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| {
            kelp(s, seal);
        },
    )
}

const KELP_CARD: &str = "Line of work: kelp";

/// KEY: a line of work named by its key brings its card to the owner alone; the same name said in a
/// channel (never alone, decision 5) is of another place the owner is not alone in, and brings nothing,
/// not even a count.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_of_another_place_brings_nothing_where_the_owner_is_not_alone() {
    let r = kelp_rig(false);
    let c = &r.core;
    let desk = session_in(c, None, &r.root);
    turn_as(c, &desk, "desk: what about thread:kelp?", true).await;
    let q = asked(&r, "desk: what about thread:kelp?").pop().unwrap();
    assert!(texts(&q).contains(KELP_CARD), "{}", texts(&q));

    c.bind_places(vec![crate::places::BoundPlace {
        persona: None,
        target: format!("discord:channel:{}", crate::tests_places::LAB),
        name: "#lab".into(),
        private: true,
        guild: Some("100000000000000001".into()),
        ceiling: None,
    }]);
    let lab = format!("channel:{}", crate::tests_places::LAB);
    let channel = crate::tests_places::session(c, Some(&lab));
    let t = turn_as(c, &channel, "lab: what about thread:kelp?", true).await;
    let q = asked(&r, "lab: what about thread:kelp?").pop().unwrap();
    assert!(!texts(&q).contains(KELP_CARD), "{}", texts(&q));
    assert_eq!(cards_counted(&c.store, &t.turn_id), 0);
}

/// KEY: a sealed thread's thing is not named: its card is absent, and nothing of it is counted in the
/// view's record, shown, dropped or as residue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sealed_threads_thing_is_absent_and_uncounted() {
    let r = kelp_rig(true);
    let c = &r.core;
    let desk = session_in(c, None, &r.root);
    let t = turn_as(c, &desk, "desk: what about thread:kelp?", true).await;
    let q = asked(&r, "desk: what about thread:kelp?").pop().unwrap();
    assert!(!texts(&q).contains(KELP_CARD), "{}", texts(&q));
    assert_eq!(cards_counted(&c.store, &t.turn_id), 0);
}
