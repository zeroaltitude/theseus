//! The flow spec, version 1 (RSI loop 1 §1.3, §2.5, §5.2): a frozen JSON
//! description of steps over tools that already exist. Parsed and validated
//! here, digested over a canonical form, and bound to its inputs and the
//! earlier steps' results at run time. Pure: no store, no clock, no tool.
//!
//! ```json
//! { "v": 1, "kind": "flow", "name": "repo_land",
//!   "scope": { "repos": ["r:5f1c0a"], "places": ["cli", "task"] },
//!   "inputs": { "branch": {"type": "ref"},
//!               "test": {"type": "argv", "default": ["cargo", "nextest", "run"]} },
//!   "steps": [
//!     {"id": "test",  "tool": "proc.run", "args": {"argv": "$test"}, "on": {"err": "exit:tests-red"}},
//!     {"id": "merge", "tool": "proc.run", "args": {"argv": ["git", "merge", "--no-ff", "$branch"]},
//!      "on": {"conflict": "goto:rerere"}},
//!     {"id": "push",  "tool": "proc.run", "args": {"argv": ["git", "push"]}} ],
//!   "exits": { "ok": "landed $branch",
//!              "handback:conflicts-left": {"state": ["conflict_files", "rerere_resolved"]} },
//!   "evidence": { "occurrences": 11, "sessions": 5 } }
//! ```
//!
//! - **Inputs** are typed (`ref`, `path`, `num`, `str`, `argv`), so a value a
//!   caller gives can never begin with `-` and be read as an option, and may
//!   carry a default. A step's `args` bind a whole string `$name` to an input,
//!   or `$id.out` / `$id.exit` to an earlier step's output text or exit code;
//!   inside an argv a scalar binds as its text. A string starting `$` that is
//!   not one of these fails validation.
//! - **Steps** run in order. `on` maps a result class (`class::CLASSES`) to
//!   `goto:<id>`, `exit:<name>` or `handback:<name>`; a class with no entry
//!   falls through to the next step (the last step's `ok` is the exit `ok`),
//!   except `err`, which with no entry hands back `step-failed`.
//! - **Bounds.** At most [`MAX_STEPS`] steps. A `goto` that makes a cycle
//!   needs `max` (2 to [`MAX_VISITS`]) on every step in it, and a run
//!   executes at most [`MAX_STEPS`] steps in all (`run`).
//! - **The allow-list** ([`ALLOWED_TOOLS`]) is the whole ceiling of a flow
//!   (§5.2): `proc.run`, file read, list and search, and the repository's
//!   reading tools. A spec naming any other tool fails with its name, and no
//!   step may carry `sandbox`, `env`, `background`, `command` or `steps`.
//! - **The digest** is the SHA-256 of a canonical form (sorted keys, no
//!   whitespace) of the spec without its `digest` field, hex. The `digest` a
//!   frozen spec carries is `sha256:<hex>`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::class;
use crate::tools_defer::PlaceKind;

/// The spec's version.
pub const VERSION: u64 = 1;
/// The most steps a spec holds, and a run executes.
pub const MAX_STEPS: usize = 24;
/// The most times a step may run in one run, with `max`.
pub const MAX_VISITS: u32 = 8;
/// Longest a description, an input's text or an exit's text may be.
const MAX_TEXT: usize = 600;
/// The most items an `argv` input holds, and the longest of them.
const MAX_ARGV: usize = 64;
const MAX_ITEM: usize = 4096;

/// The tools a step may name (§5.2 item 1): `proc.run`, file read, list and
/// search, and the git and text reading tools. Nothing here writes a file,
/// speaks to anyone, spends, or changes Theseus.
pub const ALLOWED_TOOLS: &[&str] = &[
    "proc.run",
    "fs.read",
    "fs.list",
    "fs.glob",
    "fs.grep",
    "git.diff",
    "git.log",
    "text.diff",
];

/// Argument names no step carries: a sandbox override, an environment (the
/// broker's grants ride it), a background job, a shell line and a batch (a
/// flow's steps are its batch).
pub const DENIED_ARGS: &[&str] = &["sandbox", "env", "background", "command", "steps"];

/// The state names a hand-back may ask for (`run`).
pub const STATE_NAMES: &[&str] = &["conflict_files", "rerere_resolved", "output_tail", "step"];

