//! Self-improvement's state in words (theseus-pw1q.2): the mode and the kill
//! switch in a line, as `theseus self` prints it and as health's `self` line
//! shows it. Apart from `render.rs`, whose length the shape budget caps.

use theseus_protocol::rsi::{SelfMode, SelfState};

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
    format!("{mode}; the kill switch is {switch}")
}

/// Health's `self` line: what a self step would hear now, then the state.
/// Nothing from a daemon before it.
pub(super) fn push_health(o: &mut Vec<Line>, s: Option<&SelfState>) {
    if let Some(s) = s {
        push(
            o,
            Tag::Plain,
            &format!("self: {} · {}", s.gate, self_state_line(s)),
        );
    }
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
            ["self: off · mode off: nothing self-directed runs; the kill switch is halted (never resumed)"]
        );
        let halted = lines(Some(&state(SelfMode::Act, true, false, "halted")));
        assert_eq!(halted.len(), 1);
        assert!(
            halted[0]
                .starts_with("self: halted · mode act; the kill switch is halted by the CLI at "),
            "{halted:?}"
        );
        assert!(halted[0].ends_with(": the gate went red"), "{halted:?}");
        let allowed = lines(Some(&state(SelfMode::Act, false, false, "allowed")));
        assert!(
            allowed[0].starts_with(
                "self: allowed · mode act; the kill switch is released by the CLI at "
            ),
            "{allowed:?}"
        );
        assert!(lines(None).is_empty(), "a daemon before it shows no line");
    }
}
