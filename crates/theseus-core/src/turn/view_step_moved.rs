//! The node seal at read time (issue 44): an exchange of a DM whose message a split moved into a sealed
//! thread is absent from the DM's window facts, from a history page (whose counts do not betray it), and
//! from a fold built before the move (`GatedLadder`); the thread's own reader still reads it; an exchange
//! with no moved node is untouched.

use super::GatedLadder;
use crate::exchange::{self, Exchange};
use crate::history_tool::page::Lens;
use crate::ladder::tests_moved::{folded, history, move_ask, thread, PLACE};
use crate::ladder::view::StoreLadder;
use crate::ladder::N;
use crate::store::Store;
use crate::view::fold::Ladder as _;
use crate::view::history::{HistoryArgs, PageEnd};
use crate::view::select::sources::Exchanges as _;
use crate::view::Present;
use crate::view_gate::Moved;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

fn board(s: &Store) -> Option<std::sync::Arc<crate::threads::Board>> {
    Some(crate::threads::Threads::default().all(s).unwrap())
}

fn ex(s: &Store, turn: &str) -> Exchange {
    exchange::exchange(s, turn).unwrap().unwrap()
}

fn clear(_: &str) -> bool {
    false
}

fn private(_: &str) -> bool {
    true
}

/// The newest history page of the DM, the owner alone, for `moved`.
fn page(s: &Store, moved: &Moved<'_>) -> (Vec<String>, PageEnd) {
    let held = |e: &Exchange| moved.holds(e);
    let lens = Lens {
        store: s,
        here: PLACE.into(),
        present: Present::OwnerAlone,
        hidden: &clear,
        moved: &held,
        private: &private,
        released: &crate::history_tool::page::none_released,
        in_view: None,
        words: None,
    };
    let p = lens.walk(&HistoryArgs::default(), None, 0).unwrap().page;
    (p.lines.into_iter().map(|l| l.handle.0).collect(), p.end)
}

#[test]
fn a_moved_message_holds_its_exchange_for_this_reader_and_not_for_its_thread() {
    let (_d, s) = store();
    let kelp = thread(&s, "kelp", true);
    let open = thread(&s, "open", false);
    let turns = history(&s, 3);
    move_ask(&s, &turns[0], "kelp");
    move_ask(&s, &turns[1], "open");
    let dm = Moved::of(&s, board(&s), Some("ses_dm"));
    let held: Vec<bool> = turns.iter().map(|t| dm.holds(&ex(&s, t))).collect();
    assert_eq!(
        held,
        [true, false, false],
        "only the sealed thread's move holds"
    );
    let own = Moved::of(&s, board(&s), Some(kelp.as_str()));
    assert!(
        !own.holds(&ex(&s, &turns[0])),
        "the thread's own reader reads it"
    );
    let unread = Moved::of(&s, None, Some(open.as_str()));
    assert!(
        unread.holds(&ex(&s, &turns[1])),
        "an unread board holds every moved node"
    );
    assert!(!unread.holds(&ex(&s, &turns[2])), "and nothing never moved");
}

#[test]
fn the_window_marks_an_exchange_with_a_moved_message_sealed() {
    let (_d, s) = store();
    thread(&s, "kelp", true);
    let turns = history(&s, 2);
    move_ask(&s, &turns[0], "kelp");
    let moved = Moved::of(&s, board(&s), Some("ses_dm"));
    let held = |e: &Exchange| moved.holds(e);
    let sessions = crate::view::sources_store::Sessions::default();
    let src = crate::view::sources_store::StoreSources {
        store: &s,
        mark: None,
        recalled: Vec::new(),
        sealed: &clear,
        moved: &held,
        sessions: &sessions,
    };
    let sealed = |t: &str| src.facts(t).unwrap().unwrap().marks.sealed;
    assert!(sealed(&turns[0]), "held whole by the gate");
    assert!(
        !sealed(&turns[1]),
        "an exchange with no moved node is untouched"
    );
}

#[test]
fn a_history_page_leaves_it_out_before_counting() {
    let (_d, s) = store();
    thread(&s, "kelp", true);
    let turns = history(&s, 101);
    let (lines, end) = page(&s, &Moved::of(&s, board(&s), Some("ses_dm")));
    assert_eq!(
        (lines.len(), end),
        (100, PageEnd::More),
        "101 readable: a page and more"
    );
    move_ask(&s, &turns[50], "kelp");
    let (lines, end) = page(&s, &Moved::of(&s, board(&s), Some("ses_dm")));
    assert!(!lines.contains(&turns[50]));
    assert_eq!(
        (lines.len(), end),
        (100, PageEnd::HistoryExhausted),
        "100 readable: the page says nothing of the one held"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fold_built_before_the_move_is_held_whole() {
    let (_d, s) = store();
    thread(&s, "kelp", true);
    let turns = folded(&s).await;
    let read = |moved: &Moved<'_>| {
        let held = |e: &Exchange| moved.holds(e);
        let inner = StoreLadder(&s);
        let gated = GatedLadder {
            inner: &inner,
            store: &s,
            held: &clear,
            moved: &held,
            sessions: Default::default(),
        };
        gated.read(PLACE, N, None).unwrap().nodes.len()
    };
    assert_eq!(read(&Moved::of(&s, board(&s), Some("ses_dm"))), 1);
    move_ask(&s, &turns[3], "kelp");
    assert_eq!(
        read(&Moved::of(&s, board(&s), Some("ses_dm"))),
        0,
        "its words and account go"
    );
}
