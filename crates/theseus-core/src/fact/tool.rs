//! The tool calls' facts: the gate, the run, and the answer (`toolrun.rs`).

use serde_json::{json, Value};
use theseus_kernel::Action;
use theseus_protocol::NarrativePart::{Approval, Job, Tool};
use theseus_protocol::{
    notify, ConfirmRequest, ConfirmResolved, Event, ExternalText, GateRecord, LedgerKind,
    PolicyNotified,
};

use super::{Fact, Say};
use crate::broker::Grant;
use crate::narrative;
use crate::node::{Body, Node, ResultStatus};
use crate::policy::Posture;
use crate::provider::ToolUse;
use crate::scrub::Scrubber;
use crate::toolrun::{Gated, ToolRuntime};

/// The gate's record of a call, shown to the session's clients before it
/// runs or asks (`tool.proposed`).
pub struct ToolProposed<'a> {
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub call: &'a ToolUse,
    pub tool: &'a str,
    pub record: &'a GateRecord,
}

impl Fact for ToolProposed<'_> {
    const METHOD: Option<&'static str> = Some(notify::TOOL_PROPOSED);

    fn event(&self) -> Option<Event> {
        Some(Event::ToolProposed(theseus_protocol::ToolProposed {
            session_id: self.session_id.into(),
            turn_id: self.turn_id.into(),
            tool_use_id: self.call.id.clone(),
            tool: self.tool.into(),
            input: self.call.input.clone(),
            gate: self.record.clone(),
        }))
    }
}

/// The gate decided a call's posture: it waits, it runs and the operator is
/// told, or it runs.
pub struct GateDecided<'a> {
    pub(crate) runtime: &'a ToolRuntime,
    pub tool: &'a str,
    pub(crate) gated: &'a Gated,
}

impl Fact for GateDecided<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let (rt, g, tool) = (self.runtime, self.gated, self.tool);
        let subject = rt.subject(tool, &g.plan);
        let why = narrative::gate_why(
            &g.decision.reason,
            &g.plan.summary,
            tool,
            g.decision.posture.as_str(),
            g.plan.argv.as_deref(),
            &rt.posture_now(tool).setting,
        );
        let why = rt.scrubber.scrub(&why).0;
        say.line(
            Tool,
            match g.decision.posture {
                Posture::Approve => {
                    format!("{subject}: posture approve ({why}), waiting for approval.")
                }
                Posture::Notify => {
                    format!("{subject}: posture notify ({why}), running and telling the operator.")
                }
                Posture::Open => format!("{subject}: posture open ({why}), running."),
            },
        );
    }
}

/// A call ran under a `notify` posture, and the operator is told
/// (`tool.notified`, `policy.notified`): its row rides in the frame that
/// plans it, and the notice follows.
pub struct ToolNotified<'a> {
    pub notice: &'a PolicyNotified,
}

impl Fact for ToolNotified<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolNotified);
    const METHOD: Option<&'static str> = Some(notify::POLICY_NOTIFIED);

    fn row(&self) -> Value {
        serde_json::to_value(self.notice).unwrap_or(Value::Null)
    }

    fn event(&self) -> Option<Event> {
        Some(Event::PolicyNotified(self.notice.clone()))
    }
}

/// A call waits for the operator (`tool.confirm_requested`,
/// `confirm.requested`): the turn parks on it.
pub struct CallAsked<'a> {
    pub request: &'a ConfirmRequest,
}

impl Fact for CallAsked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolConfirmRequested);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_REQUESTED);

    fn row(&self) -> Value {
        serde_json::to_value(self.request).unwrap_or(Value::Null)
    }

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmRequested(self.request.clone()))
    }
}

/// The model called a tool that does not exist; it gets an error.
pub struct UnknownTool<'a> {
    pub name: &'a str,
}

impl Fact for UnknownTool<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "The model called an unknown tool `{}`; it gets an error.",
                self.name.chars().take(40).collect::<String>()
            ),
        );
    }
}

/// A call's input was not valid JSON, so it does not run
/// (`tool.invalid_input`).
pub struct InvalidJson<'a> {
    pub tool: &'a str,
    pub tool_use_id: &'a str,
}

