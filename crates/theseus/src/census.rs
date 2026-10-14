//! `theseus census [SESSION] [--place cli|dm|task|harness] [--exact] [--json]` (theseus-7m7w.1): the next
//! request of a session, or a fresh session's first request for a kind of place, counted part by part down to
//! each tool and its use. One read (`context.census`): it compiles once on the daemon and sends nothing.

use anyhow::Result;
use theseus_client::render::census::census_lines;
use theseus_client::{resolve, Conn};
use theseus_protocol::census::{ContextCensusParams, ContextCensusResult};
use theseus_protocol::method;

/// `theseus census`, as `run` in main.rs hands it on whole (one line there: that match is at clippy's
/// length limit).
pub(crate) async fn cmd(conn: &mut Conn, json: bool, cmd: crate::Cmd) -> Result<()> {
    let crate::Cmd::Census {
        session,
        place,
        exact,
    } = cmd
    else {
        unreachable!("main.rs hands census::cmd only `theseus census`")
    };
    census(conn, json, session, place, exact).await
}

async fn census(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    place: Option<String>,
    exact: bool,
) -> Result<()> {
    let session_id = match session {
        Some(s) => Some(resolve::session(conn, &s).await?),
        None => None,
    };
    let v = conn
        .request(
            method::CONTEXT_CENSUS,
            ContextCensusParams {
                session_id,
                place,
                exact,
            },
        )
        .await?;
    if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    let r: ContextCensusResult = serde_json::from_value(v)?;
    for l in census_lines(&r) {
        println!("{l}");
    }
    Ok(())
}
