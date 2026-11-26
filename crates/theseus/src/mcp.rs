//! `theseus mcp` (M7 36b): the MCP servers the config attaches, each with
//! its state and its tools (their postures, classes, and the servers' own
//! hints); `theseus mcp restart <name>` starts one again, a failed one
//! included. `theseus mcp login <name>` signs in to a server that signs in
//! (issue 55), in a browser, and `logout` forgets that sign-in. The address
//! to open holds no secret, so it is printed, after the host it signs in at;
//! the paste (the address the browser was sent to, which carries the code)
//! is read from the terminal and sent over the socket, never taken as an
//! argument, so it is in no shell history.

use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};
use theseus_client::mcp_lines::{address_lines, finish_lines, host_lines, logout_lines};
use theseus_client::render;
use theseus_client::Conn;
use theseus_protocol::mcp::{
    McpListResult, McpLoginBeginParams, McpLoginBeginResult, McpLoginFinishParams,
    McpLoginFinishResult, McpLogoutParams, McpLogoutResult, McpRestartResult,
};
use theseus_protocol::method;

#[derive(Subcommand, Debug)]
pub enum McpCmd {
    /// Start a server again now, a failed one included, its crashes forgotten.
    Restart {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Sign in to a server that signs in, once, in a browser: the address is printed (a page
    /// on this machine takes the redirect; if none comes in 90 s, paste the address the browser
    /// was sent to). From then on the daemon keeps the sign-in and renews it.
    Login {
        #[arg(value_name = "NAME")]
        name: String,
        /// Skip the wait for the redirect and go straight to the paste.
        #[arg(long)]
        manual: bool,
    },
    /// Forget a server's sign-in, revoked at its server when it offers that.
    Logout {
        #[arg(value_name = "NAME")]
        name: String,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: Option<McpCmd>) -> Result<()> {
    let v = match &cmd {
        None => conn.request(method::MCP_LIST, Value::Null).await?,
        Some(McpCmd::Restart { name }) => {
            conn.request(method::MCP_RESTART, json!({ "name": name }))
                .await?
        }
        Some(McpCmd::Login { name, manual }) => return login(conn, json, name, *manual).await,
        Some(McpCmd::Logout { name }) => {
            conn.request(
                method::MCP_LOGOUT,
                McpLogoutParams {
                    name: name.clone(),
                    ..Default::default()
                },
            )
            .await?
        }
    };
    if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    match cmd {
        None => {
            let l: McpListResult = serde_json::from_value(v)?;
            for line in render::mcp_lines(&l) {
                println!("{line}");
            }
        }
        Some(McpCmd::Restart { .. }) => {
            let r: McpRestartResult = serde_json::from_value(v)?;
            println!("MCP server {} restarting (it was {}).", r.name, r.was);
        }
        Some(McpCmd::Logout { .. }) => {
            let r: McpLogoutResult = serde_json::from_value(v)?;
            for l in logout_lines(&r) {
                println!("{l}");
            }
        }
        Some(McpCmd::Login { .. }) => {}
    }
    Ok(())
}

async fn login(conn: &mut Conn, json: bool, name: &str, manual: bool) -> Result<()> {
    let v = conn
        .request(
            method::MCP_LOGIN_BEGIN,
            McpLoginBeginParams {
                name: name.to_string(),
                manual,
                ..Default::default()
            },
        )
        .await?;
    let begun: McpLoginBeginResult = serde_json::from_value(v)?;
    for l in host_lines(&begun) {
        eprintln!("{l}");
    }
    let agreed = !begun.elsewhere.is_empty();
    if agreed && !ask_agree()? {
        // The daemon is told, and ends the sign-in: nothing was sent.
        let params = McpLoginFinishParams {
            login_id: begun.login_id.clone(),
            ..Default::default()
        };
        let _ = conn.request(method::MCP_LOGIN_FINISH, params).await;
        anyhow::bail!("not signed in: you did not agree, and nothing was sent");
    }
    for l in address_lines(&begun) {
        eprintln!("{l}");
    }
    let mut pasted: Option<String> = None;
    let mut wait = begun.paste_after_secs;
    let r: McpLoginFinishResult = loop {
        let params = McpLoginFinishParams {
            login_id: begun.login_id.clone(),
            pasted: pasted.take(),
            wait_secs: wait,
            agreed,
            ..Default::default()
        };
        let r: McpLoginFinishResult =
            serde_json::from_value(conn.request(method::MCP_LOGIN_FINISH, params).await?)?;
        if r.state != "waiting" {
            break r;
        }
        match ask_paste(name)? {
            Some(p) => {
                pasted = Some(p);
                wait = 0;
            }
            None => wait = begun.paste_after_secs.max(30),
        }
    };
    if json {
        println!("{}", serde_json::to_string(&r)?);
        return Ok(());
    }
    for l in finish_lines(&r) {
        println!("{l}");
    }
    Ok(())
}

/// The address the browser was sent to, from the terminal; none when the
/// line is empty.
fn ask_paste(name: &str) -> Result<Option<String>> {
    if io::stdin().is_terminal() {
        eprint!(
            "No redirect yet. Paste the address your browser was sent to (Enter to keep waiting): "
        );
        io::stderr().flush()?;
    }
    let mut line = String::new();
    let n = io::stdin().read_line(&mut line)?;
    anyhow::ensure!(
        n > 0,
        "no address was given and the terminal closed: run `theseus mcp login {name}` again"
    );
    let line = line.trim().to_string();
    Ok((!line.is_empty()).then_some(line))
}

/// Whether the owner typed `yes` at the terminal. Read from a terminal
/// alone: no pipe agrees for them.
fn ask_agree() -> Result<bool> {
    anyhow::ensure!(
        io::stdin().is_terminal(),
        "this sign-in sends what it returns to another host, which is agreed to at a terminal: \
         run the command in your own terminal"
    );
    eprint!("Type yes to go on: ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim() == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every host comes first, each on a line of its own, before the
    /// address; the last line says signed in and how many tools.
    #[test]
    fn the_host_comes_before_the_address_and_the_end_is_plain() {
        let mut b = McpLoginBeginResult {
            login_id: "l".into(),
            name: "docs".into(),
            host: "auth.example.invalid".into(),
            url: "https://auth.example.invalid/authorize?client_id=c".into(),
            redirect: "http://127.0.0.1:4000/callback".into(),
            expires_secs: 600,
            paste_after_secs: 90,
            elsewhere: vec![],
        };
        assert_eq!(
            host_lines(&b),
            ["You will sign in at auth.example.invalid."]
        );
        b.elsewhere = vec!["tokens.example.invalid".into()];
        assert_eq!(
            host_lines(&b),
            [
                "You will sign in at auth.example.invalid.",
                "Part of the sign-in goes through tokens.example.invalid, which is not its \
                 issuer's own address."
            ]
        );
        // Every endpoint at one place that is not the issuer's, the sign-in's
        // own included: still named, and still asked about.
        b.elsewhere = vec!["auth.example.invalid".into()];
        assert_eq!(host_lines(&b).len(), 2);
        let lines: Vec<String> = host_lines(&b)
            .into_iter()
            .chain(address_lines(&b))
            .collect();
        assert_eq!(lines[0], "You will sign in at auth.example.invalid.");
        let host_at = lines
            .iter()
            .position(|l| l.contains("auth.example.invalid."))
            .unwrap();
        let url_at = lines
            .iter()
            .position(|l| l.starts_with("https://"))
            .unwrap();
        assert!(host_at < url_at);
        let r = McpLoginFinishResult {
            state: "done".into(),
            name: "docs".into(),
            host: "auth.example.invalid".into(),
            connected: true,
            tools: 3,
            note: String::new(),
        };
        assert_eq!(
            finish_lines(&r),
            ["Signed in to the MCP server docs at auth.example.invalid: 3 tools found."]
        );
        let mut out = McpLogoutResult {
            name: "docs".into(),
            forgot: true,
            revoked: true,
            note: String::new(),
            kept_remains: false,
        };
        assert!(logout_lines(&out)[1].contains("theseus mcp login docs"));
        // A file that could not be removed is never "forgotten".
        out.forgot = false;
        out.kept_remains = true;
        let l = logout_lines(&out);
        assert!(l[0].contains("could not be removed"), "{l:?}");
        assert!(!l[0].contains("forgotten here"), "{l:?}");
    }
}
