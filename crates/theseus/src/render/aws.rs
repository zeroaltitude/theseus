//! AWS's lines (rows 29 and 30, C1 and C2): a call, each bound account's
//! health, and the bootstrap's plan.

use super::thousands;

/// `s3:ListObjectsV2 · us-west-2 · account 111122223333 · example-bucket,
/// logs/`: an AWS call as the CLI and Discord name it (row 29, C1).
pub fn aws_call_line(a: &theseus_protocol::AwsPlan) -> String {
    let mut line = format!(
        "{}:{} · {} · account {}",
        a.service, a.operation, a.region, a.account
    );
    if !a.resources.is_empty() {
        line.push_str(&format!(" · {}", a.resources.join(", ")));
    }
    if a.cost_bearing {
        line.push_str(" · $");
    }
    line
}

/// `aws: 111122223333 bound (arn:aws:iam::…:user/x) 2 min ago · us-west-2 (may
/// name us-east-1) · 4 requests` (row 29, C1): each bound AWS account, as its
/// check left it, and its requests; then, from C2, what signs its calls, its
/// budget, GuardDuty's usage, and the budget's reconcile, when known.
/// Nothing when the config binds none.
pub fn aws_lines(s: Option<&theseus_protocol::AwsStatus>, now_ms: u64) -> Vec<String> {
    let Some(s) = s else {
        return Vec::new();
    };
    let mut lines: Vec<String> = s
        .accounts
        .iter()
        .flat_map(|a| {
            let mut lines = vec![aws_account_line(a, now_ms)];
            lines.extend(aws_tended_lines(a, now_ms));
            lines
        })
        .collect();
    if !s.unknown_policy_keys.is_empty() {
        let keys: Vec<String> = s
            .unknown_policy_keys
            .iter()
            .map(|k| format!("\"{k}\""))
            .collect();
        lines.push(format!(
            "aws: [policy.aws] {} {} no service or operation, so {}; the call falls to its \
             class's line or `enforcement`",
            keys.join(", "),
            if keys.len() == 1 { "names" } else { "name" },
            if keys.len() == 1 {
                "its line never applies"
            } else {
                "their lines never apply"
            },
        ));
    }
    lines
}

