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

use super::state::Meta;

/// Evidence counted at most this many times.
pub const COUNT_CAP: u64 = 100;

const DAY_MS: f64 = 86_400_000.0;

/// An item's score, in milli-points, at `now_ms`.
pub fn score(m: &Meta, now_ms: u64) -> u64 {
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
}
