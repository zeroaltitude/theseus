//! Health's line for what a first keystroke warmed (theseus-tnky): when the
//! provider's connection and the index tender's model were last warmed.
//! Apart from `render.rs`, whose length the shape budget caps
//! (`scripts/long-files.txt`).

use theseus_protocol::warm::WarmHealth;

use super::{plural, Tag};

/// `warm: provider connection opened 12 s ago (took 84 ms); index model
/// loading 12 s ago (took 3 ms); 5 notices taken, 2 dropped`. Nothing from a
/// daemon before it.
pub fn warm_lines(w: Option<&WarmHealth>, now_ms: u64) -> Vec<(Tag, String)> {
    let Some(w) = w else { return Vec::new() };
    let mut parts = Vec::new();
    if let Some(p) = &w.provider {
        parts.push(p.line("provider connection", now_ms));
    }
    if let Some(t) = &w.tender {
        parts.push(t.line("index model", now_ms));
    }
    if parts.is_empty() {
        parts.push("nothing warmed yet (a first keystroke warms them)".into());
    }
    if w.dropped > 0 {
        parts.push(format!(
            "{} taken, {} dropped",
            plural(w.started, "notice", "notices"),
            w.dropped
        ));
    }
    vec![(Tag::Plain, format!("warm: {}", parts.join("; ")))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::warm::WarmStamp;

    fn stamp(at_ms: u64, took_ms: u64, outcome: &str) -> WarmStamp {
        WarmStamp {
            at_ms,
            took_ms,
            outcome: outcome.into(),
        }
    }

    #[test]
    fn the_line_says_when_each_was_warmed_and_what_it_found() {
        assert!(
            warm_lines(None, 0).is_empty(),
            "no line from an older daemon"
        );
        let cold = WarmHealth::default();
        assert_eq!(
            warm_lines(Some(&cold), 0)[0].1,
            "warm: nothing warmed yet (a first keystroke warms them)"
        );
        let w = WarmHealth {
            started: 3,
            dropped: 2,
            provider: Some(stamp(88_000, 84, "opened")),
            tender: Some(stamp(88_000, 3, "loading")),
        };
        assert_eq!(
            warm_lines(Some(&w), 100_000)[0].1,
            "warm: provider connection opened 12 s ago (took 84 ms); \
             index model loading 12 s ago (took 3 ms); 3 notices taken, 2 dropped"
        );
    }
}
