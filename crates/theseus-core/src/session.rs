//! Sessions (spec §3.2a, §4.4b). A session is a compiler scope with exactly one
//! execution, and that execution has the turn lock (the kernel holds it, M2).
//! Its content is nodes (§4.1, `node.rs`); its context is a compilation plus an
//! append tail (§4.4a, `compiler.rs`). The record itself is small and durable
//! by reference: a thousand waiting sessions cost a record each.

use serde::{Deserialize, Serialize};
use theseus_protocol::{SessionInfo, SessionKind, Usage};

/// What the session's last turn ran against; a continuation reuses it, so a
/// conversation does not change model under the model's own thinking blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRef {
    pub profile: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub kind: SessionKind,
    pub label: Option<String>,
    pub created_at_unix_ms: u64,
    pub turns: u64,
    pub last_turn_id: Option<String>,
    #[serde(default)]
    pub usage: Usage,
    /// The session's one kernel execution (§3.2a). Sessions written before M2
    /// have none; the turn runner opens one on their next turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    /// The current compilation (§4.4a); none until the first turn compiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compilation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_target: Option<TargetRef>,
    #[serde(default)]
    pub last_active_ms: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub tool_calls: u64,
    /// The first words of the first prompt, for pickers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// An operator asked for a recompile; the next turn applies it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_recompile: Option<crate::compiler::Recompile>,
    /// A task session's origin (DD7): the session and the call that started
    /// it, and where it reports. Written once, when the task opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskOf>,
}

/// Where a task session came from, and where it reports (DD7, theseus-qn2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOf {
    pub parent_session: String,
    pub parent_execution: String,
    /// The `task.create` call that opened it.
    pub by: String,
    /// Where its cards and its report go: the place its parent posted to
    /// when it started, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl SessionRecord {
    pub fn new(kind: SessionKind, label: Option<String>) -> Self {
        Self::with_id(crate::new_id("ses"), kind, label)
    }

    /// A record for a session whose id is decided elsewhere: a task's comes
    /// from the call that opens it (DD7).
    pub fn with_id(session_id: String, kind: SessionKind, label: Option<String>) -> Self {
        let now = theseus_protocol::now_unix_ms();
        Self {
            session_id,
            kind,
            label,
            created_at_unix_ms: now,
            turns: 0,
            last_turn_id: None,
            usage: Usage::default(),
            execution_id: None,
            compilation_id: None,
            last_target: None,
            last_active_ms: now,
            cost_usd: 0.0,
            tool_calls: 0,
            title: None,
            pending_recompile: None,
            task: None,
        }
    }
    /// What a turn writes into the stored record (theseus-xeo): the fields it
    /// owns, from its own copy. Its books (turns, usage, cost, tool calls,
    /// last activity), its target, its compilation, its execution, and the
    /// title its first input gave. Never `pending_recompile`, which the
    /// operator sets while the turn runs; a turn takes that one by itself
    /// (`update_session`) when it starts.
    pub fn take_turns_fields(&mut self, turn: &SessionRecord) {
        self.turns = turn.turns;
        self.last_turn_id.clone_from(&turn.last_turn_id);
        self.usage = turn.usage.clone();
        self.last_active_ms = turn.last_active_ms;
        self.cost_usd = turn.cost_usd;
        self.tool_calls = turn.tool_calls;
        self.last_target.clone_from(&turn.last_target);
        self.compilation_id.clone_from(&turn.compilation_id);
        self.execution_id.clone_from(&turn.execution_id);
        if self.title.is_none() {
            self.title.clone_from(&turn.title);
        }
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            kind: self.kind,
            label: self.label.clone(),
            created_at_unix_ms: self.created_at_unix_ms,
            turns: self.turns,
            usage: self.usage.clone(),
            execution_id: self.execution_id.clone(),
            execution_state: None,
            last_active_ms: self.last_active_ms.max(self.created_at_unix_ms),
            cost_usd: self.cost_usd,
            tool_calls: self.tool_calls,
            profile: self.last_target.as_ref().map(|t| t.profile.clone()),
            model: self.last_target.as_ref().map(|t| t.model.clone()),
            compilation_id: self.compilation_id.clone(),
            title: self.title.clone(),
            pending_confirms: 0,
            parent_session_id: self.task.as_ref().map(|t| t.parent_session.clone()),
            limit_usd: None,
        }
    }
}

/// A title from a prompt: the first line, trimmed to about sixty characters.
pub fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(60).collect::<String>())
    } else {
        line.to_string()
    }
}
