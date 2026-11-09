//! A failed index write is safe to retry (theseus-lu5n): `RedbIndex::apply`
//! of a batch run again over what a failed `apply` left behind equals the
//! batch applied once.
//!
//! A batch with tombstones is several transactions (the records up to a run
//! of tombstones, the keys the run ends, the rest), so a failure between two
//! leaves the index holding a prefix of the batch, and a retry applies the
//! whole of it again. For every cut expressible through the public `apply`
//! (the prefix ending before or after a tombstone run; the state with a
//! run's records in and its keys not yet removed is `index::tests`'
//! `a_retry_over_a_committed_tombstone_run_whose_keys_stayed_equals_one_apply`)
//! and for a twice-applied batch, every observer must answer as it does after one `apply`, and each
//! maintained count must equal its walked twin.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use theseus_store::index::{IndexEntry, RedbIndex};
use theseus_store::{kinds, Location, Sums};

const SEEDS: u64 = 60;
const KINDS: [u16; 4] = [1, 2, kinds::META, 5];
const SCOPES: [&str; 3] = ["s0", "s1", "s2"];

/// One seeded batch of entries at positions from `*next`.
fn batch(rng: &mut StdRng, next: &mut u64) -> Vec<IndexEntry> {
    let n = rng.random_range(8..30usize);
    let mut out = Vec::new();
    while out.len() < n {
        // A run of tombstones now and then, so a batch has several.
        let run = if rng.random_bool(0.25) {
            rng.random_range(1..4usize)
        } else {
            0
        };
        for _ in 0..run {
            out.push(entry(rng, next, kinds::TOMBSTONE));
        }
        let kind = KINDS[rng.random_range(0..KINDS.len())];
        out.push(entry(rng, next, kind));
    }
    out
}

fn entry(rng: &mut StdRng, next: &mut u64, kind: u16) -> IndexEntry {
    let position = *next;
    *next += 1;
    let keyed = kind != 5 && rng.random_bool(0.9);
    let key = match kind {
        kinds::TOMBSTONE | kinds::META => Some(format!("m{}", rng.random_range(0..5))),
        _ if keyed => Some(format!("k{}", rng.random_range(0..6))),
        _ => None,
    };
    let terms = (kind == 1 || kind == 2)
        .then(|| {
            rng.random_bool(0.6)
                .then(|| vec![format!("t{}", rng.random_range(0..4))])
        })
        .flatten();
    let sums = (kind == 1 && rng.random_bool(0.5)).then(|| {
        let mut s: Sums = [0; 8];
        s[0] = rng.random_range(1..9u128);
        s[1] = rng.random_range(0..4u128);
        s
    });
    IndexEntry {
        position,
        at_unix_ms: position * 30_000,
        kind,
        key,
        scope: rng
            .random_bool(0.7)
            .then(|| SCOPES[rng.random_range(0..SCOPES.len())].to_string()),
        loc: Location {
            segment: 1,
            offset: position * 100,
            len: 50,
        },
        terms,
        sums,
        tags: Vec::new(),
    }
}

/// Every observer's answer, by name; each maintained count is checked
/// against its walked twin as it is read.
fn observe(idx: &RedbIndex, ctx: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut put = |name: String, v: String| out.push((name, v));
    for kind in [1, 2, kinds::META, 5, kinds::TOMBSTONE] {
        let (a, b) = (
            idx.count_of_kind(kind).unwrap(),
            idx.count_of_kind_walked(kind).unwrap(),
        );
        assert_eq!(a, b, "{ctx}: count_of_kind({kind}) maintained != walked");
        put(format!("count_of_kind({kind})"), a.to_string());
        let (a, b) = (
            idx.count_keys(kind).unwrap(),
            idx.count_keys_walked(kind).unwrap(),
        );
        assert_eq!(a, b, "{ctx}: count_keys({kind}) maintained != walked");
        put(format!("count_keys({kind})"), a.to_string());
        let (a, b) = (
            idx.count_by_terms(kind, "", "~").unwrap(),
            idx.count_by_terms_walked(kind, "", "~").unwrap(),
        );
        assert_eq!(a, b, "{ctx}: count_by_terms({kind}) maintained != walked");
        put(format!("count_by_terms({kind})"), a.to_string());
        put(
            format!("term_counts({kind})"),
            format!("{:?}", idx.term_counts(kind, "", "~").unwrap()),
        );
        put(
            format!("totals({kind})"),
            format!("{:?}", idx.totals(kind).unwrap()),
        );
        put(
            format!("keys_by_birth({kind})"),
            format!("{:?}", idx.keys_by_birth(kind, None, usize::MAX).unwrap()),
        );
    }
    for scope in SCOPES {
        let (a, b) = (
            idx.count_in_scope(scope).unwrap(),
            idx.count_in_scope_walked(scope).unwrap(),
        );
        assert_eq!(a, b, "{ctx}: count_in_scope({scope}) maintained != walked");
        put(format!("count_in_scope({scope})"), a.to_string());
    }
    for kind in [1, 2, kinds::META] {
        let keys = [
            "k0", "k1", "k2", "k3", "k4", "k5", "m0", "m1", "m2", "m3", "m4",
        ];
        for key in keys {
            put(
                format!("latest_position({kind},{key})"),
                format!("{:?}", idx.latest_position(kind, key).unwrap()),
            );
            put(
                format!("terms_of({kind},{key})"),
                format!("{:?}", idx.terms_of(kind, key).unwrap()),
            );
        }
    }
    out
}

