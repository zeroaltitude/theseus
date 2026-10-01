//! The exam's statistics: paired by item, clustered by item (§2.9: "3
//! repeats per item and arm, analysed clustered by item").
//!
//! - **The unit is the item.** Each item's pass rate under an arm is the share
//!   of its runs that passed, so runs never count as independent samples.
//! - **An arm's pass rate** is the mean of its items' rates, with a Student t
//!   interval over the items (95%, two-sided).
//! - **Headroom** is the paired difference, `oracle − none`, item by item: its
//!   mean, with the t interval over the items' differences, and an exact
//!   two-sided sign test over the items that differ.
//! - Intervals are clamped to what a rate or a difference of rates can be
//!   ([0, 1] and [−1, 1]); with one item there is no interval.
//! - A run that ended in an error (no verdict) is left out, and counted.

use std::collections::BTreeMap;

/// One run's verdict.
#[derive(Debug, Clone)]
pub struct Cell {
    pub item: String,
    pub group: String,
    pub held_out: bool,
    pub arm: String,
    /// None: the run failed before a verdict (a provider error, a timeout).
    pub pass: Option<bool>,
}

/// A mean with its 95% interval.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Interval {
    pub n: usize,
    pub mean: f64,
    pub sd: f64,
    pub se: f64,
    /// None with fewer than two values.
    pub lo: Option<f64>,
    pub hi: Option<f64>,
}

/// Student's t at 0.975 (a 95% two-sided interval) for `df` degrees of
/// freedom: exact to 7 places for df ≤ 30 and at 40, 60, and 120, and
/// linear in 1/df between them (within 1e-4 of the exact value).
pub fn t975(df: usize) -> f64 {
    const T: [f64; 30] = [
        12.706_204_7,
        4.302_652_7,
        3.182_446_3,
        2.776_445_1,
        2.570_581_8,
        2.446_911_9,
        2.364_624_3,
        2.306_004_1,
        2.262_157_2,
        2.228_138_9,
        2.200_985_2,
        2.178_812_8,
        2.160_368_7,
        2.144_786_7,
        2.131_449_5,
        2.119_905_3,
        2.109_815_5,
        2.100_922_0,
        2.093_024_1,
        2.085_963_4,
        2.079_613_8,
        2.073_873_1,
        2.068_657_6,
        2.063_898_6,
        2.059_538_6,
        2.055_529_4,
        2.051_830_5,
        2.048_407_1,
        2.045_229_6,
        2.042_272_5,
    ];
    const TAIL: [(f64, f64); 4] = [
        (40.0, 2.021_075_4),
        (60.0, 2.000_297_8),
        (120.0, 1.979_930_4),
        (f64::INFINITY, 1.959_964_0),
    ];
    assert!(df >= 1, "a t interval needs at least one degree of freedom");
    if df <= 30 {
        return T[df - 1];
    }
    let x = 1.0 / df as f64;
    let mut prev = (30.0, T[29]);
    for &(d, t) in &TAIL {
        if (df as f64) <= d {
            let (x0, x1) = (1.0 / prev.0, 1.0 / d);
            return prev.1 + (x - x0) / (x1 - x0) * (t - prev.1);
        }
        prev = (d, t);
    }
    1.959_964_0
}

/// The mean of `xs` with a t interval, clamped to `[lo, hi]`.
pub fn mean_interval(xs: &[f64], clamp: (f64, f64)) -> Interval {
    let n = xs.len();
    if n == 0 {
        return Interval {
            n,
            mean: f64::NAN,
            sd: f64::NAN,
            se: f64::NAN,
            lo: None,
            hi: None,
        };
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    if n == 1 {
        return Interval {
            n,
            mean,
            sd: f64::NAN,
            se: f64::NAN,
            lo: None,
            hi: None,
        };
    }
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let sd = var.sqrt();
    let se = sd / (n as f64).sqrt();
    let h = t975(n - 1) * se;
    Interval {
        n,
        mean,
        sd,
        se,
        lo: Some((mean - h).max(clamp.0)),
        hi: Some((mean + h).min(clamp.1)),
    }
}

/// An exact two-sided sign test: `pos` items gained, `neg` lost (ties left
/// out). The probability, under no difference, of a split at least this
/// uneven.
pub fn sign_test(pos: usize, neg: usize) -> f64 {
    let n = pos + neg;
    if n == 0 {
        return 1.0;
    }
    let k = pos.min(neg);
    // P(X ≤ k) for X ~ Binomial(n, 1/2), by the running binomial coefficient.
    let mut c = 1.0_f64;
    let mut tail = 0.0_f64;
    for i in 0..=k {
        if i > 0 {
            c = c * (n - i + 1) as f64 / i as f64;
        }
        tail += c;
    }
    (2.0 * tail / 2f64.powi(n as i32)).min(1.0)
}

/// Per item, under one arm: runs with a verdict, and passes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub runs: usize,
    pub passes: usize,
    pub errors: usize,
}

