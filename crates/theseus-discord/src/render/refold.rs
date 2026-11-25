//! The renderer's held turns after a restart (issue 8): they are a view built
//! from events and held only in memory, so a late event for a call of a turn that
//! ran before the restart (a background job's result, which a later turn takes
//! in; the end of a call a stop cancelled) found no line for the call, and was
//! never shown. What a view needs lives in the durable record: the session's
//! nodes. The first time an event names a turn this renderer never saw start, or
//! the start or end of a call none of its held turns has a line for, the newest
//! turns of the session are folded back from the nodes the session stored, once,
//! and the event is then read as any other. The rebuilt turns are ended: no turn
//! from before a restart is running.
//!
//! A turn is its loops' text (an assistant message's text blocks, at its loop),
//! and its calls: each tool call node is a line, each tool result node settles
//! it. The lines are the ones the live events made (`ToolLine::proposed`), so
//! the messages the rebuilt turn writes carry the keys the first run's did
//! (`<turn>:L<n>:p<i>`, `<turn>:L<n>:tools`) and, with the lane's own rebuild
//! (`courier/recall.rs`), edit those messages instead of posting them again.
//! A call that had not ended is `Proposed`: the first run's own state past that
//! is told again by the events that follow it. What only an event told is not
//! durable, and a rebuilt message does not show it: a judge's risk score on a
//! notified call, who answered a question a person declined, and what a job's
//! own start said it was given. A call that waited for an answer is not kept
//! first in a long step, as it was live.

use std::collections::BTreeMap;

use theseus_core::fact::Fact;
use theseus_core::node::{Body, Node};
use theseus_protocol::Event;

use super::{LoopView, Renderer, ToolLine, TurnView, RECENT_TURNS};

impl Renderer {
    /// Whether `e` should first rebuild the held turns, which have not been
    /// rebuilt yet: it names a turn this renderer does not hold, or it is the
    /// start or the end of a call no held turn has a line for. That second case
    /// is the usual one after a restart: a background job's late result names
    /// a *later* turn (the one that took it in), which the renderer saw start,
    /// while the call's line is in an earlier turn that it did not. (The end of
    /// a call a stop cancelled names the call's own turn, which it did not see
    /// start: the first clause finds it.)
    pub fn needs_rebuild(&self, e: &Event) -> bool {
        if self.rebuilt {
            return false;
        }
        if e.turn_id().is_some_and(|t| !self.knows(t)) {
            return true;
        }
        let call = match e {
            Event::ToolStarted(s) => Some(&s.tool_use_id),
            Event::ToolEnded(t) => Some(&t.tool_use_id),
            _ => None,
        };
        call.is_some_and(|c| !self.holds_call(c))
    }

    /// Whether a held turn has a line for the call `tool_use_id`.
    fn holds_call(&self, tool_use_id: &str) -> bool {
        self.turns.iter().any(|t| {
            t.loops
                .values()
                .any(|lv| lv.tools.iter().any(|l| l.tool_use_id == tool_use_id))
        })
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
        // Newest first.
        let mut views: Vec<TurnView> = missing
            .into_iter()
            .map(|t| fold(t, nodes, self.exits.as_deref()))
            .collect();
        // Every result settles its call's line wherever that line is, as a late
        // result does live (`update_tool` finds a line by its call): a background
        // job's result is stored under the later turn that took it in, not under
        // the call's own. Each is read as the live `tool.ended` reads it, from the
        // result node (`ToolEnded::event`), so who stopped a call, what it
        // reached, and the rest are the live line's.
        for (_, n) in nodes {
            if !matches!(n.body, Body::ToolResult { .. }) {
                continue;
            }
            let fact = theseus_core::fact::tool::ToolEnded {
                session_id: &n.session_id,
                turn_id: n.turn_id.as_deref().unwrap_or(""),
                node: n,
            };
            let Some(Event::ToolEnded(t)) = fact.event() else {
                continue;
            };
            let line = views
                .iter_mut()
                .flat_map(|v| v.loops.values_mut())
                .flat_map(|lv| lv.tools.iter_mut())
                .find(|l| l.tool_use_id == t.tool_use_id);
            if let Some(l) = line {
                l.end(&t);
            }
        }
        // Each put at the front: the oldest ends up first, ahead of the turns
        // already held.
        for view in views {
            self.turns.push_front(view);
        }
        self.drop_past_recent();
    }
}

