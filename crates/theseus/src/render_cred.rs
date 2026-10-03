//! Credential requests in `theseus watch` (M4 18d): one line for each
//! `secret.requested`, granted, waiting for the operator, or refused. Names
//! only, never a value.

use theseus_protocol::cred::asked;
use theseus_protocol::SecretRequested;

/// The line for a request: "🔑 job a1b2c3 (`cargo publish`, L1) asked for
/// `crates_io_token`: granted at notify (proc.run ran at notify)".
pub fn line(r: &SecretRequested) -> String {
    let what = asked(&r.short, &r.command, &r.secret);
    match r.outcome.as_str() {
        "granted" => format!("  🔑 {what}: granted at {} ({})", r.posture, r.setting),
        "waiting" => format!("  🔑 {what}: it waits for you ({})", r.setting),
        _ => format!(
            "  🔑 {what}: refused ({})",
            r.why.as_deref().unwrap_or("declined")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_outcome_reads_as_one_line() {
        let r = SecretRequested {
            short: "a1b2c3".into(),
            command: "gh api".into(),
            secret: "github_token".into(),
            posture: "notify".into(),
            setting: "proc.run ran at notify".into(),
            outcome: "granted".into(),
            ..Default::default()
        };
        assert_eq!(
            line(&r),
            "  🔑 job a1b2c3 (`gh api`, L1) asked for `github_token`: granted at notify \
             (proc.run ran at notify)"
        );
        let waits = SecretRequested {
            outcome: "waiting".into(),
            setting: "[broker.secrets.github_token] posture = approve".into(),
            ..r.clone()
        };
        assert!(line(&waits)
            .ends_with("it waits for you ([broker.secrets.github_token] posture = approve)"));
        let refused = SecretRequested {
            outcome: "declined".into(),
            why: Some("not a secret a job may ask for".into()),
            ..r
        };
        assert!(line(&refused).ends_with("refused (not a secret a job may ask for)"));
    }
}
