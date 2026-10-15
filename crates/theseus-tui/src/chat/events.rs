//! What the daemon says, and what the chat makes of it: the turn's events
//! become the dock's live rows while they run and lines of the scrollback
//! when they end, each once. The events are the session's alone (the chat
//! watches one session), whoever sent the turn.

use serde_json::{json, Value};
use theseus_client::render::diffview;
use theseus_protocol::{
    method, ConfirmRequest, Event, ExecutionStopResult, NodeInfo, RpcError, SessionHistoryResult,
    SessionInfo, ToolEnded, TurnFailed, TurnSubmitResult,
};

use super::banner::model_words;
use super::live::{elapsed, took, LiveTool, Phase, TurnLive};
use super::render_seam::tool_line as seam_line;
use super::state::{call, Chat, Effect, Purpose};
use super::style::{span, Line, Sty};

/// Turn ids remembered, so an event of an old turn is not taken for ours.
const KEPT: usize = 64;

impl Chat {
    // ---------------------------------------------------------- answers

    /// An answer to one of the chat's requests.
    pub fn answered(&mut self, purpose: Purpose, result: Result<Value, RpcError>) -> Vec<Effect> {
        let mut effects = self.answered_one(purpose, result);
        effects.extend(self.release_queue());
        effects
    }

    fn answered_one(&mut self, purpose: Purpose, result: Result<Value, RpcError>) -> Vec<Effect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => return self.failed(purpose, &e),
        };
        match purpose {
            Purpose::Health => {
                let model = v.get("model").and_then(Value::as_str).unwrap_or("");
                if !model.is_empty() {
                    self.banner.model = Some(model.to_string());
                    self.model.get_or_insert_with(|| model.to_string());
                }
                self.banner_ready();
                Vec::new()
            }
            Purpose::Open => match serde_json::from_value::<SessionInfo>(v) {
                Ok(info) => {
                    self.execution = info.execution_id.clone();
                    self.opened(&info.session_id)
                }
                Err(e) => self.fatal(&format!(
                    "the daemon opened a session this build cannot read: {e}"
                )),
            },
            Purpose::ListLast => {
                let sessions = serde_json::from_value::<theseus_protocol::SessionListResult>(v)
                    .map(|l| l.sessions)
                    .unwrap_or_default();
                let found = super::banner::last_here(&sessions, &self.cfg.dir).cloned();
                self.banner.last = Some(found);
                self.banner_ready();
                Vec::new()
            }
            Purpose::ListResume(given) => self.resolve(&given, v),
            Purpose::ListContinue => self.continue_here(v),
            Purpose::ListPick => self.listed(false, v),
            Purpose::ListWide => self.listed(true, v),
            Purpose::Earlier(_) => {
                if let Ok(h) = serde_json::from_value::<SessionHistoryResult>(v) {
                    self.earlier_turns(&h);
                }
                Vec::new()
            }
            Purpose::History(sid) => {
                if let Ok(h) = serde_json::from_value::<SessionHistoryResult>(v) {
                    self.history(&sid, &h);
                }
                Vec::new()
            }
            Purpose::Submit => {
                // The turn's end came as an event; the result is the same
                // end, taken once.
                self.released = false;
                if let Ok(r) = serde_json::from_value::<TurnSubmitResult>(v) {
                    self.turn_ended(&r);
                }
                Vec::new()
            }
            Purpose::Stop => {
                if let Ok(r) = serde_json::from_value::<ExecutionStopResult>(v) {
                    self.stopped(&r);
                }
                Vec::new()
            }
            Purpose::Context => {
                self.say_context(v);
                Vec::new()
            }
            Purpose::Cost => {
                self.say_cost(v);
                Vec::new()
            }
            Purpose::Watch | Purpose::Answer | Purpose::Typing => Vec::new(),
        }
    }

    fn failed(&mut self, purpose: Purpose, e: &RpcError) -> Vec<Effect> {
        match purpose {
            Purpose::Typing => Vec::new(),
            Purpose::Health => {
                self.banner_ready();
                Vec::new()
            }
            Purpose::ListLast => {
                self.banner.last = Some(None);
                self.banner_ready();
                Vec::new()
            }
            Purpose::Open | Purpose::ListResume(_) => {
                self.fatal(&format!("could not open a conversation: {}", e.message))
            }
            Purpose::ListContinue => {
                self.say_words(
                    Sty::Yellow,
                    &format!("could not look for a conversation here: {}", e.message),
                );
                self.open_new()
            }
            Purpose::ListPick | Purpose::ListWide => {
                if let Some(p) = self.picker.as_mut() {
                    p.failed = Some(e.message.clone());
                }
                Vec::new()
            }
            Purpose::History(_) | Purpose::Watch | Purpose::Earlier(_) => {
                self.say_words(
                    Sty::Yellow,
                    &format!("could not read the conversation: {}", e.message),
                );
                Vec::new()
            }
            Purpose::Submit => {
                self.live.turn = None;
                self.released = false;
                self.say_words(Sty::Red, &format!("✗ not sent: {}", e.message));
                Vec::new()
            }
            Purpose::Stop => {
                self.say_words(Sty::Red, &format!("✗ could not stop: {}", e.message));
                Vec::new()
            }
            Purpose::Answer => {
                self.answer_refused();
                self.say_words(
                    Sty::Red,
                    &format!("✗ the answer was refused: {}", e.message),
                );
                Vec::new()
            }
            Purpose::Context | Purpose::Cost => {
                self.say_words(Sty::Yellow, &format!("could not read it: {}", e.message));
                Vec::new()
            }
        }
    }

    /// Say what is wrong and leave.
    fn fatal(&mut self, text: &str) -> Vec<Effect> {
        self.print_banner();
        self.say_words(Sty::Red, text);
        vec![Effect::Quit]
    }

    /// The session is open: watch it, and send what was typed before.
    pub(super) fn opened(&mut self, sid: &str) -> Vec<Effect> {
        self.session = Some(sid.to_string());
        let mut out = vec![call(
            method::SESSION_WATCH,
            json!({ "session_id": sid }),
            Purpose::Watch,
        )];
        for text in std::mem::take(&mut self.unsent) {
            out.push(self.submit_call(sid, text));
        }
        out
    }

    /// `theseus chat SID`: find the conversation among the sessions, then
    /// read it.
    fn resolve(&mut self, given: &str, v: Value) -> Vec<Effect> {
        let sessions = serde_json::from_value::<theseus_protocol::SessionListResult>(v)
            .map(|l| l.sessions)
            .unwrap_or_default();
        let g = theseus_client::resolve::trimmed(given);
        let hits: Vec<&SessionInfo> = sessions
            .iter()
            .filter(|s| s.session_id == g || (g.len() >= 4 && s.session_id.ends_with(g)))
            .collect();
        match hits.as_slice() {
            [one] => {
                let info = (*one).clone();
                self.resume_info(info)
            }
            [] if g.chars().count() < 4 => {
                self.fatal(&format!("`{given}` is too short to name a conversation: give at least four characters of its id"))
            }
            [] => self.fatal(&format!("no conversation `{given}` · theseus sessions lists them")),
            many => {
                let ids: Vec<&str> = many.iter().map(|s| s.session_id.as_str()).collect();
                self.fatal(&format!("`{given}` names {} conversations: {}", many.len(), ids.join(", ")))
            }
        }
    }

    /// A resumed conversation's recent turns, then what waits on you: the
    /// last [`REPLAY_TURNS`] turns through the transcript's own lines, the
    /// older ones one line that says how many (theseus-6yka).
    fn history(&mut self, _sid: &str, h: &SessionHistoryResult) {
        self.print_banner();
        let (from, folded) = super::reopen::window(h, super::reopen::REPLAY_TURNS);
        self.say_plain(
            Sty::Dim,
            &format!(
                "resumed · {} turns · {}",
                h.session.turns,
                h.session.title.as_deref().unwrap_or("untitled")
            ),
        );
        self.earlier = None;
        if folded > 0 {
            self.earlier = h.nodes.get(from).map(|n| n.position);
            self.say_plain(
                Sty::Dim,
                &format!(
                    "{folded} earlier turn{} · /history shows {}",
                    if folded == 1 { "" } else { "s" },
                    if folded == 1 { "it" } else { "them" }
                ),
            );
        }
        for n in &h.nodes[from..] {
            self.node_line(n);
        }
        self.gap();
        if self.question.is_none() {
            if let Some(q) = h.pending_confirms.first() {
                self.ask(q.clone());
            }
        }
    }

    /// One stored node, as the transcript shows it.
    pub(super) fn node_line(&mut self, n: &NodeInfo) {
        match n.kind.as_str() {
            "user_message" if !n.text.trim().is_empty() => {
                self.gap();
                let mut first = true;
                for raw in super::reopen::folded(&n.text) {
                    let raw = raw.as_str();
                    for row in
                        super::style::wrap_words(raw, usize::from(self.width).saturating_sub(2), 0)
                    {
                        let lead = if first { "› " } else { "  " };
                        first = false;
                        self.say(&vec![span(Sty::Cyan, lead), span(Sty::Bold, row)]);
                    }
                }
            }
            "assistant_message" if !n.text.trim().is_empty() => self.say_reply(&n.text),
            "tool_result"
                if n.detail
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "ok") =>
            {
                let status = n.detail.get("status").and_then(Value::as_str).unwrap_or("");
                let first = n.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                let row: String = format!("  └ {status} · {}", first.trim())
                    .chars()
                    .take(usize::from(self.width).saturating_sub(2))
                    .collect();
                self.say_plain(Sty::Dim, &row);
            }
            "tool_call" => {
                let tool = n
                    .detail
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let input = n.detail.get("input").cloned().unwrap_or(Value::Null);
                let words = self.tool_words(tool, &input);
                self.say(&vec![span(Sty::Dim, "● "), span(Sty::Plain, words)]);
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------- events

    fn ours(&self, session_id: &str) -> bool {
        self.session.as_deref() == Some(session_id)
    }

    fn turn_is_ours(&self, turn_id: &str) -> bool {
        self.mine.contains(turn_id)
    }

    /// A notification from the daemon.
    pub fn notified(&mut self, method_name: &str, params: &Value) -> Vec<Effect> {
        let mut effects = self.notified_one(method_name, params);
        effects.extend(self.release_queue());
        effects
    }

    fn notified_one(&mut self, method_name: &str, params: &Value) -> Vec<Effect> {
        let event = match Event::from_notification(method_name, params) {
            Ok(Some(e)) => e,
            Ok(None) | Err(_) => return Vec::new(),
        };
        match event {
            Event::TurnStarted(t) if self.ours(&t.session_id) => {
                if self.mine.len() > KEPT {
                    self.mine.clear();
                }
                self.mine.insert(t.turn_id.clone());
                self.released = false;
                if t.execution_id.is_some() {
                    self.execution = t.execution_id.clone();
                }
                self.live.turn = Some(TurnLive {
                    id: t.turn_id,
                    since_ms: self.now_ms,
                    phase: Phase::Thinking,
                });
            }
            Event::LoopStarted(l) if self.turn_is_ours(&l.turn_id) => {
                if !l.model.is_empty() {
                    self.model = Some(l.model);
                }
                self.phase(Phase::Thinking);
            }
            Event::ModelThinking(d) if self.turn_is_ours(&d.turn_id) => self.phase(Phase::Thinking),
            Event::ModelDelta(d) if self.turn_is_ours(&d.turn_id) => self.phase(Phase::Writing),
            Event::ModelAnswered(d) if self.turn_is_ours(&d.turn_id) => {
                if !d.text.trim().is_empty() {
                    self.answered_text.insert(d.turn_id);
                    self.say_reply(&d.text);
                }
            }
            Event::ToolProposed(p) if self.ours(&p.session_id) => {
                self.proposed.insert(p.tool_use_id, (p.tool, p.input));
            }
            Event::ToolStarted(t) if self.ours(&t.session_id) => self.tool_started(&t),
            Event::ToolEnded(t) if self.ours(&t.session_id) => self.tool_ended(&t),
            Event::ConfirmRequested(c) if self.ours(&c.session_id) => self.ask(c),
            Event::ConfirmResolved(r) => {
                if self
                    .question
                    .as_ref()
                    .is_some_and(|q| q.correlation_id == r.correlation_id)
                {
                    self.question = None;
                    self.ask_reset();
                    let words = match r.option.as_deref() {
                        Some(o) if theseus_protocol::answer::approves(o) => "approved",
                        Some(_) => "declined",
                        None if r.expired => "expired",
                        None => "closed",
                    };
                    self.say_plain(Sty::Dim, &format!("  {words}"));
                    if let Some(t) = self.live.turn.as_mut() {
                        t.phase = Phase::Thinking;
                    }
                }
            }
            Event::TaskChanged(t) if self.ours(&t.session_id) => {
                use theseus_protocol::tasks::TaskState::{Abandoned, Done, Failed, Proposed};
                let open = !matches!(t.task.state, Done | Failed | Abandoned | Proposed);
                self.task_open.insert(t.task.id, open);
                self.tasks = self.task_open.values().filter(|o| **o).count() as u32;
            }
            Event::TurnFailed(f) if self.ours(&f.session_id) => self.turn_failed(&f),
            Event::TurnEnded(r) if self.ours(&r.session_id) => self.turn_ended(&r),
            Event::EventsLost(_) => self.say_plain(
                Sty::Yellow,
                "(some updates were missed; the daemon has the whole record)",
            ),
            _ => {}
        }
        Vec::new()
    }

    fn phase(&mut self, phase: Phase) {
        if let Some(t) = self.live.turn.as_mut() {
            if t.phase != Phase::Waiting {
                t.phase = phase;
            }
        }
    }

    fn tool_started(&mut self, t: &theseus_protocol::ToolStarted) {
        let (tool, input) = self
            .proposed
            .get(&t.tool_use_id)
            .cloned()
            .unwrap_or_else(|| {
                let mut input = Value::Null;
                if let Some(argv) = &t.argv {
                    input = json!({ "argv": argv });
                }
                (t.tool.clone(), input)
            });
        self.live.tools.retain(|x| x.id != t.tool_use_id);
        self.live.tools.push(LiveTool {
            id: t.tool_use_id.clone(),
            tool,
            input,
            since_ms: self.now_ms,
            last: None,
            background: false,
        });
    }

    fn tool_ended(&mut self, t: &ToolEnded) {
        let running = self.live.tools.iter().position(|x| x.id == t.tool_use_id);
        let (tool, input, since) = match running.map(|i| self.live.tools[i].clone()) {
            Some(x) => (x.tool, x.input, Some(x.since_ms)),
            None => {
                let (tool, input) = self
                    .proposed
                    .get(&t.tool_use_id)
                    .cloned()
                    .unwrap_or_else(|| (t.tool.clone(), Value::Null));
                (tool, input, None)
            }
        };
        let background = t.status == "background";
        if background {
            if let Some(i) = running {
                self.live.tools[i].background = true;
            }
        } else if let Some(i) = running {
            self.live.tools.remove(i);
        }
        self.proposed.remove(&t.tool_use_id);
        let ms = t
            .duration_ms
            .or_else(|| since.map(|s| self.now_ms.saturating_sub(s)))
            .unwrap_or(0);
        let (mark, sty, words) = outcome(t, ms);
        let mut left = self.tool_words(&tool, &input);
        if let Some(d) = &t.diff {
            left.push_str(&format!("  +{} −{}", d.added, d.removed));
        }
        let line = self.tool_row(&left, mark, sty, &words);
        self.say(&line);
        self.result_rows(t);
    }

    /// What the call left, under its line: an edit's diff, or the first lines
    /// of a command's result behind `⎿` (a failure's last lines).
    fn result_rows(&mut self, t: &ToolEnded) {
        if matches!(
            t.status.as_str(),
            "background" | "declined" | "denied" | "cancelled"
        ) {
            return;
        }
        let mut rows = diffview::ended_lines(t);
        if t.diff.is_some() {
            // The header said the file and the counts.
            rows.remove(0);
        }
        for l in super::diff::lines(&rows, 4, usize::from(self.width)) {
            self.say(&l);
        }
    }

    /// `● <words>` on the left, its outcome on the right, the left cut to
    /// fit.
    fn tool_row(&self, left: &str, mark: &str, sty: Sty, right: &str) -> Line {
        let w = usize::from(self.width);
        let tail = format!("{mark} {right}");
        let room = w.saturating_sub(tail.chars().count() + 4);
        let mut shown: String = left.chars().take(room).collect();
        if left.chars().count() > room {
            shown.pop();
            shown.push('…');
        }
        let used = 2 + shown.chars().count() + tail.chars().count();
        vec![
            span(Sty::Dim, "● "),
            span(Sty::Plain, shown),
            span(Sty::Plain, " ".repeat(w.saturating_sub(used).max(1))),
            span(sty, tail),
        ]
    }

    /// A question for you: the dock shows it, and the terminal rings.
    pub(super) fn ask(&mut self, c: ConfirmRequest) {
        if let Some(t) = self.live.turn.as_mut() {
            t.phase = Phase::Waiting;
        }
        self.ask_reset();
        self.question = Some(c);
    }

    fn turn_failed(&mut self, f: &TurnFailed) {
        self.live.turn = None;
        let class = f
            .class
            .as_deref()
            .map(|c| format!(" ({c})"))
            .unwrap_or_default();
        let then = match f.then.as_deref() {
            Some("backoff") => " · it tries again",
            Some("retry") => " · it tries once more",
            Some("park") => " · your next message tries again",
            _ => "",
        };
        self.say_words(
            Sty::Red,
            &format!("✗ the turn failed{class}: {}{then}", f.error),
        );
    }

    /// A call in words, its path shown from the directory the chat is in.
    pub(super) fn tool_words(&self, tool: &str, input: &Value) -> String {
        let mut input = input.clone();
        let prefix = format!("{}/", self.cfg.dir.trim_end_matches('/'));
        for key in ["path", "file"] {
            let rel = input
                .get(key)
                .and_then(Value::as_str)
                .and_then(|p| p.strip_prefix(&prefix))
                .map(String::from);
            if let Some(rel) = rel {
                input[key] = json!(rel);
            }
        }
        seam_line(tool, &input)
    }

    /// A turn ended. Once, however it is told.
    pub(super) fn turn_ended(&mut self, r: &TurnSubmitResult) {
        if !self.ended.insert(r.turn_id.clone()) {
            return;
        }
        if self.ended.len() > KEPT {
            self.ended.clear();
            self.ended.insert(r.turn_id.clone());
        }
        if self.live.turn.as_ref().is_some_and(|t| t.id == r.turn_id) {
            self.live.turn = None;
        }
        // The loops' answers were told as they ended; a turn told nothing
        // shows its output now.
        if !self.answered_text.remove(&r.turn_id) && !r.output.trim().is_empty() {
            self.say_reply(&r.output);
        }
        self.note_usage(r);
        if let Some(c) = r
            .session_cost_usd
            .or(r.cost_usd.filter(|_| self.cost == 0.0))
        {
            self.cost = c;
        }
        if !r.model.is_empty() {
            self.model = Some(r.model.clone());
        }
        if !r.profile.is_empty() {
            self.profile = Some(r.profile.clone());
        }
        let mut parts = vec![took(r.elapsed_ms), model_words(&r.model)];
        if let Some(c) = r.cost_words() {
            parts.push(c);
        }
        if !matches!(
            r.stop_reason.as_str(),
            "" | "done" | "end_turn" | "no_tool_calls"
        ) {
            parts.push(r.stop_reason.clone());
        }
        self.say_plain(Sty::Dim, &format!("  {}", parts.join(" · ")));
        // The turn in the cockpit (theseus-x9swu); `/inspect` prints it again.
        if let Some(u) = &r.inspect_url {
            self.say_plain(Sty::Dim, &format!("  inspect {u}"));
            self.last_inspect = Some(u.clone());
        }
        for l in r.harness_lines() {
            self.say_plain(Sty::Dim, &format!("  {l}"));
        }
        self.gap();
    }

    // ---------------------------------------------------------- the dock's words

    /// The live line: `◐ thinking · 0:03`.
    pub fn live_line(&self) -> Option<Line> {
        let t = self.live.turn.as_ref()?;
        Some(vec![
            span(Sty::Cyan, format!("{} ", super::live::spinner(self.now_ms))),
            span(Sty::Plain, t.phase.word()),
            span(
                Sty::Dim,
                format!(" · {}", elapsed(self.now_ms.saturating_sub(t.since_ms))),
            ),
        ])
    }
}

/// A call's outcome, for the right of its line: a mark, its colour, its words.
fn outcome(t: &ToolEnded, ms: u64) -> (&'static str, Sty, String) {
    let exit = t
        .exit_code
        .filter(|c| *c != 0)
        .map(|c| format!("exit {c} · "))
        .unwrap_or_default();
    match t.status.as_str() {
        "ok" if t.late => ("✓", Sty::Green, format!("finished · {}", took(ms))),
        "ok" => ("✓", Sty::Green, took(ms)),
        "background" => ("◐", Sty::Cyan, "running in the background".to_string()),
        "declined" | "denied" => ("✗", Sty::Yellow, "declined".to_string()),
        "cancelled" => {
            let said = theseus_client::render::result_word("cancelled", t.stopped_by.as_deref());
            let verified = t
                .verified
                .as_deref()
                .map(|v| format!(" · {v}"))
                .unwrap_or_default();
            ("✗", Sty::Yellow, format!("{said}{verified}"))
        }
        other => ("✗", Sty::Red, format!("{exit}{other} · {}", took(ms))),
    }
}
