//! Health's first line (theseus-is8t): which killer features are on, as the
//! daemon words it (`theseus_core::features`). Apart from `render.rs`, whose
//! length the shape budget caps (`scripts/long-files.txt`).

use theseus_protocol::features::Features;

use super::{push, Line, Tag};

/// The line, when the daemon says one: first in health, one line.
pub(super) fn push_health(o: &mut Vec<Line>, f: Option<&Features>) {
    if let Some(f) = f {
        push(o, Tag::Plain, &f.line);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use theseus_protocol::HealthResult;

    use super::super::health_lines;

    fn health(features: Option<serde_json::Value>) -> HealthResult {
        let mut h = json!({
            "name": "theseusd", "version": "0.0.1", "protocol": "0.1", "uptime_secs": 42,
            "sessions": 3, "turns": 9, "model": "glm-x", "secrets_resolved": [],
            "usage_total": {"input_tokens": 0, "output_tokens": 0,
                            "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
            "provider_errors": 0, "ledger_rows": 1
        });
        if let Some(f) = features {
            h["features"] = f;
        }
        serde_json::from_value(h).unwrap()
    }

    /// The features line is health's first line, one line, each state in
    /// its own words; an older daemon sends none and health starts with its
    /// name line as it did.
    #[test]
    fn health_starts_with_the_killer_features_line() {
        let first = |h: &HealthResult| health_lines(h, 0)[0].text.clone();
        assert!(first(&health(None)).starts_with("theseusd 0.0.1 · protocol"));
        for line in [
            "features: memory live (baseline) · Jev on, routing live · outbound substitution on \
             · budgets: daily ceiling $200",
            "features: memory live (baseline) · Jev off (no jev_api_key entry under [secrets]) \
             · outbound substitution on · budgets: daily ceiling $200",
            "features: memory off · Jev on, routing shadow · outbound substitution on \
             · budgets: daily ceiling $50",
        ] {
            let h = health(Some(json!({
                "line": line, "memory": "m", "jev": "j", "scrub": "s", "budgets": "b"
            })));
            let lines = health_lines(&h, 0);
            assert_eq!(lines[0].text, line);
            assert!(!lines[0].text.contains('\n'));
            assert!(
                lines[1].text.starts_with("theseusd 0.0.1"),
                "{}",
                lines[1].text
            );
        }
        // And in the JSON form, the field is the daemon's own.
        let h = health(Some(
            json!({"line": "l", "memory": "m", "jev": "j", "scrub": "s", "budgets": "b"}),
        ));
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["features"]["memory"], "m");
    }
}
