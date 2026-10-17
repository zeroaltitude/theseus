//! `theseus` alone and `theseus chat [SESSION]` (theseus-cyha): both exec
//! `theseus-tui chat`, the inline conversation, as `theseus tui` execs the
//! board. The CLI has no terminal dependency and keeps none.

use std::io::IsTerminal;

use anyhow::Result;

use crate::cmd;

/// The daemon's socket when nothing names one, as clap's default writes it.
const DEFAULT_SOCKET: &str = "~/.theseus/theseus.sock";

/// Bare `theseus`: with a terminal in front and behind, the conversation
/// opens (this never returns unless the exec failed, and then it is the exit
/// code). None: not a bare invocation on a terminal, so clap goes on and
/// prints usage, exit 2, as it always has.
pub fn bare() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().collect();
    let start = short_flag(&args);
    if !bare_on_a_terminal(
        if start.is_some() { 1 } else { args.len() },
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    ) {
        return None;
    }
    let socket = std::env::var("THESEUS_SOCKET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SOCKET.to_string());
    Some(match chat(&socket, false, None, start, None) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("theseus: {e:#}");
            1
        }
    })
}

/// `theseus -c` and `theseus -r` (theseus-6yka): the whole command line is
/// the one flag. `-c` continues this directory's last conversation, `-r`
/// picks one.
fn short_flag(args: &[std::ffi::OsString]) -> Option<Start> {
    match args {
        [_, f] if f == "-c" => Some(Start::Continue),
        [_, f] if f == "-r" => Some(Start::Pick),
        _ => None,
    }
}

/// How the chat finds its conversation when none is named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Continue,
    Pick,
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
        crate::Cmd::Chat {
            session,
            cont,
            resume,
            plain,
            task,
            inspect,
        } if !line_mode_wanted(*plain) => {
            let start = match (cont, resume) {
                (true, _) => Some(Start::Continue),
                (_, true) => Some(Start::Pick),
                _ => None,
            };
            let task = task.as_deref().map(|id| (id, *inspect));
            Some(chat(socket, spawn, session.as_deref(), start, task))
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
pub async fn line_mode(cli: crate::Cli) -> Result<()> {
    let mut conn = crate::connect(&cli).await?;
    let conn = &mut conn;
    let crate::Cmd::Chat {
        session,
        task,
        inspect,
        ..
    } = cli.cmd
    else {
        unreachable!("main sends only the chat here")
    };
    // A task is its own session: `--task` names it as `watch` does, and
    // `--inspect` only follows it, with no input line.
    let watch_input = !inspect;
    let sid = match session.or(task) {
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
    crate::interactive::watch(conn, cli.json, Some(sid), false, watch_input, true).await
}

/// Exec `theseus-tui chat [SESSION] [-c | -r] [--task ID [--inspect]]` over `socket`.
pub fn chat(
    socket: &str,
    spawn: bool,
    session: Option<&str>,
    start: Option<Start>,
    task: Option<(&str, bool)>,
) -> Result<()> {
    let mut args = vec!["chat".to_string()];
    args.extend(session.map(String::from));
    match start {
        Some(Start::Continue) => args.push("-c".to_string()),
        Some(Start::Pick) => args.push("-r".to_string()),
        None => {}
    }
    if let Some((id, inspect)) = task {
        args.extend(["--task".to_string(), id.to_string()]);
        if inspect {
            args.push("--inspect".to_string());
        }
    }
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

    #[test]
    fn the_whole_command_line_is_one_short_flag() {
        let a = |v: &[&str]| {
            v.iter()
                .map(Into::into)
                .collect::<Vec<std::ffi::OsString>>()
        };
        assert_eq!(short_flag(&a(&["theseus", "-c"])), Some(Start::Continue));
        assert_eq!(short_flag(&a(&["theseus", "-r"])), Some(Start::Pick));
        assert_eq!(short_flag(&a(&["theseus"])), None);
        assert_eq!(short_flag(&a(&["theseus", "-c", "x"])), None);
        assert_eq!(short_flag(&a(&["theseus", "-v"])), None);
    }
}
