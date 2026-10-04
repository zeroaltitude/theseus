//! A place's spend limit (step 38a, theseus-ext.3; `place_limit.rs`): the
//! lower of the place's cap and the config's, pinned while the place caps
//! it, so a start's own follow leaves it to the binding, which gives it the
//! lower of the two again at each of its starts.

use crate::tests::*;
use crate::types::*;

/// A cap below the config's lowers the limit, and a start under a raised
/// config leaves it; the binding's next word gives the lower of the cap and
/// the new config's, and that raise withdraws the question the session waits
/// on, as the config's own raise does; a config below the cap wins; with no
/// cap the limit is the config's, followed at the next start. Each change of
/// the limit is one `budget.limit_changed` row naming the place.
#[test]
fn a_places_cap_is_the_lower_of_the_two_and_follows_either() {
    let w = world_with(limited(1_000_000));
    let (s, e, g) = following(&w);
    let q = parked_at_limit(&w, g, 600_000, 600_000);
    let budget = |w: &World| w.kernel.execution(&e.id).unwrap().unwrap().budget;

    let f = w.kernel.place_limit(&e.id, Some(500_000)).unwrap().unwrap();
    assert_eq!(
        (f.from_micros, f.to_micros, f.proceeds),
        (1_000_000, 500_000, false)
    );
    assert_eq!(
        (budget(&w).limit_micros, budget(&w).pinned),
        (500_000, true)
    );
    assert!(
        w.kernel
            .place_limit(&e.id, Some(500_000))
            .unwrap()
            .is_none(),
        "once"
    );

    // A start under a raised config leaves a capped limit alone.
    let (w, rep) = crash(w, limited(2_000_000));
    assert!(rep.limits_followed.is_empty(), "{:?}", rep.limits_followed);
    assert_eq!(budget(&w).limit_micros, 500_000);
    // The binding's word after it: the lower of the cap and the config's.
    let f = w
        .kernel
        .place_limit(&e.id, Some(1_500_000))
        .unwrap()
        .unwrap();
    assert_eq!((f.to_micros, f.proceeds), (1_500_000, true));
    assert_eq!(f.withdrew.as_deref(), Some(q.correlation_id.as_str()));
    let e1 = w.kernel.execution(&e.id).unwrap().unwrap();
    assert_eq!(e1.state, ExecState::Queued, "the call that waited proceeds");

    // A config below the cap is the lower.
    let (w, _) = crash(w, limited(1_000_000));
    assert_eq!(
        budget(&w).limit_micros,
        1_500_000,
        "the binding has not spoken yet"
    );
    let f = w
        .kernel
        .place_limit(&e.id, Some(1_500_000))
        .unwrap()
        .unwrap();
    assert_eq!(f.to_micros, 1_000_000);

    // No cap: the config's, unpinned (nothing to row: the limit is the
    // same), and followed at the next start.
    assert!(w.kernel.place_limit(&e.id, None).unwrap().is_none());
    assert!(!budget(&w).pinned);
    let (w, rep) = crash(w, limited(3_000_000));
    assert_eq!(rep.limits_followed.len(), 1);
    assert_eq!(budget(&w).limit_micros, 3_000_000);

    let changed = rows(&w, &s, "budget.limit_changed");
    let why: Vec<&str> = changed.iter().map(|r| r["why"].as_str().unwrap()).collect();
    assert_eq!(why, ["place", "place", "place", "config"]);
}
