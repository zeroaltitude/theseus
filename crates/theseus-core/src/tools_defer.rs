//! Deferred tools (theseus-7m7w.2): the families a request does not need
//! load on demand.
//!
//! Every model call reads the whole tool set. Most places use a few families
//! of it, so a request declares the rest *deferred*:
//!
//! - **On Anthropic's API**, for a model whose catalog entry says
//!   `tool_search`, a hidden family's tools go out with `defer_loading: true`
//!   (a constant 93 tokens in the context however many there are) and the tool
//!   search tool (BM25) is added; the model searches, the API surfaces the
//!   tools it finds, and it calls them as any other.
//! - **Anywhere else** the hidden tools are left out and `tools_find {query}`
//!   is offered; what it finds loads from the conversation's next boundary.
//!
//! What loads is a stable set per place kind (`select`), from committed
//! defaults and the ledger's use. The cache holds because the set is fixed
//! per compilation: an append takes the set the current compilation recorded
//! (`Manifest.loaded`), and only a recompile, a clearing pass or a compaction
//! (the compile's `recompiled` spec) takes the fresh one. A call to a hidden
//! tool that no search surfaced is answered with words naming the search and
//! counted (`tools.missed`); a search is counted too (`tools.searched`), and a
//! turn's row (`tools.used`) feeds the next selection.
//!
//! The module's parts: `family` (the families and place kinds), `select`
//! (the pure choice), `want` (the families a message names, for a model
//! without tool search), `board` (the counts and the conversations' notes, in
//! memory), `find` (the fallback tool and the words), `scan` (a response's
//! searches), and `warm` (the ledger read after serving).

use std::borrow::Cow;
use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::compiler::{CompileInput, RequestSpec};

pub mod board;
pub mod family;
pub mod find;
pub mod fit;
pub mod list;
pub mod scan;
pub mod select;
pub mod want;
pub mod warm;

pub use board::{Board, HiddenTool};
pub use family::PlaceKind;

/// `tools_find`'s wire name, and its family.
pub const FIND: &str = "tools_find";
pub const FAMILY: &str = "tools";
/// The family of the plan's two tools (theseus-lyxy): loaded while the
/// conversation is in plan mode, deferred everywhere else.
pub const PLAN_FAMILY: &str = "plan";
/// The provider's tool search tool: its name and its type.
pub const SEARCH_TOOL: &str = "tool_search_tool_bm25";
pub const SEARCH_TYPE: &str = "tool_search_tool_bm25_20251119";

/// What the provider charges for deferring tools, measured on Theseus's own
/// request by the tool-tree lane (theseus-0nnk, 2026-10-06): the declared tools
/// cost a constant 93 tokens however many there are, and the search tool 264.
pub const DEFERRED_TOKENS: u64 = 93;
pub const SEARCH_TOKENS: u64 = 264;
/// The same in bytes of tool JSON at Claude's rate (2.4 bytes a token), which
/// is what a request's estimate counts them as.
pub const DEFERRED_BYTES: u64 = 223;
pub const SEARCH_BYTES: u64 = 634;

/// How a request reaches the tools it hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mechanism {
    /// The provider's: `defer_loading` and its tool search tool.
    Search,
    /// The harness's: the hidden tools are left out and `tools_find` is offered.
    Find,
}

impl Mechanism {
    pub fn as_str(self) -> &'static str {
        match self {
            Mechanism::Search => "search",
            Mechanism::Find => "find",
        }
    }
}

/// What a request defers: the families it loads, and how it reaches the rest.
/// On a spec from the turn it is the fresh choice over the whole catalog
/// (`resolved` false); `compile` resolves it into the spec's tools.
#[derive(Debug, Clone, PartialEq)]
pub struct Deferral {
    pub mechanism: Mechanism,
    pub loaded: BTreeSet<String>,
    pub resolved: bool,
}

