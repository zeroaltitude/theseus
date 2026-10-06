//! A search beside the adjacency projection's builds (theseus-e21m,
//! theseus-6fn.14): a search's own build, which a person waits on inside
//! recall's deadline, never paces; and a search that finds the warm build
//! running, paced by a busy machine, answers `building` at once instead of
//! queueing behind it on the projection's lock.

use std::path::Path;
use std::time::{Duration, Instant};

use theseus_protocol::memory::MemorySearchParams;

use crate::config::memory::MemoryArm;
use crate::recall::activation::Adjacent;
use crate::recall::adjacency::PAGE;
use crate::tests_activation_arm::{by_session, kestrel};
use crate::tests_activation_pace::{busy, namespaced, write, INNER};

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

/// While the warm build waits out a busy machine between its pages, holding
/// the projection's lock, a search under `+activation` answers `building`
/// at once, and says why; once a clean stop ends the warm build's waits, a
/// search spreads. This test binary runs itself again in namespaces of its
/// own with `/proc/pressure` faked busy (`tests_activation_pace`'s
/// `namespaced`); where they can't be made, it says so and passes.
#[tokio::test]
async fn a_search_during_the_warm_build_answers_building_at_once() {
    if let Some(dir) = std::env::var_os(INNER) {
        building_inside(Path::new(&dir)).await;
        return;
    }
    let Some(text) = namespaced(
        "tests_activation_search::a_search_during_the_warm_build_answers_building_at_once",
        "SEARCH-INNER",
    ) else {
        return;
    };
    assert!(text.contains("SEARCH-INNER ok"), "{text}");
}

/// How long a search that meets the warm build may take to answer: well
/// under the search's deadline (here recall's longest, 5 s; `memory.search`
/// never waits less than 2 s), so a search queued on the lock, which answers
/// `deadline` at it, can't pass; and well over what a loaded machine adds to
/// a search that waits for nothing (a stand-in index, one blocking task).
const AT_ONCE: Duration = Duration::from_secs(1);

/// In the namespaces: `/proc/pressure` says busy.
async fn building_inside(dir: &Path) {
    println!("SEARCH-INNER started");
    busy(dir);
    assert!(theseus_store::pressure::busy().is_some(), "busy");
    let (r, _, b) = kestrel(MemoryArm::Activation, false);
    let c = &r.core;
    write(&c.store, "ses_heron", 2 * PAGE + 1);
    let adj = &c.runner.memory.adjacency;
    adj.warm(&c.store);
    // The warm build is in its first pace: counted before it waits.
    let t0 = Instant::now();
    while adj.paces() == 0 {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "the warm build never paced"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(adj.building() && !adj.built());

    let t0 = Instant::now();
    let m = c.memory_search(search()).await.unwrap();
    let took = t0.elapsed();
    let act = m.activation.as_ref().expect("the arm's report");
    println!(
        "SEARCH-INNER a search during the warm build answered {} in {took:?}",
        act.outcome
    );
    assert_eq!(act.outcome, "building", "{act:?}");
    assert!(
        act.why
            .as_deref()
            .is_some_and(|w| w.contains("warm build is running")),
        "{act:?}"
    );
    assert!(
        took < AT_ONCE,
        "the search waited for the warm build: {took:?}"
    );
    assert!(!adj.built(), "the warm build still waits");

    // A clean stop ends the warm build's waits; a search after it spreads.
    crate::startup::stop_began();
    let t0 = Instant::now();
    while !adj.built() || adj.building() {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "the warm build never ended"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    println!(
        "SEARCH-INNER the warm build ended {:?} after the stop began",
        t0.elapsed()
    );
    let m = c.memory_search(search()).await.unwrap();
    let act = m.activation.as_ref().unwrap();
    assert_eq!(act.outcome, "ran", "{act:?}");
    assert!(by_session(&m, &b).is_some(), "{m:?}");
    assert_eq!(adj.stats().unwrap().nodes, 2 * PAGE as u64 + 3);
    println!("SEARCH-INNER ok");
}
