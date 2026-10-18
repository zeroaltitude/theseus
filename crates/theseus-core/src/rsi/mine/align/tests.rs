//! Alignment (§2.3.3): equal shapes share a column; a step some windows lack
//! is a gap; a step only some took is a column of its own.

use super::*;

#[test]
fn identical_windows_align_one_to_one() {
    let w = vec![vec![1, 2, 3]; 4];
    let a = align(&w);
    assert_eq!(a.shapes, [1, 2, 3]);
    assert!(a.rows.iter().all(|r| r == &[Some(0), Some(1), Some(2)]));
    assert_eq!(a.presence(1), 1.0);
}

#[test]
fn a_skipped_step_is_a_gap_and_an_extra_step_a_column() {
    let w = vec![
        vec![1, 2, 3, 4],
        vec![1, 2, 3, 4],
        vec![1, 3, 4],
        vec![1, 2, 9, 3, 4],
    ];
    let a = align(&w);
    let col = |s: u64| a.shapes.iter().position(|x| *x == s).unwrap();
    assert_eq!(a.presence(col(2)), 0.75);
    assert_eq!(a.presence(col(9)), 0.25);
    assert!(
        a.shapes.iter().filter(|s| **s == 2).count() == 1,
        "one column per shape"
    );
    // Each row reads back its own window in order.
    for (win, row) in w.iter().zip(&a.rows) {
        let got: Vec<u64> = row.iter().flatten().map(|j| win[*j]).collect();
        assert_eq!(&got, win);
    }
}

#[test]
fn the_medoid_is_the_window_nearest_the_others() {
    let w = vec![
        vec![1, 2, 3],
        vec![1, 2, 3, 7, 8],
        vec![1, 2, 3],
        vec![9, 2, 3],
    ];
    assert_eq!(w[medoid(&w)], vec![1, 2, 3]);
}

#[test]
fn a_repeated_shape_in_one_gap_gets_a_column_each() {
    let w = vec![vec![1, 5, 5, 2], vec![1, 2], vec![1, 5, 5, 2]];
    let a = align(&w);
    assert_eq!(a.shapes.iter().filter(|s| **s == 5).count(), 2);
}