/// Where a flow is offered: the repositories it was seen in, and the kinds of
/// place. One repository's flow never appears in another's sessions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// Repository ids, `r:<hex>` (`repo_id`); at least one.
    pub repos: Vec<String>,
    /// Kinds of place (`cli`, `dm`, `channel`, `task`, `headless`); none: every kind.
    #[serde(default)]
    pub places: Vec<String>,
}

/// The type of an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    /// A git ref: letters, digits and `._/@{}~^-`, never starting with `-`.
    Ref,
    /// A path: no NUL, never starting with `-`.
    Path,
    Num,
    /// Text: no NUL, never starting with `-`.
    Str,
    /// A program and its arguments: up to 64 strings.
    Argv,
}

/// One input of a flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    #[serde(rename = "type")]
    pub ty: InputType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

/// One step: a tool, its arguments, and where each result class goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub tool: String,
    pub args: Value,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub on: BTreeMap<String, String>,
    /// How many times a run may execute this step (default 1): a cycle needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

/// Where a result class goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Jump {
    Goto(String),
    Exit(String),
    Handback(String),
}

impl Jump {
    pub fn parse(s: &str) -> Result<Jump, String> {
        let bad = || format!("{s:?} is not goto:<step>, exit:<name> or handback:<name>");
        let (kind, name) = s.split_once(':').ok_or_else(bad)?;
        if !word(name) {
            return Err(bad());
        }
        match kind {
            "goto" => Ok(Jump::Goto(name.into())),
            "exit" => Ok(Jump::Exit(name.into())),
            "handback" => Ok(Jump::Handback(name.into())),
            _ => Err(bad()),
        }
    }
}

/// An exit: the text it answers (`$name` is an input), or a hand-back with
/// the state it gives the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Exit {
    Text(String),
    Hand {
        state: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
}

/// A flow spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub v: u64,
    pub kind: String,
    pub name: String,
    /// What the model reads: the contract, never persuasion.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub scope: Scope,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    pub steps: Vec<Step>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exits: BTreeMap<String, Exit>,
    /// Occurrence counts and costs the miner saw: numbers and ids, never values.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub evidence: Map<String, Value>,
    /// `sha256:<hex>` of the rest; set by [`Spec::sealed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// A name: lower case letters, digits and single underscores, starting with a letter.
fn name_ok(s: &str) -> bool {
    let mut prev = ' ';
    s.len() <= 40
        && s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars().all(|c| {
            let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || (c == '_' && prev != '_');
            prev = c;
            ok
        })
        && !s.ends_with('_')
}

/// A step id, class or exit: letters, digits, `-` and `_`.
fn word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The id of a repository whose root is `root`: `r:` and the first 12 hex
/// characters of the SHA-256 of its canonical path.
pub fn repo_id(root: &std::path::Path) -> String {
    let h = Sha256::digest(root.to_string_lossy().as_bytes());
    format!("r:{}", &hex::encode(h)[..12])
}

/// The repository whose root holds `dir`: the nearest ancestor with a `.git`.
pub fn repo_root(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let dir = std::fs::canonicalize(dir).ok()?;
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(std::path::Path::to_path_buf)
}

