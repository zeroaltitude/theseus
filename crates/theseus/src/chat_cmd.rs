//! `theseus` alone and `theseus chat [SESSION]` (theseus-cyha): both exec
//! `theseus-tui chat`, the inline conversation, as `theseus tui` execs the
//! board. The CLI has no terminal dependency and keeps none.

use std::io::IsTerminal;

use anyhow::Result;

use theseus_client::Conn;

use crate::cmd;

/// The daemon's socket when nothing names one, as clap's default writes it.
const DEFAULT_SOCKET: &str = "~/.theseus/theseus.sock";

/// Bare `theseus`: with a terminal in front and behind, the conversation
/// opens (this never returns unless the exec failed, and then it is the exit
/// code). None: not a bare invocation on a terminal, so clap goes on and
/// prints usage, exit 2, as it always has.
pub fn bare() -> Option<i32> {
    if !bare_on_a_terminal(
        std::env::args_os().len(),
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    ) {
        return None;
    }
    let socket = std::env::var("THESEUS_SOCKET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SOCKET.to_string());
    Some(match chat(&socket, false, None) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("theseus: {e:#}");
            1
        }
    })
}

/// Whether this is the bare command on a terminal: no argument at all, and
/// stdin and stdout both a terminal.
fn bare_on_a_terminal(args: usize, stdin_tty: bool, stdout_tty: bool) -> bool {
    args == 1 && stdin_tty && stdout_tty
}

/// `theseus tui [ARGS]` and `theseus chat [SESSION]`: exec `theseus-tui`.
/// None: another command.
pub fn exec_tui(cmd: &crate::Cmd, socket: &str, spawn: bool) -> Option<Result<()>> {
    match cmd {
        crate::Cmd::Tui { args } => Some(cmd::tui(socket, spawn, args)),
        crate::Cmd::Chat { session, plain } if !line_mode_wanted(*plain) => {
            Some(chat(socket, spawn, session.as_deref()))
        }
        _ => None,
    }
}

/// Line mode: asked for with `--plain`, or the terminal is no screen (a pipe
/// in or out, `TERM=dumb`). The same conversation as `watch --interactive`,
/// with the tasks' events as lines.
fn line_mode_wanted(plain: bool) -> bool {
    plain
        || !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
        || std::env::var("TERM").is_ok_and(|t| t == "dumb")
}

/// `theseus chat --plain [SESSION]`: SESSION's conversation, or a new one in
/// this directory, followed by `watch --interactive`'s loop.
pub async fn line_mode(conn: &mut Conn, json: bool, session: Option<String>) -> Result<()> {
    let sid = match session {
        Some(s) => s,
        None => {
            let dir = std::env::current_dir()?;
            let v = conn
                .request(
                    theseus_protocol::method::SESSION_OPEN,
                    serde_json::json!({ "dir": dir.to_string_lossy() }),
                )
                .await?;
            let info: theseus_protocol::SessionInfo = serde_json::from_value(v)?;
            info.session_id
        }
    };
    crate::interactive::watch(conn, json, Some(sid), false, true, true).await
}

/// Exec `theseus-tui chat [SESSION]` over `socket`.
pub fn chat(socket: &str, spawn: bool, session: Option<&str>) -> Result<()> {
    let mut args = vec!["chat".to_string()];
    args.extend(session.map(String::from));
    cmd::tui(socket, spawn, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bare_command_on_a_terminal_opens_the_chat() {
        assert!(bare_on_a_terminal(1, true, true));
        assert!(!bare_on_a_terminal(1, false, true), "a pipe in");
        assert!(!bare_on_a_terminal(1, true, false), "a pipe out");
        assert!(
            !bare_on_a_terminal(2, true, true),
            "any argument, --help included"
        );
    }
}
