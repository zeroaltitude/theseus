//! Alignment (§2.3.3). Each occurrence of a pattern is a window of calls; the
//! windows are aligned to their medoid, the one nearest to all the others, so
//! that the same step of the flow sits in the same column however the
//! occurrences differ: a step one occurrence skipped is a gap, a step only
//! some occurrences took (the arm of a conflict) is a column of its own.
//!
//! The alignment is Needleman–Wunsch over shape ids with gaps, and only equal
//! shapes pair (a mismatch scores worse than two gaps), so a column is always
//! one shape.

use std::collections::BTreeMap;

/// Windows sampled to choose the medoid.
const SAMPLE: usize = 24;

/// The windows, aligned: one shape per column, and for each window the index
/// of its call in each column (or none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aligned {
    pub shapes: Vec<u64>,
    pub rows: Vec<Vec<Option<usize>>>,
}

impl Aligned {
    /// The share of windows that have a call in `col`.
    pub fn presence(&self, col: usize) -> f64 {
        if self.rows.is_empty() {
            return 0.0;
        }
        self.rows.iter().filter(|r| r[col].is_some()).count() as f64 / self.rows.len() as f64
    }
}

/// The length of the longest common subsequence's table, for the pairing and
/// the distance.
fn table(a: &[u64], b: &[u64]) -> Vec<Vec<u16>> {
    let mut t = vec![vec![0u16; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            t[i][j] = if a[i] == b[j] {
                t[i + 1][j + 1] + 1
            } else {
                t[i + 1][j].max(t[i][j + 1])
            };
        }
    }
    t
}

/// The window nearest to the others: the least summed distance (a window's
/// length less the common subsequence, twice over), ties to the earliest.
pub fn medoid(windows: &[Vec<u64>]) -> usize {
    let stride = windows.len().div_ceil(SAMPLE).max(1);
    let sample: Vec<usize> = (0..windows.len()).step_by(stride).collect();
    let mut best = (u64::MAX, 0);
    for &i in &sample {
        let sum: u64 = sample
            .iter()
            .filter(|&&j| j != i)
            .map(|&j| {
                let l = u64::from(table(&windows[i], &windows[j])[0][0]);
                (windows[i].len() + windows[j].len()) as u64 - 2 * l
            })
            .sum();
        if sum < best.0 {
            best = (sum, i);
        }
    }
    best.1
}

/// Pair a window with the medoid: for each medoid position, the window index
/// that matched it; and the window indices that matched none, by the gap
/// (before which medoid position) they fell in.
fn pair(m: &[u64], w: &[u64]) -> (Vec<Option<usize>>, Vec<(usize, usize)>) {
    let t = table(m, w);
    let (mut i, mut j) = (0, 0);
    let mut matched = vec![None; m.len()];
    let mut extra = Vec::new();
    while j < w.len() {
        if i < m.len() && m[i] == w[j] && t[i][j] == t[i + 1][j + 1] + 1 {
            matched[i] = Some(j);
            i += 1;
            j += 1;
        } else if i < m.len() && t[i + 1][j] >= t[i][j + 1] {
            i += 1;
        } else {
            extra.push((i, j));
            j += 1;
        }
    }
    (matched, extra)
}

/// Align the windows (shape ids) to their medoid.
pub fn align(windows: &[Vec<u64>]) -> Aligned {
    if windows.is_empty() {
        return Aligned {
            shapes: vec![],
            rows: vec![],
        };
    }
    let m = &windows[medoid(windows)];
    let pairs: Vec<_> = windows.iter().map(|w| pair(m, w)).collect();
    // Columns for what the medoid lacks: by gap, shape and how many of that
    // shape the same window already put in the gap.
    let mut extra_cols: BTreeMap<usize, Vec<u64>> = BTreeMap::new();
    let mut seq: BTreeMap<(usize, u64, usize), usize> = BTreeMap::new();
    // For each window: (gap, the gap's column, the window's index).
    let mut slot_of: Vec<Vec<(usize, usize, usize)>> = Vec::new();
    for (w, (_, extra)) in windows.iter().zip(&pairs) {
        let mut seen: BTreeMap<(usize, u64), usize> = BTreeMap::new();
        let mut mine = Vec::new();
        for &(gap, j) in extra {
            let nth = seen.entry((gap, w[j])).or_default();
            let key = (gap, w[j], *nth);
            *nth += 1;
            let cols = extra_cols.entry(gap).or_default();
            let col = *seq.entry(key).or_insert_with(|| {
                cols.push(w[j]);
                cols.len() - 1
            });
            mine.push((gap, col, j));
        }
        slot_of.push(mine);
    }
    // The final order: a gap's extra columns, then the medoid position after.
    let mut shapes = Vec::new();
    let mut base_col = vec![0usize; m.len() + 1];
    let mut extra_col = BTreeMap::new();
    for gap in 0..=m.len() {
        for (k, s) in extra_cols
            .get(&gap)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .enumerate()
        {
            extra_col.insert((gap, k), shapes.len());
            shapes.push(*s);
        }
        base_col[gap] = shapes.len();
        if gap < m.len() {
            shapes.push(m[gap]);
        }
    }
    let rows = pairs
        .iter()
        .zip(&slot_of)
        .map(|((matched, _), extras)| {
            let mut row = vec![None; shapes.len()];
            for (i, j) in matched.iter().enumerate() {
                row[base_col[i]] = *j;
            }
            for &(gap, k, j) in extras {
                if let Some(c) = extra_col.get(&(gap, k)) {
                    row[*c] = Some(j);
                }
            }
            row
        })
        .collect();
    Aligned { shapes, rows }
}

#[cfg(test)]
mod tests;
