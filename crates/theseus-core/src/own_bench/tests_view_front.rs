//! A view turn's front kept per block (issue 44, `compiler::view::kept`): turns on either side of a minute,
//! with the owner's person and guidance (its "Who is speaking" piece is stamped with the minute) and a large
//! ladder fold that does not change, and then a new small fold: the later turn stores only the few blocks
//! that changed, never the large fold again, and every turn is rebuilt as it was sent.

use crate::compiler::view::kept::Kept;
use crate::tests_view_compile::{asked, kelp, turn_as};

/// A fold over the exchange `turn` of the thread's place, its account `account`.
fn fold(c: &crate::Core, id: &str, turn: &str, account: String) {
    let ex = crate::exchange::exchange(&c.store, turn).unwrap().unwrap();
    let node = crate::ladder::fold::FoldNode {
        id: id.into(),
        place: ex.place.clone(),
        level: 1,
        exchanges: vec![turn.into()],
        first_ms: ex.start_ms,
        last_ms: ex.start_ms,
        his: vec![],
        counted: 0,
        children: vec![],
        title: id.into(),
        account,
        marks: crate::view::Marks {
            place: Some(ex.place.clone()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        model: None,
        writer: 1,
        at_ms: ex.start_ms,
    };
    (c.store)
        .append(&crate::ladder::fold::records(&node).unwrap())
        .unwrap();
}

/// The owner's held person, with guidance for it.
fn person(c: &crate::Core) {
    use theseus_ontology::{Category, CategoryId, Guidance, Origin, Record};
    let me = || CategoryId::parse(&format!("person:{}", crate::tests_places::OWNER)).unwrap();
    (c.runner.ontology)
        .write(
            &c.store,
            vec![
                Record::Category(Category::new(me(), "Ada", "ada")),
                Record::Guidance(Guidance::new(me(), "Answer Ada in short lines.", 1, "ada")),
            ],
            Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
}

fn kept_of(c: &crate::Core, turn: &str) -> Kept {
    let v = crate::view::record::newest(&c.store, turn).unwrap();
    v.expect("a view record").kept.expect("kept for a replay")
}

/// The blobs a turn's front and beside block name.
fn named(k: &Kept) -> Vec<String> {
    k.blocks.iter().chain(&k.beside).cloned().collect()
}

#[tokio::test]
async fn a_front_kept_per_block_stores_only_what_changed_across_a_minute() {
    const MINUTE: u64 = 60_000;
    // A fixed noon and ten seconds, so the next turns are in the same minute, then the next.
    let t0 = 1_767_787_210_000;
    theseus_protocol::time::move_clock_to(t0);
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup(|_| {}, |s| *thread.borrow_mut() = kelp(s, false));
    let c = &r.core;
    let thread = thread.into_inner();
    person(c);
    let one = turn_as(c, &thread, "kelp: the hives wintered", true)
        .await
        .turn_id;
    let two = turn_as(c, &thread, "kelp: the queen was lost", true)
        .await
        .turn_id;
    // A large fold that never changes.
    let large = format!(
        "ACCOUNT: {}",
        "the hives wintered in the orchard; ".repeat(1_500)
    );
    assert!(large.len() > 40_000);
    fold(c, "fold-large", &one, large.clone());
    let desk = crate::tests_places::session(c, None);
    theseus_protocol::time::move_clock_to(t0 + 1_000);
    let a = turn_as(c, &desk, "desk: first", true).await.turn_id;
    // A minute on, and a new small fold.
    theseus_protocol::time::move_clock_to(t0 + MINUTE + 1_000);
    fold(c, "fold-small", &two, "ACCOUNT: the queen was lost".into());
    let b = turn_as(c, &desk, "desk: second", true).await.turn_id;
    theseus_protocol::time::reset_clock();
    let (ka, kb) = (kept_of(c, &a), kept_of(c, &b));
    assert!(
        ka.front.is_empty() && !ka.blocks.is_empty(),
        "kept by block: {ka:?}"
    );
    let blobs = c.store.blobs();
    let size = |d: &String| blobs.read(d).map_or(0, |b| b.len());
    // The large fold is one block, the same blob for both turns.
    let big = (ka.blocks.iter()).max_by_key(|d| size(d)).unwrap().clone();
    assert!(
        size(&big) >= large.len(),
        "the large fold is a block of its own"
    );
    assert!(
        kb.blocks.contains(&big),
        "the unchanged fold is not stored again"
    );
    // What the later turn named that the earlier did not: the new fold and what the minute changed, a few
    // bytes; never the large fold.
    let earlier = named(&ka);
    let new: usize = (named(&kb).iter())
        .filter(|d| !earlier.contains(d))
        .map(size)
        .sum();
    assert!(kb.blocks != ka.blocks, "the front changed: a new fold");
    assert!(
        new < 4_000,
        "the later turn stored {new} new bytes of front and beside"
    );
    // Both turns are rebuilt as they were sent.
    for (t, said) in [(&a, "desk: first"), (&b, "desk: second")] {
        let got = c.rebuild_first(&desk, t).expect("rebuildable");
        assert_eq!(
            got.request,
            asked(&r, said).remove(0),
            "{said} byte for byte"
        );
    }
}
