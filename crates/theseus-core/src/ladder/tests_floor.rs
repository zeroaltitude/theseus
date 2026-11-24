//! The window's floor and the fold step (2026-11-24): the window keeps at least [`WINDOW_FLOOR`] exchanges
//! whole, and once it holds [`WINDOW_FLOOR`] + [`FOLD_STEP`] its oldest [`FOLD_STEP`] fold. A place whose
//! ladder was written while the window held 20 to 39 reads and goes on under the new rule: no exchange lost,
//! none laid twice, no fold made early. Every name here is invented.

use super::fold::{self, NoFlags};
use super::read::{self, Step};
use super::tests::{exchange_at, store, Fake};
use super::view::StoreLadder;
use super::{DayPurse, FOLD_STEP, WINDOW_FLOOR};
use crate::exchange::{self, Exchange};
use crate::store::Store;
use crate::view::fold::Ladder;

const PLACE: &str = "discord:channel:3030";

/// One fold step over `PLACE`, as the job at an exchange's end runs it.
async fn step(s: &Store, w: &Fake) -> read::Stepped {
    let purse = DayPurse::new(s, None, false);
    let st = Step {
        writer: w,
        flags: &NoFlags,
        purse: &purse,
    };
    read::fold_step(s, PLACE, None, &st).await.unwrap()
}

/// The window's exchanges, oldest first.
fn window(s: &Store) -> Vec<String> {
    let l = read::ladder(s, PLACE, None).unwrap();
    l.window.into_iter().map(|e| e.turn_id).collect()
}

/// The exchanges the first-level nodes hold, oldest node first.
fn held(s: &Store) -> Vec<String> {
    let nodes = fold::newest(s, PLACE, 1, 1_000).unwrap();
    nodes.into_iter().rev().flat_map(|n| n.exchanges).collect()
}

/// Every exchange is in one node or in the window, once, in order.
fn each_once(s: &Store, turns: &[String]) {
    let mut all = held(s);
    all.extend(window(s));
    assert_eq!(all, turns.to_vec());
}

#[tokio::test]
async fn the_window_holds_thirty_to_forty_nine_exchanges_and_folds_twenty_at_a_time() {
    let (_d, s) = store();
    let w = Fake::default();
    let mut turns = Vec::new();
    for i in 0..(WINDOW_FLOOR + FOLD_STEP - 1) as u64 {
        turns.push(exchange_at(&s, "ses_a", PLACE, 1_000 + i, false));
        assert!(step(&s, &w).await.made.is_empty(), "{} exchanges", i + 1);
    }
    assert_eq!(window(&s), turns, "49 exchanges: all whole, no fold");
    for i in (WINDOW_FLOOR + FOLD_STEP - 1) as u64..120 {
        turns.push(exchange_at(&s, "ses_a", PLACE, 1_000 + i, false));
        let made = step(&s, &w).await.made;
        let len = window(&s).len();
        assert!(
            (WINDOW_FLOOR..WINDOW_FLOOR + FOLD_STEP).contains(&len),
            "{} exchanges: a window of {len}",
            i + 1
        );
        if len == WINDOW_FLOOR {
            assert_eq!(made.len(), 1, "a fold at 50 leaves 30");
            assert_eq!(made[0].exchanges.len(), FOLD_STEP);
        } else {
            assert!(made.is_empty());
        }
    }
    each_once(&s, &turns);
    // 120 exchanges: folds at 50, 70, 90 and 110; the window holds the newest 40.
    assert_eq!(held(&s), turns[..80].to_vec());
    assert_eq!(window(&s), turns[80..].to_vec());
}