/// `v` as canonical JSON: keys sorted, no whitespace.
pub fn canonical(v: &Value) -> String {
    fn go(v: &Value, out: &mut String) {
        match v {
            Value::Object(m) => {
                out.push('{');
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&Value::String(k.clone()).to_string());
                    out.push(':');
                    go(&m[k], out);
                }
                out.push('}');
            }
            Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    go(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut out = String::new();
    go(v, &mut out);
    out
}

impl Spec {
    /// Read a spec from its bytes: parsed, validated, and its `digest`, when
    /// it carries one, checked against the canonical form.
    pub fn parse(bytes: &[u8]) -> Result<Spec, String> {
        let spec: Spec = serde_json::from_slice(bytes)
            .map_err(|e| format!("the flow spec does not read: {e}"))?;
        spec.validate()?;
        if let Some(d) = &spec.digest {
            let want = format!("sha256:{}", spec.digest_hex());
            if *d != want {
                return Err(format!(
                    "the spec's digest {d} is not its content's {want}: it changed after it was sealed"
                ));
            }
        }
        Ok(spec)
    }

    /// The SHA-256 of the canonical form without the `digest` field, hex.
    pub fn digest_hex(&self) -> String {
        let mut bare = self.clone();
        bare.digest = None;
        let v = serde_json::to_value(&bare).unwrap_or(Value::Null);
        let mut h = Sha256::new();
        h.update(b"theseus-flow-v1\0");
        h.update(canonical(&v).as_bytes());
        hex::encode(h.finalize())
    }

    /// This spec with its `digest` set.
    pub fn sealed(mut self) -> Spec {
        self.digest = Some(format!("sha256:{}", self.digest_hex()));
        self
    }

    /// The bytes a frozen copy holds: the sealed spec, canonical.
    pub fn frozen_bytes(&self) -> Vec<u8> {
        let v = serde_json::to_value(self.clone().sealed()).unwrap_or(Value::Null);
        canonical(&v).into_bytes()
    }

    /// Whether this version of the spec is a well-formed v1 flow.
    pub fn validate(&self) -> Result<(), String> {
        if self.v != VERSION {
            return Err(format!("flow spec version {} is not {VERSION}", self.v));
        }
        if self.kind != "flow" {
            return Err(format!("kind {:?} is not \"flow\"", self.kind));
        }
        if !name_ok(&self.name) {
            return Err(format!(
                "the name {:?} must be lower case letters, digits and single underscores, \
                 starting with a letter, at most 40",
                self.name
            ));
        }
        if self.description.len() > MAX_TEXT {
            return Err(format!("the description is over {MAX_TEXT} characters"));
        }
        self.scope_ok()?;
        self.inputs_ok()?;
        if self.steps.is_empty() {
            return Err("a flow needs at least one step".into());
        }
        if self.steps.len() > MAX_STEPS {
            return Err(format!(
                "{} steps is over the {MAX_STEPS} a flow may have",
                self.steps.len()
            ));
        }
        let ids = self.step_ids()?;
        for s in &self.steps {
            self.step_ok(s, &ids)?;
        }
        self.cycles_ok()?;
        self.exits_ok()?;
        evidence_ok(&self.evidence)
    }

    fn scope_ok(&self) -> Result<(), String> {
        if self.scope.repos.is_empty() {
            return Err(
                "scope.repos must name at least one repository: a flow is offered \
                        only where its motif was seen"
                    .into(),
            );
        }
        for r in &self.scope.repos {
            let hex = r.strip_prefix("r:").unwrap_or("");
            if !(6..=64).contains(&hex.len()) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "{r:?} is not a repository id (r:<6 to 64 hex digits>)"
                ));
            }
        }
        for p in &self.scope.places {
            if PlaceKind::parse(p).is_none() {
                return Err(format!("{p:?} is not a kind of place"));
            }
        }
        Ok(())
    }

    fn inputs_ok(&self) -> Result<(), String> {
        for (n, i) in &self.inputs {
            if !name_ok(n) {
                return Err(format!("the input name {n:?} must be a lower case name"));
            }
            if let Some(d) = &i.default {
                check_input(i.ty, d).map_err(|e| format!("the default of input {n}: {e}"))?;
            }
        }
        Ok(())
    }

    fn step_ids(&self) -> Result<BTreeSet<&str>, String> {
        let mut ids = BTreeSet::new();
        for s in &self.steps {
            if !word(&s.id) || s.id.contains('.') {
                return Err(format!("the step id {:?} must be a short word", s.id));
            }
            if !ids.insert(s.id.as_str()) {
                return Err(format!("the step id {:?} is used twice", s.id));
            }
        }
        Ok(ids)
    }

    fn step_ok(&self, s: &Step, ids: &BTreeSet<&str>) -> Result<(), String> {
        if !ALLOWED_TOOLS.contains(&s.tool.as_str()) {
            return Err(format!(
                "step {}: the tool {} is not one a flow may name (allowed: {})",
                s.id,
                s.tool,
                ALLOWED_TOOLS.join(", ")
            ));
        }
        let Value::Object(args) = &s.args else {
            return Err(format!("step {}: args must be an object", s.id));
        };
        for k in args.keys() {
            if DENIED_ARGS.contains(&k.as_str()) {
                return Err(format!(
                    "step {}: {k} is not an argument a flow may give (a flow keeps the \
                     model's posture, and its steps are the batch)",
                    s.id
                ));
            }
        }
        if s.tool == "proc.run" && !args.contains_key("argv") {
            return Err(format!("step {}: proc.run needs argv", s.id));
        }
        for v in args.values() {
            self.args_ok(&s.id, v, ids)?;
        }
        if let Some(m) = s.max {
            if !(2..=MAX_VISITS).contains(&m) {
                return Err(format!("step {}: max must be 2 to {MAX_VISITS}", s.id));
            }
        }
        for (class, to) in &s.on {
            if !class::CLASSES.contains(&class.as_str()) {
                return Err(format!(
                    "step {}: {class:?} is not a result class ({})",
                    s.id,
                    class::CLASSES.join(", ")
                ));
            }
            match Jump::parse(to).map_err(|e| format!("step {}: {e}", s.id))? {
                Jump::Goto(id) if !ids.contains(id.as_str()) => {
                    return Err(format!("step {}: goto:{id} names no step", s.id));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Constants and bindings of one argument.
    fn args_ok(&self, step: &str, v: &Value, ids: &BTreeSet<&str>) -> Result<(), String> {
        match v {
            Value::String(s) => {
                if s.starts_with('$') {
                    let (name, field) = ref_of(s).ok_or_else(|| {
                        format!("step {step}: {s:?} is not a binding ($name, $id.out or $id.exit)")
                    })?;
                    match field {
                        None if self.inputs.contains_key(name) => {}
                        Some(_) if ids.contains(name) => {}
                        _ => return Err(format!("step {step}: {s} names no input or step")),
                    }
                } else if s.starts_with('/') || s.starts_with('~') {
                    return Err(format!(
                        "step {step}: {s:?} is a path from outside the repository: make it an input"
                    ));
                }
                Ok(())
            }
            Value::Array(a) => a.iter().try_for_each(|x| self.args_ok(step, x, ids)),
            Value::Object(m) => m.values().try_for_each(|x| self.args_ok(step, x, ids)),
            _ => Ok(()),
        }
    }

    /// Every step that sits on a cycle of gotos and fall-throughs has `max`.
    fn cycles_ok(&self) -> Result<(), String> {
        let n = self.steps.len();
        let at: BTreeMap<&str, usize> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.as_str(), i))
            .collect();
        let mut reach = vec![vec![false; n]; n];
        for (i, s) in self.steps.iter().enumerate() {
            if i + 1 < n {
                reach[i][i + 1] = true;
            }
            for to in s.on.values() {
                if let Ok(Jump::Goto(id)) = Jump::parse(to) {
                    if let Some(&j) = at.get(id.as_str()) {
                        reach[i][j] = true;
                    }
                }
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    if reach[i][k] && reach[k][j] {
                        reach[i][j] = true;
                    }
                }
            }
        }
        for (i, s) in self.steps.iter().enumerate() {
            if reach[i][i] && s.max.is_none() {
                return Err(format!(
                    "step {} is on a cycle of gotos: give it a bound (\"max\": 2 to {MAX_VISITS})",
                    s.id
                ));
            }
        }
        Ok(())
    }

    fn exits_ok(&self) -> Result<(), String> {
        for (k, e) in &self.exits {
            let name = k.strip_prefix("handback:").unwrap_or(k);
            if !word(name) {
                return Err(format!("the exit name {k:?} must be a short word"));
            }
            match e {
                Exit::Text(t) if k.starts_with("handback:") => {
                    return Err(format!("{k} gives a state list, not the text {t:?}"));
                }
                Exit::Text(t) | Exit::Hand { text: Some(t), .. } if t.len() > MAX_TEXT => {
                    return Err(format!("exit {k}: the text is over {MAX_TEXT} characters"));
                }
                Exit::Hand { .. } if !k.starts_with("handback:") => {
                    return Err(format!("exit {k} gives a state list: name it handback:{k}"));
                }
                Exit::Hand { state, .. } => {
                    if let Some(s) = state.iter().find(|s| !STATE_NAMES.contains(&s.as_str())) {
                        return Err(format!(
                            "exit {k}: {s:?} is not a state a flow gives ({})",
                            STATE_NAMES.join(", ")
                        ));
                    }
                }
                Exit::Text(_) => {}
            }
        }
        Ok(())
    }

    /// The step with this id.
    pub fn step(&self, id: &str) -> Option<(usize, &Step)> {
        self.steps.iter().enumerate().find(|(_, s)| s.id == id)
    }

    /// The tool's input schema: one property per input.
    pub fn input_schema(&self) -> Value {
        let mut props = Map::new();
        let mut required = Vec::new();
        for (n, i) in &self.inputs {
            let mut p = match i.ty {
                InputType::Ref => json!({"type": "string", "description": "a git ref"}),
                InputType::Path => json!({"type": "string", "description": "a path"}),
                InputType::Str => json!({"type": "string"}),
                InputType::Num => json!({"type": "number"}),
                InputType::Argv => {
                    json!({"type": "array", "items": {"type": "string"}, "description": "a program and its arguments"})
                }
            };
            if let Some(d) = &i.default {
                p["default"] = d.clone();
            } else {
                required.push(json!(n));
            }
            props.insert(n.clone(), p);
        }
        json!({"type": "object", "properties": props, "required": required,
               "additionalProperties": false})
    }

    /// What the model reads of the tool.
    pub fn tool_description(&self) -> String {
        let steps: Vec<String> = self
            .steps
            .iter()
            .map(|s| format!("{} ({})", s.id, s.tool))
            .collect();
        let what = if self.description.is_empty() {
            format!("Run the frozen flow {}.", self.name)
        } else {
            self.description.clone()
        };
        format!(
            "{what} Steps, each gated as your own call: {}. A step that asks for approval, or \
             that policy refuses, stops the flow and hands back; so does a result needing your judgment.",
            steps.join(", ")
        )
    }

    /// The values of the inputs a caller gave, with defaults, each checked.
    pub fn resolve_inputs(&self, given: &Value) -> Result<BTreeMap<String, Value>, String> {
        let empty = Map::new();
        let given = match given {
            Value::Object(m) => m,
            Value::Null => &empty,
            _ => return Err("the flow's input must be an object".into()),
        };
        if let Some(k) = given.keys().find(|k| !self.inputs.contains_key(*k)) {
            return Err(format!("{k:?} is not an input of this flow"));
        }
        let mut out = BTreeMap::new();
        for (n, i) in &self.inputs {
            let v = given
                .get(n)
                .or(i.default.as_ref())
                .ok_or_else(|| format!("the input {n} is required"))?;
            out.insert(
                n.clone(),
                check_input(i.ty, v).map_err(|e| format!("the input {n}: {e}"))?,
            );
        }
        Ok(out)
    }

    /// The text of the exit `name`, with `$input`s filled in; the name itself
    /// when the spec gives none.
    pub fn exit_text(&self, name: &str, inputs: &BTreeMap<String, Value>) -> String {
        match self.exits.get(name) {
            Some(Exit::Text(t)) => fill(t, inputs),
            _ => name.to_string(),
        }
    }

    /// The state names the hand-back `name` asks for: the output tail and the
    /// step when the spec gives none.
    pub fn handback_state(&self, name: &str) -> Vec<String> {
        match self.exits.get(&format!("handback:{name}")) {
            Some(Exit::Hand { state, .. }) if !state.is_empty() => state.clone(),
            _ => vec!["step".into(), "output_tail".into()],
        }
    }
}