/// Dollars from cents: `1234` is `$12.34`.
fn dollars(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// What signs an account's calls, its budget, GuardDuty's usage, the
/// reconcile (C2), and the durability tender: a line each, when known.
fn aws_tended_lines(a: &theseus_protocol::AwsAccountStatus, now_ms: u64) -> Vec<String> {
    let ago = |t: u64| format!("{} min ago", now_ms.saturating_sub(t) / 60_000);
    let mut lines = Vec::new();
    if let Some(s) = &a.signer {
        lines.push(format!("aws: {} signs with {s}", a.account));
    }
    if let Some(b) = &a.budget {
        lines.push(format!(
            "aws: {} budget {} of {} this month{} · read {}",
            a.account,
            dollars(b.actual_cents),
            dollars(b.limit_cents),
            b.forecast_cents
                .map(|f| format!(" (forecast {})", dollars(f)))
                .unwrap_or_default(),
            ago(b.read_at_unix_ms)
        ));
    }
    if let Some(g) = &a.guardduty {
        lines.push(if g.cents_30_days > g.warn_cents {
            format!(
                "aws: {} WARNING: GuardDuty's usage projects {} a month, past {} · read {}",
                a.account,
                dollars(g.cents_30_days),
                dollars(g.warn_cents),
                ago(g.read_at_unix_ms)
            )
        } else {
            format!(
                "aws: {} GuardDuty {} a month projected (warns past {}) · read {}",
                a.account,
                dollars(g.cents_30_days),
                dollars(g.warn_cents),
                ago(g.read_at_unix_ms)
            )
        });
    }
    if let Some(r) = &a.reconcile {
        lines.push(format!("aws: {} the budget's reconcile: {r}", a.account));
    }
    if let Some(d) = &a.durability {
        lines.extend(durability_lines(&a.account, d, now_ms));
    }
    if let Some(h) = &a.hands {
        lines.extend(hands_lines(&a.account, h, now_ms));
    }
    lines
}

/// The durability tender's line (step 15, theseus-9ai1): its state (and
/// why), the position shipped and when, the lag, then the counts since the
/// start and where it ships. `failing` and `stopped` read loud, as
/// GuardDuty's warning does: the store is not being shipped off the machine.
fn durability_lines(
    account: &str,
    d: &theseus_protocol::AwsDurabilityStatus,
    now_ms: u64,
) -> Vec<String> {
    let plural =
        |n: u64, one: &str| format!("{} {one}{}", thousands(n), if n == 1 { "" } else { "s" });
    let state = match (d.state.as_str(), d.error.as_deref()) {
        ("failing", e) => format!(
            "WARNING: durability failing: {} (it retries)",
            e.unwrap_or("no message")
        ),
        ("stopped", e) => format!(
            "WARNING: durability stopped: {} (it will not retry; the store is not shipped \
             off the machine)",
            e.unwrap_or("no message")
        ),
        // A waiting tender's reason reads on: `waiting for the start to settle`.
        ("waiting", Some(e)) => format!("durability waiting {e}"),
        (s, Some(e)) => format!("durability {s}: {e}"),
        (s, None) => format!("durability {s}"),
    };
    let shipped = match (d.shipped_to_position, d.last_shipped_unix_ms) {
        (n, Some(t)) => format!(
            "shipped to position {}, {} min ago",
            thousands(n),
            now_ms.saturating_sub(t) / 60_000
        ),
        (0, None) => "nothing shipped yet".to_string(),
        (n, None) => format!("shipped to position {} (before this start)", thousands(n)),
    };
    let lag = match d.oldest_unshipped_unix_ms {
        Some(t) => format!(
            "lag {}: the oldest record not yet shipped was written {} min ago",
            seconds(d.lag_ms),
            now_ms.saturating_sub(t) / 60_000
        ),
        None => "nothing unshipped".to_string(),
    };
    vec![
        format!("aws: {account} {state} · {shipped} · {lag}"),
        format!(
            "aws: {account} durability since the start: {}, {}, {}, {}, {} · to s3://{}/{} \
             and the table {}",
            plural(d.segments, "segment"),
            plural(d.tails, "tail"),
            plural(d.blobs, "blob"),
            plural(d.rows, "row"),
            plural(d.bytes, "byte"),
            d.bucket,
            d.prefix,
            d.table
        ),
    ]
}

/// A span in seconds, or minutes past two of them: `45 s`, `3 min`.
fn seconds(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 120 {
        format!("{secs} s")
    } else {
        format!("{} min", secs / 60)
    }
}

/// Health's hands block (step 40 part 2): the hands running by backend, the
/// oldest, what they hold reserved, the hour's meter against its line, and
/// the TTL reaper's failures.
fn hands_lines(account: &str, h: &theseus_protocol::AwsHandsStatus, now_ms: u64) -> Vec<String> {
    let usd = |m: u64| format!("${:.2}", m as f64 / 1e6);
    let running = h.running_lambda + h.running_fargate;
    let mut out = vec![format!(
        "aws: {account} hands: {}{} · this hour {} of its {} line{}",
        if running == 0 {
            "none running".to_string()
        } else {
            format!(
                "{running} running ({} Lambda, {} Fargate), {} reserved",
                h.running_lambda,
                h.running_fargate,
                usd(h.reserved_micros)
            )
        },
        h.oldest_unix_ms
            .map(|t| format!(", the oldest {} min", now_ms.saturating_sub(t) / 60_000))
            .unwrap_or_default(),
        usd(h.hour_micros),
        usd(h.hour_line_micros),
        if h.alerted_hour_unix_ms.is_some() {
            " (PAST IT: alerted)"
        } else {
            ""
        }
    )];
    if let Some(r) = &h.runaway {
        out.push(format!("aws: {account} RUNAWAY: {r}"));
    }
    if h.reaper_failures > 0 {
        out.push(format!(
            "aws: {account} WARNING: the hands' TTL reaper failed {} time{} since the start; last: {}",
            h.reaper_failures,
            if h.reaper_failures == 1 { "" } else { "s" },
            h.reaper_last_failure.as_deref().unwrap_or("no message")
        ));
    }
    out
}

/// The bootstrap's plan (C2): each stack, its action, and what it makes or
/// changes; the warnings; the digest; and what comes next.
pub fn bootstrap_lines(r: &theseus_protocol::AwsBootstrapResult) -> Vec<String> {
    let mut lines = vec![format!(
        "AWS account {} ({}): {}{}",
        r.account,
        r.region,
        if r.applied {
            "the bootstrap applied"
        } else {
            "the bootstrap's plan, read-only"
        },
        if r.changes || r.applied {
            ""
        } else {
            ": nothing to do"
        }
    )];
    for s in &r.stacks {
        lines.push(format!(
            "  {} in {}: {}{}",
            s.stack,
            s.region,
            s.action,
            unset(s)
        ));
        let params: Vec<String> = s
            .parameters
            .iter()
            .map(|(k, v)| format!("{k}={}", if v.is_empty() { "(empty)" } else { v }))
            .collect();
        lines.push(format!("    parameters: {}", params.join(", ")));
        if !s.resources.is_empty() {
            lines.push(format!("    creates {} resources:", s.resources.len()));
            lines.extend(s.resources.iter().map(|x| format!("      + {x}")));
        }
        lines.extend(s.changes.iter().map(|c| format!("    ~ {c}")));
        if s.sets.iter().any(|x| x == "stack policy") {
            lines.push(format!("    stack policy: {}", policy_line(&s.policy)));
        }
    }
    lines.extend(r.warnings.iter().map(|w| format!("  warning: {w}")));
    if !r.applied {
        lines.push(format!("  digest: {}", r.digest));
    }
    lines.extend(r.next.iter().map(|n| format!("  next: {n}")));
    lines
}

/// What an existing stack lacks, which the apply sets: `; its stack policy
/// and termination protection are not set, and the apply sets them`. A
/// create gets both, and says nothing.
fn unset(s: &theseus_protocol::AwsBootstrapStack) -> String {
    if s.action == "create" || s.sets.is_empty() {
        return String::new();
    }
    let one = s.sets.len() == 1;
    format!(
        "; its {} {} not set, and the apply sets {}",
        s.sets.join(" and "),
        if one { "is" } else { "are" },
        if one { "it" } else { "them" }
    )
}

/// A stack policy in one line: each statement's effect, actions, and
/// resources, a logical id by its name.
fn policy_line(policy: &str) -> String {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(policy) else {
        return format!("(not JSON) {policy}");
    };
    let names = |v: &serde_json::Value| -> String {
        let one = |x: &serde_json::Value| {
            x.as_str()
                .unwrap_or("?")
                .trim_start_matches("LogicalResourceId/")
                .to_string()
        };
        match v {
            serde_json::Value::Array(a) => a.iter().map(one).collect::<Vec<_>>().join(", "),
            v => one(v),
        }
    };
    doc["Statement"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|st| {
            format!(
                "{} {} on {}",
                st["Effect"].as_str().unwrap_or("?"),
                names(&st["Action"]),
                names(&st["Resource"])
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// One account's check and its requests.
fn aws_account_line(a: &theseus_protocol::AwsAccountStatus, now_ms: u64) -> String {
    let ago = a.checked_at_unix_ms.map(|t| {
        let secs = now_ms.saturating_sub(t) / 1000;
        if secs < 120 {
            format!(" {secs} s ago")
        } else {
            format!(" {} min ago", secs / 60)
        }
    });
    let state = match a.state.as_str() {
        "bound" => format!(
            "bound ({}){}",
            a.arn.as_deref().unwrap_or("?"),
            ago.unwrap_or_default()
        ),
        "failed" => format!(
            "NOT BOUND{}: {}; its calls fail closed",
            ago.unwrap_or_default(),
            a.error.as_deref().unwrap_or("?")
        ),
        "waiting" => format!(
            "waiting for its key{}",
            a.error
                .as_deref()
                .map(|e| format!(": {e}"))
                .unwrap_or_default()
        ),
        other => other.to_string(),
    };
    let more: Vec<&str> = a
        .regions
        .iter()
        .map(String::as_str)
        .filter(|r| *r != a.region)
        .collect();
    format!(
        "aws: {} {state} · {}{} · {} {}{}",
        a.account,
        a.region,
        if more.is_empty() {
            String::new()
        } else {
            format!(" (may name {})", more.join(", "))
        },
        thousands(a.calls),
        if a.calls == 1 { "request" } else { "requests" },
        if a.failed > 0 {
            format!(", {} failed", thousands(a.failed))
        } else {
            String::new()
        }
    )
}

#[cfg(test)]
mod tests {
    use theseus_protocol::{
        AwsAccountStatus, AwsBootstrapResult, AwsBootstrapStack, AwsDurabilityStatus, AwsStatus,
    };

    const NOW: u64 = 1_800_000_000_000;

    /// An account with the tender in `state`, and what it has shipped.
    fn tended(state: &str, error: Option<&str>) -> AwsAccountStatus {
        AwsAccountStatus {
            account: "111122223333".into(),
            region: "us-west-2".into(),
            state: "bound".into(),
            durability: Some(AwsDurabilityStatus {
                state: state.into(),
                bucket: "theseus-111122223333-us-west-2".into(),
                prefix: "durability/theseus-lab/".into(),
                table: "theseus-durability".into(),
                shipped_to_position: 1234,
                last_shipped_unix_ms: Some(NOW - 3 * 60_000),
                segments: 2,
                tails: 1,
                blobs: 0,
                rows: 1500,
                bytes: 2_097_152,
                error: error.map(String::from),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn durability(a: &AwsAccountStatus) -> Vec<String> {
        super::aws_tended_lines(a, NOW)
            .into_iter()
            .filter(|l| l.contains("durability"))
            .collect()
    }

    /// Health's durability line (theseus-9ai1): caught up, it says what is
    /// shipped and when, nothing unshipped, the counts, and where.
    #[test]
    fn a_caught_up_tender_says_what_it_shipped_and_where() {
        let lines = durability(&tended("caught_up", None));
        assert_eq!(
            lines,
            [
                "aws: 111122223333 durability caught_up · shipped to position 1,234, 3 min ago \
                 · nothing unshipped",
                "aws: 111122223333 durability since the start: 2 segments, 1 tail, 0 blobs, \
                 1,500 rows, 2,097,152 bytes · to s3://theseus-111122223333-us-west-2/\
                 durability/theseus-lab/ and the table theseus-durability",
            ]
        );
    }

    /// Waiting, its reason reads on, and the exposure since the oldest
    /// record not yet shipped is said.
    #[test]
    fn a_waiting_tender_says_why_and_its_lag() {
        let mut a = tended(
            "waiting",
            Some("for the WAL's sync: position 1240 is written, not yet synced"),
        );
        let d = a.durability.as_mut().unwrap();
        d.oldest_unshipped_unix_ms = Some(NOW - 150_000);
        d.lag_ms = 150_000;
        let lines = durability(&a);
        assert_eq!(
            lines[0],
            "aws: 111122223333 durability waiting for the WAL's sync: position 1240 is \
             written, not yet synced · shipped to position 1,234, 3 min ago · lag 2 min: the \
             oldest record not yet shipped was written 2 min ago"
        );
        assert!(!lines[0].contains("WARNING"), "{lines:?}");
        let mut a = tended("waiting", Some("for the start to settle"));
        let d = a.durability.as_mut().unwrap();
        (d.shipped_to_position, d.last_shipped_unix_ms) = (0, None);
        let lines = durability(&a);
        assert!(
            lines[0].starts_with(
                "aws: 111122223333 durability waiting for the start to settle · nothing shipped yet"
            ),
            "{lines:?}"
        );
    }

    /// `failing` and `stopped` read loud, with their error, as GuardDuty's
    /// warning does: a tender that does not ship must not look as if it did.
    #[test]
    fn a_failing_or_stopped_tender_reads_loud_with_its_error() {
        let lines = durability(&tended("failing", Some("s3 PutObject: AccessDenied")));
        assert!(
            lines[0].starts_with(
                "aws: 111122223333 WARNING: durability failing: s3 PutObject: AccessDenied \
                 (it retries) · shipped to position 1,234"
            ),
            "{lines:?}"
        );
        let lines = durability(&tended("stopped", Some("the cursor is past the WAL's end")));
        assert!(
            lines[0].starts_with(
                "aws: 111122223333 WARNING: durability stopped: the cursor is past the WAL's \
                 end (it will not retry"
            ),
            "{lines:?}"
        );
        for s in ["caught_up", "shipping", "waiting"] {
            assert!(
                !durability(&tended(s, None))[0].contains("WARNING"),
                "{s} is not loud"
            );
        }
    }

    /// No status (durability off, or no account): no line.
    #[test]
    fn no_durability_status_no_line() {
        let mut a = tended("caught_up", None);
        a.durability = None;
        assert!(durability(&a).is_empty());
        let s = AwsStatus {
            accounts: vec![tended("caught_up", None)],
            ..Default::default()
        };
        let all = super::aws_lines(Some(&s), NOW);
        assert_eq!(
            all.iter().filter(|l| l.contains(" durability ")).count(),
            2,
            "{all:?}"
        );
    }

    /// Keys of `[policy.aws]` that match nothing are named in health's `aws:`
    /// lines (theseus-snhr), and no line says anything when there are none.
    #[test]
    fn unknown_policy_keys_are_named() {
        let mut s = AwsStatus::default();
        assert!(super::aws_lines(Some(&s), 0).is_empty());
        s.unknown_policy_keys = vec!["ec2:TerminateInstance".into(), "cloudformaton".into()];
        let lines = super::aws_lines(Some(&s), 0);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("\"ec2:TerminateInstance\", \"cloudformaton\" name no service")
                && lines[0].contains("their lines never apply"),
            "{lines:?}"
        );
        s.unknown_policy_keys = vec!["s3:ListBuckts".into()];
        let lines = super::aws_lines(Some(&s), 0);
        assert!(
            lines[0].contains("\"s3:ListBuckts\" names no service"),
            "{lines:?}"
        );
    }

    /// A stopped bootstrap's re-plan (theseus-oszz): an existing stack says
    /// what it lacks, and the policy the apply sets; a stack with nothing to
    /// set says neither.
    #[test]
    fn a_stack_says_what_the_apply_sets_on_it() {
        let stack = |name: &str, action: &str, sets: &[&str]| AwsBootstrapStack {
            stack: name.into(),
            region: "us-west-2".into(),
            action: action.into(),
            sets: sets.iter().map(|x| x.to_string()).collect(),
            policy: r#"{"Statement":[{"Effect":"Allow","Action":"Update:*","Resource":"*"},
                {"Effect":"Deny","Action":["Update:Replace","Update:Delete"],
                 "Resource":["LogicalResourceId/Trail","LogicalResourceId/TrailBucket"]}]}"#
                .into(),
            ..Default::default()
        };
        let r = AwsBootstrapResult {
            stacks: vec![
                stack(
                    "theseus-posture",
                    "none",
                    &["stack policy", "termination protection"],
                ),
                stack("theseus-foundation", "none", &["termination protection"]),
                stack("theseus-posture-relay", "none", &[]),
            ],
            changes: true,
            ..Default::default()
        };
        let lines = super::bootstrap_lines(&r);
        let has = |l: &str| lines.iter().any(|x| x == l);
        assert!(
            has(
                "  theseus-posture in us-west-2: none; its stack policy and termination \
                 protection are not set, and the apply sets them"
            ),
            "{lines:#?}"
        );
        assert!(has(
            "    stack policy: Allow Update:* on *; Deny Update:Replace, Update:Delete on \
             Trail, TrailBucket"
        ));
        assert!(has(
            "  theseus-foundation in us-west-2: none; its termination protection is not set, \
             and the apply sets it"
        ));
        assert!(has("  theseus-posture-relay in us-west-2: none"));
        assert_eq!(
            lines.iter().filter(|l| l.contains("stack policy:")).count(),
            1
        );
    }
}
