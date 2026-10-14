//! Self-improvement's state in words (theseus-pw1q.2): the mode and the kill
//! switch in a line, as `theseus self` prints it and as health's `self` line
//! shows it. Apart from `render.rs`, whose length the shape budget caps.

use theseus_protocol::rsi::{SelfBudgetView, SelfMode, SelfState};

use super::{fmt_date, push, Line, Tag};

/// The switch and the mode in a line.
pub fn self_state_line(s: &SelfState) -> String {
    let mode = match s.mode {
        SelfMode::Off => "mode off: nothing self-directed runs",
        SelfMode::Act => "mode act",
    };
    let at = s
        .at_ms
        .map(|t| format!(" at {}", fmt_date(t)))
        .unwrap_or_default();
    let switch = match (s.halted, s.never_resumed) {
        (true, true) => "halted (never resumed)".to_string(),
        (true, false) => format!(
            "halted by {}{at}{}",
            s.by.as_deref().unwrap_or("?"),
            s.why
                .as_deref()
                .map(|w| format!(": {w}"))
                .unwrap_or_default()
        ),
        (false, _) => format!("released by {}{at}", s.by.as_deref().unwrap_or("?")),
    };
    format!("{mode}; the kill switch is {switch}; {}", sealed_words(s))
}

/// Whether a sealed dir is set and how many items each set holds: counts
/// only (theseus-pw1q.5).
fn sealed_words(s: &SelfState) -> String {
    let Some(sealed) = &s.sealed else {
        return "no sealed dir set".into();
    };
    if sealed.sets.is_empty() {
        return "sealed dir set, no set in it yet".into();
    }
    let sets: Vec<String> = sealed
        .sets
        .iter()
        .map(|(set, n)| format!("{set} {n} item{}", if *n == 1 { "" } else { "s" }))
        .collect();
    format!("sealed dir set: {}", sets.join(", "))
}

/// Money as the self budget shows it: to the cent, four places under a cent.
fn usd(v: f64) -> String {
    if v != 0.0 && v.abs() < 0.01 {
        format!("${v:.4}")
    } else {
        format!("${v:.2}")
    }
}

/// Health's `self` line: what a self step would hear now, then the state,
/// then the day's spend against its line (theseus-pw1q.3; none while the
/// mode is off). Nothing from a daemon before it.
pub(super) fn push_health(o: &mut Vec<Line>, s: Option<&SelfState>) {
    if let Some(s) = s {
        let day = match (s.day_spent_usd, s.day_line_usd) {
            (Some(spent), Some(line)) => format!(" · {} of {} today", usd(spent), usd(line)),
            _ => String::new(),
        };
        push(
            o,
            Tag::Plain,
            &format!("self: {} · {}{day}", s.gate, self_state_line(s)),
        );
    }
}

/// `theseus self log`'s budget lines: the day's spend, then each branch's
/// against its line.
pub fn budget_lines(b: &SelfBudgetView) -> Vec<String> {
    let mut out = vec![format!(
        "Budget {}: {} of {} spent or reserved today; a branch's line is {}.",
        b.day,
        usd(b.day_spent_usd),
        usd(b.day_line_usd),
        usd(b.branch_line_usd)
    )];
    for br in &b.branches {
        let open = if br.open_usd > 0.0 {
            format!(", {} of it reserved", usd(br.open_usd))
        } else {
            String::new()
        };
        let stopped = if br.stopped {
            " (stopped at its line)"
        } else {
            ""
        };
        out.push(format!(
            "  {}: {} of {}{open}{stopped}",
            br.branch,
            usd(br.spent_usd),
            usd(b.branch_line_usd)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(mode: SelfMode, halted: bool, never: bool, gate: &str) -> SelfState {
        SelfState {
            mode,
            halted,
            never_resumed: never,
            at_ms: (!never).then_some(1_760_000_000_000),
            by: (!never).then(|| "the CLI".to_string()),
            via: (!never).then(|| "cli".to_string()),
            why: (halted && !never).then(|| "the gate went red".to_string()),
            gate: gate.into(),
            day_spent_usd: None,
            day_line_usd: None,
            sealed: None,
        }
    }

    fn lines(s: Option<&SelfState>) -> Vec<String> {
        let mut o = Vec::new();
        push_health(&mut o, s);
        o.into_iter().map(|l| l.text).collect()
    }

    #[test]
    fn health_has_a_self_line_with_the_gate_the_mode_and_the_switch() {
        assert_eq!(
            lines(Some(&state(SelfMode::Off, true, true, "off"))),
            ["self: off · mode off: nothing self-directed runs; the kill switch is halted (never resumed); no sealed dir set"]
        );
        let halted = lines(Some(&state(SelfMode::Act, true, false, "halted")));
        assert_eq!(halted.len(), 1);
        assert!(
            halted[0]
                .starts_with("self: halted · mode act; the kill switch is halted by the CLI at "),
            "{halted:?}"
        );
        assert!(
            halted[0].ends_with(": the gate went red; no sealed dir set"),
            "{halted:?}"
        );
        let allowed = lines(Some(&state(SelfMode::Act, false, false, "allowed")));
        assert!(
            allowed[0].starts_with(
                "self: allowed · mode act; the kill switch is released by the CLI at "
            ),
            "{allowed:?}"
        );
        assert!(lines(None).is_empty(), "a daemon before it shows no line");
        // A sealed dir: counts only, a set's name and how many items.
        let mut sealed = state(SelfMode::Off, true, true, "off");
        sealed.sealed = Some(theseus_protocol::rsi::SealedState::default());
        assert!(lines(Some(&sealed))[0].ends_with("; sealed dir set, no set in it yet"));
        let sets = [("core".to_string(), 40), ("tools".to_string(), 1)];
        sealed.sealed = Some(theseus_protocol::rsi::SealedState {
            sets: sets.into_iter().collect(),
        });
        assert!(
            lines(Some(&sealed))[0].ends_with("; sealed dir set: core 40 items, tools 1 item"),
            "{:?}",
            lines(Some(&sealed))
        );
    }

    #[test]
    fn the_budget_shows_the_days_spend_and_each_branchs() {
        let mut st = state(SelfMode::Act, false, false, "allowed");
        st.day_spent_usd = Some(12.5);
        st.day_line_usd = Some(30.0);
        let l = lines(Some(&st));
        assert!(l[0].ends_with(" · $12.50 of $30.00 today"), "{l:?}");
        let b = SelfBudgetView {
            day: "2026-10-14".into(),
            day_spent_usd: 12.5,
            day_line_usd: 30.0,
            branch_line_usd: 10.0,
            branches: vec![theseus_protocol::rsi::SelfBranchSpend {
                branch: "tsk_a".into(),
                spent_usd: 10.0,
                open_usd: 0.0,
                stopped: true,
            }],
        };
        let out = budget_lines(&b);
        assert_eq!(
            out,
            [
                "Budget 2026-10-14: $12.50 of $30.00 spent or reserved today; a branch's line is $10.00.",
                "  tsk_a: $10.00 of $10.00 (stopped at its line)",
            ]
        );
    }
}
