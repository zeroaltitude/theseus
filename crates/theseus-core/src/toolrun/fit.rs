//! A call that fits the tools (theseus-9dt2): the names and shapes a model
//! reaches for, read as the call it meant where the runtime takes it from the
//! model (`ToolRuntime::fit_uses`), so the gate, the ledger and a resume all
//! see the call that runs. The rules are `theseus_tools::fit`'s; this is
//! where they meet the runtime: the note a result starts with, the words of
//! an unknown tool and an unparsed input, and where a call ran.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;
use theseus_tools::fit::{nearest, recover_line, SHELL_SHAPES};

use super::{ResultNode, ToolRuntime};
use crate::node::ResultStatus;
use crate::provider::ToolUse;

/// Each fitted call's note, by its `tool_use` id, until its result is
/// written, and the calls per tool whose input was not JSON. In memory: a
/// note lost to a restart costs a result its first line, never a call its
/// run, and the count is `tool.list`'s, since the daemon started.
#[derive(Default)]
pub struct Notes(
    Mutex<BTreeMap<String, String>>,
    Mutex<BTreeMap<String, u64>>,
);

impl Notes {
    pub(super) fn count_invalid(&self, tool: &str) {
        if let Ok(mut m) = self.1.lock() {
            *m.entry(tool.into()).or_default() += 1;
        }
    }

    pub fn invalid_json(&self) -> BTreeMap<String, u64> {
        self.1.lock().map(|m| m.clone()).unwrap_or_default()
    }

    fn put(&self, id: &str, note: String) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(id.into(), note);
        }
    }

    /// The note, kept while the call still runs in the background.
    fn get(&self, id: &str, keep: bool) -> Option<String> {
        let mut m = self.0.lock().ok()?;
        if keep {
            m.get(id).cloned()
        } else {
            m.remove(id)
        }
    }
}

impl ToolRuntime {
    /// The directory a call runs in when it names none: the session's. Until
    /// a session has its own (session-dir) it is the tools' `cwd`; this is
    /// the one place that says so.
    pub(super) fn session_dir(&self) -> PathBuf {
        self.ctx.cwd.clone()
    }

    /// The calls as the model meant them: `bash {command}` as `proc_run`, a
    /// string for `argv` as a shell line, `command` as its argv, and an input
    /// that did not parse, `{"argv": cp /a /b}`, as its line when that is the
    /// only reading. The ids whose unparsed input was read that way come back
    /// too: they are no longer invalid.
    pub fn fit_uses(
        &self,
        uses: &[ToolUse],
        invalid: &BTreeMap<String, String>,
    ) -> (Vec<ToolUse>, BTreeSet<String>) {
        let mut recovered = BTreeSet::new();
        let fitted = uses
            .iter()
            .map(|u| self.fit_use(u, invalid.get(&u.id), &mut recovered))
            .collect();
        (fitted, recovered)
    }

    pub(super) fn fit_use(
        &self,
        u: &ToolUse,
        raw: Option<&String>,
        recovered: &mut BTreeSet<String>,
    ) -> ToolUse {
        let shell = self
            .tool_by_wire(&u.name)
            .is_some_and(|t| t.name() == "proc.run");
        let mut input = u.input.clone();
        let mut note = None;
        if let (true, Some(line)) = (shell, raw.and_then(|r| recover_line(r))) {
            input = line;
            recovered.insert(u.id.clone());
            note = Some(
                "[ran as proc_run {command}: the input was not valid JSON; its shell line ran]"
                    .to_string(),
            );
        } else if raw.is_some() {
            return u.clone();
        }
        let Some(f) = self.registry.fit(&u.name, &input) else {
            if let Some(n) = note {
                self.fit_notes.put(&u.id, n);
            }
            return ToolUse { input, ..u.clone() };
        };
        if let Some(n) = note.or(f.note) {
            self.fit_notes.put(&u.id, n);
        }
        ToolUse {
            id: u.id.clone(),
            name: f.wire,
            input: f.input,
        }
    }

    /// A result with its call's note as its first line, once it is written.
    pub(super) fn fit_noted<'a>(&self, mut r: ResultNode<'a>) -> ResultNode<'a> {
        let keep = r.status == ResultStatus::Background;
        if let Some(note) = self.fit_notes.get(r.tool_use_id, keep) {
            r.text = format!("{note}\n{}", r.text);
        }
        r
    }

    /// An unknown tool's answer: the one or two real tools nearest its name.
    pub(super) fn unknown_text(&self, name: &str) -> String {
        let wires: Vec<String> = self
            .registry
            .all()
            .map(|t| t.wire_name())
            .chain(self.mcp.all().iter().map(|t| t.wire.clone()))
            .collect();
        let near = nearest(name, wires.iter().map(String::as_str));
        let near: Vec<String> = near.iter().map(|n| format!("`{n}`")).collect();
        format!("Unknown tool `{name}`. Nearest: {}.", near.join(", "))
    }

    /// What a call whose input did not parse is told: for the shell tool the
    /// field and its two shapes, for any other the input as it came.
    pub(super) fn invalid_json_text(tool: &str, raw: &str) -> String {
        match tool {
            "proc.run" => format!("Invalid input: {SHELL_SHAPES}."),
            _ => serde_json::json!({"INVALID_JSON": raw}).to_string(),
        }
    }

    /// An invalid input's message, naming the field and the tool that takes
    /// it where the error is an unknown field.
    pub(super) fn field_hinted(&self, tool: &str, error: &str) -> String {
        self.registry
            .get(tool)
            .and_then(|t| self.registry.field_hint(&t.wire_name(), error))
            .unwrap_or_else(|| error.to_string())
    }

    /// `[exit code 0 · in /dir]`'s tail for a `proc.run` result: where the
    /// call ran, so a model never has to `pwd`. A batch ran its last step
    /// last.
    pub(super) fn ran_in(&self, tool: &str, input: Option<&Value>) -> String {
        if tool != "proc.run" {
            return String::new();
        }
        let input = input.unwrap_or(&Value::Null);
        let last = input
            .get("steps")
            .and_then(Value::as_array)
            .and_then(|s| s.last());
        let cwd = last
            .and_then(|s| s.get("cwd"))
            .or_else(|| input.get("cwd"))
            .and_then(Value::as_str);
        let dir = cwd.map_or_else(|| self.session_dir(), |p| self.ctx.resolve(p));
        format!(" · in {}", dir.display())
    }
}
