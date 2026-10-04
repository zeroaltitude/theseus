//! The MCP servers (M7 36b): health's `mcp:` line, and `theseus mcp`'s
//! servers and tools.

use theseus_protocol::mcp::{McpListResult, McpServerStatus};

/// `mcp: github ready (23 tools, 41 calls) · docs failed (crashed 3 times: …)`,
/// or None without a server.
pub fn mcp_line(servers: &[McpServerStatus]) -> Option<String> {
    if servers.is_empty() {
        return None;
    }
    let each: Vec<String> = servers.iter().map(server_words).collect();
    Some(format!("mcp: {}", each.join(" · ")))
}

fn server_words(s: &McpServerStatus) -> String {
    let mut parts = vec![format!(
        "{} {}",
        s.tools,
        if s.tools == 1 { "tool" } else { "tools" }
    )];
    if s.stored {
        parts.push("the stored list".into());
    }
    if s.calls > 0 {
        parts.push(format!("{} calls, {} failed", s.calls, s.errors));
    }
    if s.crashes > 0 {
        parts.push(format!("crashed {}×", s.crashes));
    }
    if !s.unknown_read.is_empty() {
        parts.push(format!("read names no tool: {}", s.unknown_read.join(", ")));
    }
    let why = match (&s.last_error, s.state.as_str()) {
        (Some(e), "failed" | "restarting" | "starting") => format!(": {e}"),
        _ => String::new(),
    };
    format!("{} {} ({}){why}", s.name, s.state, parts.join(", "))
}

/// `theseus mcp`: each server, then its tools with their postures and
/// classes, and the server's own hints, which never loosen anything.
pub fn mcp_lines(l: &McpListResult) -> Vec<String> {
    let mut out = Vec::new();
    if l.servers.is_empty() {
        out.push(
            "No MCP server is configured: add a [mcp.servers.<name>] table to the config.".into(),
        );
        return out;
    }
    for s in &l.servers {
        let pid = s.pid.map(|p| format!(" · pid {p}")).unwrap_or_default();
        let protocol = s
            .protocol
            .as_ref()
            .map(|p| format!(" · protocol {p}"))
            .unwrap_or_default();
        out.push(format!(
            "{} ({}){pid}{protocol} · {}",
            s.name,
            s.transport,
            server_words(s)
        ));
        for t in l.tools.iter().filter(|t| t.server == s.name) {
            let hints = match t.hints.is_empty() {
                true => String::new(),
                false => format!(" · server says {}", t.hints.join(", ")),
            };
            out.push(format!(
                "  {:<32} {:<5} {:<8} {:>5} calls{hints}",
                t.wire_name, t.class, t.posture, t.calls
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::mcp::McpToolInfo;

    #[test]
    fn health_names_each_server_and_why_a_failed_one_is_down() {
        assert_eq!(mcp_line(&[]), None);
        let ready = McpServerStatus {
            name: "fake".into(),
            transport: "stdio".into(),
            state: "ready".into(),
            tools: 5,
            calls: 2,
            ..Default::default()
        };
        let failed = McpServerStatus {
            name: "docs".into(),
            transport: "http".into(),
            state: "failed".into(),
            crashes: 3,
            last_error: Some("HTTP 500".into()),
            ..Default::default()
        };
        assert_eq!(
            mcp_line(&[ready.clone(), failed]).unwrap(),
            "mcp: fake ready (5 tools, 2 calls, 0 failed) · docs failed (0 tools, crashed 3×): HTTP 500"
        );
        let lines = mcp_lines(&McpListResult {
            servers: vec![ready],
            tools: vec![McpToolInfo {
                server: "fake".into(),
                tool: "echo".into(),
                name: "mcp:fake/echo".into(),
                wire_name: "mcp__fake__echo".into(),
                class: "run".into(),
                posture: "notify".into(),
                hints: vec!["read-only".into()],
                calls: 2,
                ..Default::default()
            }],
        });
        assert_eq!(
            lines[0],
            "fake (stdio) · fake ready (5 tools, 2 calls, 0 failed)"
        );
        assert_eq!(
            lines[1],
            "  mcp__fake__echo                  run   notify       2 calls · server says read-only"
        );
    }
}
