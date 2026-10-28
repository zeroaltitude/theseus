//! `theseus open turn [TURN]` (theseus-x9swu): the link to one turn in the
//! cockpit's inspector, printed, and opened in a browser where one is found.
//! The daemon builds the link from `[cockpit] public_url` (`turn.inspect`
//! answers it), so a relay's port is the config's, never this file's.

use anyhow::Result;
use serde_json::json;
use theseus_client::{resolve, Conn};
use theseus_protocol::inspect::{TurnInspectParams, TurnInspectResult};
use theseus_protocol::method;

#[derive(clap::Subcommand, Debug)]
pub enum What {
    /// A turn in the inspector.
    Turn {
        /// A turn's id, or a unique start or end of one; default the session's last.
        turn: Option<String>,
        /// The session; defaults to the most recently active, as `history` does.
        #[arg(long, value_name = "SESSION")]
        session: Option<String>,
        /// Print the link and open nothing.
        #[arg(long)]
        print: bool,
    },
}

pub async fn open(conn: &mut Conn, json: bool, what: What) -> Result<()> {
    let What::Turn {
        turn,
        session,
        print,
    } = what;
    let session_id = match session {
        Some(s) => resolve::session(conn, &s).await?,
        None => crate::why::latest_session(conn).await?,
    };
    let turn_id = turn_of(conn, &session_id, turn).await?;
    let v = conn
        .request(
            method::TURN_INSPECT,
            TurnInspectParams {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
            },
        )
        .await?;
    let r: TurnInspectResult = serde_json::from_value(v)?;
    if json {
        println!(
            "{}",
            json!({"session_id": r.session_id, "turn_id": r.turn_id, "url": r.url})
        );
        return Ok(());
    }
    if r.url.is_empty() {
        println!("no web UI runs, so there is no link: set `[web] enabled = true`, or `[cockpit] public_url` where a relay serves it");
        return Ok(());
    }
    println!("{}", r.url);
    if !print {
        browse(&r.url);
    }
    Ok(())
}

/// The turn named (a whole id, or a unique start or end of one), else the
/// session's last.
pub(crate) async fn turn_of(
    conn: &mut Conn,
    session_id: &str,
    turn: Option<String>,
) -> Result<String> {
    match turn {
        Some(t) => crate::why::find_started_turn(conn, session_id, &t).await,
        None => last_turn(conn, session_id).await,
    }
}

/// The session's last turn: its newest `turn.started` row, which a turn that
/// compiled no context (a detour's) has too (theseus-v8h48).
async fn last_turn(conn: &mut Conn, session_id: &str) -> Result<String> {
    crate::why::started_turns(conn, session_id)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("{session_id} has no turn yet"))
}

/// The first opener found: `$BROWSER`, `wslview`, `xdg-open`, `open`. Nothing
/// found, or one that fails, is not an error: the link is printed.
fn browse(url: &str) {
    let env = std::env::var("BROWSER").ok();
    for opener in env
        .iter()
        .map(String::as_str)
        .chain(["wslview", "xdg-open", "open"])
    {
        if std::process::Command::new(opener)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}
