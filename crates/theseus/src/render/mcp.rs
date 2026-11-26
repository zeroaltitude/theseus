//! The MCP servers (M7 36b): health's `mcp:` line, and `theseus mcp`'s
//! servers, tools, and prompts (36c).

use theseus_protocol::mcp::{McpListResult, McpPromptInfo, McpServerStatus};

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
        // A server that signs in, with no sign-in that works (issue 55).
        (e, "needs sign-in") => format!(
            ": {}; `theseus mcp login {}`",
            e.as_deref().unwrap_or("it has not been signed in"),
            s.name
        ),
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
        if let Some(by) = &s.attached_by {
            let at = s
                .attached_at_ms
                .and_then(|ms| jiff::Timestamp::from_millisecond(ms as i64).ok())
                .map(|t| {
                    let local = t.to_zoned(jiff::tz::TimeZone::system());
                    format!(" on {}", local.strftime("%Y-%m-%d at %H:%M"))
                });
            out.push(format!(
                "  attached by asking: approved by {by}{}",
                at.unwrap_or_default()
            ));
        }
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
        for p in l.prompts.iter().filter(|p| p.server == s.name) {
            out.push(prompt_line(p));
        }
    }
    out
}

/// One prompt: `  prompt fake/greet (name*, topic) · Greets someone.`, a
/// required argument starred, and what `theseus prompt` takes.
fn prompt_line(p: &McpPromptInfo) -> String {
    let args: Vec<String> = p
        .arguments
        .iter()
        .map(|a| match a.required {
            true => format!("{}*", a.name),
            false => a.name.clone(),
        })
        .collect();
    let args = match args.is_empty() {
        true => String::new(),
        false => format!(" ({})", args.join(", ")),
    };
    let what = p
        .description
        .as_deref()
        .or(p.title.as_deref())
        .map(|d| format!(" · {}", d.lines().next().unwrap_or("")))
        .unwrap_or_default();
    format!("  prompt {}{args}{what}", p.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::mcp::{McpPromptArgument, McpToolInfo};

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
        // A server that signs in, with no sign-in that works: the command,
        // and no crash (issue 55).
        let signing = McpServerStatus {
            name: "wiki".into(),
            transport: "http".into(),
            state: "needs sign-in".into(),
            last_error: Some("its sign-in was refused when renewed".into()),
            ..Default::default()
        };
        assert_eq!(
            mcp_line(&[signing]).unwrap(),
            "mcp: wiki needs sign-in (0 tools): its sign-in was refused when renewed; \
             `theseus mcp login wiki`"
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
            prompts: vec![McpPromptInfo {
                server: "fake".into(),
                prompt: "greet".into(),
                name: "fake/greet".into(),
                description: Some("Greets someone.".into()),
                arguments: vec![
                    McpPromptArgument {
                        name: "name".into(),
                        required: true,
                        ..Default::default()
                    },
                    McpPromptArgument {
                        name: "tone".into(),
                        ..Default::default()
                    },
                ],
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
        assert_eq!(
            lines[2],
            "  prompt fake/greet (name*, tone) · Greets someone."
        );
    }

    /// `theseus mcp` says which servers were attached by asking, by whom, and
    /// when; a server written in the file says nothing of the kind.
    #[test]
    fn a_server_attached_by_asking_says_so() {
        let by_asking = McpServerStatus {
            name: "wc".into(),
            transport: "stdio".into(),
            state: "ready".into(),
            attached_by: Some("owner through discord".into()),
            attached_at_ms: Some(1_700_000_000_000),
            ..Default::default()
        };
        let in_the_file = McpServerStatus {
            name: "base".into(),
            ..by_asking.clone()
        };
        let in_the_file = McpServerStatus {
            attached_by: None,
            attached_at_ms: None,
            ..in_the_file
        };
        let lines = mcp_lines(&McpListResult {
            servers: vec![by_asking, in_the_file],
            tools: vec![],
            prompts: vec![],
        });
        let said: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("attached by asking"))
            .collect();
        assert_eq!(said.len(), 1, "{lines:?}");
        assert!(
            said[0].contains("approved by owner through discord on 2023-11-1"),
            "{said:?}"
        );
    }
}
