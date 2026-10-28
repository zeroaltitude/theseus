//! The work board's builders: one view from its entity's inputs. A builder
//! reads the board and writes nothing; `settle` compares what it makes with
//! the view held and fills what only the comparison knows (`position`,
//! `previous`, `state_since_ms`, `attention`), and the rollup pass fills
//! `below` and `progress`.

use theseus_kernel::{ActionState, BUDGET_TOOL};
use theseus_protocol::tasks::TaskState;
use theseus_protocol::work::{
    answer_options, Doing, DoingKind, Question, QuestionKind, WorkKind, WorkRef, WorkReport,
    WorkState, WorkView,
};
use theseus_protocol::{Attention, Level};

use super::{
    answered, eta, hm, horizon, is_job, step_paused, step_state, ActIn, EndedWake, ExecIn,
    WorkBoard,
};

/// A title's longest: a command or a note cut to a line.
const TITLE_CHARS: usize = 120;

pub(super) fn line(s: &str) -> String {
    let l = s
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if l.chars().count() <= TITLE_CHARS {
        l.to_string()
    } else {
        let cut: String = l.chars().take(TITLE_CHARS - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

/// A view with nothing in it but what every view has.
pub(super) fn blank(id: &str, kind: WorkKind, at_ms: u64) -> WorkView {
    WorkView {
        moved_to: None,
        id: id.to_string(),
        kind,
        position: 0,
        at_ms,
        parent: None,
        root: id.to_string(),
        session_id: None,
        title: String::new(),
        place: None,
        state: WorkState::Unknown,
        previous: None,
        attention: Attention {
            level: Level::Working,
            label: String::new(),
            since_ms: at_ms,
        },
        started_at_ms: 0,
        state_since_ms: at_ms,
        ended_at_ms: None,
        doing: None,
        progress: None,
        question: None,
        cost: None,
        eta: None,
        forecast: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        expect: None,
        flagged: false,
        dismissed: false,
        retry_of: None,
        thread: None,
        purpose: None,
        next_at_ms: None,
        series: None,
        paused: false,
        routine: None,
        watch: None,
        project: None,
        data: None,
    }
}

impl WorkBoard {
    /// The root of an execution's tree: the topmost execution the board
    /// knows above it.
    pub(super) fn root_of(&self, exec: &str) -> String {
        let mut at = exec.to_string();
        for _ in 0..16 {
            match self.execs.get(&at).and_then(|e| e.parent.clone()) {
                Some(p) if self.execs.contains_key(&p) => at = p,
                _ => break,
            }
        }
        at
    }

    /// An execution's parent, when the board holds it.
    fn parent_exec(&self, ei: &ExecIn) -> Option<String> {
        ei.parent.clone().filter(|p| self.execs.contains_key(p))
    }

    // ----------------------------------------------------- an execution

    pub(super) fn exec_view(&self, id: &str) -> Option<WorkView> {
        let ei = self.execs.get(id)?;
        let mut v = theseus_protocol::work::WorkView::from_execution(&ei.x);
        v.id = ei.x.execution_id.clone();
        v.kind = ei.kind;
        v.parent = self.parent_exec(ei);
        v.root = self.root_of(id);
        v.title = ei.title.clone();
        v.started_at_ms = ei.created_at_ms;
        if let Some(w) = self.routine_wake_of(ei) {
            // A routine's run hangs from the routine, not from its execution.
            v.parent = Some(w.id.clone());
        }
        Self::decorate_exec(&mut v, ei);
        v.woke = ei.woke.clone().or_else(|| Self::due_wake(ei));
        v.forecast = self.forecast_of(ei);
        v.position = 0;
        v.previous = None;
        v.state_since_ms = ei.x.attention.since_ms;
        if let Some(q) = v.question.as_mut() {
            q.asked_by.title = ei.title.clone();
            q.may_answer = true;
        }
        self.own_ask(&mut v, ei);
        v.doing = self.doing_of(ei, &v);
        v.progress = super::progress::said_only(ei.said.as_deref(), 0);
        v.moved_to = ei.on.clone();
        v.report = self.report_of(ei, &v);
        v.flagged = ei.flagged;
        v.expect = ei.expect;
        v.paused = ei.paused;
        v.ended_at_ms = v.state.ended().then_some(ei.x.at_ms);
        Some(v)
    }

    /// An execution that waits on an `owner_ask` carries that question whole:
    /// `from_execution` only knows a pending call in brief (an approval with
    /// approve and decline), which would hide the choices from every reader
    /// of the task's own view (`theseus tasks`, the board's needs-you row).
    fn own_ask(&self, v: &mut WorkView, ei: &ExecIn) {
        let Some(id) = v.question.as_ref().map(|q| q.id.as_str()) else {
            return;
        };
        let Some(ai) = self
            .acts
            .get(id)
            .filter(|ai| ai.a.awaits_confirm() && crate::ask::input_of(&ai.a).is_some())
        else {
            return;
        };
        v.question = self.question_view(ai, ei, None).question;
    }

    /// The wake an execution was queued for, while the wake is still on its
    /// list: the oldest turn wake that is due, so the notice says which wake
    /// woke it when the due scan queues it, not a frame later when its turn
    /// takes the wake.
    fn due_wake(ei: &ExecIn) -> Option<theseus_protocol::work::Woke> {
        if ei.x.why.as_deref() != Some("wake")
            || !matches!(ei.x.state.as_str(), "queued" | "running")
        {
            return None;
        }
        ei.wakes
            .iter()
            .filter(|w| w.run.is_turn() && w.paused_at_ms.is_none() && w.due_at_ms <= ei.x.at_ms)
            .min_by_key(|w| w.due_at_ms)
            .map(woke_of)
    }

    /// What an execution is doing: its model, a tool call, or a job (the
    /// oldest open one), else what `from_execution` reads from its wait.
    fn doing_of(&self, ei: &ExecIn, v: &WorkView) -> Option<Doing> {
        let turn = ei.x.turns;
        let loop_index = ei.loop_index;
        if matches!(v.state, WorkState::Running | WorkState::Waiting) {
            let open: Vec<&ActIn> = self
                .acts_by_exec
                .get(&ei.x.execution_id)
                .into_iter()
                .flatten()
                .filter_map(|c| self.acts.get(c))
                .filter(|a| !a.a.state.is_settled() && !a.a.awaits_confirm())
                .filter(|a| a.a.state != ActionState::Planned || a.a.confirm.is_some())
                .collect();
            if let Some(j) = open.iter().find(|a| is_job(&a.a)) {
                return Some(Doing {
                    what: DoingKind::Job,
                    label: job_title(j),
                    since_ms: j.a.dispatched_at_ms.unwrap_or(j.a.planned_at_ms),
                    turn,
                    loop_index,
                    last_line: None,
                });
            }
            if let Some(t) = open.iter().min_by_key(|a| a.a.planned_at_ms) {
                return Some(Doing {
                    what: DoingKind::Tool,
                    label: t.a.tool.clone(),
                    since_ms: t.a.planned_at_ms,
                    turn,
                    loop_index,
                    last_line: None,
                });
            }
        }
        let mut d = v.doing.clone()?;
        d.loop_index = loop_index;
        if d.what == DoingKind::Thinking {
            // The model has thought since the turn began, or since the last call's result came back.
            d.since_ms = if ei.turn_started_ms > 0 {
                ei.turn_started_ms.max(ei.last_settled_ms)
            } else {
                d.since_ms
            };
        }
        Some(d)
    }

    /// A task's end in its own words: its outcome, the first line of the
    /// report its record carries as evidence, and why it ended.
    fn report_of(&self, ei: &ExecIn, v: &WorkView) -> Option<WorkReport> {
        if !v.state.ended() && v.report.is_none() {
            return None;
        }
        let first_line = self
            .task_of_exec
            .get(&ei.x.execution_id)
            .and_then(|t| {
                t.evidence
                    .iter()
                    .rev()
                    .find(|e| e.identity.starts_with("report:"))
            })
            .and_then(|e| e.note.as_deref())
            .map(line)
            .filter(|l| !l.is_empty());
        let outcome = match ei.x.state.as_str() {
            "complete" => "done".to_string(),
            other => other.to_string(),
        };
        Some(WorkReport {
            outcome,
            first_line,
            reason: ei
                .ended_reason
                .clone()
                .or_else(|| ei.x.ended_reason.clone())
                .filter(|r| !r.is_empty()),
        })
    }

    // ------------------------------------------------------ an action

    /// The view of an action: its open question, its job, or its question
    /// as it ended.
    pub(super) fn act_view(&self, corr: &str) -> Option<WorkView> {
        let ai = self.acts.get(corr)?;
        let ei = self.execs.get(&ai.a.execution_id)?;
        if ai.a.awaits_confirm() {
            return Some(self.question_view(ai, ei, None));
        }
        if ai.a.dispatched_at_ms.is_some() && is_job(&ai.a) {
            return Some(self.job_view(ai, ei));
        }
        if ai.asked {
            let outcome = ai.answer.clone().or_else(|| outcome_of(&ai.a))?;
            return Some(self.question_view(ai, ei, Some(outcome)));
        }
        None
    }

    fn question_view(
        &self,
        ai: &ActIn,
        ei: &ExecIn,
        ended: Option<(String, Option<String>)>,
    ) -> WorkView {
        let a = &ai.a;
        let me = WorkRef {
            id: ei.x.execution_id.clone(),
            kind: ei.kind,
            session_id: Some(ei.x.session_id.clone()),
            title: ei.title.clone(),
        };
        let pending =
            ei.x.pending
                .iter()
                .find(|p| p.correlation_id == a.correlation_id);
        let budget = a.tool == BUDGET_TOOL;
        let (tool, prompt) = question_words(ai);
        let reason = pending
            .map(|p| p.reason.clone())
            .filter(|r| !r.is_empty())
            .or_else(|| ai.detail.clone());
        let asked = crate::ask::input_of(a);
        let kind = if budget {
            QuestionKind::Budget
        } else if asked.is_some() {
            QuestionKind::Ask
        } else if tool == crate::extend::ACK {
            QuestionKind::Extension
        } else if ai.change {
            QuestionKind::Change
        } else {
            QuestionKind::Approval
        };
        let expires = pending
            .map(|p| p.expires_at_ms)
            .filter(|e| *e > 0)
            .or(ai.expires_at_ms)
            .or_else(|| asked.as_ref().map(|i| a.planned_at_ms + i.life_ms()));
        let mut q = Question {
            id: a.correlation_id.clone(),
            kind,
            asked_by: me,
            prompt: prompt.clone(),
            detail: reason,
            input: a.proposal.as_ref().map(|p| p.args.clone()),
            options: Vec::new(),
            on_expiry: expires.map(|_| match &asked {
                Some(i) => i.on_expiry(),
                None => "not run".to_string(),
            }),
            expires_at_ms: expires,
            floor: pending.is_some_and(|p| p.floor) || ai.floor,
            may_answer: true,
            tool: Some(tool),
            asked_at_ms: a.planned_at_ms,
            choices: asked.as_ref().map(|i| i.choices()).unwrap_or_default(),
        };
        q.options = answer_options(&q);
        let mut v = blank(&a.correlation_id, WorkKind::Question, a.planned_at_ms);
        v.parent = Some(ei.x.execution_id.clone());
        v.root = self.root_of(&ei.x.execution_id);
        v.session_id = Some(a.session_id.clone());
        v.title = prompt;
        v.started_at_ms = a.planned_at_ms;
        v.state_since_ms = a.planned_at_ms;
        match ended {
            None => v.state = WorkState::NeedsYou,
            Some((outcome, by)) => {
                v.state = answered(&outcome);
                v.ended_at_ms = Some(
                    a.settled_at_ms
                        .or(a.authorized_at_ms)
                        .unwrap_or(a.planned_at_ms),
                );
                v.state_since_ms = v.ended_at_ms.unwrap_or(a.planned_at_ms);
                v.report = Some(WorkReport {
                    outcome,
                    first_line: None,
                    reason: by,
                });
            }
        }
        if budget {
            v.cost = Some(theseus_protocol::work::Cost {
                spent_usd: ei.x.spent_usd,
                limit_usd: ei.x.limit_usd,
                turn_usd: None,
            });
        }
        v.question = Some(q);
        v
    }

    fn job_view(&self, ai: &ActIn, ei: &ExecIn) -> WorkView {
        let a = &ai.a;
        let started = a.dispatched_at_ms.unwrap_or(a.planned_at_ms);
        let mut v = blank(&a.correlation_id, WorkKind::Job, started);
        v.parent = Some(ei.x.execution_id.clone());
        v.root = self.root_of(&ei.x.execution_id);
        v.session_id = Some(a.session_id.clone());
        v.title = job_title(ai);
        v.started_at_ms = started;
        v.state_since_ms = started;
        v.state = match a.state {
            ActionState::Planned | ActionState::Authorized => WorkState::Queued,
            ActionState::Dispatched => WorkState::Running,
            ActionState::Succeeded => WorkState::Done,
            ActionState::Failed => WorkState::Failed,
            ActionState::Cancelled => WorkState::Cancelled,
            ActionState::OutcomeUnknown => WorkState::Unknown,
        };
        if v.state.ended() {
            let at = a.settled_at_ms.unwrap_or(started);
            v.ended_at_ms = Some(at);
            v.state_since_ms = at;
            v.report = Some(WorkReport {
                outcome: match v.state {
                    WorkState::Done => "done",
                    WorkState::Failed => "failed",
                    _ => "cancelled",
                }
                .to_string(),
                first_line: None,
                reason: None,
            });
        } else if v.state == WorkState::Running {
            v.doing = Some(Doing {
                what: DoingKind::Job,
                label: v.title.clone(),
                since_ms: started,
                turn: ei.x.turns,
                loop_index: ei.loop_index,
                last_line: self.last.get(&a.correlation_id).cloned(),
            });
            v.eta = eta::key_of(&ai.argv).and_then(|k| self.eta.usual(&k));
        }
        v
    }

    // --------------------------------------------------------- a wake

    pub(super) fn wake_view(&self, id: &str) -> Option<WorkView> {
        if let Some(ended) = self.ended_wakes.get(id) {
            return self.ended_wake_view(ended);
        }
        let (ei, w) = self
            .execs
            .values()
            .find_map(|e| e.wakes.iter().find(|w| w.id == id).map(|w| (e, w)))?;
        let mut v = blank(&w.id, WorkKind::Wake, w.set_at_ms);
        v.parent = Some(ei.x.execution_id.clone());
        v.root = self.root_of(&ei.x.execution_id);
        v.session_id = Some(ei.x.session_id.clone());
        v.title = line(&w.note);
        v.state = WorkState::Waiting;
        v.started_at_ms = w.set_at_ms;
        v.state_since_ms = w.set_at_ms;
        let paused = w.paused_at_ms.is_some();
        v.doing = Some(Doing {
            what: DoingKind::Sleeping,
            label: if paused {
                "paused".to_string()
            } else {
                format!("due {}", hm(w.due_at_ms))
            },
            since_ms: w.set_at_ms,
            turn: 0,
            loop_index: 0,
            last_line: None,
        });
        self.decorate_wake(&mut v, ei, w);
        Some(v)
    }

    fn ended_wake_view(&self, e: &EndedWake) -> Option<WorkView> {
        let ei = self.execs.get(&e.exec)?;
        let w = &e.wake;
        let mut v = blank(&w.id, horizon::kind_of(w), w.set_at_ms);
        v.parent = Some(ei.x.execution_id.clone());
        v.root = self.root_of(&ei.x.execution_id);
        v.session_id = Some(ei.x.session_id.clone());
        v.title = line(&w.note);
        v.purpose = Some(w.purpose.as_str().to_string());
        v.state = match self.watches.get(&w.id).and_then(|h| h.end) {
            Some("met") => WorkState::Done,
            Some(_) => WorkState::Expired,
            None if e.fired => WorkState::Done,
            None => WorkState::Cancelled,
        };
        if w.purpose == theseus_kernel::Purpose::Watch {
            self.watch_part(&mut v, w);
            v.doing = None;
        }
        v.started_at_ms = w.set_at_ms;
        v.ended_at_ms = Some(e.at_ms);
        v.state_since_ms = e.at_ms;
        v.report = Some(WorkReport {
            outcome: match self.watches.get(&w.id).and_then(|h| h.end) {
                Some(end) => end,
                None if e.fired => "fired",
                None => "cancelled",
            }
            .to_string(),
            first_line: None,
            reason: None,
        });
        Some(v)
    }

    // --------------------------------------------------------- a step

    pub(super) fn step_view(&self, id: &str) -> Option<WorkView> {
        let t = self.steps.get(id)?;
        let parent = match t.parent.as_deref() {
            Some(p) if self.steps.contains_key(p) => Some(p.to_string()),
            _ => self.owner_exec(t),
        }?;
        if !self.execs.contains_key(&parent) && !self.steps.contains_key(&parent) {
            return None;
        }
        let root = if self.execs.contains_key(&parent) {
            self.root_of(&parent)
        } else {
            let owner = self.owner_exec(t)?;
            self.root_of(&owner)
        };
        let mut v = blank(&t.id, WorkKind::Step, t.updated_at_ms);
        v.parent = Some(parent);
        v.root = root;
        v.session_id = Some(t.origin.session.clone());
        v.title = line(&t.title);
        v.state = step_state(t.state);
        // A suspended record (cancelled or out of budget, recoverable) is held,
        // as an owner-paused task is: one attention rule reads both (theseus-g00vv).
        v.paused = step_paused(t.state);
        v.started_at_ms = t.created_at_ms;
        v.state_since_ms = t.updated_at_ms;
        if t.state.is_closed() {
            v.ended_at_ms = Some(t.updated_at_ms);
        }
        if t.state == TaskState::Failed {
            v.report = Some(WorkReport {
                outcome: "failed".into(),
                first_line: t.evidence.last().and_then(|e| e.note.as_deref()).map(line),
                reason: None,
            });
        }
        Some(v)
    }
}

/// The tool a question is about and its words: what the call would run, or
/// the budget's.
fn question_words(ai: &ActIn) -> (String, String) {
    let a = &ai.a;
    let tool = a
        .proposal
        .as_ref()
        .map_or_else(|| a.tool.clone(), |p| p.tool.clone());
    let prompt = if a.tool == BUDGET_TOOL {
        "reset the session's spend".to_string()
    } else if let Some(i) = crate::ask::input_of(a) {
        line(&i.question)
    } else if !ai.argv.is_empty() {
        format!("{tool} {}", line(&ai.argv.join(" ")))
    } else {
        match a
            .proposal
            .as_ref()
            .and_then(|p| p.resource.clone())
            .or_else(|| a.resource.clone())
        {
            Some(r) => format!("{tool} {}", line(&r)),
            None => tool.clone(),
        }
    };
    (tool, prompt)
}

/// A wake that woke an execution, as its view says it.
pub(super) fn woke_of(w: &theseus_kernel::PendingWake) -> theseus_protocol::work::Woke {
    theseus_protocol::work::Woke {
        note: crate::session::title_from(&w.note),
        at_ms: w.due_at_ms,
        purpose: Some(w.purpose.as_str().to_string()),
    }
}

/// A job's title: its command as the model wrote it, else its tool.
fn job_title(ai: &ActIn) -> String {
    if ai.argv.is_empty() {
        ai.a.tool.clone()
    } else {
        line(&ai.argv.join(" "))
    }
}

/// How a question ended when no row said: from what became of its action.
fn outcome_of(a: &theseus_kernel::Action) -> Option<(String, Option<String>)> {
    if a.awaits_confirm() {
        return None;
    }
    if let Some(r) = a.resolution.as_deref() {
        if r.starts_with("declined") || r.starts_with("denied") {
            return Some(("declined".into(), None));
        }
        if r.contains("expired") {
            return Some(("expired".into(), None));
        }
    }
    match a.state {
        ActionState::Cancelled => Some(("declined".into(), None)),
        ActionState::Planned => None,
        _ => Some((
            "approved".into(),
            a.confirm.as_ref().map(|c| format!("by {}", c.by)),
        )),
    }
}
