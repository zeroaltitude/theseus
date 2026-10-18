//! Cost per occurrence (§2.4): tokens kept apart, a loop's cost shared among
//! its calls, the saving the loops removed, and the score on the backlog's
//! scale.

use super::*;
use crate::rsi::mine::{Call, Loop};

fn call(loop_ix: u32, ms: u32) -> Call {
    Call {
        shape: 1,
        res: 1,
        slots: vec![],
        outs: vec![],
        pos: 1,
        loop_ix,
        ms,
        flags: 0,
    }
}

fn run() -> Run {
    Run {
        calls: vec![call(0, 1000), call(0, 500), call(1, 2000), call(2, 0)],
        loops: vec![
            Loop {
                tokens: [1000, 40_000, 2000, 300],
                usd_micros: 20_000,
                model_ms: 4000,
                calls: 2,
            },
            Loop {
                tokens: [200, 42_000, 0, 100],
                usd_micros: 10_000,
                model_ms: 3000,
                calls: 1,
            },
            Loop {
                tokens: [100, 43_000, 0, 50],
                usd_micros: 5_000,
                model_ms: 1000,
                calls: 5,
            },
        ],
        ..Run::default()
    }
}

#[test]
fn tokens_dollars_and_seconds_are_attributed_to_the_loops_that_emitted_the_calls() {
    // Calls 0..3: loops 0 (both its calls) and 1 (its only call).
    let (c, s) = occurrence(&run(), 0..3);
    assert_eq!(c.decisions, 2.0);
    assert_eq!(c.tokens.input, 1200.0);
    assert_eq!(c.tokens.cache_read, 82_000.0);
    assert_eq!(c.tokens.cache_write, 2000.0);
    assert_eq!(c.tokens.output, 400.0);
    assert!((c.usd - 0.03).abs() < 1e-9, "{}", c.usd);
    assert!((c.model_secs - 7.0).abs() < 1e-9);
    assert!(
        (c.tool_secs - 3.5).abs() < 1e-9,
        "tool time stays: {}",
        c.tool_secs
    );
    // m = 2: one loop of the two is removed.
    assert!((s.usd - 0.015).abs() < 1e-9);
    assert!((s.model_secs - 3.5).abs() < 1e-9);
    assert_eq!(s.decisions, 1.0);
    assert_eq!(s.tokens.cache_read, 41_000.0);
}

#[test]
fn a_loop_is_shared_among_its_calls() {
    // Only the third call of loop 2, which emitted five: a fifth of it.
    let (c, s) = occurrence(&run(), 3..4);
    assert!((c.usd - 0.001).abs() < 1e-9);
    assert_eq!(c.decisions, 1.0);
    assert_eq!(s.usd, 0.0, "one loop has nothing to remove");
}

#[test]
fn the_mean_projects_over_the_window() {
    let parts = vec![occurrence(&run(), 0..3), occurrence(&run(), 0..3)];
    let (c, s) = mean(&parts, 14);
    assert!((c.usd - 0.03).abs() < 1e-9);
    assert!((s.per_day - 2.0 / 14.0).abs() < 1e-9);
    assert!((s.usd_per_day - 0.015 * 2.0 / 14.0).abs() < 1e-9);
}

#[test]
fn the_score_rises_with_each_term_and_falls_with_work() {
    let s = Saving {
        usd: 0.1,
        model_secs: 20.0,
        decisions: 3.0,
        ..Default::default()
    };
    let now = 30 * 86_400_000;
    let base = score(10, &s, now, now, 14, 1);
    assert_eq!(base, score(10, &s, now, now, 14, 1));
    assert!(score(40, &s, now, now, 14, 1) > base, "often");
    assert!(
        score(10, &Saving { usd: 1.0, ..s }, now, now, 14, 1) > base,
        "dollars"
    );
    assert!(
        score(
            10,
            &Saving {
                model_secs: 200.0,
                ..s
            },
            now,
            now,
            14,
            1
        ) > base,
        "seconds"
    );
    assert!(
        score(
            10,
            &Saving {
                decisions: 9.0,
                ..s
            },
            now,
            now,
            14,
            1
        ) > base,
        "decisions"
    );
    assert!(
        score(10, &s, now - 5 * 86_400_000, now, 14, 1) < base,
        "recency"
    );
    assert!(score(10, &s, now, now, 14, 2) < base, "work");
    // The formula, by hand: 1000 × ln 11 × (1 + 1/2 + 200/600 + 3/2) × 1.5.
    let want = (1000.0 * 11f64.ln() * (1.0 + 0.5 + 200.0 / 600.0 + 1.5) * 1.5).round() as u64;
    assert_eq!(base, want);
}