fn evidence_ok(m: &Map<String, Value>) -> Result<(), String> {
    for (k, v) in m {
        let ok = match v {
            Value::Number(_) | Value::Bool(_) => true,
            Value::String(s) => s.len() <= 200,
            Value::Array(a) => a.iter().all(Value::is_number),
            _ => false,
        };
        if !ok {
            return Err(format!(
                "evidence.{k} must be a number, a short string or numbers: ids and counts, never values"
            ));
        }
    }
    Ok(())
}

/// A binding string: its name and the field after a dot, if any.
fn ref_of(s: &str) -> Option<(&str, Option<&str>)> {
    let body = s.strip_prefix('$')?;
    match body.split_once('.') {
        None => word(body).then_some((body, None)),
        Some((id, f @ ("out" | "exit"))) => word(id).then_some((id, Some(f))),
        Some(_) => None,
    }
}

/// `v` as an input of type `ty`, or why not.
pub fn check_input(ty: InputType, v: &Value) -> Result<Value, String> {
    let text = |what: &str| -> Result<String, String> {
        let s = v.as_str().ok_or_else(|| format!("must be {what}"))?;
        if s.is_empty() || s.len() > MAX_ITEM || s.contains('\0') {
            return Err(format!(
                "must be {what}, 1 to {MAX_ITEM} characters with no NUL"
            ));
        }
        if s.starts_with('-') {
            return Err(format!("{s:?} begins with '-': it would read as an option"));
        }
        Ok(s.to_string())
    };
    match ty {
        InputType::Str => text("text").map(Value::String),
        InputType::Path => text("a path").map(Value::String),
        InputType::Ref => {
            let s = text("a git ref")?;
            let ok = |c: char| c.is_ascii_alphanumeric() || "._/@{}~^-".contains(c);
            if !s.chars().all(ok) {
                return Err(format!("{s:?} is not a git ref"));
            }
            Ok(Value::String(s))
        }
        InputType::Num => match v {
            Value::Number(_) => Ok(v.clone()),
            _ => Err("must be a number".into()),
        },
        InputType::Argv => {
            let a = v.as_array().ok_or("must be an array of strings")?;
            if a.is_empty() || a.len() > MAX_ARGV {
                return Err(format!("must hold 1 to {MAX_ARGV} strings"));
            }
            for x in a {
                let s = x.as_str().ok_or("must be an array of strings")?;
                if s.len() > MAX_ITEM || s.contains('\0') {
                    return Err("an item is too long or holds a NUL".into());
                }
            }
            Ok(v.clone())
        }
    }
}

