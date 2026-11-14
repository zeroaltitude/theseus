//! The renderer's held turns after a restart (issue 8): they are a view built
//! from events and held only in memory, so a late event for a turn that ran
//! before the restart (a background job's result, an answer to a parked call, the
//! end of a turn that was running) found no turn, and its tool line and its
//! text were never shown again. What a view needs lives in the durable record:
//! the session's nodes. The first time an event names a turn this renderer never
//! saw start, the newest turns of the session are folded back from the nodes the
//! session stored, once, and the event is then read as any other.
//!
//! A turn is its loops' text (an assistant message's text blocks, at its loop),
//! and its calls: each tool call node is a line, each tool result node settles
//! it. The lines are the ones the live events made (`ToolLine::proposed`), so
//! the messages the rebuilt turn writes carry the keys the first run's did
//! (`<turn>:L<n>:p<i>`, `<turn>:L<n>:tools`) and, with the lane's own rebuild
//! (`courier/recall.rs`), edit those messages instead of posting them again.
//! A call that had not ended is `Proposed`: the first run's own state past that
//! is told again by the events that follow it.

use std::collections::BTreeMap;

use theseus_core::node::{Body, Node};

use super::{LoopView, Renderer, ToolLine, ToolState, TurnView, RECENT_TURNS};

impl Renderer {
    /// Whether an event for `turn_id` should first rebuild the held turns: it
    /// is not one this renderer holds, and they have not been rebuilt yet.
    pub fn needs_rebuild(&self, turn_id: &str) -> bool {
        !self.rebuilt && !self.knows(turn_id)
    }

    /// Fold the newest turns of a session back from its `nodes` (oldest
    /// first): those this renderer does not hold, as many as it holds at most
    /// (`RECENT_TURNS`), placed before the turns it already holds. It does this
    /// once: a turn older than these, or one with no nodes, stays unknown, as a
    /// turn past `RECENT_TURNS` always did.
    pub fn rebuild(&mut self, nodes: &[(u64, Node)]) {
        if std::mem::replace(&mut self.rebuilt, true) {
            return;
        }
        let mut order: Vec<&str> = Vec::new();
        for (_, n) in nodes {
            if let Some(t) = n.turn_id.as_deref().filter(|t| !t.is_empty()) {
                if !order.contains(&t) {
                    order.push(t);
                }
            }
        }
        let room = RECENT_TURNS.saturating_sub(self.turns.len());
        let missing: Vec<&str> = order
            .into_iter()
            .filter(|t| !self.knows(t))
            .rev()
            .take(room)
            .collect();
        // Newest first, each put at the front: the oldest ends up first, ahead
        // of the turns already held.
        for turn_id in missing {
            let view = fold(turn_id, nodes);
            self.turns.push_front(view);
        }
        self.drop_past_recent();
    }
}