impl Tally {
    pub fn rate(&self) -> Option<f64> {
        (self.runs > 0).then(|| self.passes as f64 / self.runs as f64)
    }
    /// Its runs disagree.
    pub fn mixed(&self) -> bool {
        self.passes > 0 && self.passes < self.runs
    }
}

/// `(item, arm)` → its tally, over the cells `keep` admits.
pub fn tallies(cells: &[Cell], keep: impl Fn(&Cell) -> bool) -> BTreeMap<(String, String), Tally> {
    let mut m: BTreeMap<(String, String), Tally> = BTreeMap::new();
    for c in cells.iter().filter(|c| keep(c)) {
        let t = m.entry((c.item.clone(), c.arm.clone())).or_default();
        match c.pass {
            Some(p) => {
                t.runs += 1;
                t.passes += usize::from(p);
            }
            None => t.errors += 1,
        }
    }
    m
}

/// Two arms compared item by item.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Paired {
    /// Items with a verdict under both arms.
    pub items: usize,
    /// Items with a verdict under one arm only (left out).
    pub unpaired: usize,
    pub runs_a: usize,
    pub runs_b: usize,
    pub errors: usize,
    pub a: Interval,
    pub b: Interval,
    /// `b − a`, per item.
    pub diff: Interval,
    pub gained: usize,
    pub lost: usize,
    pub tied: usize,
    pub p_sign: f64,
    /// Item-and-arm cells whose runs disagree, of all such cells.
    pub mixed: usize,
    pub cells: usize,
}