/// The ladder the old rule left (a window of 20 to 39, folding 20 at 40): 79 exchanges, two first-level
/// nodes of 20 written as its fold step wrote them, and a window of 39. Read, folded and laid under the new
/// rule, nothing is lost or laid twice, no fold is made before the window reaches 50, and the window starts
/// where the newest node ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_place_laddered_while_the_window_held_twenty_to_thirty_nine_reads_and_goes_on() {
    let (_d, s) = store();
    let w = Fake::default();
    let mut turns: Vec<String> = (0..79u64)
        .map(|i| exchange_at(&s, "ses_old", PLACE, 1_000 + i, false))
        .collect();
    let purse = DayPurse::new(&s, None, false);
    let st = Step {
        writer: &w,
        flags: &NoFlags,
        purse: &purse,
    };
    for k in 0..2 {
        let oldest: Vec<Exchange> = turns[k * 20..(k + 1) * 20]
            .iter()
            .map(|t| exchange::exchange(&s, t).unwrap().unwrap())
            .collect();
        read::first(&s, &oldest, &st).await.unwrap().unwrap();
    }
    // Read as it was written.
    let read = StoreLadder(&s).read(PLACE, FOLD_STEP, None).unwrap();
    assert_eq!(read.nodes.len(), 2);
    assert!(read.nodes.iter().all(|n| n.exchanges == 20));
    assert_eq!(
        read.nodes[1].last.id, turns[39],
        "the newest node ends at 40"
    );
    let ids: Vec<&String> = read.window.iter().map(|e| &e.turn_id).collect();
    assert_eq!(
        ids,
        turns[40..].iter().collect::<Vec<_>>(),
        "and the window starts after it"
    );
    each_once(&s, &turns);
    // No fold before the window reaches 50: the old rule would have folded at 40.
    assert!(step(&s, &w).await.made.is_empty(), "a window of 39");
    for i in 79..89u64 {
        turns.push(exchange_at(&s, "ses_new", PLACE, 1_000 + i, false));
        assert!(step(&s, &w).await.made.is_empty(), "a window of {}", i - 39);
    }
    // The fiftieth: the oldest 20 of the window fold, and 30 stay whole.
    turns.push(exchange_at(&s, "ses_new", PLACE, 1_089, false));
    let made = step(&s, &w).await.made;
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].exchanges, turns[40..60].to_vec());
    assert_eq!(window(&s), turns[60..].to_vec());
    each_once(&s, &turns);
    laid_once(&s, &turns);
}

/// What slot G lays from the store after the change: the three nodes, and the window's 30 exchanges, each
/// once; no exchange a node holds is laid again.
fn laid_once(s: &Store, turns: &[String]) {
    use crate::view::budget::Budget;
    use crate::view::select::sources::Sources;
    use crate::view::select::{assemble, Band, Here, Selector};
    use crate::view::sources_store::{Sessions, StoreSources};
    use crate::view::wire::{StoreSnapshot, StoreTimeline};
    use crate::view::{Anchor, PointOfView, Present, Space};
    use theseus_memory::gate::{PointOfView as RulePov, Present as RulePresent};
    let sessions = Sessions::default();
    let src = StoreSources {
        store: s,
        mark: None,
        recalled: Vec::new(),
        sessions: &sessions,
        own: None,
    };
    let gate = crate::view::permit::Permit::of(crate::view_gate::Verified::for_tests(RulePov {
        anchor: PLACE.into(),
        place: theseus_memory::recall::Place::Private,
        present: RulePresent::OwnerAlone,
    }));
    let ladder = StoreLadder(s);
    let sources = Sources {
        gate: &gate,
        exchanges: &src,
        ladder: &ladder,
        folds: &src,
        places: &src,
        waiting: &src,
        things: &src,
        names: &src,
        tasks: &src,
        recall: &src,
        people: &src,
        message: "",
    };
    let selectors: Vec<Box<dyn Selector + '_>> = [Band::Floor, Band::WindowWords, Band::Ladder]
        .into_iter()
        .map(|b| Box::new(Here::new(b, sources)) as Box<dyn Selector + '_>)
        .collect();
    let snap = StoreSnapshot {
        now_ms: 10_000,
        timeline: StoreTimeline(s),
        at_hand: None,
    };
    let pov = PointOfView {
        anchor: Anchor::Place(PLACE.into()),
        person: None,
        present: Present::OwnerAlone,
        at_hand: None,
        now_ms: 10_000,
    };
    let v = assemble(
        &pov,
        &snap,
        &selectors,
        &gate,
        &Budget::new(1_000_000, 8_000),
    )
    .unwrap()
    .expect("fits");
    let mut exchanges: Vec<String> = Vec::new();
    let mut folds = 0;
    for x in &v.shown {
        match &x.piece.range.space {
            Space::Exchanges { .. } => exchanges.push(x.piece.range.last.id.clone()),
            Space::Folds { level: 1, .. } => folds += 1,
            other => panic!("slot G laid {other:?}"),
        }
    }
    exchanges.sort_by_key(|t| turns.iter().position(|x| x == t));
    assert_eq!(folds, 3, "three nodes: two of the old rule, one of the new");
    assert_eq!(
        exchanges,
        turns[60..].to_vec(),
        "the window's 30, each once"
    );
}
