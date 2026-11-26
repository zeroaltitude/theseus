//! `config.propose_file` and `config.read_file` (issue 23): the owner's own
//! turn reads a prompt file the settings name and proposes a change to it.
//! Neither writes anything; the daemon does, after the owner's yes
//! (`write.rs`). The wall is issue 22's (`wall.rs`): the owner, alone in a
//! private place, in their own conversation. A job, a task, the self loop, a
//! session an MCP client opened and a shared place are each refused in words.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::put::{put_file, Ask};
use super::{named, read_text, resolve, Named, PROPOSE, READ};
use crate::config_change::{card, shown, wall};
use crate::rpc::Core;
use crate::toolrun::{ToolRuntime, TurnCtx};

/// The most exact replacements one proposal may carry.
pub const MAX_REPLACEMENTS: usize = 40;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replace {
    old: String,
    new: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeInput {
    file: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    replace: Option<Vec<Replace>>,
    reason: String,
}

/// The text the file would become: the whole text the model gave, or the file
/// as it is with each replacement made where its old text is found exactly
/// once. The replacements are all found in the text as it is now, and none may
/// touch another.
fn new_text(current: &str, i: &ProposeInput) -> Result<String, String> {
    let replace = match (&i.text, &i.replace) {
        (Some(t), None) => return Ok(t.clone()),
        (None, Some(r)) if !r.is_empty() && r.len() <= MAX_REPLACEMENTS => r,
        (None, Some(_)) => {
            return Err(format!(
                "give between 1 and {MAX_REPLACEMENTS} replacements"
            ));
        }
        _ => {
            return Err("give either `text` (the whole new text) or `replace`, and not both".into())
        }
    };
    let mut spans: Vec<(usize, usize, &str)> = Vec::new();
    for (n, r) in replace.iter().enumerate() {
        let n = n + 1;
        if r.old.is_empty() {
            return Err(format!("replacement {n} has no old text to find"));
        }
        let at = current.find(&r.old).ok_or_else(|| {
            format!("the old text of replacement {n} is not in the file as it is now")
        })?;
        // Found once, counting an occurrence that overlaps another.
        let next = at + r.old.chars().next().map_or(1, char::len_utf8);
        if current[next..].contains(&r.old) {
            return Err(format!(
                "the old text of replacement {n} is in the file more than once: give more of the \
                 text around it, so that it is found exactly once"
            ));
        }
        spans.push((at, at + r.old.len(), &r.new));
    }
    spans.sort_by_key(|s| s.0);
    if spans.windows(2).any(|w| w[0].1 > w[1].0) {
        return Err("two replacements touch the same text: make them one".into());
    }
    let mut out = String::with_capacity(current.len());
    let mut from = 0;
    for (start, end, new) in spans {
        out.push_str(&current[from..start]);
        out.push_str(new);
        from = end;
    }
    out.push_str(&current[from..]);
    Ok(out)
}

fn core_of(rt: &ToolRuntime) -> Result<Arc<Core>, String> {
    rt.config_change
        .core()
        .ok_or_else(|| "Theseus is not ready to change a prompt file: try again in a moment".into())
}

/// The file named, resolved and read.
fn target(core: &Core, given: &str) -> Result<(Named, String), String> {
    let named = resolve(core, given)?;
    let text = read_text(&named.path).map_err(|e| format!("{given}: {e}"))?;
    Ok((named, text))
}

/// Run `config.propose_file` for the call `correlation_id` of the turn `tc`.
pub fn propose(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = wall::refusal_for(tc, PROPOSE) {
        return Err(why);
    }
    // Run again after a restart, it answers with what the first run asked.
    if let Ok(done) = crate::config_change::proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let i: ProposeInput = parse(input)?;
    let reason = card::reason_line(&i.reason);
    if reason.is_empty() {
        return Err("give a reason in the owner's words, one line".into());
    }
    let core = core_of(rt)?;
    let (named, before) = target(&core, &i.file)?;
    let after = new_text(&before, &i)?;
    let ask = Ask {
        named: &named,
        before: &before,
        after: &after,
        reason: &reason,
        undoes: None,
    };
    put_file(rt, tc, &core, correlation_id, ask)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadInput {
    #[serde(default)]
    file: Option<String>,
}

/// Run `config.read_file`: the files that can be changed, or one of them whole.
pub fn read(rt: &ToolRuntime, tc: &TurnCtx<'_>, input: &Value) -> Result<(String, Value), String> {
    if let Some(why) = wall::refusal_for(tc, READ) {
        return Err(why);
    }
    let i: ReadInput = parse(input)?;
    let core = core_of(rt)?;
    let Some(file) = i.file else {
        return Ok(list(&core));
    };
    let (named, text) = target(&core, &file)?;
    let f = shown::facts(&text);
    let said = format!(
        "{} ({}): {}. The text follows exactly, between the two lines of dashes.\n-----\n{text}\n-----",
        named.path.display(),
        named.words(),
        f.words()
    );
    Ok((
        said,
        json!({"bytes": f.bytes, "fingerprint": f.fingerprint}),
    ))
}

/// The files the settings name as prompt text that a change can reach.
fn list(core: &Core) -> (String, Value) {
    let mut out = String::from(
        "The files the settings name as prompt text, each of which `config.propose_file` can change:\n",
    );
    let mut n = 0;
    for f in named(core) {
        let Ok(shown) = resolve(core, &f.path.display().to_string()) else {
            continue;
        };
        let Ok(text) = read_text(&shown.path) else {
            continue;
        };
        n += 1;
        out.push_str(&format!(
            "- {} ({}): {}\n",
            shown.path.display(),
            shown.words(),
            shown::facts(&text).words()
        ));
    }
    if n == 0 {
        out = "The settings name no prompt file that can be changed here.".into();
    }
    (out, json!({"files": n}))
}

/// `config.propose_file`.
pub struct Propose;

impl Tool for Propose {
    fn name(&self) -> &str {
        PROPOSE
    }

    fn description(&self) -> &str {
        "Propose a change to one of the files the settings name as Theseus's own prompt text (a \
         context file, or a persona's file), on the owner's word. Give the file's path as \
         `config.read_file` lists it, and EITHER the whole new `text` OR `replace`, a list of \
         exact replacements, each an `old` text that is found exactly once in the file as it is \
         now and the `new` text for it (prefer replacements for a long file: never retype a long \
         file). Give a one-line reason in the owner's words. Theseus builds the difference \
         itself and shows the owner one card with the file's size and fingerprint before and \
         after, the changed lines, and a way to read the whole text; only their yes changes \
         anything, and the daemon writes the file itself. It never makes a \
         file or names a new path. Offered only to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {"type": "string", "description": "The file's absolute path, as `config.read_file` lists it."},
                "text": {"type": "string", "description": "The whole new text of the file. Instead of `replace`."},
                "replace": {
                    "type": "array",
                    "description": "Exact replacements, at most 40, each found once in the file as it is now. Instead of `text`.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old": {"type": "string", "description": "Text found exactly once in the file."},
                            "new": {"type": "string", "description": "What it becomes; may be empty to remove the old text."}
                        },
                        "required": ["old", "new"],
                        "additionalProperties": false
                    }
                },
                "reason": {"type": "string", "description": "One line, in the owner's words: why they want it."}
            },
            "required": ["file", "reason"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a restart, it answers with what it asked.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: ProposeInput = parse(input)?;
        let (file, _) = shown::escape_line(&i.file);
        Ok(Plan {
            summary: format!("propose changing the prompt file {file}"),
            ..Default::default()
        })
    }
}