/// What a request's tools cost in bytes of tool JSON, as its estimate counts
/// them: a deferred tool is a constant for all of them, the search tool its own.
pub fn tools_bytes(tools: &[Value]) -> u64 {
    let len = |v: &Value| serde_json::to_string(v).map_or(0, |s| s.len() as u64);
    let mut bytes = 0;
    let mut deferred = false;
    for t in tools {
        if t.get("defer_loading") == Some(&Value::Bool(true)) {
            deferred = true;
        } else if t["name"] == SEARCH_TOOL {
            bytes += SEARCH_BYTES;
        } else {
            bytes += len(t);
        }
    }
    bytes + if deferred { DEFERRED_BYTES } else { 0 }
}

/// The tools a request on `spec` would send after the compilation
/// `current` (an append's): its recorded loaded set, or with none the whole
/// catalog; with no compilation the spec's fresh choice. `context.explain`
/// says the parts of the request this way.
pub fn sent_tools(spec: &RequestSpec, current: Option<&crate::compiler::Manifest>) -> Vec<Value> {
    let Some(fresh) = spec.defer.as_ref().filter(|d| !d.resolved) else {
        return spec.tools.to_vec();
    };
    match current {
        None => resolve(&spec.tools, fresh),
        Some(m) => match &m.loaded {
            Some(l) => resolve(
                &spec.tools,
                &Deferral {
                    loaded: l.iter().cloned().collect(),
                    ..fresh.clone()
                },
            ),
            None => spec
                .tools
                .iter()
                .filter(|t| t["name"] != FIND)
                .cloned()
                .collect(),
        },
    }
}

/// The families a spec's request loaded, as its manifest records them.
pub fn loaded_of(spec: &RequestSpec) -> Option<Vec<String>> {
    spec.defer
        .as_ref()
        .map(|d| d.loaded.iter().cloned().collect())
}

/// Whether a tool is listed in the catalog the model is offered: `tools_find`
/// is not, since it joins a request only where something is deferred.
pub fn listed(canonical: &str) -> bool {
    canonical != "tools.find"
}

/// `tools_find`'s definition, as it goes out.
pub fn find_definition() -> Value {
    use theseus_tools::Tool;
    let t = find::FindTool;
    json!({
        "name": FIND,
        "description": t.description(),
        "input_schema": t.input_schema(),
    })
}

fn family_of(def: &Value) -> String {
    def.get("name")
        .and_then(Value::as_str)
        .map(family::of_wire)
        .unwrap_or_else(|| family::OTHER.to_string())
}

/// The families of a catalog's tools.
pub fn families_of(tools: &[Value]) -> BTreeSet<String> {
    tools.iter().map(family_of).collect()
}

/// The tools a request sends: `tools` (the whole catalog the place is
/// offered) with every tool of a family outside `d.loaded` deferred or left
/// out. Nothing is hidden when that would leave nothing loaded (the API
/// refuses a request whose tools are all deferred).
pub fn resolve(tools: &[Value], d: &Deferral) -> Vec<Value> {
    let plain: Vec<&Value> = tools
        .iter()
        .filter(|t| t.get("name").and_then(Value::as_str) != Some(FIND))
        .collect();
    let hidden = |t: &Value| {
        let f = family_of(t);
        family::hideable(&f) && !d.loaded.contains(&f)
    };
    let any_loaded = plain.iter().any(|t| !hidden(t));
    let any_hidden = plain.iter().any(|t| hidden(t));
    if !any_hidden || !any_loaded {
        return plain.into_iter().cloned().collect();
    }
    match d.mechanism {
        Mechanism::Search => {
            let mut out = vec![json!({"type": SEARCH_TYPE, "name": SEARCH_TOOL})];
            for t in plain {
                let mut t = t.clone();
                if hidden(&t) {
                    t["defer_loading"] = Value::Bool(true);
                }
                out.push(t);
            }
            out
        }
        Mechanism::Find => {
            let mut out: Vec<Value> = plain.into_iter().filter(|t| !hidden(t)).cloned().collect();
            out.push(find_definition());
            out.sort_by(|a, b| {
                a.get("name")
                    .and_then(Value::as_str)
                    .cmp(&b.get("name").and_then(Value::as_str))
            });
            out
        }
    }
}