impl Fact for InvalidJson<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolInvalidInput);

    fn row(&self) -> Value {
        json!({"tool": self.tool, "tool_use_id": self.tool_use_id})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "{}: the input is not valid JSON, so it does not run.",
                self.tool
            ),
        );
    }
}

/// The toollet refused a call's input, or its place may not make it (the
/// place rule: a reason `place: …`), so it does not run (`tool.invalid_input`,
/// with the reason and the input).
pub struct InvalidInput<'a> {
    pub tool: &'a str,
    pub call: &'a ToolUse,
    pub reason: &'a str,
}

impl Fact for InvalidInput<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolInvalidInput);

    fn row(&self) -> Value {
        json!({"tool": self.tool, "tool_use_id": self.call.id, "reason": self.reason, "input": self.call.input})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let place = format!("{}: ", crate::toolrun::PLACE_REFUSAL);
        let why = match self.reason.starts_with(&place) {
            true => "this place is shared, and the call reaches past what a shared place may",
            false => "the input is invalid",
        };
        say.line(Tool, format!("{}: {why}, so it does not run.", self.tool));
    }
}

/// A call began to run in the process or the harness (`tool.started`).
pub struct ToolStarted<'a> {
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub tool_use_id: &'a str,
    pub tool: &'a str,
    pub correlation_id: &'a str,
    pub backend: &'a str,
}

impl Fact for ToolStarted<'_> {
    const METHOD: Option<&'static str> = Some(notify::TOOL_STARTED);

    fn event(&self) -> Option<Event> {
        Some(Event::ToolStarted(theseus_protocol::ToolStarted {
            session_id: self.session_id.into(),
            turn_id: self.turn_id.into(),
            tool_use_id: self.tool_use_id.into(),
            tool: self.tool.into(),
            correlation_id: self.correlation_id.into(),
            backend: self.backend.into(),
            ..Default::default()
        }))
    }
}

/// A job began through its wrapper (`tool.job_started`, `tool.started`):
/// its pid, its argv, and what the broker granted and withheld; the turn
/// waits up to `bound_ms` for it.
pub struct JobStarted<'a> {
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub tool_use_id: &'a str,
    pub tool: &'a str,
    pub correlation_id: &'a str,
    pub pid: u32,
    pub argv: &'a [String],
    pub cwd: &'a std::path::Path,
    pub timeout_secs: u64,
    pub granted: Option<&'a str>,
    pub withheld: &'a [String],
    pub bound_ms: u64,
    /// For the narrative's subject, which may name a secret's value.
    pub scrubber: &'a Scrubber,
    /// Its class (M4 17b): an L1 job's row and notification say so, and
    /// the notification names its egress list (18c).
    pub class: &'a crate::sandbox::Bound,
}

impl JobStarted<'_> {
    fn l1(&self) -> bool {
        self.class.l1()
    }
}

impl Fact for JobStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolJobStarted);
    const METHOD: Option<&'static str> = Some(notify::TOOL_STARTED);

    fn row(&self) -> Value {
        let mut row = json!({"correlation_id": self.correlation_id, "pid": self.pid, "argv": self.argv, "cwd": self.cwd, "timeout_secs": self.timeout_secs});
        if self.l1() {
            row["class"] = json!("l1");
        }
        row
    }

    fn event(&self) -> Option<Event> {
        Some(Event::ToolStarted(theseus_protocol::ToolStarted {
            session_id: self.session_id.into(),
            turn_id: self.turn_id.into(),
            tool_use_id: self.tool_use_id.into(),
            tool: self.tool.into(),
            correlation_id: self.correlation_id.into(),
            backend: "job".into(),
            pid: Some(self.pid),
            argv: Some(self.argv.to_vec()),
            cwd: Some(self.cwd.to_path_buf()),
            granted: Some(self.granted.map(str::to_string)),
            withheld: Some(self.withheld.to_vec()),
            class: self.l1().then(|| "l1".into()),
            egress: self.l1().then(|| self.class.egress.clone()),
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "{} started as job {} (pid {}{}){}; the turn waits up to {} for it.",
                self.scrubber
                    .scrub(&narrative::subject(
                        self.tool,
                        Some(self.argv),
                        None,
                        self.cwd
                    ))
                    .0,
                narrative::short(self.correlation_id),
                self.pid,
                if self.l1() { ", in L1" } else { "" },
                self.granted.map_or_else(String::new, |g| format!("; {g}")),
                narrative::duration(self.bound_ms)
            ),
        );
    }
}