/// `config.read_file`.
pub struct Read;

impl Tool for Read {
    fn name(&self) -> &str {
        READ
    }

    fn description(&self) -> &str {
        "Read one of the files the settings name as Theseus's own prompt text, whole and exactly, \
         so a change to it can be proposed with exact replacements; with no `file`, list the \
         files that can be changed. Changes nothing. Offered only to the owner, in a private \
         place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {"type": "string", "description": "The file's absolute path; leave out to list the files."}
            },
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: ReadInput = parse(input)?;
        Ok(Plan {
            summary: match i.file {
                Some(f) => format!("read the prompt file {}", shown::escape_line(&f).0),
                None => "list the prompt files that can be changed".into(),
            },
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(old: &str, new: &str) -> ProposeInput {
        ProposeInput {
            file: "/x".into(),
            text: None,
            replace: Some(vec![Replace {
                old: old.into(),
                new: new.into(),
            }]),
            reason: "r".into(),
        }
    }

    /// An occurrence that overlaps another is a second occurrence: "aa" is not
    /// found once in "aaa", so the daemon does not guess which one is meant.
    #[test]
    fn an_overlapping_occurrence_counts_as_a_second_one() {
        assert!(new_text("aaa", &input("aa", "b"))
            .unwrap_err()
            .contains("more than once"));
        assert_eq!(new_text("xaay", &input("aa", "b")).unwrap(), "xby");
        assert_eq!(new_text("é-é", &input("-é", "+")).unwrap(), "é+");
    }
}