fn first_divergence(a: &[(String, String)], b: &[(String, String)]) -> Option<String> {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .find(|(x, y)| x != y)
        .map(|(x, y)| format!("{}: once {} != retried {}", x.0, x.1, y.1))
}

fn fresh(dir: &std::path::Path, name: &str, history: &[Vec<IndexEntry>]) -> RedbIndex {
    let idx = RedbIndex::open(&dir.join(format!("{name}.redb"))).unwrap();
    for b in history {
        idx.apply(b, false).unwrap();
    }
    idx
}

/// The ends a failed `apply` leaves: before each tombstone run (the first
/// transaction committed) and after it (the key removals committed too).
fn cuts(whole: &[IndexEntry]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < whole.len() {
        if whole[i].kind == kinds::TOMBSTONE {
            out.push(i);
            while i < whole.len() && whole[i].kind == kinds::TOMBSTONE {
                i += 1;
            }
            out.push(i);
        } else {
            i += 1;
        }
    }
    out.retain(|&c| c > 0);
    out
}

#[test]
fn a_batch_applied_again_over_what_a_failed_apply_left_equals_it_applied_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut batches, mut tombs, mut prefixes, mut keyed) = (0, 0, 0, 0);
    for seed in 0..SEEDS {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut next = 1;
        let history: Vec<Vec<IndexEntry>> = (0..3).map(|_| batch(&mut rng, &mut next)).collect();
        let whole = batch(&mut rng, &mut next);
        assert!(!whole.is_empty());
        batches += 1;
        tombs += whole.iter().filter(|e| e.kind == kinds::TOMBSTONE).count();
        keyed += whole.iter().filter(|e| e.key.is_some()).count();

        let once = fresh(dir.path(), &format!("once{seed}"), &history);
        once.apply(&whole, false).unwrap();
        let want = observe(&once, &format!("seed {seed}, applied once"));

        let twice = fresh(dir.path(), &format!("twice{seed}"), &history);
        twice.apply(&whole, false).unwrap();
        twice.apply(&whole, false).unwrap();
        if let Some(d) = first_divergence(
            &want,
            &observe(&twice, &format!("seed {seed}, applied twice")),
        ) {
            panic!("seed {seed}: whole applied twice: {d}");
        }

        for end in cuts(&whole) {
            prefixes += 1;
            let idx = fresh(dir.path(), &format!("cut{seed}-{end}"), &history);
            idx.apply(&whole[..end], false).unwrap();
            idx.apply(&whole, false).unwrap();
            if let Some(d) =
                first_divergence(&want, &observe(&idx, &format!("seed {seed}, cut {end}")))
            {
                panic!(
                    "seed {seed}: prefix of {end} of {} then whole: {d}",
                    whole.len()
                );
            }
        }
    }
    // Not an empty pass: batches ran, with tombstones, keys and prefixes.
    assert_eq!(batches, SEEDS);
    assert!(tombs >= SEEDS as usize, "{tombs} tombstones");
    assert!(keyed > 500, "{keyed} keyed entries");
    assert!(prefixes >= SEEDS as usize, "{prefixes} prefixes");
}
