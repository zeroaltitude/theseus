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
                serde_json::from_value(v).map_or_else(|_| Vec::new(), |r| self.submit_ended(&r))
            }
            Purpose::Stop => {
                if let Ok(r) = serde_json::from_value::<ExecutionStopResult>(v) {
                    self.stopped(&r);
                }
                Vec::new()
            }
            Purpose::WorkWatch => {
                match serde_json::from_value::<theseus_protocol::WorkWatchResult>(v) {
                    Ok(r) => self.work_snapshot(r),
                    Err(_) => Vec::new(),
                }
            }
            Purpose::Context => {
                self.say_context(v);
                Vec::new()
            }
            Purpose::Cost => {
                self.say_cost(v);
                Vec::new()
            }
            Purpose::Mode => {
                if let Ok(r) =
                    serde_json::from_value::<theseus_protocol::modes::SessionModeResult>(v)
                {
                    self.mode = r.mode;
                    self.plan = r.plan;
                }
                Vec::new()
            }
            Purpose::Welcome => self.welcome_said(v),
            Purpose::Watch | Purpose::Answer | Purpose::Route | Purpose::Typing => Vec::new(),
            p @ (Purpose::ListTask { .. }
            | Purpose::TaskAnswer { .. }
            | Purpose::Cancel(_)
            | Purpose::TaskSubmit { .. }
            | Purpose::TaskInspect
            | Purpose::FollowUp
            | Purpose::TaskStart { .. }
            | Purpose::Hold { .. }) => self.answered_view(p, v),
            Purpose::PickerTasks => self.picker_tasks(v),
            Purpose::RewindPoints => self.rewind_points(v),
            Purpose::RewindRestore(going) => self.rewind_restored(going, v),
            Purpose::RewindFork(going) => self.rewind_forked(going, v),
        }
    }

    fn failed(&mut self, purpose: Purpose, e: &RpcError) -> Vec<Effect> {
        match purpose {
            // An older daemon has no digest, and the chat says nothing of it.
            Purpose::Typing | Purpose::Welcome => Vec::new(),
            Purpose::Route => {
                self.thread_answer_refused(&e.message);
                Vec::new()
            }
            Purpose::Health => {
                self.banner_ready();
                Vec::new()
            }
            Purpose::ListLast => {
                self.banner.last = Some(None);
                self.banner_ready();
                Vec::new()
            }
            Purpose::Open | Purpose::ListResume(_) | Purpose::ListTask { .. } => {
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
                self.thread_ask_over();
                self.say_words(Sty::Red, &format!("✗ not sent: {}", e.message));
                Vec::new()
            }
            Purpose::Stop => {
                self.say_words(Sty::Red, &format!("✗ could not stop: {}", e.message));
                Vec::new()
            }
            Purpose::TaskSubmit { text } => self.task_submit_refused(&text, e),
            Purpose::TaskInspect | Purpose::FollowUp => {
                self.say_words(Sty::Red, &format!("✗ refused: {}", e.message));
                Vec::new()
            }
            Purpose::TaskStart { .. } | Purpose::Hold { .. } => {
                self.held_refused(e);
                Vec::new()
            }
            Purpose::PickerTasks => self.picker_tasks_failed(),
            Purpose::Answer | Purpose::TaskAnswer { .. } => {
                self.answer_refused();
                self.say_words(
                    Sty::Red,
                    &format!("✗ the answer was refused: {}", e.message),
                );
                Vec::new()
            }
            // A daemon that predates the board has no footer to draw.
            Purpose::WorkWatch => Vec::new(),
            Purpose::Cancel(_) => {
                self.say_words(Sty::Red, &format!("✗ refused: {}", e.message));
                Vec::new()
            }
            Purpose::Context | Purpose::Cost => {
                self.say_words(Sty::Yellow, &format!("could not read it: {}", e.message));
                Vec::new()
            }
            Purpose::RewindPoints => {
                self.rewind_failed("could not list your messages", &e.message);
                Vec::new()
            }
            Purpose::RewindRestore(_) => {
                self.rewind_failed("the files were not restored", &e.message);
                Vec::new()
            }
            Purpose::RewindFork(_) => {
                self.rewind_failed("the conversation was not rewound", &e.message);
                Vec::new()
            }
            // A mode that did not change says why where the keys are
            // (theseus-lyxy); an older daemon has no modes, and says nothing.
            Purpose::Mode => {
                if e.code != theseus_protocol::error_code::METHOD_NOT_FOUND {
                    self.flash(e.message.clone());
                }
                Vec::new()
            }
        }
    }

    /// Say what is wrong and leave.
    pub(super) fn fatal(&mut self, text: &str) -> Vec<Effect> {
        self.print_banner();
        self.say_words(Sty::Red, text);
        vec![Effect::Quit]
    }

    /// The session is open: watch it, and send what was typed before.
    pub(super) fn opened(&mut self, sid: &str) -> Vec<Effect> {
        self.session = Some(sid.to_string());
        let mut out = vec![
            call(
                method::SESSION_WATCH,
                json!({ "session_id": sid }),
                Purpose::Watch,
            ),
            Self::watch_work(sid),
            // Its mode and plan, which its notifications then keep (theseus-lyxy).
            call(
                method::SESSION_MODE,
                json!({ "session_id": sid }),
                Purpose::Mode,
            ),
        ];
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
    fn history(&mut self, sid: &str, h: &SessionHistoryResult) {
        self.print_banner();
        let (from, folded) = super::reopen::window(h, super::reopen::REPLAY_TURNS);
        self.note_last_turn(sid, h.nodes.iter().rev().find_map(|n| n.turn_id.clone()));
        let title = h.session.title.as_deref().unwrap_or("");
        let head = self
            .task_head(sid, h.session.turns, title)
            .unwrap_or_else(|| {
                format!(
                    "resumed · {} turns · {}",
                    h.session.turns,
                    if title.is_empty() { "untitled" } else { title }
                )
            });
        self.say_plain(Sty::Dim, &head);
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
        if let Some(from) = self.rewound.take() {
            self.say_words(
                Sty::Cyan,
                &format!("↩ rewound · the original is kept: theseus chat {from}"),
            );
        }
        if self.question.is_none() {
            if let Some(q) = h.pending_confirms.first() {
                self.ask(q.clone());
            }
        }
        self.say_back();
    }

    /// One stored node, as the transcript shows it.
    pub(super) fn node_line(&mut self, n: &NodeInfo) {
        match n.kind.as_str() {
            "user_message" if !n.text.trim().is_empty() => {
                if self.view.is_none() {
                    self.last_said = Some(n.text.clone());
                }
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

    pub(super) fn ours(&self, session_id: &str) -> bool {
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
        let mut effects = Vec::new();
        self.note_away(&event);
        match event {
            Event::TurnStarted(t) if self.ours_in(&t.session_id, Some(&t.turn_id)) => {
                if self.mine.len() > KEPT {
                    self.mine.clear();
                }
                self.mine.insert(t.turn_id.clone());
                self.released = false;
                self.thread_ask_over();
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
            Event::ToolProposed(p) if self.ours_in(&p.session_id, Some(&p.turn_id)) => {
                self.proposed.insert(p.tool_use_id, (p.tool, p.input));
            }
            Event::ToolStarted(t) if self.ours_in(&t.session_id, Some(&t.turn_id)) => {
                self.tool_started(&t);
            }
            Event::ToolEnded(t) if self.ours_in(&t.session_id, Some(&t.turn_id)) => {
                self.tool_ended(&t);
            }
            Event::ConfirmRequested(c) if self.ours_in(&c.session_id, c.turn_id.as_deref()) => {
                self.ask(c)
            }
            e @ (Event::ThreadAsked(_) | Event::ThreadPlaced(_)) => {
                effects.extend(self.thread_event(e));
            }
            // The mode or the plan moved (theseus-lyxy): the status line
            // follows, and the checklist goes into the transcript.
            Event::SessionMode(m) if self.ours(&m.session_id) => self.moded(&m),
            Event::ConfirmResolved(r) => {
                if self
                    .question
                    .as_ref()
                    .is_some_and(|q| q.correlation_id == r.correlation_id)
                {
                    self.question = None;
                    self.ask_sync();
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
            Event::TaskChanged(t) if self.ours_in(&t.session_id, t.turn_id.as_deref()) => {
                self.task_changed(&t);
            }
            Event::WorkChanged(c) => self.work_changed(c),
            Event::WorkNotice(n) => effects = self.work_notice(&n),
            Event::TurnFailed(f) if self.ours_in(&f.session_id, f.turn_id.as_deref()) => {
                self.turn_failed(&f);
            }
            Event::TurnEnded(r) if self.ours_in(&r.session_id, Some(&r.turn_id)) => {
                self.turn_ended(&r);
            }
            Event::EventsLost(_) => self.say_plain(
                Sty::Yellow,
                "(some updates were missed; the daemon has the whole record)",
            ),
            _ => {}
        }
        effects
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
    /// A `session.mode` notification: the mode and the plan are the
    /// conversation's, and the checklist is said in the transcript each time
    /// it changes, whole.
    fn moded(&mut self, m: &theseus_protocol::modes::SessionModeChanged) {
        let plan_changed = m.plan != self.plan;
        let mode_changed = m.mode != self.mode;
        self.mode = m.mode;
        self.plan.clone_from(&m.plan);
        if plan_changed {
            for l in theseus_client::render::modes::lines(m) {
                self.say_plain(Sty::Dim, &l);
            }
        } else if mode_changed {
            self.say_plain(Sty::Dim, &format!("  mode: {}", m.mode.status()));
        }
    }

    pub(super) fn ask(&mut self, c: ConfirmRequest) {
        if let Some(t) = self.live.turn.as_mut() {
            t.phase = Phase::Waiting;
        }
        self.ask.focus = None;
        self.question = Some(c);
        self.ask_sync();
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
        if let Some(tag) = r.thread.as_ref() {
            if self.placed.told.insert(r.turn_id.clone()) {
                self.placed_line(tag, &r.session_id);
            }
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
        // A held turn ran no model: its line says so, and the status line
        // keeps the last real one (theseus-fzt4v).
        let held = r.stop_reason == "held";
        if !r.model.is_empty() && !held {
            self.model = Some(r.model.clone());
        }
        if !r.profile.is_empty() {
            self.profile = Some(r.profile.clone());
        }
        let mut parts = vec![took(r.elapsed_ms)];
        parts.push(match held {
            true => "held: no model ran".to_string(),
            false => model_words(&r.model),
        });
        if let Some(c) = r.cost_words() {
            parts.push(c);
        }
        if !held
            && !matches!(
                r.stop_reason.as_str(),
                "" | "done" | "end_turn" | "no_tool_calls"
            )
        {
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
