//! Health's `mcp server:` line (step 41b): Theseus's own MCP server.

use theseus_protocol::mcp_server::McpServerHealth;

use super::{push, Line, Tag};

/// Health's line, when `[mcp_server]` is on.
pub(super) fn push_health(o: &mut Vec<Line>, m: Option<&McpServerHealth>) {
    if let Some(m) = m {
        push(o, Tag::Plain, &mcp_server_line(m));
    }
}

/// `mcp server: listening on 127.0.0.1:7434 · 2 sessions (claude-code) · 5
/// opened, last by claude-code · 41 calls, 1 an error · refused 3 (key 2,
/// origin 1)`, or the state and why while it does not listen.
pub fn mcp_server_line(m: &McpServerHealth) -> String {
    let mut s = match m.state.as_str() {
        "listening" => format!("mcp server: listening on 127.0.0.1:{}", m.port),
        "failed" => format!(
            "mcp server: ⚠ failed: {}",
            m.error.as_deref().unwrap_or("no reason given")
        ),
        other => format!("mcp server: {other}"),
    };
    if m.state != "listening" {
        return s;
    }
    s.push_str(&format!(" · {} session(s)", m.sessions));
    if !m.clients.is_empty() {
        s.push_str(&format!(" ({})", m.clients.join(", ")));
    }
    s.push_str(&format!(" · {} opened", m.opened));
    if let Some(c) = &m.last_client {
        s.push_str(&format!(", last by {c}"));
    }
    s.push_str(&format!(" · {} call(s), {} an error", m.calls, m.errors));
    let r = &m.refused;
    if r.total() > 0 {
        let kinds: Vec<String> = [
            ("key", r.key),
            ("origin", r.origin),
            ("host", r.host),
            ("rate", r.rate),
            ("peer", r.peer),
        ]
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| format!("{k} {n}"))
        .collect();
        s.push_str(&format!(" · refused {} ({})", r.total(), kinds.join(", ")));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::mcp_server::McpServerRefusals;

    #[test]
    fn the_line_says_where_it_listens_and_what_it_refused() {
        let m = McpServerHealth {
            state: "listening".into(),
            port: 7434,
            sessions: 1,
            clients: vec!["lantern-agent".into()],
            opened: 2,
            last_client: Some("lantern-agent".into()),
            calls: 9,
            errors: 1,
            refused: McpServerRefusals {
                key: 2,
                origin: 1,
                ..McpServerRefusals::default()
            },
            error: None,
        };
        assert_eq!(
            mcp_server_line(&m),
            "mcp server: listening on 127.0.0.1:7434 · 1 session(s) (lantern-agent) · 2 opened, \
             last by lantern-agent · 9 call(s), 1 an error · refused 3 (key 2, origin 1)"
        );
        let failed = McpServerHealth {
            state: "failed".into(),
            error: Some("its key (mcp_server_key) did not resolve: no vault".into()),
            ..McpServerHealth::default()
        };
        assert_eq!(
            mcp_server_line(&failed),
            "mcp server: ⚠ failed: its key (mcp_server_key) did not resolve: no vault"
        );
    }
}
