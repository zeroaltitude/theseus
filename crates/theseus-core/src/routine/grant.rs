//! A routine's grant (theseus-52tut; the horizon report's §5.4, "Authority,
//! settled at creation", and the owner's decision D-H6): the tools a routine's
//! runs use without asking, even after a run reads outside text. Everything
//! else still waits, and a call that waits with nobody there ends the run
//! `blocked` at its deadline (`routine/watch.rs`).
//!
//! - **What it is.** `TaskRun.tools` of the routine's wake, names of tools as
//!   the model calls them (`gog.gmail.search`, `web.fetch`), and for a posting
//!   tool the place as well (`channel.post #digest`): the owner's yes to that
//!   place for that routine. It is set with the routine (`routine.create`,
//!   `--grant`), changed by `routine.update` (`Add to grant` on a blocked
//!   run), and shown on the creation card.
//! - **Never more than the owner holds.** `check` refuses, at creation, a
//!   tool that does not exist, a tool no grant may name (one that runs what
//!   it is given, or changes who may act: `NEVER`), and a posting tool with
//!   no place.
//! - **What the gate does with it.** `gate`, after the place's own layers and
//!   for the routine's own run only (`Book::grant_for`: a
//!   task whose parent is the routine; a task the run starts has the run as
//!   its parent and gets nothing): a granted call that only the tool's
//!   posture would make wait runs at `notify`. It never loosens
//!   the floor, a layer-1 change, the approve lists, a granted secret, a
//!   place's ceiling or an L1 call's own asks.

use std::sync::Arc;

use serde_json::Value;
use theseus_tools::{Plan, Tool};

use crate::policy::{Decision, Notice, Posture};
use crate::toolrun::ToolRuntime;

/// Tools no grant names: a program or a prompt the model writes runs whatever
/// it is given, and these change what may run or who may act. A routine that
/// needs one asks, as any task does.
pub const NEVER: [&str; 10] = [
    "proc.run",
    "term.open",
    "term.send",
    "routine.create",
    "extend.propose",
    "task.update",
    "task.split",
    "task.close",
    "aws.call",
    "aws.hands.run",
];

/// A notice's `setting` begins so: `ran under the routine's grant:
/// gog.gmail.search`.
pub const SETTING: &str = "ran under the routine's grant: ";

/// A posting tool, whose entry names where it posts.
const POSTS: &str = crate::glide::POST;

/// One entry: a tool, and for a posting tool its place.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    tool: String,
    place: Option<String>,
}

/// The tools a routine's runs use without asking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grant {
    entries: Vec<Entry>,
}