/// `spec` with its tools resolved by `d` (none: whole, `tools_find` out).
fn tooled<'a>(spec: Cow<'a, RequestSpec>, d: Option<&Deferral>) -> Cow<'a, RequestSpec> {
    let mut s = spec.into_owned();
    match d {
        Some(d) => {
            s.tools = s.tools.resolved(d, |all| resolve(all, d));
            s.defer = Some(Deferral {
                resolved: true,
                ..d.clone()
            });
        }
        None => {
            let plain = |t: &&Value| t.get("name").and_then(Value::as_str) != Some(FIND);
            s.tools = s
                .tools
                .iter()
                .filter(plain)
                .cloned()
                .collect::<Vec<_>>()
                .into();
            s.defer = None;
        }
    }
    Cow::Owned(s)
}

/// The two specs a compile renders from: the one an append renders and the
/// one a recompile does (`compile_with`'s `appended` and `recompiled`). An
/// append keeps the walk's recorded memberships and the loaded set the
/// current compilation recorded, so its request is byte for byte the last
/// one's; a recompile (and the clearing and ring passes, which render from
/// it) takes the current memberships and the spec's fresh selection. `None`
/// for the first means the two are one spec.
pub fn forks<'a>(input: &CompileInput<'a>) -> (Option<Cow<'a, RequestSpec>>, Cow<'a, RequestSpec>) {
    let spec = input.spec;
    let (guided_rec, guided_app) = match spec.walk.as_deref() {
        None => (Cow::Borrowed(spec), None),
        Some(walk) => (
            crate::ontology::guided(spec, walk.compose(&walk.current)),
            input.current.map(|c| {
                crate::ontology::guided(
                    spec,
                    walk.compose(&crate::ontology::recorded(&c.manifest.memberships)),
                )
            }),
        ),
    };
    let Some(fresh) = spec.defer.as_ref().filter(|d| !d.resolved) else {
        return (guided_app, guided_rec);
    };
    let app = input.current.map(|c| {
        let sticky = c.manifest.loaded.as_ref().map(|l| Deferral {
            mechanism: fresh.mechanism,
            loaded: l.iter().cloned().collect(),
            resolved: false,
        });
        let base = guided_app.unwrap_or_else(|| guided_rec.clone());
        tooled(base, sticky.as_ref())
    });
    (app, tooled(guided_rec, Some(fresh)))
}

/// `spec` as an append to the compilation of `current` renders it: the
/// families that compilation recorded loading, as `forks` takes them (a
/// turn's request rebuilt, own-benchmark's replay). With deferral off now,
/// `spec` as it is.
pub fn as_recorded(spec: RequestSpec, current: &crate::compiler::Manifest) -> RequestSpec {
    let Some(fresh) = spec.defer.clone().filter(|d| !d.resolved) else {
        return spec;
    };
    let sticky = current.loaded.as_ref().map(|l| Deferral {
        mechanism: fresh.mechanism,
        loaded: l.iter().cloned().collect(),
        resolved: false,
    });
    tooled(Cow::Owned(spec), sticky.as_ref()).into_owned()
}

/// The tools a request hides, for the conversation's notes: those it sends
/// with `defer_loading`, and those of the catalog it leaves out.
pub fn hidden_of(catalog: &[Value], sent: &[Value]) -> Vec<HiddenTool> {
    let sent_names: BTreeSet<&str> = sent
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    let deferred = |name: &str| {
        sent.iter().any(|t| {
            t.get("name").and_then(Value::as_str) == Some(name)
                && t.get("defer_loading") == Some(&Value::Bool(true))
        })
    };
    catalog
        .iter()
        .filter_map(|t| {
            let name = t.get("name").and_then(Value::as_str)?;
            if name == FIND || (sent_names.contains(name) && !deferred(name)) {
                return None;
            }
            let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
            let line = desc.split_inclusive(". ").next().unwrap_or(desc).trim();
            Some(HiddenTool {
                wire: name.to_string(),
                family: family::of_wire(name),
                line: line.chars().take(160).collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
