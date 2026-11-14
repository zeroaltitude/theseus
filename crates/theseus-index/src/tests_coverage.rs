//! The fusion weighs the vector source by how much of the index it can see
//! (theseus-pv7m): while vectors are still being made, the few chunks that
//! have one fill the vector list's best ranks whatever they say, and at the
//! tender's vector weight they outranked every exact match.

use crate::proto::{QueryParams, QueryResult};
use crate::tests::{settle, user};
use crate::vtests::{settle_all, VRig, TEXTS};

const EXACT: &str = "the web ui listens on port 7433";
/// Words only the exact match holds, so BM25 finds it alone.
const QUERY: &str = "listens port 7433";

fn ask(shared: &crate::tender::Shared, weights: &[(&str, f64)]) -> QueryResult {
    let mut p = QueryParams::new(QUERY);
    p.k = 5;
    p.wait_ms = 10_000;
    p.weights = weights.iter().map(|(s, x)| (s.to_string(), *x)).collect();
    shared.query(&p).unwrap()
}

/// Two chunks have vectors and ten do not yet, among them the exact match
/// of the query: the exact match leads, and the vector weight reported is
/// the default's, scaled by the coverage squared. Once every chunk has a vector the
/// default weight is whole again, and a weight the query names is never
/// scaled.
#[test]
fn a_backfill_in_progress_does_not_bury_the_exact_match() {
    let v = VRig::new();
    v.rig.put(&[
        user("ses_1", "tides follow the moon"),
        user("ses_1", "jazz improvisation follows the chord changes"),
    ]);
    let mut t = v.open();
    settle_all(&mut t);
    let rest: Vec<_> = TEXTS
        .iter()
        .filter(|t| !t.starts_with("tides") && !t.starts_with("jazz"))
        .map(|t| user("ses_1", t))
        .collect();
    v.rig.put(&rest);
    settle(&mut t);
    let shared = t.shared();
    let s = crate::vtests::status(&shared);
    assert_eq!((s.chunks, s.vectors), (12, 2), "{s:?}");

    let r = ask(&shared, &[]);
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert_eq!(r.hits[0].text, EXACT, "{:?}", r.hits);
    let default = shared.weights.vector;
    let covered = 2.0 / 12.0;
    assert!((r.weights["vector"] - default * covered * covered).abs() < 1e-9);
    assert_eq!(r.weights["bm25"], 1.0);

    // A weight the query names is its own.
    let named = ask(&shared, &[("vector", 6.0)]);
    assert_eq!(named.weights["vector"], 6.0);
    assert_ne!(
        named.hits[0].text, EXACT,
        "the old order, by the weight asked"
    );

    // Every chunk has its vector: the default, whole.
    settle_all(&mut t);
    let full = ask(&shared, &[]);
    assert_eq!(full.weights["vector"], default);
    assert_eq!(full.hits[0].text, EXACT);
}

/// A table whose lock is held (a batch's append is what holds it, mostly
/// during a backfill) is read by its last counts, never as covered: the
/// share stays what it was, so the query does not get the whole weight back
/// in the very case the weighting is for.
#[test]
fn a_busy_table_is_weighed_by_its_last_counts_not_as_covered() {
    let v = VRig::new();
    v.rig.put(&[
        user("ses_1", "tides follow the moon"),
        user("ses_1", "jazz improvisation follows the chord changes"),
    ]);
    let mut t = v.open();
    settle_all(&mut t);
    let rest: Vec<_> = TEXTS
        .iter()
        .filter(|t| !t.starts_with("tides") && !t.starts_with("jazz"))
        .map(|t| user("ses_1", t))
        .collect();
    v.rig.put(&rest);
    settle(&mut t);
    let shared = t.shared();
    let share = 2.0 / 12.0;
    assert!((shared.vectors.coverage() - share).abs() < 1e-9, "read now");
    let held = shared.vectors.hold_table();
    assert!(
        (shared.vectors.coverage() - share).abs() < 1e-9,
        "the lock is held: the last counts answer, not 1.0"
    );
    drop(held);
}
