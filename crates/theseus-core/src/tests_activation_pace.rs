//! The adjacency projection's warm build is paced by the machine's pressure
//! between its pages of the walk (theseus-3edq); a refresh, which a turn
//! runs inside recall's deadline, never is.

use crate::recall::activation::Adjacent;
use crate::recall::adjacency::{Projection, PACES, PAGE};
use crate::store::Store;
use crate::tests_activation::{put, user};

/// `n` nodes of a session, in one frame.
fn write(store: &Store, session: &str, n: usize) {
    let nodes: Vec<_> = (0..n)
        .map(|i| user(session, &format!("note {i}")))
        .collect();
    put(store, &nodes.iter().collect::<Vec<_>>());
}

fn paces() -> u64 {
    PACES.with(|c| c.get())
}

/// A store of more than a page of nodes: the warm build paces once between
/// its two pages, a search's own build never does, and a refresh over more
/// than a page of new writes paces zero times.
#[test]
fn the_warm_build_paces_between_its_pages_and_a_refresh_never_does() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    write(&store, "ses_heron", PAGE + 1);

    let before = paces();
    Adjacent::default().build(&store, false).unwrap();
    assert_eq!(paces() - before, 0, "a search's build waits for no one");

    let before = paces();
    let warm = Adjacent::default();
    warm.build(&store, true).unwrap();
    assert_eq!(
        paces() - before,
        1,
        "the warm build paces once, between pages"
    );
    assert_eq!(warm.stats().unwrap().nodes, PAGE as u64 + 1);

    // More than a page again, written after: the refresh folds both pages
    // and waits for neither.
    let mut p = Projection::build(&store).unwrap();
    write(&store, "ses_osprey", PAGE + 1);
    let before = paces();
    p.refresh(&store).unwrap();
    assert_eq!(paces() - before, 0, "a refresh inside a turn never waits");
    assert_eq!(p.stats().nodes, 2 * PAGE as u64 + 2);
}
