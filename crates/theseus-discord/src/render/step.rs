//! A turn's live messages, and the bound on how many it may have
//! (theseus-8wm). A builder's step is one turn of many tool loops, and one
//! message a loop made a channel of 49 messages and 56 edits for a single
//! step in the dogfood pilot: a log nobody can skim, with the two messages
//! that wanted an answer buried in it.
//!
//! So a turn's first `STEP_LOOPS - 1` loops keep a message each, exactly as
//! they always did, and every loop from there on shares one more, the step's.
//! The step's message carries those loops' tool lines in the order they were
//! called, and `tool_lines` folds whatever will not fit into its "… N earlier
//! call(s)" line, as it already does for a single loop that calls more than a
//! message holds. It goes out when it is due -- a loop's end, a call waiting
//! for or getting an answer, the turn's end -- rather than at every state a
//! call passes through, so the step costs about an edit a loop. Its first
//! post never waits, and the turn's end always writes its last state.
//!
//! `render::process` shares the bound through [`step_loop`], so a loop's
//! thinking goes to the same message as its tool lines.

use std::collections::BTreeMap;

use super::{notified_tools, split_text, tool_lines, Asked, ToolLine, TurnView, PART_LIMIT};

/// The live progress messages one turn shows: a turn of any length costs its
/// place this many.
pub(crate) const STEP_LOOPS: u32 = 6;

/// The loop whose message a turn's later loops roll into.
pub(crate) const STEP_ROLL: u32 = STEP_LOOPS - 1;

/// The message a turn's loop `li` writes: its own up to [`STEP_ROLL`], and
/// the step's from there on.
pub(crate) fn step_loop(li: u32) -> u32 {
    li.min(STEP_ROLL)
}

/// One message of a turn as rendered: its key, its text, and, on a tool
/// message when menus are on, its "should have asked" choices.
pub(super) struct Rendered {
    pub(super) key: String,
    pub(super) content: String,
    pub(super) menu: Option<Vec<Asked>>,
    /// It is the step's message, which a turn's later loops share
    /// (theseus-8wm): it goes out when it is due, not at every change.
    pub(super) rolled: bool,
}

impl Rendered {
    pub(super) fn text(key: String, content: String) -> Self {
        Self {
            key,
            content,
            menu: None,
            rolled: false,
        }
    }
}

