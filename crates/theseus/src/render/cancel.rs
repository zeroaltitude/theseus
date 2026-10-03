//! A cancel's verdicts (M4 18a): how each call a cancel or a stop ended is
//! known to have stopped, and health's counts of them. Apart from
//! `render.rs`, whose length the shape budget caps (`scripts/long-files.txt`).

use theseus_protocol::{CancelCount, CancelVerdict};

/// One line per call a cancel or a stop ended: `  ⏹️ cancelled proc.run
/// act_…a1b2c3 (verified: pid namespace, 4 processes)`. `verb`: `cancelled`
/// or `stopped`.
pub fn verdict_lines(verb: &str, verdicts: &[CancelVerdict]) -> Vec<String> {
    verdicts
        .iter()
        .map(|v| {
            let id = &v.correlation_id;
            let short = &id[id.len().saturating_sub(6)..];
            format!("  ⏹️ {verb} {} …{short} ({})", v.tool, v.words())
        })
        .collect()
}

/// Health's `cancels:` line, once a cancel has stopped something: each
/// backend's cancels since the daemon started, by how they ended. `cancels:
/// l0 2 verified · l1 1 verified, 1 uncertain · async 1 verified`.
pub fn cancels_line(counts: &[CancelCount]) -> Option<String> {
    if counts.is_empty() {
        return None;
    }
    let mut by_backend: Vec<(&str, Vec<String>)> = Vec::new();
    for c in counts {
        let said = format!("{} {}", c.n, c.state);
        match by_backend.iter_mut().find(|(b, _)| *b == c.backend) {
            Some((_, list)) => list.push(said),
            None => by_backend.push((&c.backend, vec![said])),
        }
    }
    let parts: Vec<String> = by_backend
        .into_iter()
        .map(|(b, list)| {
            let b = if b == "inproc" { "in process" } else { b };
            format!("{b} {}", list.join(", "))
        })
        .collect();
    Some(format!("cancels since the start: {}", parts.join(" · ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_stopped_call_says_how_it_is_known_and_health_counts_them() {
        let v = CancelVerdict {
            correlation_id: "act_0123456789a1b2c3".into(),
            tool: "proc.run".into(),
            state: "termination_verified".into(),
            verified_by: "pidns".into(),
            killed: Some(4),
            survivors: Some(0),
            ..Default::default()
        };
        assert_eq!(
            verdict_lines("cancelled", &[v]),
            vec!["  ⏹️ cancelled proc.run …a1b2c3 (verified: pid namespace, 4 processes)"]
        );
        let c = |backend: &str, state: &str, n| CancelCount {
            backend: backend.into(),
            state: state.into(),
            n,
        };
        assert_eq!(cancels_line(&[]), None);
        assert_eq!(
            cancels_line(&[
                c("inproc", "unsupported", 2),
                c("l0", "verified", 3),
                c("l1", "uncertain", 1),
                c("l1", "verified", 1),
            ])
            .as_deref(),
            Some(
                "cancels since the start: in process 2 unsupported · l0 3 verified · l1 1 \
                 uncertain, 1 verified"
            )
        );
    }
}
