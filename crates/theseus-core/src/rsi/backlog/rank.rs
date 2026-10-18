//! The ranking: deterministic, no model call. An item's score, in
//! milli-points, is
//!
//! ```text
//! 1000 × ln(1 + count) × cost × (0.5 + recency) ÷ work
//! ```
//!
//! - **how often**: `count`, the distinct evidence seen over every night,
//!   at most [`COUNT_CAP`], under a logarithm so a hundredth failure adds
//!   little;
//! - **what it costs the owner**: `cost = 1 + 2·[a person waited] +
//!   1.5·[a task or call failed] + min(usd, 20) ÷ 5`: a person waited
//!   (a turn failed, a routing correction), a task failed, money spent;
//! - **how recent**: `recency = 1 ÷ (1 + days since it was last seen)`;
//! - **the work**: `work`, 1 (a tool's input check) to 4 (a daemon-wide
//!   fix), set by the cause's class.
//!
//! Ties break by the cause key. The owner's `theseus self move` pins an item
//! to a rank; pinned items take their ranks first (by pin, then key) and the
//! rest fill the other places by score, so the order is a function of the
//! items and the clock alone.

use super::state::{Meta, PatternScore};

/// Evidence counted at most this many times.
pub const COUNT_CAP: u64 = 100;

const DAY_MS: f64 = 86_400_000.0;

/// An item's score, in milli-points, at `now_ms`.
pub fn score(m: &Meta, now_ms: u64) -> u64 {
    if let Some(p) = &m.pattern {
        return pattern_score(p, m.last_ms, now_ms);
    }
    let freq = (1.0 + m.count.min(COUNT_CAP) as f64).ln();
    let cost = 1.0
        + if m.person { 2.0 } else { 0.0 }
        + if m.task_failed { 1.5 } else { 0.0 }
        + m.usd.clamp(0.0, 20.0) / 5.0;
    let age_days = now_ms.saturating_sub(m.last_ms) as f64 / DAY_MS;
    let recency = 1.0 / (1.0 + age_days);
    let work = f64::from(m.work.clamp(1, 4));
    (1000.0 * freq * cost * (0.5 + recency) / work).round() as u64
}

/// A pattern cause's score on the same scale (theseus-pw1q.21.3, §2.6):
///
/// ```text
/// 1000 × ln(1 + occurrences) × (1 + usd_saved/2 + secs_saved/600 + decisions/2)
///      × (0.5 + recency) ÷ work
/// ```
///
/// with the saving summed over the occurrences; `work` is 1 for a flow, 2 for
/// an MCP extension, 3 for a Rust built-in.
pub fn pattern_score(p: &PatternScore, last_ms: u64, now_ms: u64) -> u64 {
    let saving = crate::rsi::mine::Saving {
        usd: p.usd,
        model_secs: p.secs,
        decisions: p.decisions,
        ..Default::default()
    };
    crate::rsi::mine::cost::score(p.occurrences, &saving, last_ms, now_ms, 0, p.work)
}

