//! The counting (§2.3.2) on token sequences alone: closed patterns, support
//! per run, the apriori bound.

use super::*;

fn stretch(run: usize) -> Stretch {
    Stretch {
        run,
        start: 0,
        end: 0,
        ends: End::Natural,
    }
}

fn grams(toks: &[Vec<u64>], runs: &[usize], min: u32) -> Vec<(usize, Vec<u64>)> {
    let st: Vec<Stretch> = runs.iter().map(|r| stretch(*r)).collect();
    closed_grams(toks, &st, min)
        .into_iter()
        .map(|(n, _, at)| {
            (
                n,
                toks[at.0 as usize][at.1 as usize..at.1 as usize + n].to_vec(),
            )
        })
        .collect()
}

#[test]
fn only_the_longest_gram_with_the_same_support_is_closed() {
    let toks: Vec<Vec<u64>> = (0..6).map(|_| vec![1, 2, 3, 4]).collect();
    let runs: Vec<usize> = (0..6).collect();
    assert_eq!(grams(&toks, &runs, 5), [(4, vec![1, 2, 3, 4])]);
}

#[test]
fn a_sub_pattern_with_more_support_than_its_extension_stays() {
    let mut toks: Vec<Vec<u64>> = (0..6).map(|_| vec![1, 2, 3, 4]).collect();
    toks.extend((0..2).map(|_| vec![1, 2, 3, 9]));
    let runs: Vec<usize> = (0..8).collect();
    let g = grams(&toks, &runs, 5);
    assert!(g.contains(&(3, vec![1, 2, 3])), "{g:?}");
    assert!(g.contains(&(4, vec![1, 2, 3, 4])), "{g:?}");
    assert_eq!(g.len(), 2, "{g:?}");
}

#[test]
fn a_gram_is_counted_once_per_run_however_often_it_repeats() {
    // One run holds [1,2,3] ten times; four others once: five runs, not fourteen.
    let mut toks = vec![vec![1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3]];
    toks.extend((0..4).map(|_| vec![1, 2, 3]));
    let g = grams(&toks, &[0, 1, 2, 3, 4], 5);
    assert!(g.iter().any(|(n, _)| *n == 3), "{g:?}");
    // The same stretches in four runs only: below five, however many repeats.
    let g = grams(&toks[..4], &[0, 1, 2, 3], 5);
    assert!(g.is_empty(), "{g:?}");
}

#[test]
fn a_run_with_two_stretches_counts_once() {
    let toks = vec![
        vec![1, 2, 3],
        vec![1, 2, 3],
        vec![1, 2, 3],
        vec![1, 2, 3],
        vec![1, 2, 3],
    ];
    // Five stretches in three runs.
    assert!(grams(&toks, &[0, 0, 1, 1, 2], 4).is_empty());
    assert!(!grams(&toks, &[0, 1, 2, 3, 4], 4).is_empty());
}