/// The broker granted a job's program a secret (`secret.granted`).
pub struct SecretGranted<'a> {
    pub grant: &'a Grant,
    pub correlation_id: &'a str,
    pub tool: &'a str,
}

impl Fact for SecretGranted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SecretGranted);

    fn row(&self) -> Value {
        let g = self.grant;
        json!({"program": g.to, "variable": g.variable, "secret": g.secret,
            "correlation_id": self.correlation_id, "tool": self.tool})
    }
}

/// The broker withheld a secret from a job's program, and why
/// (`secret.withheld`).
pub struct SecretWithheld<'a> {
    pub grant: &'a Grant,
    pub why: &'a str,
    pub correlation_id: &'a str,
    pub tool: &'a str,
}

impl Fact for SecretWithheld<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SecretWithheld);

    fn row(&self) -> Value {
        let g = self.grant;
        json!({"program": g.to, "variable": g.variable, "secret": g.secret,
            "correlation_id": self.correlation_id, "tool": self.tool, "why": self.why})
    }
}

/// An in-process toollet read a granted secret through the broker
/// (`secret.granted`).
pub struct SecretHanded<'a> {
    pub tool: &'a str,
    pub secret: &'a str,
    pub correlation_id: &'a str,
}

impl Fact for SecretHanded<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SecretGranted);

    fn row(&self) -> Value {
        json!({"tool": self.tool, "secret": self.secret, "correlation_id": self.correlation_id})
    }
}

/// One request an AWS call made (`aws.called`, AWS design §3.8). The row is
/// its binding's (`aws::Account::row`): never a credential, never the
/// result. Its span is the trace's own (`Aws::spans`).
pub struct AwsCalled<'a> {
    pub row: &'a Value,
}

impl Fact for AwsCalled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::AwsCalled);

    fn row(&self) -> Value {
        self.row.clone()
    }
}

/// A role session minted for an AWS call (`aws.session.minted`, AWS design
/// §3.5, §3.8): its kind, name, role, policies, and lifetime, never its
/// credentials (`aws::session`).
pub struct AwsSessionMinted<'a> {
    pub row: &'a Value,
}

impl Fact for AwsSessionMinted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::AwsSessionMinted);

    fn row(&self) -> Value {
        self.row.clone()
    }
}

/// The session took a hold on external text (theseus-9bp): a result it
/// read, or a task's report that carried one. A call that acts now waits,
/// or is notified.
pub struct HoldTaken<'a> {
    pub hold: &'a ExternalText,
    pub mode: crate::external::Mode,
}

impl Fact for HoldTaken<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Approval, crate::external::narrated(self.hold, self.mode));
    }
}

/// Below the disk's floor no job starts (theseus-102): `job.refused`.
pub struct JobRefused<'a> {
    pub correlation_id: &'a str,
    pub tool: &'a str,
    pub free_mb: u64,
    pub floor_mb: u64,
}

impl Fact for JobRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JobRefused);

    fn row(&self) -> Value {
        json!({"correlation_id": self.correlation_id, "tool": self.tool,
            "free_mb": self.free_mb, "floor_mb": self.floor_mb})
    }
}

/// A running job the daemon stopped because the disk fell below its floor
/// (theseus-ht82): `job.stopped_below_floor`. The floor refuses the next job;
/// this is the one that was already writing. Which job fills the disk cannot
/// be told from here, so each running job is stopped and each has its row.
pub struct JobStoppedBelowFloor<'a> {
    pub correlation_id: &'a str,
    pub tool: &'a str,
    pub free_mb: u64,
    pub floor_mb: u64,
}

