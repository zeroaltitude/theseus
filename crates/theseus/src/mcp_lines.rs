//! What `theseus mcp login` and `theseus mcp logout` print (issue 55): the
//! lines are made from the daemon's results alone, so a test of the daemon
//! reads exactly what the owner would be shown (round 4: no label that holds
//! a kept value). In the library, which links the protocol crate only, so
//! the core's tests can call them.

use theseus_protocol::mcp::{McpLoginBeginResult, McpLoginFinishResult, McpLogoutResult};

/// Every host the sign-in involves, each on a line of its own, before
/// anything else: where the owner signs in, and every place of its endpoints
/// that is not its issuer's own, which the owner agrees to (the daemon
/// requires the agreement exactly when there is one: the same set).
pub fn host_lines(b: &McpLoginBeginResult) -> Vec<String> {
    let mut v = vec![format!("You will sign in at {}.", b.host)];
    for h in &b.elsewhere {
        v.push(format!(
            "Part of the sign-in goes through {h}, which is not its issuer's own address."
        ));
    }
    v
}

/// The address to open, and how the redirect comes back.
pub fn address_lines(b: &McpLoginBeginResult) -> Vec<String> {
    let mut v = vec![
        format!(
            "Open this address in a browser to sign in to the MCP server {}:",
            b.name
        ),
        String::new(),
        b.url.clone(),
        String::new(),
    ];
    if b.redirect.is_empty() {
        v.push(
            "After you sign in, the page may fail to load: copy the whole address it was sent to."
                .into(),
        );
    } else {
        v.push(format!(
            "Waiting for the redirect on {} (up to {} minutes)…",
            b.redirect,
            b.expires_secs / 60
        ));
    }
    v
}

pub fn finish_lines(r: &McpLoginFinishResult) -> Vec<String> {
    let mut v = vec![match r.connected {
        true => format!(
            "Signed in to the MCP server {} at {}: {} {} found.",
            r.name,
            r.host,
            r.tools,
            if r.tools == 1 { "tool" } else { "tools" }
        ),
        false => format!(
            "Signed in to the MCP server {} at {}, but it has not come up yet: `theseus mcp` says how it is.",
            r.name, r.host
        ),
    }];
    if !r.note.is_empty() {
        v.push(r.note.clone());
    }
    v
}

pub fn logout_lines(r: &McpLogoutResult) -> Vec<String> {
    let mut v = vec![match (r.kept_remains, r.forgot, r.revoked) {
        (true, _, revoked) => {
            format!(
            "The MCP server {} is signed out until the daemon restarts{}, but its kept sign-in \
             could not be removed, so it is not forgotten.",
            r.name,
            if revoked { " and was revoked at its server" } else { "" }
        )
        }
        (false, true, true) => format!(
            "Signed out of the MCP server {}: revoked at its server, and forgotten here.",
            r.name
        ),
        (false, true, false) => format!("Signed out of the MCP server {}: forgotten here.", r.name),
        (false, false, _) => format!("The MCP server {} had no sign-in kept.", r.name),
    }];
    if !r.note.is_empty() {
        v.push(r.note.clone());
    }
    v.push(format!(
        "It needs a sign-in now: `theseus mcp login {}`.",
        r.name
    ));
    v
}