/// Arm `b` against arm `a` over the cells `keep` admits.
pub fn paired(cells: &[Cell], a: &str, b: &str, keep: impl Fn(&Cell) -> bool) -> Paired {
    let t = tallies(cells, keep);
    let items: std::collections::BTreeSet<&String> = t.keys().map(|(i, _)| i).collect();
    let (mut ra, mut rb, mut d) = (Vec::new(), Vec::new(), Vec::new());
    let (mut unpaired, mut runs_a, mut runs_b, mut errors) = (0, 0, 0, 0);
    let (mut gained, mut lost, mut tied) = (0, 0, 0);
    for i in items {
        let ta = t
            .get(&(i.clone(), a.to_string()))
            .copied()
            .unwrap_or_default();
        let tb = t
            .get(&(i.clone(), b.to_string()))
            .copied()
            .unwrap_or_default();
        errors += ta.errors + tb.errors;
        match (ta.rate(), tb.rate()) {
            (Some(x), Some(y)) => {
                ra.push(x);
                rb.push(y);
                d.push(y - x);
                runs_a += ta.runs;
                runs_b += tb.runs;
                match (y - x).partial_cmp(&0.0) {
                    Some(std::cmp::Ordering::Greater) => gained += 1,
                    Some(std::cmp::Ordering::Less) => lost += 1,
                    _ => tied += 1,
                }
            }
            (None, None) => {}
            _ => unpaired += 1,
        }
    }
    let (mixed, n_cells) = t
        .iter()
        .filter(|((_, arm), _)| arm == a || arm == b)
        .fold((0, 0), |(m, n), (_, t)| {
            (m + usize::from(t.mixed()), n + usize::from(t.runs > 0))
        });
    Paired {
        items: d.len(),
        unpaired,
        runs_a,
        runs_b,
        errors,
        a: mean_interval(&ra, (0.0, 1.0)),
        b: mean_interval(&rb, (0.0, 1.0)),
        diff: mean_interval(&d, (-1.0, 1.0)),
        gained,
        lost,
        tied,
        p_sign: sign_test(gained, lost),
        mixed,
        cells: n_cells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn t_quantiles_match_the_tables() {
        assert!(close(t975(1), 12.706_204_7, 1e-7));
        assert!(close(t975(3), 3.182_446_3, 1e-7));
        assert!(close(t975(19), 2.093_024_1, 1e-7));
        assert!(close(t975(30), 2.042_272_5, 1e-7));
        assert!(close(t975(40), 2.021_075_4, 1e-7));
        // Between table rows, linear in 1/df: t(39) = 2.0226909 exactly.
        assert!(close(t975(39), 2.022_690_9, 1e-4), "{}", t975(39));
        // t(50) = 2.0085591; t(100) = 1.9839715; t(1000) = 1.9623391.
        assert!(close(t975(50), 2.008_559_1, 1e-4), "{}", t975(50));
        assert!(close(t975(100), 1.983_971_5, 1e-4), "{}", t975(100));
        assert!(close(t975(1000), 1.962_339_1, 1e-4), "{}", t975(1000));
        // Monotone down to the normal's 1.96.
        let mut prev = f64::INFINITY;
        for df in 1..2000 {
            let t = t975(df);
            assert!(t < prev && t > 1.959_963, "df {df}: {t}");
            prev = t;
        }
    }

    /// By hand: differences [1, 0, 1, 1]. Mean 0.75; the squared deviations
    /// are 0.0625 × 3 and 0.5625, so the variance is 0.75 / 3 = 0.25, the sd
    /// 0.5, the se 0.5 / √4 = 0.25, and the half-width 3.1824463 × 0.25 =
    /// 0.7956116: [−0.0456116, 1.5456116], clamped above to 1.
    #[test]
    fn a_mean_interval_by_hand() {
        let i = mean_interval(&[1.0, 0.0, 1.0, 1.0], (-1.0, 1.0));
        assert_eq!(i.n, 4);
        assert!(close(i.mean, 0.75, 1e-12));
        assert!(close(i.sd, 0.5, 1e-12));
        assert!(close(i.se, 0.25, 1e-12));
        assert!(close(i.lo.unwrap(), -0.045_611_6, 1e-6), "{:?}", i.lo);
        assert_eq!(i.hi, Some(1.0));
        // Unclamped, the upper end is the hand value.
        let u = mean_interval(&[1.0, 0.0, 1.0, 1.0], (-9.0, 9.0));
        assert!(close(u.hi.unwrap(), 1.545_611_6, 1e-6));
        // One value: a mean, no interval. None: nothing.
        let one = mean_interval(&[0.5], (0.0, 1.0));
        assert_eq!((one.n, one.lo, one.hi), (1, None, None));
        assert_eq!(one.mean, 0.5);
        assert_eq!(mean_interval(&[], (0.0, 1.0)).n, 0);
        // No spread: a zero-width interval at the mean.
        let flat = mean_interval(&[1.0, 1.0, 1.0], (0.0, 1.0));
        assert_eq!((flat.lo, flat.hi), (Some(1.0), Some(1.0)));
    }

    /// By hand: n = 10, 8 gained and 2 lost. P(X ≤ 2) = (1 + 10 + 45) / 1024 =
    /// 56 / 1024, so p = 112 / 1024 = 0.109375. Three gained and none lost:
    /// 2 × 1/8 = 0.25. An even split: 1.
    #[test]
    fn the_sign_test_by_hand() {
        assert!(close(sign_test(8, 2), 0.109_375, 1e-12));
        assert!(close(sign_test(2, 8), 0.109_375, 1e-12));
        assert!(close(sign_test(3, 0), 0.25, 1e-12));
        assert!(close(sign_test(5, 5), 1.0, 1e-12));
        assert!(close(sign_test(0, 0), 1.0, 1e-12));
        // 20 to 0: 2 / 2^20.
        assert!(close(sign_test(20, 0), 2.0 / 1_048_576.0, 1e-15));
    }

    fn cell(item: &str, arm: &str, pass: Option<bool>) -> Cell {
        Cell {
            item: item.into(),
            group: "g".into(),
            held_out: false,
            arm: arm.into(),
            pass,
        }
    }

    /// By hand, four items × two arms × three runs:
    ///
    /// | item | none  | oracle | difference |
    /// |------|-------|--------|------------|
    /// | a    | 0/3   | 3/3    | +1         |
    /// | b    | 1/3   | 3/3    | +2/3       |
    /// | c    | 2/3   | 2/3    | 0          |
    /// | d    | 3/3   | 2/3    | −1/3       |
    ///
    /// none: mean (0 + 1/3 + 2/3 + 1) / 4 = 0.5; deviations ±0.5, ±1/6, so the
    /// variance is (0.25 + 1/36) × 2 / 3 = 0.185185…, the sd 0.4303315, the se
    /// 0.2151657, the half-width 0.6847459: [0, 1] after clamping.
    /// oracle: mean (1 + 1 + 2/3 + 2/3) / 4 = 5/6; deviations ±1/6, so the
    /// variance is (1/36) × 4 / 3 = 1/27, the sd 0.1924501, the se 0.0962250,
    /// the half-width 0.3062310: [0.5271023, 1] after clamping.
    /// difference: mean (1 + 2/3 + 0 − 1/3) / 4 = 1/3; deviations 2/3, 1/3,
    /// −1/3, −2/3, so the variance is (4/9 + 1/9) × 2 / 3 = 10/27, the sd
    /// 0.6085806, the se 0.3042903, the half-width 0.9683876:
    /// [−0.6350542, 1] after clamping. Two gained, one lost, one tied: the
    /// sign test over three is p = 2 × (1 + 3) / 8 = 1. Mixed cells: b none,
    /// c none, c oracle, d oracle: 4 of 8.
    #[test]
    fn paired_headroom_by_hand() {
        let mut cells = Vec::new();
        for (item, none, oracle) in [("a", 0, 3), ("b", 1, 3), ("c", 2, 2), ("d", 3, 2)] {
            for r in 0..3 {
                cells.push(cell(item, "none", Some(r < none)));
                cells.push(cell(item, "oracle", Some(r < oracle)));
            }
        }
        // An errored run is left out, and counted; a third arm is ignored.
        cells.push(cell("a", "none", None));
        cells.push(cell("a", "bm25", Some(true)));
        let p = paired(&cells, "none", "oracle", |_| true);
        assert_eq!(
            (p.items, p.unpaired, p.runs_a, p.runs_b, p.errors),
            (4, 0, 12, 12, 1)
        );
        assert!(close(p.a.mean, 0.5, 1e-12));
        assert!(close(p.a.sd, 0.430_331_5, 1e-7), "{}", p.a.sd);
        assert_eq!((p.a.lo, p.a.hi), (Some(0.0), Some(1.0)));
        assert!(close(p.b.mean, 5.0 / 6.0, 1e-12));
        assert!(close(p.b.lo.unwrap(), 0.527_102_3, 1e-6), "{:?}", p.b.lo);
        assert_eq!(p.b.hi, Some(1.0));
        assert!(close(p.diff.mean, 1.0 / 3.0, 1e-12));
        assert!(close(p.diff.sd, 0.608_580_6, 1e-7), "{}", p.diff.sd);
        assert!(
            close(p.diff.lo.unwrap(), -0.635_054_2, 1e-6),
            "{:?}",
            p.diff.lo
        );
        assert_eq!(p.diff.hi, Some(1.0));
        assert_eq!((p.gained, p.lost, p.tied), (2, 1, 1));
        assert!(close(p.p_sign, 1.0, 1e-12));
        assert_eq!((p.mixed, p.cells), (4, 8));
        // A filter: items a and b only. Differences 1 and 2/3: mean 5/6, sd
        // 0.2357023 (deviations ±1/6, variance 1/18), se 1/6, half-width
        // 12.7062047 / 6 = 2.1177008: clamped to [−1, 1].
        let ab = paired(&cells, "none", "oracle", |c| c.item == "a" || c.item == "b");
        assert_eq!(ab.items, 2);
        assert!(close(ab.diff.mean, 5.0 / 6.0, 1e-12));
        assert!(close(ab.diff.sd, 0.235_702_3, 1e-7));
        assert_eq!((ab.diff.lo, ab.diff.hi), (Some(-1.0), Some(1.0)));
        // An item run under one arm only is unpaired, and left out.
        cells.push(cell("e", "none", Some(true)));
        let e = paired(&cells, "none", "oracle", |_| true);
        assert_eq!((e.items, e.unpaired), (4, 1));
    }
}