impl Fact for JobStoppedBelowFloor<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JobStoppedBelowFloor);

    fn row(&self) -> Value {
        json!({"correlation_id": self.correlation_id, "tool": self.tool,
            "free_mb": self.free_mb, "floor_mb": self.floor_mb})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "Stopped {} {}: the disk under the state dir has {} MB free, below the floor of \
                 {} MB.",
                self.tool,
                crate::task::short(self.correlation_id),
                narrative::thousands(self.free_mb),
                narrative::thousands(self.floor_mb)
            ),
        );
    }
}

/// A job a stop or a cancel reached before its launch is never started
/// (theseus-36to): `job.not_started`.
pub struct JobNotStarted<'a> {
    pub action: &'a Action,
    pub tool: &'a str,
}

impl Fact for JobNotStarted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JobNotStarted);

    fn row(&self) -> Value {
        let a = self.action;
        json!({"correlation_id": a.correlation_id, "tool": self.tool,
            "cancel": a.cancel, "resolution": a.resolution})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let a = self.action;
        say.line(
            Tool,
            format!(
                "{} was not started as job {}: {} before its launch.",
                self.tool,
                narrative::short(&a.correlation_id),
                a.resolution
                    .as_deref()
                    .unwrap_or("its execution was cancelled")
            ),
        );
    }
}

/// A job a stop or a cancel reached during its launch was stopped once its
/// pid was known (theseus-36to): `job.stopped_at_launch`.
pub struct JobStoppedAtLaunch<'a> {
    pub correlation_id: &'a str,
    pub pid: u32,
    pub gone: bool,
}

impl Fact for JobStoppedAtLaunch<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JobStoppedAtLaunch);

    fn row(&self) -> Value {
        json!({"correlation_id": self.correlation_id, "pid": self.pid, "gone": self.gone})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "Job {} was told to stop as it launched, before its pid was known; {}.",
                narrative::short(self.correlation_id),
                if self.gone {
                    "it is stopped"
                } else {
                    "it is not gone yet"
                }
            ),
        );
    }
}

/// A call's result was written (`tool.ended`): done, failed, not run, sent
/// to the background, or late.
pub struct ToolEnded<'a> {
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub node: &'a Node,
}

impl Fact for ToolEnded<'_> {
    const METHOD: Option<&'static str> = Some(notify::TOOL_ENDED);

    fn event(&self) -> Option<Event> {
        let Body::ToolResult {
            tool_use_id,
            tool,
            status,
            duration_ms,
            correlation_id,
            late,
            truncated,
            bytes_total,
            content,
            meta,
            ..
        } = &self.node.body
        else {
            return None;
        };
        Some(Event::ToolEnded(theseus_protocol::ToolEnded {
            session_id: self.session_id.into(),
            turn_id: self.turn_id.into(),
            tool_use_id: tool_use_id.clone(),
            tool: tool.clone(),
            status: status.as_str().into(),
            duration_ms: *duration_ms,
            correlation_id: correlation_id.clone(),
            late: *late,
            truncated: *truncated,
            bytes: *bytes_total,
            node_id: self.node.id.clone(),
            exit_code: meta.get("exit_code").and_then(Value::as_i64),
            // A `/stop` ended it, and who stopped it (theseus-4uw).
            stopped_by: meta
                .get("stopped_by")
                .and_then(Value::as_str)
                .map(str::to_string),
            preview: content.chars().take(2000).collect(),
            scratch: meta
                .pointer("/detail/scratch/summary")
                .and_then(Value::as_str)
                .map(str::to_string),
            reached: crate::egress::reached_line(&meta["detail"]),
            // How a cancel or a stop knows it stopped (M4 18a).
            verified: meta
                .get("verified")
                .and_then(Value::as_str)
                .map(str::to_string),
        }))
    }

    /// Sizes and the exit code, never the output.
    fn narrate(&self, say: &mut Say<'_>) {
        let Body::ToolResult {
            tool,
            status,
            duration_ms,
            correlation_id,
            late,
            bytes_total,
            content,
            meta,
            ..
        } = &self.node.body
        else {
            return;
        };
        let size = format!(
            "{} ({})",
            narrative::count(narrative::lines_in(content), "line", "lines"),
            narrative::bytes(*bytes_total)
        );
        let exit = meta
            .get("exit_code")
            .and_then(Value::as_i64)
            .map(|c| format!("exit code {c}, "))
            .unwrap_or_default();
        let took = duration_ms
            .map(|ms| format!(" in {}", narrative::duration(ms)))
            .unwrap_or_default();
        let job = correlation_id
            .as_deref()
            .map(narrative::short)
            .unwrap_or_default();
        if *late {
            say.line(
                Job,
                format!(
                    "A late result for {tool} (job {job}) arrived: {}, {exit}{size}; the model \
                     reads it next.",
                    status.as_str()
                ),
            );
            return;
        }
        let (part, line) = match status {
            ResultStatus::Ok => (Tool, format!("{tool} done{took}: {exit}{size}.")),
            ResultStatus::Error => (
                Tool,
                format!("{tool} ended with an error{took}: {exit}{size}."),
            ),
            ResultStatus::Background => (
                Job,
                format!(
                    "{tool} continues in the background as job {job}; its result comes in a \
                     later message."
                ),
            ),
            ResultStatus::Declined => (Approval, format!("{tool} not run: it was declined.")),
            // A `/stop` ended it: what the operator asked for (theseus-4uw).
            ResultStatus::Cancelled if meta.get("stopped_by").is_some() => (
                Tool,
                format!(
                    "{tool} stopped by {}{}.",
                    meta["stopped_by"].as_str().unwrap_or("the operator"),
                    duration_ms
                        .map(|ms| format!(", after {}", narrative::duration(ms)))
                        .unwrap_or_default()
                ),
            ),
            ResultStatus::Cancelled => (
                Tool,
                format!(
                    "{tool} not run: {}.",
                    meta.get("not_run")
                        .and_then(Value::as_str)
                        .unwrap_or("it was cancelled")
                ),
            ),
            ResultStatus::Unknown => (
                Tool,
                format!(
                    "{tool}: the outcome is unknown; the harness could not establish whether it \
                     finished."
                ),
            ),
        };
        say.line(part, line);
    }
}

