//! The long form's progress column (theseus-xlias): each task's line from the
//! daemon's work board, `3/5 · Running the tests again with the fix…`, the
//! same wording `theseus work` shows (`Progress::line`). One `work.watch`
//! snapshot, read on the connection the board already holds; a daemon that
//! predates the method, or one that does not answer, leaves the column out.

use std::collections::HashMap;

use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::{WorkWatchParams, WorkWatchResult};

/// Each session's progress line, for the sessions whose view has one.
pub async fn lines(conn: &mut Conn) -> HashMap<String, String> {
    let params = WorkWatchParams {
        limit: Some(0),
        ..Default::default()
    };
    let Ok(params) = serde_json::to_value(params) else {
        return HashMap::new();
    };
    let Ok(v) = conn.request(method::WORK_WATCH, params).await else {
        return HashMap::new();
    };
    let Ok(r) = serde_json::from_value::<WorkWatchResult>(v) else {
        return HashMap::new();
    };
    of_views(&r.views)
}

/// The lines of a snapshot's views, by session.
pub fn of_views(views: &[theseus_protocol::work::WorkView]) -> HashMap<String, String> {
    views
        .iter()
        .filter_map(|v| {
            let line = v.progress.as_ref()?.line()?;
            Some((v.session_id.clone()?, line))
        })
        .collect()
}
