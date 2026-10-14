//! `theseus watches …` (theseus-f2r2b): waiting on the world with no model.
//! `theseus watch SESSION` is the session follower and stays; this is the
//! other thing, so it is `watches`:
//!
//! - `watches add TITLE --run 'gh pr view 123 --json state -q .state' --until
//!   'equals MERGED' [--every 5m] [--for 3d] [--then notice|turn|task]`: a
//!   check on a timer until the condition holds, with no model. `--run` is
//!   one command and its arguments, split as a shell splits words (quotes and
//!   backslashes), and run as that argv in the sandbox: no pipes, no
//!   redirects, no expansion (`--run "sh -c '...'"` for a script). `--url` and
//!   `--file` check a page or a file instead. The owner's act (refused inside
//!   a job, `client::OPERATORS`); the owner typed the command, so no card asks.
//! - `watches` / `watches list`: each open watch, its last value, its checks
//!   and when it gives up.
//! - `watches rm ID`: end one (`wake.cancel`).

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::{render, Conn};
use theseus_protocol::{
    method, WatchCheck, WatchCreateParams, WatchCreateResult, WatchListParams, WatchListResult,
    WatchThen,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum WatchesCmd {
    /// The open watches (default): what each looks at, its last value and checks, and when it
    /// gives up.
    List {
        /// Only this conversation's.
        #[arg(long)]
        session: Option<String>,
    },
    /// Watch something until a condition holds, with no model: say what, then what to wait for.
    Add(Box<AddArgs>),
    /// End a watch by its id (or the end of it).
    Rm { id: String },
}

/// The arguments of `watches add`.
#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// What it waits for, as a question: "PR 123 merged?".
    title: String,
    /// A command and its arguments, run in the sandbox: its output is the value.
    #[arg(long, value_name = "COMMAND", conflicts_with_all = ["url", "file"])]
    run: Option<String>,
    /// A URL to GET: the body is the value.
    #[arg(long, conflicts_with_all = ["run", "file"])]
    url: Option<String>,
    /// A file (an absolute path): its text is the value.
    #[arg(long, conflicts_with_all = ["run", "url"])]
    file: Option<String>,
    /// With --run or --url: a JSON path into the output, such as `.state`.
    #[arg(long, value_name = "PATH")]
    field: Option<String>,
    /// The condition: `exit 0`, `equals X`, `contains X`, `matches RE`, `changes`, `above N`,
    /// `below N`, or `exists`.
    #[arg(long)]
    until: String,
    /// How often it checks: `1m` at the most often, `5m` by default.
    #[arg(long, value_name = "SPAN")]
    every: Option<String>,
    /// It gives up after this long: `3d`; 30 days at the most, and by default.
    #[arg(long = "for", value_name = "SPAN")]
    for_at_most: Option<String>,
    /// What happens when it holds: `notice` (default), `turn` (the conversation gets a turn),
    /// or `task` (with --brief).
    #[arg(long, default_value = "notice")]
    then: String,
    /// With `--then task`: what to do.
    #[arg(long)]
    brief: Option<String>,
    /// The conversation it lives in; none is your Home.
    #[arg(long)]
    session: Option<String>,
}

impl Default for WatchesCmd {
    fn default() -> Self {
        Self::List { session: None }
    }
}

/// `text` as the words a shell would split it into: spaces separate, single
/// quotes keep everything, double quotes keep spaces, a backslash escapes the
/// next character. No expansion, no operators.
pub fn words(text: &str) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => bail!("an unclosed ' in the command"),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => cur.push(c),
                            Some(c) => {
                                cur.push('\\');
                                cur.push(c);
                            }
                            None => bail!("an unclosed \" in the command"),
                        },
                        Some(c) => cur.push(c),
                        None => bail!("an unclosed \" in the command"),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(c) => cur.push(c),
                    None => bail!("a trailing \\ in the command"),
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

pub async fn go(conn: &mut Conn, json: bool, cmd: Option<WatchesCmd>) -> Result<()> {
    run(conn, json, cmd.unwrap_or_default()).await
}

async fn run(conn: &mut Conn, json: bool, cmd: WatchesCmd) -> Result<()> {
    match cmd {
        WatchesCmd::List { session } => {
            let v = conn
                .request(
                    method::WATCH_LIST,
                    serde_json::to_value(WatchListParams {
                        session_id: session,
                    })?,
                )
                .await?;
            output(json, v, |r: WatchListResult| {
                if r.watches.is_empty() {
                    println!("no open watches");
                }
                let now = theseus_protocol::now_unix_ms();
                for w in &r.watches {
                    println!("{}", render::watch_line(w, now));
                }
                Ok(())
            })
        }
        WatchesCmd::Add(a) => {
            let AddArgs {
                title,
                run,
                url,
                file,
                field: path,
                until,
                every,
                for_at_most,
                then,
                brief,
                session,
            } = *a;
            let check = match (run, url, file) {
                (Some(r), None, None) => {
                    let argv = words(&r)?;
                    if argv.is_empty() {
                        bail!("--run needs a command");
                    }
                    WatchCheck::Run { argv, json: path }
                }
                (None, Some(url), None) => WatchCheck::Fetch { url, json: path },
                (None, None, Some(path)) => WatchCheck::File { path },
                _ => bail!("say what to watch: --run COMMAND, --url URL, or --file PATH"),
            };
            let p = WatchCreateParams {
                title,
                check,
                until,
                every,
                for_at_most,
                then: Some(WatchThen { kind: then, brief }),
                session_id: session,
                author: None,
                discord: None,
            };
            let v = conn
                .request(method::WATCH_CREATE, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: WatchCreateResult| {
                let w = &r.watch;
                let now = theseus_protocol::now_unix_ms();
                println!(
                    "Watching \"{}\" ({}): every {}, until {}, giving up {}; then {}. No model is called while it waits. End it with `theseus watches rm {}`.",
                    w.title,
                    w.short,
                    w.every,
                    w.until,
                    render::until_due(w.expires_at_ms, now),
                    w.then,
                    w.short
                );
                Ok(())
            })
        }
        WatchesCmd::Rm { id } => {
            let v = conn
                .request(method::WAKE_CANCEL, serde_json::json!({"wake": id}))
                .await?;
            output(json, v, |_: serde_json::Value| {
                println!("ended {id}");
                Ok(())
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_split_as_a_shell_splits_them() {
        let w = |s: &str| words(s).unwrap();
        assert_eq!(
            w("gh pr view 123 --json state -q .state"),
            ["gh", "pr", "view", "123", "--json", "state", "-q", ".state"]
        );
        assert_eq!(
            w(r#"sh -c 'test -f /tmp/x && echo "yes"'"#),
            ["sh", "-c", r#"test -f /tmp/x && echo "yes""#]
        );
        assert_eq!(w(r#"echo "a b" c\ d ''"#), ["echo", "a b", "c d", ""]);
        assert_eq!(w("  "), Vec::<String>::new());
        assert!(words("echo 'x")
            .unwrap_err()
            .to_string()
            .contains("unclosed '"));
        assert!(words("echo \"x").is_err());
        assert!(words("echo x\\").is_err());
    }
}