/// One turn's view, from the nodes that name it.
fn fold(
    turn_id: &str,
    nodes: &[(u64, Node)],
    exits: Option<&theseus_core::exits::Exits>,
) -> TurnView {
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
                    let lv = loops.entry(li).or_default();
                    lv.text = text;
                    lv.complete = true;
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
                let mut input = input.clone();
                if let Some(exits) = exits {
                    super::leaving::strings(exits, &mut input);
                }
                let mut line = ToolLine::proposed(tool_use_id, tool, &input, gate);
                line.correlation_id.clone_from(correlation_id);
                loops.entry(li).or_default().tools.push(line);
            }
            _ => {}
        }
    }
    // Ended: no turn from before a restart is running (the core resumes work as
    // a new turn), so none is busy or the one a `/stop` stops, and a late end of
    // one of its calls shows at once, since no loop's end is coming to carry it.
    TurnView {
        turn_id: turn_id.to_string(),
        loops,
        ended: true,
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
            parts: Vec::new(),
        };
        (
            0,
            Node::tool_result("s", Some(turn), Some(loop_index), body),
        )
    }

    /// The event of a call's end, as the stream sends it.
    fn ended_call(turn: &str, id: &str) -> theseus_protocol::Event {
        theseus_protocol::Event::from_notification(
            "tool.ended",
            &json!({"session_id": "s", "turn_id": turn, "tool_use_id": id, "status": "ok", "duration_ms": 12}),
        )
        .unwrap()
        .unwrap()
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
        assert!(
            r.needs_rebuild(&ended_call("t1", "u1")),
            "a turn it never saw start"
        );
        r.rebuild(&before_the_restart("t1"));
        assert!(r.knows("t1") && !r.needs_rebuild(&ended_call("t1", "u1")));
        assert_eq!(r.held(), ["t1"]);
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 12}),
        );
        // An ended turn's text is its reply's post, not the stream's: only the
        // call's line is live, and it shows at the next tick.
        let shown: Vec<(String, String)> = upserts(&r.tick());
        assert!(
            !shown.iter().any(|(k, _)| k == "t1:L0:p0"),
            "the text is the reply's: {shown:?}"
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
        assert!(
            !r.needs_rebuild(&ended_call("t00", "u9")),
            "and not asked for again"
        );
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

    /// No turn from before a restart is running: a rebuilt turn is not busy,
    /// is not the one a `/stop` stops, and keeps no "typing…" alive.
    #[test]
    fn a_rebuilt_turn_is_not_running() {
        let mut r = Renderer::default();
        r.rebuild(&before_the_restart("t1"));
        assert!(
            !r.busy(),
            "typing would be kept alive for a turn from before the restart"
        );
        assert!(r.running_turn().is_none());
    }

    /// A call the nodes say went to the background is rebuilt as the live
    /// `tool.ended` makes it (waiting for its late result), a declined one as
    /// not run: the one mapping serves both.
    #[test]
    fn a_background_or_declined_call_is_rebuilt_as_the_live_line() {
        let mut nodes = before_the_restart("t1");
        nodes.push(call("t1", 0, "u2", "b.txt"));
        let bg = |id: &str, status| {
            let (p, mut n) = result("t1", 0, id, 7);
            if let Body::ToolResult { status: s, .. } = &mut n.body {
                *s = status;
            }
            (p, n)
        };
        nodes.push(bg("u1", ResultStatus::Background));
        nodes.push(bg("u2", ResultStatus::Declined));
        let mut r = Renderer::default();
        r.rebuild(&nodes);
        let ops = r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0}));
        let shown = upserts(&ops);
        let tools = shown
            .iter()
            .find(|(k, _)| k == "t1:L0:tools")
            .map(|(_, c)| c.clone());
        let live = {
            let mut l = Renderer::default();
            l.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
            for (id, path, status) in [
                ("u1", "src/lib.rs", "background"),
                ("u2", "b.txt", "declined"),
            ] {
                l.on_notification(
                    "tool.proposed",
                    &json!({"turn_id": "t1", "tool_use_id": id, "tool": "fs.read",
                    "input": {"path": path}, "gate": {"result": {"gate": "allow"}}}),
                );
                l.on_notification("tool.ended", &json!({"turn_id": "t1", "tool_use_id": id, "status": status, "duration_ms": 7}));
            }
            let ops = l.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0}));
            upserts(&ops)
                .into_iter()
                .find(|(k, _)| k == "t1:L0:tools")
                .map(|(_, c)| c)
        };
        assert_eq!(tools, live, "rebuilt and live say the same");
        assert!(!tools.unwrap().contains('❌'));
    }

    /// The usual path after a restart: a call's late end names a later turn,
    /// one the renderer saw start, while the call's line is in an earlier one.
    /// The call's absence is what asks for the rebuild, and the line is then
    /// found as a late result finds it, by the call.
    #[test]
    fn a_late_end_that_names_a_later_turn_finds_the_call_of_an_earlier_one() {
        let mut nodes = before_the_restart("t1");
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t2"}));
        let late = ended_call("t2", "u1");
        assert!(r.needs_rebuild(&late), "no held turn has a line for u1");
        nodes.push(assistant("t2", 0, "later"));
        r.rebuild(&nodes);
        assert!(!r.needs_rebuild(&late), "now one has");
        let mut ops = r.on_event(&late);
        ops.extend(r.tick());
        let shown = upserts(&ops);
        assert!(
            shown
                .iter()
                .any(|(k, c)| k == "t1:L0:tools" && c.contains("12 ms")),
            "{shown:?}"
        );
    }

    /// A call the owner stopped is rebuilt as the live line says it (stopped,
    /// by whom), not as a failure, and an L1 job's scratch and reach stay on
    /// its line: the rebuild reads the end from the result node as the live
    /// event does.
    #[test]
    fn a_stopped_call_and_a_jobs_suffixes_are_rebuilt_as_the_live_line() {
        let mut nodes = before_the_restart("t1");
        nodes.push(call("t1", 0, "u2", "b.txt"));
        let end = |id: &str, status: ResultStatus, meta: serde_json::Value| {
            let (p, mut n) = result("t1", 0, id, 7);
            if let Body::ToolResult {
                status: s, meta: m, ..
            } = &mut n.body
            {
                *s = status;
                *m = meta;
            }
            (p, n)
        };
        nodes.push(end(
            "u1",
            ResultStatus::Cancelled,
            json!({"stopped_by": "alice"}),
        ));
        nodes.push(end(
            "u2",
            ResultStatus::Ok,
            json!({"detail": {"scratch": {"summary": "wrote 2 files to scratch"}}}),
        ));
        let mut r = Renderer::default();
        r.rebuild(&nodes);
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u2", "status": "ok", "duration_ms": 7}),
        );
        let shown = upserts(&r.tick());
        let tools = shown
            .iter()
            .find(|(k, _)| k == "t1:L0:tools")
            .map(|(_, c)| c.clone())
            .unwrap();
        assert!(tools.contains("stopped by alice"), "{tools}");
        assert!(!tools.contains('❌'), "{tools}");
        assert!(tools.contains("wrote 2 files to scratch"), "{tools}");
    }

    /// A background job's result taken in by a later turn before the restart is
    /// not undone: its result is under that turn, and settles the call's line in
    /// the earlier one, as a late result does live.
    #[test]
    fn a_late_result_taken_in_before_the_restart_is_not_undone() {
        let mut nodes = before_the_restart("t1");
        nodes.push(call("t1", 0, "u2", "b.txt"));
        let placeholder = |id: &str| {
            let (p, mut n) = result("t1", 0, id, 7);
            if let Body::ToolResult { status, .. } = &mut n.body {
                *status = ResultStatus::Background;
            }
            (p, n)
        };
        nodes.push(placeholder("u1"));
        nodes.push(placeholder("u2"));
        nodes.push(assistant("t2", 0, "back"));
        nodes.push(result("t2", 0, "u1", 40));
        let mut r = Renderer::default();
        r.rebuild(&nodes);
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t2", "tool_use_id": "u2", "status": "ok", "duration_ms": 12}),
        );
        let shown = upserts(&r.tick());
        let tools = shown
            .iter()
            .find(|(k, _)| k == "t1:L0:tools")
            .map(|(_, c)| c.clone())
            .unwrap();
        assert!(
            !tools.contains(
                "running in the background; the result will come back here\n-# ✅ `fs.read` b.txt"
            ) && !tools.starts_with("-# ⏳ `fs.read` src/lib.rs"),
            "u1 went back to running: {tools}"
        );
        assert!(tools.contains("40 ms"), "{tools}");
    }

    /// The end that asks for the rebuild is already in the store when the
    /// rebuild reads it (the core writes a result, then announces it), and its
    /// event then comes: an L1 job's scratch note must show once, not twice.
    #[test]
    fn the_end_that_starts_the_rebuild_is_applied_once() {
        use theseus_core::fact::Fact;
        let mut nodes = before_the_restart("t1");
        let placeholder = {
            let (p, mut n) = result("t1", 0, "u1", 7);
            if let Body::ToolResult { status, .. } = &mut n.body {
                *status = ResultStatus::Background;
            }
            (p, n)
        };
        nodes.push(placeholder);
        nodes.push(assistant("t2", 0, "back"));
        let late = {
            let (p, mut n) = result("t2", 0, "u1", 40);
            if let Body::ToolResult { meta, .. } = &mut n.body {
                *meta = json!({"detail": {"scratch": {"summary": "wrote 2 files to scratch"}}});
            }
            (p, n)
        };
        let event = theseus_core::fact::tool::ToolEnded {
            session_id: "s",
            turn_id: "t2",
            node: &late.1,
        }
        .event()
        .unwrap();
        nodes.push(late);
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t2"}));
        assert!(r.needs_rebuild(&event));
        r.rebuild(&nodes);
        let mut ops = r.on_event(&event);
        ops.extend(r.tick());
        let shown = upserts(&ops);
        let tools = shown
            .iter()
            .rev()
            .find(|(k, _)| k == "t1:L0:tools")
            .map(|(_, c)| c.clone())
            .unwrap();
        assert_eq!(
            tools.matches("wrote 2 files to scratch").count(),
            1,
            "{tools}"
        );
    }
}
