//! A session's directory in the CLI's lines (theseus-aab7): the column
//! `theseus sessions` shows last, and the one notice `ask` gives when it
//! starts a session outside the workspace roots.

use theseus_protocol::SessionInfo;

/// `theseus sessions`' last column, from a daemon that sends a directory:
/// `\tin /work/harbour-tides`; nothing for a session without one.
pub(super) fn column(s: &SessionInfo) -> String {
    s.dir
        .as_deref()
        .map(|d| format!("\tin {d}"))
        .unwrap_or_default()
}

/// What `ask` says once, on stderr, when the daemon puts its session in a
/// directory outside every workspace root: the roots are a label and no gate
/// (theseus-m9fdg), so a read or a command there follows the policy's posture
/// and names the path.
pub fn outside_line(dir: &str, roots: &[String]) -> String {
    let roots = match roots.is_empty() {
        true => "none configured".to_string(),
        false => roots.join(", "),
    };
    format!(
        "{dir} is outside the workspace roots ({roots}): what runs there follows your policy and names the path"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_names_the_directory_and_the_roots() {
        assert_eq!(
            outside_line("/tmp/x", &["/work".into()]),
            "/tmp/x is outside the workspace roots (/work): what runs there follows your policy and names the path"
        );
        assert_eq!(
            outside_line("/tmp/x", &[]),
            "/tmp/x is outside the workspace roots (none configured): what runs there follows your policy and names the path"
        );
    }

    #[test]
    fn a_session_without_a_directory_shows_no_column() {
        let mut s: SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": "ses_tide01", "kind": "conversation", "label": null,
            "created_at_unix_ms": 0, "turns": 1,
        }))
        .unwrap();
        assert_eq!(column(&s), "");
        s.dir = Some("/work/harbour-tides".into());
        assert_eq!(column(&s), "\tin /work/harbour-tides");
    }
}
