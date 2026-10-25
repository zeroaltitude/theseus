//! A span of time as people say it, one rule for every surface (theseus-hnm37): the cockpit's `span`, `ago` and `until`
//! in `cockpit/src/lib/figures.ts` say the same words. Each unit is floored: `40s`, `12m`, `2h 46m` (`2h` when no minutes
//! remain), `1d 21h` (`1d`), and from seven days only the days, `9d`. Pure: no clock, the caller passes both times.

/// A bare span: `40s`, `12m`, `2h 46m`, `1d 21h`, `9d`.
pub fn span(ms: u64) -> String {
    let s = ms / 1000;
    let m = s / 60;
    let h = m / 60;
    let d = h / 24;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{m}m"),
        3600..=86_399 if m.is_multiple_of(60) => format!("{h}h"),
        3600..=86_399 => format!("{h}h {}m", m % 60),
        86_400..=604_799 if h.is_multiple_of(24) => format!("{d}d"),
        86_400..=604_799 => format!("{d}d {}h", h % 24),
        _ => format!("{d}d"),
    }
}

/// How long ago `then_ms` was at `now_ms`: `now` under five seconds, else `12m ago`.
pub fn ago(now_ms: u64, then_ms: u64) -> String {
    let d = now_ms.saturating_sub(then_ms);
    if d < 5000 {
        "now".to_string()
    } else {
        format!("{} ago", span(d))
    }
}

/// How long until `due_ms`: `in 2h 46m`; a time that has come reads `due 3m ago` (`due now` under five seconds).
pub fn until(due_ms: u64, now_ms: u64) -> String {
    if due_ms > now_ms {
        format!("in {}", span(due_ms - now_ms))
    } else if now_ms - due_ms < 5000 {
        "due now".to_string()
    } else {
        format!("due {} ago", span(now_ms - due_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1000;
    const M: u64 = 60 * S;
    const H: u64 = 60 * M;
    const D: u64 = 24 * H;

    #[test]
    fn a_span_floors_each_unit_and_drops_a_zero() {
        let rows = [
            (0, "0s"),
            (4 * S, "4s"),
            (40 * S, "40s"),
            (59 * S, "59s"),
            (60 * S, "1m"),
            (12 * M, "12m"),
            (59 * M + 59 * S, "59m"),
            (H, "1h"),
            (166 * M, "2h 46m"),
            (2 * H + 46 * M + 59 * S, "2h 46m"),
            (23 * H + 59 * M, "23h 59m"),
            (24 * H, "1d"),
            (45 * H, "1d 21h"),
            (6 * D + 23 * H, "6d 23h"),
            (7 * D, "7d"),
            (9 * D + 5 * H, "9d"),
        ];
        for (ms, want) in rows {
            assert_eq!(span(ms), want, "span({ms})");
        }
    }

    #[test]
    fn ago_is_now_under_five_seconds() {
        let now = 10 * D;
        assert_eq!(ago(now, now), "now");
        assert_eq!(ago(now, now - 4 * S), "now");
        assert_eq!(ago(now, now - 5 * S), "5s ago");
        assert_eq!(ago(now, now - 40 * S), "40s ago");
        assert_eq!(ago(now, now - 60 * S), "1m ago");
        assert_eq!(ago(now, now - 166 * M), "2h 46m ago");
        assert_eq!(ago(now, now - 45 * H), "1d 21h ago");
        assert_eq!(ago(now, now - 9 * D - 5 * H), "9d ago");
        assert_eq!(ago(now, now + H), "now", "a then in the future is now");
    }

    #[test]
    fn until_words_the_future_and_the_overdue() {
        let now = 10 * D;
        assert_eq!(until(now + 40 * S, now), "in 40s");
        assert_eq!(until(now + 166 * M, now), "in 2h 46m");
        assert_eq!(until(now + 45 * H, now), "in 1d 21h");
        assert_eq!(until(now + 9 * D + 5 * H, now), "in 9d");
        assert_eq!(until(now, now), "due now");
        assert_eq!(until(now - 3 * M, now), "due 3m ago");
        assert_eq!(until(now - 2 * H - 5 * M, now), "due 2h 5m ago");
    }
}