impl Grant {
    /// The grant the entries (as written) make; one that does not read is
    /// left out, since `check` refused it at creation.
    pub fn of(written: &[String]) -> Self {
        Self {
            entries: written.iter().filter_map(|w| entry(w)).collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether the grant names `tool`, for a posting tool to one of `dests`
    /// (what the call named and what that place is called), any case.
    pub fn covers(&self, tool: &str, dests: &[&str]) -> bool {
        self.entries.iter().any(|e| {
            e.tool == tool
                && match &e.place {
                    None => tool != POSTS,
                    Some(p) => dests.iter().any(|d| same_place(d, p)),
                }
        })
    }

    /// The entry that covers the call, as written, for the notice.
    fn entry_for(&self, tool: &str, dests: &[&str]) -> Option<String> {
        self.entries
            .iter()
            .find(|e| {
                e.tool == tool
                    && match &e.place {
                        None => tool != POSTS,
                        Some(p) => dests.iter().any(|d| same_place(d, p)),
                    }
            })
            .map(|e| match &e.place {
                Some(p) => format!("{} {p}", e.tool),
                None => e.tool.clone(),
            })
    }
}

fn same_place(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// `tool` or `tool place...`.
fn entry(written: &str) -> Option<Entry> {
    let w = written.trim();
    let (tool, place) = match w.split_once(char::is_whitespace) {
        Some((t, p)) => (t, Some(p.trim().to_string()).filter(|p| !p.is_empty())),
        None => (w, None),
    };
    (!tool.is_empty()).then(|| Entry {
        tool: tool.to_string(),
        place,
    })
}

/// Read and check a grant as written: each entry names a tool the owner has
/// (`known`), none that `NEVER` lists, and a posting tool its place. Returns
/// the entries cleaned (one space, no repeats), or the first refusal in words.
pub fn check(written: &[String], known: &dyn Fn(&str) -> bool) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for w in written {
        let Some(e) = entry(w) else {
            return Err(
                "a grant entry names a tool: `gog.gmail.search`, or `channel.post #digest`".into(),
            );
        };
        if NEVER.contains(&e.tool.as_str()) {
            return Err(format!(
                "`{}` can't be in a grant: it runs what it is given or changes who may act, so a \
                 run that needs it asks, as any task does",
                e.tool
            ));
        }
        if e.tool.starts_with("mcp:ext-") {
            return Err(format!(
                "`{}` is a proposed extension's tool, and its load holds it to its own ceiling: \
                 it can't be in a grant",
                e.tool
            ));
        }
        if !known(&e.tool) {
            return Err(format!(
                "`{}` is not a tool you have: check its name with `theseus tools`",
                e.tool
            ));
        }
        if e.tool == POSTS && e.place.is_none() {
            return Err(format!(
                "`{POSTS}` is granted to a place: write it as `{POSTS} #digest`"
            ));
        }
        if e.tool != POSTS && e.place.is_some() {
            return Err(format!(
                "`{}` takes no place: only `{POSTS}` is granted to one",
                e.tool
            ));
        }
        let clean = match &e.place {
            Some(p) => format!("{} {p}", e.tool),
            None => e.tool.clone(),
        };
        if !out.contains(&clean) {
            out.push(clean);
        }
    }
    Ok(out)
}

/// The entries `add` joins to `have`, each once.
pub fn joined(have: &[String], add: &[String]) -> Vec<String> {
    let mut out = have.to_vec();
    for a in add {
        if !out.contains(a) {
            out.push(a.clone());
        }
    }
    out
}

/// The grant the gate reads for one call: `Some(entry)` when the routine's
/// run is granted this tool (for a posting tool, to the place the call
/// names), `None` for everything else, which keeps the gate's decision.
pub(crate) fn covering(
    grant: Option<&Arc<Grant>>,
    tool: &str,
    input: &Value,
    glide: Option<&crate::glide::Resolved>,
) -> Option<String> {
    let g = grant?;
    let named = input.get("to").and_then(Value::as_str);
    let mut dests: Vec<&str> = Vec::new();
    if let Some(n) = named {
        dests.push(n);
    }
    if let Some(r) = glide {
        dests.push(&r.other.name);
    }
    g.entry_for(tool, &dests)
}

/// The gate's step for a routine's run (see the module's doc). `d` is the
/// decision after the glide's layer.
pub(crate) fn gate(
    rt: &ToolRuntime,
    at: &crate::toolrun::order::At<'_>,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    l1: bool,
    d: Decision,
) -> Decision {
    let Some(entry) = covering(at.routine, tool.name(), input, at.glide) else {
        return d;
    };
    // Whatever a place, a secret, the floor or L1 asked stays asked.
    if l1
        || d.floor
        || d.granted.is_some()
        || at.place.ceiling.is_some()
        || at.glide.is_some_and(|g| g.floor.is_some())
        || rt.tightened.get(tool.name()).is_some()
        || !rt.policy.posture_only(tool, plan)
    {
        return d;
    }
    // An open call needs no word; a notified or asked one runs at notify
    // and says whose grant it ran under.
    if d.posture == Posture::Open {
        return d;
    }
    let rule = format!(
        "{} — notify ({SETTING}{entry}; `theseus routine update --grant` changes it)",
        tool.name()
    );
    Decision {
        notify: Some(Notice {
            kind: "notify".into(),
            setting: format!("{SETTING}{entry}"),
            rule: rule.clone(),
        }),
        ..Decision::new(Posture::Notify, rule)
    }
}

#[cfg(test)]
#[path = "grant/tests.rs"]
mod tests;
