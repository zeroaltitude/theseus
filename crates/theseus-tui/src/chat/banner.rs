//! The first screen's words, from the store and the machine with no model
//! call: the model in words, the directory and its branch, and the
//! directory's last conversation and when.

use theseus_protocol::SessionInfo;

use super::style::{one, span, Line, Sty};

/// `claude-sonnet-5-5` as `Sonnet 5.5`; an id this does not know is shown as
/// it is. A date or a bracketed suffix is left off.
pub fn model_words(id: &str) -> String {
    let base = id.split('[').next().unwrap_or(id);
    let Some(rest) = base.strip_prefix("claude-") else {
        return id.to_string();
    };
    let mut family = String::new();
    let mut version: Vec<&str> = Vec::new();
    for part in rest.split('-') {
        if part.chars().all(|c| c.is_ascii_digit()) {
            if family.is_empty() {
                return id.to_string();
            }
            if part.len() <= 2 {
                version.push(part);
            }
        } else if family.is_empty() && part.chars().all(|c| c.is_ascii_alphabetic()) {
            let mut cs = part.chars();
            family = cs
                .next()
                .map(|f| f.to_ascii_uppercase().to_string() + cs.as_str())
                .unwrap_or_default();
        } else {
            return id.to_string();
        }
    }
    if family.is_empty() {
        return id.to_string();
    }
    if version.is_empty() {
        family
    } else {
        format!("{family} {}", version.join("."))
    }
}

/// `now`, `3m ago`, `2h 5m ago`, `1d 21h ago` (the shared rule, theseus-hnm37).
pub fn ago(now_ms: u64, then_ms: u64) -> String {
    theseus_protocol::human_time::ago(now_ms, then_ms)
}

/// The directory's newest conversation that had a turn.
pub fn last_here<'a>(sessions: &'a [SessionInfo], dir: &str) -> Option<&'a SessionInfo> {
    sessions
        .iter()
        .filter(|s| s.dir.as_deref() == Some(dir) && s.turns > 0)
        .max_by_key(|s| s.last_active_ms)
}

/// What the banner shows of the daemon.
pub struct Facts<'a> {
    pub model: &'a str,
    pub shown_dir: &'a str,
    pub branch: Option<&'a str>,
    pub last: Option<(&'a SessionInfo, u64)>,
}

/// The banner's two lines.
pub fn lines(f: &Facts<'_>) -> Vec<Line> {
    let mut first = vec![
        span(Sty::Bold, "theseus"),
        span(Sty::Dim, " · "),
        span(Sty::Plain, model_words(f.model)),
        span(Sty::Dim, " · "),
        span(Sty::Plain, f.shown_dir.to_string()),
    ];
    if let Some(b) = f.branch {
        first.push(span(Sty::Dim, format!(" ({b})")));
    }
    let mut second = String::from("? help · / commands");
    if let Some((s, now)) = f.last {
        let title = s.title.as_deref().unwrap_or("a conversation");
        let end: String = s
            .session_id
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        second.push_str(&format!(
            " · last here: “{}” {} (theseus chat {end})",
            title.chars().take(40).collect::<String>(),
            ago(now, s.last_active_ms)
        ));
    }
    vec![first, one(Sty::Dim, second)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_id_is_read_as_words() {
        assert_eq!(model_words("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(model_words("claude-opus-5-5[1m]"), "Opus 5.5");
        assert_eq!(model_words("claude-fable-5-1"), "Fable 5.1");
        assert_eq!(model_words("claude-haiku-5-5-20260101"), "Haiku 5.5");
        assert_eq!(model_words("gpt-9"), "gpt-9");
        assert_eq!(model_words("claude-3-5-sonnet"), "claude-3-5-sonnet");
    }

    #[test]
    fn a_time_since_is_in_a_person_s_units() {
        let now = 10_000_000_000;
        assert_eq!(ago(now, now - 3_000), "now");
        assert_eq!(ago(now, now - 30_000), "30s ago");
        assert_eq!(ago(now, now - 5 * 60_000), "5m ago");
        assert_eq!(ago(now, now - 3 * 3_600_000), "3h ago");
        assert_eq!(ago(now, now - 30 * 3_600_000), "1d 6h ago");
        assert_eq!(ago(now, now - 5 * 86_400_000), "5d ago");
    }
}
