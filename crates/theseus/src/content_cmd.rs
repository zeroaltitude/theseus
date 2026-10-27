//! `theseus content` (theseus-9ymz0): a session's content labels. `theseus
//! content SESSION` lists them, one line each (the flag, when, what raised it,
//! from where); `theseus content remove SESSION FLAG` removes one, and
//! `--all` removes every label on the session.
//!
//! A label is raised by what entered the session (a tool result, a page, the
//! operator's own words), never by a command: an `injection` label stops the
//! session's calls that act, and a `secret`, `confidential` or `personal` one
//! keeps it off a China-origin model, until the operator removes it. A removal
//! loosens, so it is the owner's act (`content.unlabel` is in
//! `client::OPERATORS`: a job's shell is refused before anything is sent, and
//! the daemon refuses it again).

use anyhow::{anyhow, bail, Result};
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use theseus_client::{render, Conn};
use theseus_protocol::{method, ContentUnlabelParams, LabelView, SessionListResult, UnlabelResult};

use crate::cmd::output;

/// The flags' words, as the daemon takes them.
const FLAGS: [&str; 4] = ["injection", "secret", "confidential", "personal"];

#[derive(Args, Debug)]
pub struct ContentArgs {
    /// The conversation or task: its id, or at least its last four characters.
    #[arg(value_name = "SESSION")]
    pub session: Option<String>,
    #[command(subcommand)]
    pub cmd: Option<ContentCmd>,
}

#[derive(Subcommand, Debug)]
pub enum ContentCmd {
    /// Remove a label from SESSION: `injection` lets its calls that act run at their postures
    /// again, and `secret`, `confidential` or `personal` lets it use a China-origin model again.
    /// It loosens, so it counts only where an approval would, and never from a job's shell.
    Remove {
        #[arg(value_name = "SESSION")]
        session: String,
        /// `injection`, `secret`, `confidential` or `personal`.
        #[arg(value_name = "FLAG", value_parser = FLAGS, required_unless_present = "all",
              conflicts_with = "all")]
        flag: Option<String>,
        /// Remove every label on the session.
        #[arg(long)]
        all: bool,
    },
}

pub async fn run(conn: &mut Conn, json: bool, a: ContentArgs) -> Result<()> {
    match (a.cmd, a.session) {
        (Some(ContentCmd::Remove { session, flag, all }), _) => {
            remove(conn, json, &session, flag, all).await
        }
        (None, Some(session)) => list(conn, json, &session).await,
        (None, None) => bail!("name a session: `theseus content SESSION`"),
    }
}

/// The session's labels, from `session.list`.
async fn labels_of(conn: &mut Conn, session: &str) -> Result<(String, Vec<LabelView>)> {
    let id = theseus_client::resolve::session(conn, session).await?;
    let l: SessionListResult = serde_json::from_value(
        conn.request(method::SESSION_LIST, json!({ "ids": [id] }))
            .await?,
    )?;
    let s = l
        .sessions
        .into_iter()
        .find(|s| s.session_id == id)
        .ok_or_else(|| anyhow!("no session is named `{session}`"))?;
    Ok((id, s.labels))
}

async fn list(conn: &mut Conn, json: bool, session: &str) -> Result<()> {
    let (id, labels) = labels_of(conn, session).await?;
    output(
        json,
        json!({"session_id": id, "labels": labels}),
        |v: Value| {
            let labels: Vec<LabelView> = serde_json::from_value(v["labels"].clone())?;
            for l in render::content::labels_lines(&id, &labels) {
                println!("{l}");
            }
            Ok(())
        },
    )
}

async fn remove(
    conn: &mut Conn,
    json: bool,
    session: &str,
    flag: Option<String>,
    all: bool,
) -> Result<()> {
    let (id, labels) = labels_of(conn, session).await?;
    let flags: Vec<String> = match (flag, all) {
        (Some(f), false) => vec![f],
        (None, true) => labels.iter().map(|l| l.flag.clone()).collect(),
        _ => bail!("name a flag, or pass --all"),
    };
    if flags.is_empty() {
        output(
            json,
            json!({"session_id": id, "removed": []}),
            |_: Value| {
                println!("{}", render::content::labels_lines(&id, &[])[0]);
                Ok(())
            },
        )?;
        return Ok(());
    }
    let mut done: Vec<UnlabelResult> = Vec::new();
    for f in flags {
        let v = conn
            .request(
                method::CONTENT_UNLABEL,
                ContentUnlabelParams {
                    session_id: id.clone(),
                    flag: f,
                    author: None,
                    discord: None,
                },
            )
            .await?;
        done.push(serde_json::from_value(v)?);
    }
    if json {
        println!("{}", serde_json::to_string(&done)?);
        return Ok(());
    }
    for r in &done {
        println!("{}", render::content::unlabelled_line(r));
    }
    Ok(())
}
