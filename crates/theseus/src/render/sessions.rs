//! A session's state in the CLI's lines (theseus-emqx): the column
//! `theseus sessions` shows after each pill, and `retire`'s and `reopen`'s
//! answer.

use theseus_protocol::SessionInfo;

use super::fmt_time;

/// A session's state after its pill, from a daemon that sends one
/// (theseus-emqx): `\tlive`, `\tquiet`, `\tretired (superseded)`.
pub(super) fn state_column(s: &SessionInfo) -> String {
    let Some(state) = s.state else {
        return String::new();
    };
    match &s.retired {
        Some(r) if state == theseus_protocol::SessionState::Retired => {
            format!("\tretired ({})", r.reason.words())
        }
        _ => format!("\t{}", state.as_str()),
    }
}

/// `theseus sessions retire` and `reopen`: the session's state now, and its
/// way back or its links.
pub fn session_state_line(s: &SessionInfo) -> String {
    let state = state_column(s);
    let mut out = format!("{}: {}", s.session_id, state.trim_start_matches('\t'));
    if s.state == Some(theseus_protocol::SessionState::Retired) {
        out += &format!(
            ". Nothing is deleted; `theseus sessions reopen {}` brings it back",
            s.session_id
        );
    }
    if let Some(l) = &s.superseded_by {
        out += &format!("; replaced by {} on {}", l.session_id, fmt_time(l.at_ms));
    }
    if let Some(l) = &s.supersedes {
        out += &format!("; replaces {}", l.session_id);
    }
    out
}
