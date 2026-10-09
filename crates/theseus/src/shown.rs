//! What a command showed (theseus-yus0; design `stage2` §2.9, decision D7):
//! the one call by which `history`, `watch` and `confirm` (and, at its join,
//! `status`) record in the machine's seen file that a session was displayed.
//! A recording never fails a command: it is made after the output, and a
//! failure is one line on stderr.

use theseus_protocol::{method, ExecutionView, SessionWaitParams, SessionWaitResult, WaitUntil};

use crate::client::Conn;
use crate::seen::{self, Seen};

/// The view of each of `sessions` as the board holds it now, whose position a
/// mark is kept at. One `session.wait` each, answered at once (`timeout_ms:
/// 0`): about 450 bytes a session, where an `executions.watch` snapshot is
/// 70 KB at its 200 views and misses a session past them. A session the
/// daemon does not know, or one with no execution, is left out.
pub async fn views(conn: &mut Conn, sessions: &[String]) -> anyhow::Result<Vec<ExecutionView>> {
    let mut seen = Vec::new();
    let mut views = Vec::new();
    for session_id in sessions {
        if seen.contains(&session_id) {
            continue;
        }
        seen.push(session_id);
        let params = SessionWaitParams {
            session_id: session_id.clone(),
            until: WaitUntil::Settled,
            after_position: None,
            timeout_ms: Some(0),
        };
        let r: SessionWaitResult =
            serde_json::from_value(conn.request(method::SESSION_WAIT, params).await?)?;
        views.extend(r.execution);
    }
    Ok(views)
}

/// Record `views` as displayed in the seen file; a failure is said once on
/// stderr and returned to no one.
pub fn record(views: &[ExecutionView]) {
    let Some(path) = Seen::default_path() else {
        return;
    };
    if views.is_empty() {
        return;
    }
    if let Err(e) = seen::record(&path, views) {
        eprintln!(
            "theseus: could not record what was shown in {}: {e}",
            path.display()
        );
    }
}

/// Record that `sessions` were shown, at the position each has now: ask the
/// daemon for it, then write the file. Call it after the output.
pub async fn shown(conn: &mut Conn, sessions: &[String]) {
    // A job's reading is the agent's, not the operator's: it marks nothing seen.
    if sessions.is_empty() || crate::client::job_session().is_some() {
        return;
    }
    match views(conn, sessions).await {
        Ok(v) => record(&v),
        Err(e) => eprintln!("theseus: could not record what was shown: {e}"),
    }
}
