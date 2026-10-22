//! The long form's progress column (theseus-xlias): each task's line from the
//! daemon's work board, `step 4 of 5 · Running the tests again with the fix…`, the
//! same wording `theseus work` shows (`Progress::line`). One `work.watch`
//! snapshot, read on the connection the board already holds; a daemon that
//! predates the method, or one that does not answer, leaves the column out.

use std::collections::HashMap;

use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::work::progress_line::{aged_said, step_words};
use theseus_protocol::work::WorkView;
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
    of_views(&r.views, crate::herdr::now_ms())
}

/// The lines of a snapshot's views, by session: `Progress::line`'s wording, with
/// the model's words carrying their age when they are old news (`aged_said`).
pub fn of_views(views: &[WorkView], now_ms: u64) -> HashMap<String, String> {
    views
        .iter()
        .filter_map(|v| {
            let p = v.progress.as_ref()?;
            let doing = v.doing.as_ref().map(|d| d.since_ms);
            let said = p
                .said
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| aged_said(s, p.at_ms, doing, now_ms));
            let line = match (step_words(p.steps_done, p.steps_total), said) {
                (Some(c), Some(s)) => format!("{c} · {s}"),
                (c, s) => c.or(s)?,
            };
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
        let got = of_views(&views, 1);
        assert_eq!(
            got["ses_a"],
            "step 4 of 5 · Running the tests again with the fix…"
        );
        assert_eq!(got["ses_b"], "Reading the config");
        assert!(!got.contains_key("ses_c"));
    }

    /// The words carry their age when they are over a minute older than what
    /// the task does now, and none when they are fresh (theseus-ztkhg.1).
    #[test]
    fn old_words_carry_their_age() {
        let said = |at_ms: u64| {
            view(json!({"session_id": "ses_a",
                "doing": {"what": "tool", "label": "fs.read", "since_ms": 200_000, "turn": 1, "loop_index": 0},
                "progress": {"steps_done": 3, "steps_total": 5, "said": "Round 1 done. Continuing.", "at_ms": at_ms}}))
        };
        let old = of_views(&[said(80_000)], 200_000);
        assert_eq!(
            old["ses_a"],
            "step 4 of 5 · Round 1 done. Continuing. (2 min ago)"
        );
        let fresh = of_views(&[said(170_000)], 200_000);
        assert_eq!(fresh["ses_a"], "step 4 of 5 · Round 1 done. Continuing.");
    }
}
