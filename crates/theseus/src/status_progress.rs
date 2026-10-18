//! The long form's progress column (theseus-xlias): each task's line from the
//! daemon's work board, `step 4 of 5 · Running the tests again with the fix…`, the
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use theseus_protocol::work::WorkView;

    fn view(v: serde_json::Value) -> WorkView {
        let mut base = json!({"id": "exe_1", "kind": "task", "position": 1, "at_ms": 1, "root": "exe_1",
            "title": "t", "state": "running", "attention": {"level": "working", "label": "turn 1", "since_ms": 1},
            "started_at_ms": 1, "state_since_ms": 1, "flagged": false});
        base.as_object_mut()
            .unwrap()
            .extend(v.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    /// The long form's column reads the same wording as `theseus work`: the
    /// count, then the model's line; a line alone; nothing for a view with none.
    #[test]
    fn a_session_gets_its_count_and_its_line() {
        let views = vec![
            view(json!({"session_id": "ses_a", "progress":
                {"steps_done": 3, "steps_total": 5, "said": "Running the tests again with the fix…", "at_ms": 1}})),
            view(json!({"session_id": "ses_b", "progress":
                {"steps_done": 0, "steps_total": 0, "said": "Reading the config", "at_ms": 1}})),
            view(json!({"session_id": "ses_c"})),
        ];
        let got = of_views(&views);
        assert_eq!(
            got["ses_a"],
            "step 4 of 5 · Running the tests again with the fix…"
        );
        assert_eq!(got["ses_b"], "Reading the config");
        assert!(!got.contains_key("ses_c"));
    }
}
