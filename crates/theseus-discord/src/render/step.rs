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

use super::{split_text, Asked, ToolLine, ToolState, TurnView, MAX_ASKED, PART_LIMIT};

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

/// A tool message's distinct notified tools, in the order they first ran:
/// those still offered to "should have asked", each with its newest
/// notified call (at most `MAX_ASKED`), and those that ask first now, with
/// who tightened them.
fn notified_tools(
    tools: &[&ToolLine],
    tightened: &BTreeMap<String, String>,
) -> (Vec<Asked>, Vec<(String, String)>) {
    let mut asked: Vec<Asked> = Vec::new();
    let mut tight: Vec<(String, String)> = Vec::new();
    for l in tools.iter().filter(|l| l.notice.is_some()) {
        if let Some(by) = tightened.get(&l.tool) {
            if !tight.iter().any(|(t, _)| *t == l.tool) {
                tight.push((l.tool.clone(), by.clone()));
            }
            continue;
        }
        if let Some(a) = asked.iter_mut().find(|a| a.tool == l.tool) {
            if l.correlation_id.is_some() {
                a.correlation_id = l.correlation_id.clone();
            }
        } else if asked.len() < MAX_ASKED {
            asked.push(Asked {
                tool: l.tool.clone(),
                correlation_id: l.correlation_id.clone(),
            });
        }
    }
    (asked, tight)
}