/// The items' order, best first: `(id, key, score, pin)` in, ids out.
pub fn order(items: &[(String, String, u64, Option<u32>)]) -> Vec<String> {
    let mut free: Vec<&(String, String, u64, Option<u32>)> =
        items.iter().filter(|i| i.3.is_none()).collect();
    free.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut pinned: Vec<&(String, String, u64, Option<u32>)> =
        items.iter().filter(|i| i.3.is_some()).collect();
    pinned.sort_by(|a, b| {
        a.3.cmp(&b.3)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut out: Vec<String> = free.iter().map(|i| i.0.clone()).collect();
    for p in pinned {
        let at = (p.3.unwrap_or(1).max(1) as usize - 1).min(out.len());
        out.insert(at, p.0.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(count: u64, person: bool, last_ms: u64, work: u8) -> Meta {
        Meta {
            cause: "c".into(),
            source: "turn.failed".into(),
            count,
            usd: 0.0,
            person,
            task_failed: false,
            work,
            last_ms,
            keel: None,
            score: 0,
            rank: 0,
            pin: None,
            pattern: None,
        }
    }

    /// More often, a person waited, more recent and less work all score
    /// higher, and the same input scores the same.
    #[test]
    fn the_score_rises_with_each_of_its_four_terms() {
        let now = 10 * 86_400_000;
        let base = score(&meta(3, false, now, 2), now);
        assert_eq!(base, score(&meta(3, false, now, 2), now), "deterministic");
        assert!(score(&meta(30, false, now, 2), now) > base, "often");
        assert!(score(&meta(3, true, now, 2), now) > base, "a person waited");
        assert!(
            score(&meta(3, false, now - 5 * 86_400_000, 2), now) < base,
            "recent"
        );
        assert!(score(&meta(3, false, now, 1), now) > base, "less work");
    }

    /// Pinned items take their places; the rest fill by score, ties by key.
    #[test]
    fn a_pin_takes_its_place_and_ties_break_by_key() {
        let it = |id: &str, key: &str, s: u64, pin: Option<u32>| {
            (id.to_string(), key.to_string(), s, pin)
        };
        let items = [
            it("a", "k3", 500, None),
            it("b", "k1", 500, None),
            it("c", "k2", 900, None),
            it("d", "k4", 10, Some(1)),
        ];
        assert_eq!(order(&items), ["d", "c", "b", "a"]);
        let mut rev = items.to_vec();
        rev.reverse();
        assert_eq!(
            order(&rev),
            order(&items),
            "the input's order changes nothing"
        );
    }

    /// A fix cause scores as it always did (a hand-computed golden), and a
    /// pattern cause scores by §2.6 on the same scale, so one ordered list
    /// holds both and a pin still takes its place.
    #[test]
    fn fix_causes_rank_exactly_as_before_and_a_pattern_ranks_beside_them() {
        let now = 10 * 86_400_000;
        // 1000 × ln 4 × 1 × (0.5 + 1) ÷ 2.
        assert_eq!(score(&meta(3, false, now, 2), now), 1040);
        assert_eq!(score(&meta(30, true, now - 86_400_000, 3), now), {
            let f = 31f64.ln();
            (1000.0 * f * 3.0 * (0.5 + 0.5) / 3.0).round() as u64
        });
        let mut p = meta(1, false, now, 1);
        p.source = "pattern".into();
        p.pattern = Some(PatternScore {
            occurrences: 10,
            usd: 0.1,
            secs: 20.0,
            decisions: 3.0,
            work: 1,
        });
        let ps = score(&p, now);
        // 0.1 dollars × 10 occurrences saved, 20 s × 10, 3 decisions.
        let gain = 1.0 + 1.0 / 2.0 + 200.0 / 600.0 + 3.0 / 2.0;
        assert_eq!(ps, (1000.0 * 11f64.ln() * gain * 1.5).round() as u64);
        let fix = score(&meta(3, false, now, 2), now);
        let big = {
            let mut m = p;
            m.pattern.as_mut().unwrap().occurrences = 200;
            score(&m, now)
        };
        let items = [
            ("fix".to_string(), "k1".to_string(), fix, None),
            ("pat".to_string(), "pattern:r:1:aa".to_string(), ps, None),
            ("big".to_string(), "pattern:r:1:bb".to_string(), big, None),
            ("low".to_string(), "k0".to_string(), 10, None),
        ];
        let by_score = {
            let mut v: Vec<_> = items.iter().map(|i| (i.2, i.0.clone())).collect();
            v.sort_by(|a, b| b.cmp(a));
            v.into_iter().map(|x| x.1).collect::<Vec<_>>()
        };
        assert_eq!(order(&items), by_score);
        assert_eq!(order(&items)[3], "low");
        // A mixed list's order does not depend on its input order.
        let mut rev = items.to_vec();
        rev.reverse();
        assert_eq!(order(&rev), order(&items));
    }

    /// A pattern's score rises with its occurrences, its saving and its
    /// recency, and falls with the work its kind needs.
    #[test]
    fn a_patterns_score_follows_its_terms() {
        let now = 30 * 86_400_000;
        let p = PatternScore {
            occurrences: 10,
            usd: 0.1,
            secs: 20.0,
            decisions: 3.0,
            work: 1,
        };
        let base = pattern_score(&p, now, now);
        assert!(
            pattern_score(
                &PatternScore {
                    occurrences: 40,
                    ..p
                },
                now,
                now
            ) > base
        );
        assert!(pattern_score(&PatternScore { usd: 0.5, ..p }, now, now) > base);
        assert!(
            pattern_score(
                &PatternScore {
                    decisions: 9.0,
                    ..p
                },
                now,
                now
            ) > base
        );
        assert!(pattern_score(&p, now - 10 * 86_400_000, now) < base);
        assert!(pattern_score(&PatternScore { work: 3, ..p }, now, now) < base);
    }
}