/// Every live message a turn shows, in the order Discord should first see
/// them: its streamed text while it runs (then its reply's post owns the
/// text), and its tool messages. `tightened` is tool → who tightened it;
/// `menus` puts a "Should I have asked?" select on each tool message that lists
/// a notified call.
pub(super) fn render_turn(
    t: &TurnView,
    tightened: &BTreeMap<String, String>,
    menus: bool,
) -> Vec<Rendered> {
    let mut out: Vec<Rendered> = Vec::new();
    // A turn's loops past `STEP_ROLL` share one message, the step's: their
    // tool lines go on it in the order they were called, and `tool_lines`
    // folds whatever will not fit into its "… N earlier call(s)" line, as it
    // does for a single loop that calls more than a message holds.
    let mut groups: BTreeMap<u32, Vec<&ToolLine>> = BTreeMap::new();
    for (li, lv) in &t.loops {
        if !t.ended {
            for (i, part) in split_text(&lv.text, PART_LIMIT).into_iter().enumerate() {
                out.push(Rendered::text(format!("{}:L{li}:p{i}", t.turn_id), part));
            }
        }
        if !lv.tools.is_empty() {
            groups.entry(step_loop(*li)).or_default().extend(&lv.tools);
        }
    }
    let rolls = t.rolls();
    for (li, tools) in groups {
        let (asked, tight) = notified_tools(&tools, tightened);
        // A line per notified tool that asks first now, under the calls.
        let said: Vec<String> = tight
            .iter()
            .map(|(tool, by)| format!("-# 🔒 `{tool}` asks first from now on: tightened by {by}"))
            .collect();
        let reserve: usize = said.iter().map(|l| l.len() + 1).sum();
        let mut content = tool_lines(&tools, reserve);
        for l in &said {
            content.push('\n');
            content.push_str(l);
        }
        out.push(Rendered {
            key: format!("{}:L{li}:tools", t.turn_id),
            content,
            menu: menus.then_some(asked),
            rolled: rolls && li == STEP_ROLL,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::tests::{ended, upserts};
    use super::super::{Op, Renderer};
    use super::STEP_LOOPS;

    /// A loop of the same step that starts a background job instead: the
    /// call is notified, runs, and is handed to the background. Its end
    /// comes back much later, in a wake turn of its own.
    fn background_loop(r: &mut Renderer, li: u32, use_id: &str) {
        r.on_notification(
            "model.answered",
            &json!({"turn_id": "t1", "loop_index": li, "text": ""}),
        );
        r.on_notification(
            "tool.proposed",
            &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "proc.run",
                    "input": {"argv": ["sleep", "75"]}, "gate": {"result": {"gate": "allow"},
                    "decision": {"mode": "allow", "posture": "notify",
                                 "notify": {"kind": "notify", "setting": "enforcement = notify",
                                            "rule": "proc.run — notify (enforcement = notify)"}}}}),
        );
        r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "proc.run"}),
        );
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "proc.run",
                    "status": "background", "duration_ms": 2000}),
        );
        r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": li}));
        r.tick();
    }

    /// An ordinary turn that calls nothing, as theseus-9zif's tests use.
    fn plain(r: &mut Renderer, turn: &str) {
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": turn}));
        r.on_notification("turn.ended", &json!({"session_id": "s", "turn_id": turn}));
        r.tick();
    }

    /// One tool-only loop of a builder's step: the loop opens with no prose,
    /// proposes a notified call, runs it, and ends. Every op it makes is
    /// counted into `ops`, and every message key it writes into `keys`.
    fn tool_loop(r: &mut Renderer, li: u32, ops: &mut usize, keys: &mut Vec<String>) {
        let use_id = format!("use_{li}");
        let mut note = |got: Vec<Op>| {
            for o in got {
                if let Some(k) = o.key() {
                    if !keys.iter().any(|seen| seen == k) {
                        keys.push(k.to_string());
                    }
                    *ops += 1;
                }
            }
        };
        // A loop with no text of its own, as a builder's tool loops are.
        note(r.on_notification(
            "model.answered",
            &json!({"turn_id": "t1", "loop_index": li, "text": ""}),
        ));
        note(r.on_notification(
            "tool.proposed",
            &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "fs.edit",
                    "input": {"path": format!("src/f{li}.rs")}, "gate": {"result": {"gate": "allow"},
                    "decision": {"mode": "allow", "posture": "notify",
                                 "notify": {"kind": "notify", "setting": "enforcement = notify",
                                            "rule": "fs.edit — notify (enforcement = notify)"}}}}),
        ));
        note(r.tick());
        note(r.on_notification(
            "tool.started",
            &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "fs.edit"}),
        ));
        note(r.tick());
        note(r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "fs.edit",
                    "status": "ok", "duration_ms": 12}),
        ));
        note(r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": li})));
        note(r.tick());
    }

    /// theseus-8wm: a builder's step is one turn of many tool loops, and the
    /// dogfood pilot measured one of them at 49 messages and 56 edits in its
    /// channel. Its live progress is bounded now, however many loops it runs:
    /// the first `STEP_LOOPS - 1` loops keep a message each, every loop from
    /// there on rolls into the one step message, and that message is written
    /// at a loop's end rather than at each call's every state change. The
    /// step's last call is always on it when the turn ends.
    #[test]
    fn a_step_of_fifty_tool_loops_stays_within_its_message_budget() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let (mut ops, mut keys) = (0usize, Vec::new());
        for li in 0..50 {
            tool_loop(&mut r, li, &mut ops, &mut keys);
        }
        for (key, _) in upserts(&r.on_notification("turn.ended", &ended("t1", None))) {
            if !keys.contains(&key) {
                keys.push(key);
            }
            ops += 1;
        }
        assert!(ops > 50, "the step really ran: {ops} ops");
        assert!(
            keys.len() <= STEP_LOOPS as usize,
            "a step posts at most {STEP_LOOPS} live messages, not {} ({ops} ops): {keys:?}",
            keys.len()
        );
        assert!(
            ops <= 2 * 50 + STEP_LOOPS as usize,
            "at most an op a loop and the step's own: {ops}"
        );
        // The step's own message carries its last call when the turn ends.
        let step = format!("t1:L{}:tools", STEP_LOOPS - 1);
        let shown = r.emitted.get(&step).cloned().unwrap_or_default();
        assert!(
            shown.contains("src/f49.rs"),
            "the step's last call is on it: {shown:?}"
        );
        // A result that lands after the turn parked has no loop end coming
        // to carry it, so the next tick writes it to the step's message
        // rather than holding it until something else falls due.
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "use_40", "tool": "fs.edit",
                    "status": "failed", "duration_ms": 900_000}),
        );
        let late = upserts(&r.tick());
        assert!(
            late.iter()
                .any(|(k, c)| *k == step && c.contains("src/f40.rs") && c.contains("failed")),
            "the late result is on the step's message: {late:?}"
        );
    }

    /// The two fixes meet here. theseus-9zif keeps a turn alive while one of
    /// its calls is still in the background, so a job's end finds its line
    /// however many turns came between; theseus-8wm rolls a step's later
    /// loops onto one message, where `tool_lines` folds away whatever will
    /// not fit, newest kept. A job started deep in a long step is therefore
    /// pinned (the turn survives) and folded (its line does not show), and
    /// its end lands nowhere: neither row's own test sees it, because
    /// 9zif's job is in loop 0, which keeps a message of its own.
    #[test]
    fn a_background_job_deep_in_a_step_still_shows_its_end() {
        let mut r = Renderer::new(false);
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let (mut ops, mut keys) = (0usize, Vec::new());
        for li in 0..50 {
            if li == 10 {
                background_loop(&mut r, li, "use_job");
            } else {
                tool_loop(&mut r, li, &mut ops, &mut keys);
            }
        }
        r.on_notification("turn.ended", &ended("t1", None));
        r.tick();
        let step = format!("t1:L{}:tools", STEP_LOOPS - 1);
        // The job's line is on the step's message, not one of its own: this
        // is what makes the fold reach it.
        assert!(
            r.emitted.contains_key(&step),
            "the step's message exists: {:?}",
            r.emitted.keys().collect::<Vec<_>>()
        );
        // More turns go by than the renderer holds.
        for n in 0..8 {
            plain(&mut r, &format!("turn_{n}"));
        }
        assert!(
            r.knows("t1"),
            "theseus-9zif holds the job's turn: {:?}",
            r.held()
        );
        // The job's end, in a wake turn of its own.
        r.on_notification(
            "turn.started",
            &json!({"session_id": "s", "turn_id": "wake"}),
        );
        let mut late = r.on_notification(
            "tool.ended",
            &json!({"turn_id": "wake", "tool_use_id": "use_job", "tool": "proc.run",
                    "status": "ok", "duration_ms": 75001}),
        );
        late.extend(r.tick());
        let shown: Vec<String> = upserts(&late)
            .into_iter()
            .filter(|(k, _)| *k == step)
            .map(|(_, c)| c)
            .collect();
        assert_eq!(shown.len(), 1, "one edit of the step's message: {late:?}");
        assert!(
            shown[0].contains("✅ `proc.run`") && shown[0].contains("75001 ms"),
            "the job's end shows on the step's message: {}",
            shown[0]
        );
        assert!(
            !shown[0].contains("running in the background"),
            "{}",
            shown[0]
        );
    }
}
