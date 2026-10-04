//! Health's `lsp:` line (L2): each language server the board started, its
//! state, its time to ready, and its memory. Apart from `render.rs`, whose
//! length the shape budget caps (`scripts/long-files.txt`).

use theseus_protocol::lsp::LspServerStatus;

use super::fmt_bytes;

/// `lsp: rust-analyzer on /p/app (pid 4242, ready in 18.2 s, 4.1 GB) · ty on
/// /p/tool starting`, or `lsp: none up (a server starts at its first call)`.
pub fn lsp_line(servers: &[LspServerStatus]) -> String {
    if servers.is_empty() {
        return "lsp: none up (a server starts at the first call for a file of its language)"
            .into();
    }
    let each: Vec<String> = servers
        .iter()
        .map(|s| {
            let mut parts = Vec::new();
            if let Some(pid) = s.pid {
                parts.push(format!("pid {pid}"));
            }
            match (s.state.as_str(), s.ready_ms) {
                ("ready", Some(ms)) => parts.push(format!("ready in {:.1} s", ms as f64 / 1000.0)),
                (state, _) => parts.push(state.to_string()),
            }
            if let Some(k) = s.memory_kib {
                parts.push(fmt_bytes(k * 1024));
            }
            if s.requests > 0 {
                parts.push(format!("{} requests", s.requests));
            }
            if s.edit_blocks > 0 {
                parts.push(format!("{} edit results with its errors", s.edit_blocks));
            }
            if let Some(w) = &s.why {
                parts.push(w.clone());
            }
            format!("{} on {} ({})", s.server, s.root, parts.join(", "))
        })
        .collect();
    format!("lsp: {}", each.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lsp_line_names_each_server_its_readiness_and_memory() {
        assert!(lsp_line(&[]).starts_with("lsp: none up"));
        let s = LspServerStatus {
            server: "rust-analyzer".into(),
            root: "/p/app".into(),
            state: "ready".into(),
            pid: Some(4242),
            ready_ms: Some(18_200),
            memory_kib: Some(4 << 20),
            requests: 3,
            edit_blocks: 2,
            ..Default::default()
        };
        let f = LspServerStatus {
            server: "ty".into(),
            root: "/p/tool".into(),
            state: "failed".into(),
            why: Some("initialize: no answer".into()),
            ..Default::default()
        };
        let line = lsp_line(&[s, f]);
        assert!(
            line.starts_with("lsp: rust-analyzer on /p/app (pid 4242, ready in 18.2 s, 4"),
            "{line}"
        );
        assert!(
            line.contains("3 requests, 2 edit results with its errors"),
            "{line}"
        );
        assert!(
            line.contains("ty on /p/tool (failed, initialize: no answer)"),
            "{line}"
        );
    }
}