/// New input came instead of an answer: the waiting call is declined, and
/// its question is resolved as superseded (`confirm.resolved`).
pub struct CallSuperseded<'a> {
    pub session_id: &'a str,
    pub correlation_id: &'a str,
    pub tool: &'a str,
}

impl Fact for CallSuperseded<'_> {
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.session_id.into(),
            correlation_id: self.correlation_id.into(),
            superseded: true,
            ..Default::default()
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "{}: new input came instead of an answer, so it is declined.",
                self.tool
            ),
        );
    }
}

/// A call planned and never asked, found by a continuation (theseus-ni5): a
/// restart came between its plan and its authorization, so nothing asked the
/// operator and nothing ran it. It is declined by the harness (the
/// `action.declined` row says so), and its result says it did not run.
pub struct CallNeverAsked<'a> {
    pub tool: &'a str,
}

impl Fact for CallNeverAsked<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "{}: planned before a restart and never asked, so it did not run.",
                self.tool
            ),
        );
    }
}

/// An approved call runs (`action.confirm` announced the answer).
pub struct ApprovedRunning<'a> {
    pub tool: &'a str,
}

impl Fact for ApprovedRunning<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!("{}: approved; running it now.", self.tool),
        );
    }
}

/// An approval expired or no longer matches its call: it does not run.
pub struct ApprovalVoid<'a> {
    pub tool: &'a str,
    pub error: &'a anyhow::Error,
}

impl Fact for ApprovalVoid<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "{}: the approval no longer holds ({}), so it does not run.",
                self.tool, self.error
            ),
        );
    }
}

/// A call authorized before a restart and never dispatched runs now.
pub struct AuthorizedResumed<'a> {
    pub tool: &'a str,
}

impl Fact for AuthorizedResumed<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Tool,
            format!(
                "{}: authorized before a restart; running it now.",
                self.tool
            ),
        );
    }
}

/// A background job's result is written as a late result node
/// (`tool.late_result`), in the frame that writes the node.
pub struct LateResult<'a> {
    pub correlation_id: &'a str,
    pub tool: &'a str,
    pub state: theseus_kernel::ActionState,
}