/// A scalar as the text an argv item holds.
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `$name`s inside an exit's text, filled from the inputs.
fn fill(t: &str, inputs: &BTreeMap<String, Value>) -> String {
    let mut names: Vec<&String> = inputs.keys().collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let mut out = t.to_string();
    for n in names {
        out = out.replace(&format!("${n}"), &text_of(&inputs[n]));
    }
    out
}

/// What an earlier step left for later steps to bind.
#[derive(Debug, Clone, Default)]
pub struct StepOut {
    pub out: String,
    pub exit: Option<i64>,
}

/// Bind `args`: each `$name`, `$id.out` and `$id.exit` replaced by its value.
/// An output bound into an argument that begins with `-` is refused, as an
/// input is.
pub fn bind(
    args: &Value,
    inputs: &BTreeMap<String, Value>,
    steps: &BTreeMap<String, StepOut>,
) -> Result<Value, String> {
    bind_in(args, inputs, steps, false)
}

fn bind_in(
    v: &Value,
    inputs: &BTreeMap<String, Value>,
    steps: &BTreeMap<String, StepOut>,
    in_array: bool,
) -> Result<Value, String> {
    match v {
        Value::String(s) if s.starts_with('$') => {
            let (name, field) = ref_of(s).ok_or_else(|| format!("{s:?} is not a binding"))?;
            let val = match field {
                None => inputs.get(name).cloned(),
                Some("out") => steps
                    .get(name)
                    .map(|o| Value::String(o.out.trim_end().to_string())),
                Some(_) => steps.get(name).and_then(|o| o.exit).map(|c| json!(c)),
            }
            .ok_or_else(|| format!("{s} has no value: step {name} has not run"))?;
            if field == Some("out") && val.as_str().is_some_and(|t| t.starts_with('-')) {
                return Err(format!("{s} begins with '-': it would read as an option"));
            }
            // Inside an argv a scalar is its text; an argv input spreads.
            Ok(match (&val, in_array) {
                (Value::Array(_), _) => val,
                (_, true) => Value::String(text_of(&val)),
                _ => val,
            })
        }
        Value::Array(a) => {
            let mut out = Vec::new();
            for x in a {
                match bind_in(x, inputs, steps, true)? {
                    Value::Array(items) if matches!(x, Value::String(_)) => out.extend(items),
                    one => out.push(one),
                }
            }
            Ok(Value::Array(out))
        }
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                out.insert(k.clone(), bind_in(x, inputs, steps, false)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}
