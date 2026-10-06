//! A search beside the adjacency projection's builds (theseus-e21m): a
//! search's own build, which a person waits on inside recall's deadline,
//! never paces.

use theseus_protocol::memory::MemorySearchParams;

use crate::config::memory::MemoryArm;
use crate::recall::activation::Adjacent;
use crate::recall::adjacency::PAGE;
use crate::tests_activation_arm::{by_session, kestrel};
use crate::tests_activation_pace::write;

fn search() -> MemorySearchParams {
    MemorySearchParams {
        query: "What fixed the Kestrel relay?".into(),
        arm: Some("+activation".into()),
        ..Default::default()
    }
}

/// A search under `+activation` that finds the projection unbuilt builds it
/// itself on the blocking pool, unpaced: over more than a page of nodes, the
/// projection's count of paces stays at zero, and the search spreads. The
/// count is the projection's, not the thread's, so the blocking pool's
/// thread is counted too; a warm build of the same store takes its pace.
#[tokio::test]
async fn a_searchs_own_build_never_paces() {
    let (r, _, b) = kestrel(MemoryArm::Activation, false);
    let c = &r.core;
    write(&c.store, "ses_heron", PAGE + 1);
    let adj = &c.runner.memory.adjacency;
    assert!(!adj.built() && !adj.building());

    let m = c.memory_search(search()).await.unwrap();
    let act = m.activation.as_ref().expect("the arm's report");
    assert_eq!(act.outcome, "ran", "{act:?}");
    assert!(by_session(&m, &b).is_some(), "the spread reached B: {m:?}");
    assert!(adj.built());
    assert!(adj.stats().unwrap().nodes > PAGE as u64 + 1);
    assert_eq!(adj.paces(), 0, "a search's own build never paces");

    // The count counts: the warm build's walk of the same store paces once
    // between its two pages of nodes.
    let warm = Adjacent::default();
    warm.build(&c.store, true).unwrap();
    assert_eq!(warm.paces(), 1);
}
