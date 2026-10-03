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
    s.accounts
        .iter()
        .flat_map(|a| {
            let mut lines = vec![aws_account_line(a, now_ms)];
            lines.extend(aws_tended_lines(a, now_ms));
            lines
        })
        .collect()
}

/// Dollars from cents: `1234` is `$12.34`.
fn dollars(cents: u64) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// What signs an account's calls, its budget, GuardDuty's usage, and the
/// reconcile (C2): a line each, when known.
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
    lines
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
        lines.push(format!("  {} in {}: {}", s.stack, s.region, s.action));
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
    }
    lines.extend(r.warnings.iter().map(|w| format!("  warning: {w}")));
    if !r.applied {
        lines.push(format!("  digest: {}", r.digest));
    }
    lines.extend(r.next.iter().map(|n| format!("  next: {n}")));
    lines
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