/// One turn's view, from the nodes that name it.
fn fold(turn_id: &str, nodes: &[(u64, Node)]) -> TurnView {
    let mut loops: BTreeMap<u32, LoopView> = BTreeMap::new();
    for (_, n) in nodes
        .iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn_id))
    {
        let li = n.loop_index.unwrap_or(0);
        match &n.body {
            Body::AssistantMessage { blocks, .. } => {
                let text: String = blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("");
                if !text.trim().is_empty() {
                    loops.entry(li).or_default().text = text;
                }
            }
            Body::ToolCall {
                tool_use_id,
                tool,
                input,
                correlation_id,
                gate: Some(gate),
                ..
            } => {
                let mut line = ToolLine::proposed(tool_use_id, tool, input, gate);
                line.correlation_id.clone_from(correlation_id);
                loops.entry(li).or_default().tools.push(line);
            }
            Body::ToolResult {
                tool_use_id,
                status,
                duration_ms,
                late,
                ..
            } => {
                let end = ToolState::Done {
                    status: status.as_str().to_string(),
                    ms: duration_ms.unwrap_or(0),
                };
                let found = loops
                    .values_mut()
                    .flat_map(|lv| lv.tools.iter_mut())
                    .find(|l| l.tool_use_id == *tool_use_id);
                if let Some(l) = found {
                    l.state = end;
                    l.late |= *late;
                }
            }
            _ => {}
        }
    }
    TurnView {
        turn_id: turn_id.to_string(),
        loops,
        ended: false,
        dirty: false,
        step_due: false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use theseus_core::node::{Body, Node, ResultStatus};

    use super::super::tests::{ended, upserts};
    use super::super::{Op, Renderer, RECENT_TURNS};

    pub(crate) fn assistant(turn: &str, loop_index: u32, text: &str) -> (u64, Node) {
        let body = Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": text})],
            model: "glm-x".into(),
            provider: "zed".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        };
        (0, Node::assistant("s", turn, loop_index, body))
    }

    pub(crate) fn call(turn: &str, loop_index: u32, id: &str, path: &str) -> (u64, Node) {
        let gate = serde_json::from_value(json!({"result": {"gate": "allow"}})).unwrap();
        let body = Body::ToolCall {
            tool_use_id: id.into(),
            tool: "fs.read".into(),
            wire_name: "fs_read".into(),
            input: json!({"path": path}),
            assistant_node: "msg_1".into(),
            correlation_id: None,
            gate: Some(Box::new(gate)),
        };
        (0, Node::tool_call("s", Some(turn), Some(loop_index), body))
    }

    pub(crate) fn result(turn: &str, loop_index: u32, id: &str, ms: u64) -> (u64, Node) {
        let body = Body::ToolResult {
            tool_use_id: id.into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: "x".into(),
            correlation_id: None,
            bytes_total: 1,
            truncated: false,
            full_ref: None,
            duration_ms: Some(ms),
            late: false,
            meta: json!(null),
            image: None,
            external: None,
            payload: None,
        };
        (
            0,
            Node::tool_result("s", Some(turn), Some(loop_index), body),
        )
    }

    /// A turn that was running when the daemon restarted: its first loop's
    /// text and one call that had not ended.
    fn before_the_restart(turn: &str) -> Vec<(u64, Node)> {
        vec![
            assistant(turn, 0, "Reading the chart now."),
            call(turn, 0, "u1", "src/lib.rs"),
        ]
    }

    /// The red: before, an event for a turn this renderer never saw start was
    /// dropped (no turn to hold it), so a late end of a call showed nowhere.
    /// Now the held turn is read back from the nodes, and the call's end
    /// writes the very keys the first run's messages had, with the text and
    /// the line the nodes held.
    #[test]
    fn a_renderer_that_restarted_reads_a_turn_back_and_shows_a_late_end_under_its_keys() {
        let mut r = Renderer::default();
        assert!(r.needs_rebuild("t1"), "a turn it never saw start");
        r.rebuild(&before_the_restart("t1"));
        assert!(r.knows("t1") && !r.needs_rebuild("t1"));
        assert_eq!(r.held(), ["t1"]);
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 12}),
        );
        let ops = r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0}));
        let shown: Vec<(String, String)> = upserts(&ops);
        let text = shown.iter().find(|(k, _)| k == "t1:L0:p0");
        assert_eq!(
            text.map(|(_, c)| c.as_str()),
            Some("Reading the chart now.")
        );
        let tools = shown.iter().find(|(k, _)| k == "t1:L0:tools");
        assert_eq!(
            tools.map(|(_, c)| c.as_str()),
            Some("-# ✅ `fs.read` src/lib.rs · 12 ms"),
            "{shown:?}"
        );
    }

    /// A call whose result the nodes hold is rebuilt as ended, with its time.
    #[test]
    fn a_call_the_nodes_say_ended_is_rebuilt_ended() {
        let mut nodes = before_the_restart("t1");
        nodes.push(result("t1", 0, "u1", 7));
        let mut r = Renderer::default();
        r.rebuild(&nodes);
        let ops = r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0}));
        let shown = upserts(&ops);
        assert!(
            shown
                .iter()
                .any(|(k, c)| k == "t1:L0:tools" && c.contains("7 ms")),
            "{shown:?}"
        );
    }

    /// It reads the nodes back once; a turn past the newest `RECENT_TURNS`
    /// stays unknown, as a turn dropped from the held list always did; and the
    /// turns come back oldest first, ahead of one the renderer already holds.
    #[test]
    fn it_is_read_back_once_the_newest_turns_only_and_before_the_held_ones() {
        let mut nodes = Vec::new();
        for n in 0..RECENT_TURNS + 4 {
            nodes.push(assistant(&format!("t{n:02}"), 0, "hi"));
        }
        let mut r = Renderer::default();
        r.on_notification(
            "turn.started",
            &json!({"session_id": "s", "turn_id": "live"}),
        );
        r.rebuild(&nodes);
        let held = r.held();
        assert_eq!(held.len(), RECENT_TURNS, "{held:?}");
        assert_eq!(
            held.last().map(String::as_str),
            Some("live"),
            "the held one stays last"
        );
        let newest = format!("t{:02}", RECENT_TURNS + 3);
        assert_eq!(held[held.len() - 2], newest);
        assert!(
            held.windows(2).all(|w| w[0] < w[1] || w[1] == "live"),
            "{held:?}"
        );
        assert!(
            !held.contains(&"t00".to_string()),
            "the oldest stay unknown"
        );
        let before = r.held();
        r.rebuild(&nodes);
        assert_eq!(r.held(), before, "read back once");
        assert!(!r.needs_rebuild("t00"), "and not asked for again");
    }

    /// A turn that ended is rebuilt as the live one is: nothing of the loop's
    /// text is streamed again, only the tool message is live (`ended`).
    #[test]
    fn the_rebuilt_turn_takes_its_end_as_any_turn_does() {
        let mut r = Renderer::default();
        r.rebuild(&before_the_restart("t1"));
        let ops = r.on_notification("turn.ended", &ended("t1", None));
        assert!(!ops.iter().any(|o| matches!(o, Op::Typing)));
        assert!(!r.busy());
    }
}