impl Fact for LateResult<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ToolLateResult);

    fn row(&self) -> Value {
        json!({"correlation_id": self.correlation_id, "tool": self.tool, "state": self.state})
    }
}

/// A task started (DD7): its own session runs with a budget carved from
/// this one's, and reports where it says.
pub struct TaskStarted<'a> {
    /// The task's short id, its title, and its session.
    pub short: &'a str,
    pub title: &'a str,
    pub session_id: &'a str,
    pub limit: theseus_kernel::Micros,
    /// What this session had left before the carve.
    pub available_before: theseus_kernel::Micros,
    /// Where it reports; `None`, in this session.
    pub target: Option<&'a str>,
    /// Its arrangement's pieces (M5 27).
    pub pieces: usize,
}

impl Fact for TaskStarted<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            theseus_protocol::NarrativePart::Session,
            format!(
                "Task {} started (\"{}\", {}): its session {} runs on its own with {} carved \
                 from the {} this session had left, and reports {}.",
                self.short,
                self.title,
                crate::arrangement::clip(self.pieces),
                narrative::short(self.session_id),
                narrative::dollars(self.limit),
                narrative::dollars(self.available_before),
                match self.target {
                    Some(t) => format!("to {t}"),
                    None => "in this session".to_string(),
                }
            ),
        );
    }
}

/// A task started from a session that holds external text holds it too
/// (theseus-9bp): its calls that act wait until the operator trusts it.
pub struct TaskHoldsExternal<'a> {
    pub short: &'a str,
}

impl Fact for TaskHoldsExternal<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Task {} holds this session's external text from its start, so its calls that \
                 act wait for approval too, until the operator trusts it.",
                self.short
            ),
        );
    }
}

/// A wake was set (DD8): at its time the session gets a turn whose input is
/// its note.
pub struct WakeSet<'a> {
    pub short: &'a str,
    /// When, as the result says it: the local time and the span until it.
    pub when: &'a str,
    pub note: &'a str,
    pub pending: usize,
    /// A repeating wake's series, as people say it (`every 1d`; 37a).
    pub series: Option<&'a str>,
}

impl Fact for WakeSet<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let series = self
            .series
            .map_or(String::new(), |s| format!(", {s}, first"));
        say.line(
            theseus_protocol::NarrativePart::Session,
            format!(
                "Wake {} set{series} for {}: \"{}\"; {} of {} pending.",
                self.short,
                self.when,
                crate::session::title_from(self.note),
                self.pending,
                theseus_kernel::MAX_PENDING
            ),
        );
    }
}

/// A file read for a model (theseus-c9l6): an attachment kept when it
/// arrived, or a PDF a tool read (`file.read`), with its conversion's time
/// and whether it ran in the capped child.
pub struct FileRead<'a> {
    pub read: &'a crate::attach::FileRead,
}

impl Fact for FileRead<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::FileRead);

    fn row(&self) -> Value {
        let r = self.read;
        json!({"via": r.via, "name": r.name, "media_type": r.media_type, "bytes": r.bytes,
               "digest": r.digest, "pages": r.pages, "parts": r.parts,
               "text_bytes": r.text_bytes, "outcome": r.outcome(), "why": r.why,
               "ms": r.ms, "capped": r.capped})
    }

    fn span(&self, trace: &mut crate::trace::Trace) {
        let r = self.read;
        let end = trace.now_us();
        trace.record(
            "file.read",
            "file",
            end.saturating_sub(r.ms * 1000),
            end,
            json!({"via": r.via, "media_type": r.media_type, "bytes": r.bytes,
                   "pages": r.pages, "outcome": r.outcome(), "capped": r.capped}),
        );
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.read;
        let what = match r.pages {
            Some(n) => format!(
                "{}, {}",
                theseus_files::pdf::count(n),
                narrative::bytes(r.bytes)
            ),
            None => narrative::bytes(r.bytes),
        };
        let how = match (&r.why, r.outcome()) {
            (Some(why), _) => format!("its text was not read: {why}"),
            (None, "kept") => "kept, not read".to_string(),
            (None, _) => format!("read in {} ms", r.ms),
        };
        say.line(Tool, format!("{} ({what}): {how}.", r.name));
    }
}
