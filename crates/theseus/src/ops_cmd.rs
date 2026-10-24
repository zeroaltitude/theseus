//! The Operations page's verbs from a shell (theseus-5iqf8): `task rehome`, `task reschedule` and
//! `sessions model`. One call each, no model turn. The three methods are in `client::OPERATORS`,
//! so a job's shell is refused before anything is sent.

use anyhow::{bail, Result};
use theseus_client::render::{short_id, task_tree, time::fmt_stamp};
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::ops::{
    SessionProfileParams, SessionProfileResult, TaskRehomeParams, TaskRehomeResult,
    TaskRescheduleParams, TaskRescheduleResult,
};

use crate::cmd::output;

/// `theseus task rehome ID HOME`: a conversation is resolved as `task start --in` does;
/// `thread:<slug>` goes as typed.
pub async fn rehome(conn: &mut Conn, json: bool, id: &str, home: &str) -> Result<()> {
    let home = match home.starts_with("thread:") {
        true => home.to_string(),
        false => theseus_client::resolve::session(conn, home).await?,
    };
    let p = TaskRehomeParams {
        task: id.to_string(),
        home,
        author: None,
        discord: None,
    };
    let v = conn.request(method::TASK_REHOME, p).await?;
    output(json, v, |r: TaskRehomeResult| {
        for l in rehome_lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

fn rehome_lines(r: &TaskRehomeResult) -> Vec<String> {
    let place = |p: &Option<String>| match p {
        Some(p) => p.clone(),
        None => "the board alone".to_string(),
    };
    vec![
        format!(
            "task {}: its report goes to {} (was {})",
            task_tree::short_id(&r.task),
            place(&r.to),
            place(&r.from)
        ),
        r.note.clone(),
    ]
}

/// `theseus task reschedule ID (--at WHEN | --in-time SPAN)`: `start_when` is `task start`'s.
pub async fn reschedule(
    conn: &mut Conn,
    json: bool,
    id: &str,
    start_when: Option<String>,
) -> Result<()> {
    let p = TaskRescheduleParams {
        task: id.to_string(),
        start_at_ms: None,
        start_after_ms: None,
        start_when,
        author: None,
        discord: None,
    };
    let v = conn.request(method::TASK_RESCHEDULE, p).await?;
    output(json, v, |r: TaskRescheduleResult| {
        println!(
            "task {} will start at {} (was {}); to undo: theseus task reschedule {id} --at \"{}\"",
            task_tree::short_id(&r.task),
            fmt_stamp(r.starts_at_ms),
            fmt_stamp(r.previous),
            fmt_stamp(r.previous)
        );
        Ok(())
    })
}

/// `theseus sessions model SESSION (PROFILE | --off)`: `--off` sends null, the let-go. A
/// `refused` answer is ok on the wire; it is printed and the exit is 1.
pub async fn model(
    conn: &mut Conn,
    json: bool,
    session: &str,
    profile: Option<String>,
) -> Result<()> {
    let session_id = theseus_client::resolve::session(conn, session).await?;
    let p = SessionProfileParams {
        session_id,
        profile,
        author: None,
        discord: None,
    };
    let v = conn.request(method::SESSION_PROFILE, p).await?;
    let mut refused = None;
    output(json, v.clone(), |r: SessionProfileResult| {
        for l in model_lines(&r) {
            println!("{l}");
        }
        refused = r.refused;
        Ok(())
    })?;
    if json {
        refused = v["refused"].as_str().map(str::to_string);
    }
    match refused {
        Some(why) => bail!("refused: {why}"),
        None => Ok(()),
    }
}

fn model_lines(r: &SessionProfileResult) -> Vec<String> {
    let sid = short_id(&r.session_id);
    let mut out = match (&r.refused, &r.profile) {
        (Some(_), _) => vec![format!("session {sid}: nothing changed")],
        (None, Some(p)) => vec![format!(
            "session {sid}: its turns run on {p} from the next one"
        )],
        (None, None) => vec![format!(
            "session {sid}: held to no profile; the default routes"
        )],
    };
    if let Some(prev) = &r.previous {
        out.push(format!("was {prev}"));
    }
    if let Some(n) = &r.note {
        out.push(n.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    /// Every `OPERATORS` entry names a command the CLI has: its words (before any `|` or
    /// ` --`) walk clap's tree. A job's refusal says "run `theseus x y`", so the words must
    /// be a real command.
    #[test]
    fn every_operators_command_names_a_real_subcommand() {
        let root = crate::Cli::command();
        let mut bad = Vec::new();
        for (method, command) in theseus_client::client::OPERATORS {
            let words = command.strip_prefix("theseus ").unwrap_or(command);
            let words = words.split('|').next().unwrap_or(words);
            let words = words.split(" --").next().unwrap_or(words);
            let mut at = &root;
            for w in words.split_whitespace() {
                match at.find_subcommand(w) {
                    Some(next) => at = next,
                    None => {
                        bad.push(format!("{method}: `{command}` (no `{w}`)"));
                        break;
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }
}
