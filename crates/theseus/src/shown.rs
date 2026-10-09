//! What a command showed (theseus-yus0; design `stage2` §2.9, decision D7):
//! the one call by which `history`, `watch` and `confirm` (and, at its join,
//! `status`) record in the machine's seen file that a session was displayed.
//! A recording never fails a command: it is made after the output, and a
//! failure is one line on stderr.

use theseus_protocol::{method, ExecutionView, ExecutionsWatchParams, ExecutionsWatchResult};

use crate::client::Conn;
use crate::seen::{self, Seen};

/// The views of `sessions` as the board holds them now: one `executions.watch`
/// snapshot, whose rows carry the positions a mark is kept at. A session older
/// than the snapshot's recent ones is absent, and so not recorded.
pub async fn views(conn: &mut Conn, sessions: &[String]) -> anyhow::Result<Vec<ExecutionView>> {
    let snap: ExecutionsWatchResult = serde_json::from_value(
        conn.request(method::EXECUTIONS_WATCH, ExecutionsWatchParams::default())
            .await?,
    )?;
    Ok(snap
        .executions
        .into_iter()
        .filter(|v| sessions.contains(&v.session_id))
        .collect())
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
    if sessions.is_empty() {
        return;
    }
    match views(conn, sessions).await {
        Ok(v) => record(&v),
        Err(e) => eprintln!("theseus: could not record what was shown: {e}"),
    }
}