/// The tool message's lines, in at most `PART_LIMIT - 60 - reserve`
/// characters.
fn tool_lines(tools: &[&ToolLine], reserve: usize) -> String {
    let lines: Vec<String> = tools
        .iter()
        .map(|l| {
            let risk = l.risk.as_ref().map_or(String::new(), |r| format!(" · {r}"));
            let mark = match l.notice.as_deref() {
                Some("") => format!(" · 🔔 notified{risk}"),
                Some(setting) => format!(" · 🔔 notified ({setting}){risk}"),
                None => String::new(),
            };
            let key = l
                .granted
                .as_deref()
                .map_or_else(String::new, |g| format!(" · 🔑 {g}"));
            let l1 =
                l.l1.as_ref()
                    .map_or(String::new(), |r| format!("🛡️ L1 · {r} · "));
            let head = format!("`{}` {l1}{}{key}{mark}", l.tool, l.summary);
            let line = match &l.state {
                ToolState::Proposed => format!("▫️ {head}"),
                ToolState::Running => format!("⏳ {head}"),
                ToolState::Waiting => format!("⏸️ {head} · waiting for approval"),
                ToolState::Background => {
                    format!("⏳ {head} · running in the background; the result will come back here")
                }
                ToolState::Done { status, ms } if status == "ok" => format!("✅ {head} · {ms} ms"),
                ToolState::Done { status, ms } => format!("❌ {head} · {status} · {ms} ms"),
                ToolState::NotRun => format!("🚫 {head} · not run"),
                ToolState::Stopped { by } => format!("⏹️ {head} · stopped by {by}"),
                ToolState::Answered { approved: true, by } => {
                    format!("👍 {head} · approved by {by}")
                }
                ToolState::Answered {
                    approved: false,
                    by,
                } => {
                    format!("👎 {head} · declined by {by}")
                }
            };
            // The owner, 2026-11-13 12:05 (S24-1): "I take it back: I do
            // want all tools to be grey and small so I can easily
            // distinguish them from the output text". Discord's small grey
            // subtext, the same the footer uses, on every state a call
            // passes through -- one prefix here rather than ten, so a state
            // added later cannot be the big bright one.
            format!("-# {line}")
        })
        .collect();
    // Keep the newest lines when a loop made more calls than one message holds,
    // with room for the line that counts the rest.
    let mut keep = vec![false; lines.len()];
    let mut len = 0;
    let budget = (PART_LIMIT - 60).saturating_sub(reserve);
    // A call whose end comes back in a later turn (a background job, a call
    // waiting for an answer) keeps its line first, however many calls the
    // step made after it: theseus-9zif holds its turn alive so that its end
    // has a line to edit, and since theseus-8wm a step's later loops share
    // one message, where the newest-first fold below would otherwise hide
    // exactly that line.
    for (i, (l, t)) in lines.iter().zip(tools).enumerate() {
        if t.late && len + l.len() < budget {
            len += l.len() + 1;
            keep[i] = true;
        }
    }
    // Then the newest that still fit, with room for the line counting the rest.
    for (i, l) in lines.iter().enumerate().rev() {
        if keep[i] {
            continue;
        }
        if len + l.len() + 1 > budget {
            break;
        }
        len += l.len() + 1;
        keep[i] = true;
    }
    let hidden = keep.iter().filter(|k| !**k).count();
    let mut s = String::new();
    if hidden > 0 {
        s.push_str(&format!("-# … {hidden} earlier call(s)"));
        // This line may be the only place Discord shows their notices.
        let notified = tools
            .iter()
            .zip(&keep)
            .filter(|(l, k)| !**k && l.notice.is_some())
            .count();
        if notified > 0 {
            s.push_str(&format!(" · 🔔 {notified} notified"));
        }
        s.push('\n');
    }
    s.push_str(
        &lines
            .iter()
            .zip(&keep)
            .filter(|(_, k)| **k)
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    s
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

    /// The owner, in his own words (Discord DM 2026-11-13 12:05:24 MST,
    /// S24-1): "I take it back: I do want all tools to be grey and small so
    /// I can easily distinguish them from the output text". So every line a
    /// tool message shows is Discord's small grey subtext: each call's line
    /// in every state it passes through, and the "… N earlier call(s)" line
    /// that stands in for the calls folded away, which is tool activity too.
    /// What he is telling them apart from -- the reply's own text -- is not
    /// rendered here and keeps its full size.
    #[test]
    fn every_tool_activity_line_is_small_grey_subtext() {
        // Every content a tool message shows, including the states a call
        // passes through before its end.
        let mut shown: Vec<(String, String)> = Vec::new();
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let (mut ops, mut keys) = (0usize, Vec::new());
        // Fifty calls, so the step rolls and the fold line is written.
        for li in 0..50 {
            tool_loop(&mut r, li, &mut ops, &mut keys);
        }
        shown.extend(upserts(
            &r.on_notification("turn.ended", &ended("t1", None)),
        ));
        shown.extend(r.emitted.clone());
        // A call waiting for an answer, and one stopped: states the loops
        // above never reach.
        let mut w = Renderer::default();
        w.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t2"}));
        w.on_notification(
            "tool.proposed",
            &json!({"turn_id": "t2", "tool_use_id": "u1", "tool": "fs.write",
                    "input": {"path": "a.txt"}, "gate": {"result": {"gate": "ask"},
                    "decision": {"mode": "ask", "posture": "ask"}}}),
        );
        shown.extend(upserts(&w.tick()));
        w.on_notification(
            "tool.started",
            &json!({"turn_id": "t2", "tool_use_id": "u1", "tool": "fs.write"}),
        );
        shown.extend(upserts(&w.tick()));
        shown.extend(w.emitted.clone());

        let tools: Vec<&(String, String)> = shown
            .iter()
            .filter(|(k, _)| k.ends_with(":tools"))
            .collect();
        assert!(
            !tools.is_empty(),
            "the steps wrote tool messages: {shown:?}"
        );
        for (key, content) in &tools {
            for line in content.lines().filter(|l| !l.trim().is_empty()) {
                assert!(
                    line.starts_with("-# "),
                    "S24-1: every tool activity line is small grey subtext, \
                     but {key} shows {line:?}"
                );
            }
        }
        // The fold line was really among them, so the assertion above judged
        // it rather than passing for want of one.
        assert!(
            tools.iter().any(|(_, c)| c.contains("earlier call(s)")),
            "the fold line is among the lines judged: {tools:?}"
        );
    }
}
