//! Health's ledger gap (theseus-lu5n): one line while the index lags the log,
//! and nothing when it does not. Apart from `render.rs`, whose length the
//! shape budget caps (`scripts/long-files.txt`).

use theseus_protocol::LedgerGap;

use super::{fmt_date, push, Line, Tag};

/// The line, while a gap stands.
pub(super) fn push_health(o: &mut Vec<Line>, g: Option<&LedgerGap>) {
    if let Some(g) = g {
        push(o, Tag::Plain, &gap_line(g));
    }
}

/// `ledger gap: the log holds frames the index lacks, since 2026-11-09 14:02 (3 batches
/// since): the log is still recording, and the next open replays from the last
/// good checkpoint; why: the index write failed 3 times: disk full`.
pub fn gap_line(g: &LedgerGap) -> String {
    let n = g.batches_since;
    format!(
        "ledger gap: the log holds frames the index lacks, since {} ({n} batch{} since): the log is \
         still recording, and the next open replays from the last good checkpoint; why: {}",
        fmt_date(g.since_ms),
        if n == 1 { "" } else { "es" },
        g.reason
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use theseus_protocol::HealthResult;

    use super::super::health_lines;
    use super::*;

    fn health(gap: Option<serde_json::Value>) -> HealthResult {
        let mut h = json!({
            "name": "theseusd", "version": "0.0.1", "protocol": "0.1", "uptime_secs": 42,
            "sessions": 3, "turns": 9, "model": "glm-x", "secrets_resolved": [],
            "usage_total": {"input_tokens": 0, "output_tokens": 0,
                            "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
            "provider_errors": 0, "ledger_rows": 1
        });
        if let Some(g) = gap {
            h["ledger_gap"] = g;
        }
        serde_json::from_value(h).unwrap()
    }

    /// One line for the gap, which says the log keeps recording and the next
    /// open replays from the last good checkpoint; none for a whole index,
    /// and an older daemon's health (no field) reads as whole.
    #[test]
    fn health_says_the_gap_in_one_line_and_says_nothing_when_there_is_none() {
        let said = |h: &HealthResult| {
            health_lines(h, 0)
                .into_iter()
                .filter(|l| l.text.starts_with("ledger gap:"))
                .map(|l| l.text)
                .collect::<Vec<_>>()
        };
        assert!(said(&health(None)).is_empty());
        let g = json!({"since_ms": 1_790_000_000_000u64, "batches_since": 2,
                       "reason": "the index write failed 3 times: disk full"});
        let lines = said(&health(Some(g)));
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        assert!(!line.contains('\n'), "{line}");
        assert!(line.contains("2 batches since"), "{line}");
        assert!(line.contains("still recording"), "{line}");
        assert!(
            line.contains("the next open replays from the last good checkpoint"),
            "{line}"
        );
        assert!(line.contains("disk full"), "{line}");
        let one = LedgerGap {
            since_ms: 1_790_000_000_000,
            batches_since: 1,
            reason: "r".into(),
        };
        assert!(gap_line(&one).contains("(1 batch since)"));
    }
}
