//! `tools_find`, the fallback where the provider has no tool search
//! (theseus-7m7w.2), and the words of a miss.
//!
//! A model without `defer_loading` sees only the loaded families. `tools_find
//! {query}` tells it the hidden families that match, with their tools, and
//! asks for them to load: they join the turn's tools at its next request
//! (`Board::take_found`, theseus-342qy), so the model calls a found tool in the
//! turn that found it. That request is a recompile (the tools lead the cached
//! prefix, so it misses the cache either way), and the conversation keeps the
//! families loaded after it.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::{Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput};

use super::board::{Board, HiddenTool};
use super::{family, Mechanism, FIND, SEARCH_TOOL};

pub struct FindTool;

impl Tool for FindTool {
    fn name(&self) -> &'static str {
        "tools.find"
    }
    fn description(&self) -> &'static str {
        "Find tools that are not loaded here. Some families of tools are left out of this conversation to keep its requests small; give a few words for what you need to do (\"aws bucket\", \"open a terminal\") and the answer names the matching families and their tools. They load at once: call a tool the answer names in your next step."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "What you need to do, in a few words."}
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let q = query_of(input)?;
        Ok(Plan {
            summary: format!("find tools: {q}"),
            ..Default::default()
        })
    }
    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> theseus_tools::AsyncRun {
        Box::pin(std::future::ready(Err(ToolFailure::new(
            "tools.find runs through the runtime, which knows the conversation's tools",
        ))))
    }
}

fn query_of(input: &Value) -> Result<String, String> {
    let q = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or("give a query: a few words for what you need to do")?;
    Ok(q.chars().take(200).collect())
}

pub(super) fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(str::to_string)
        .collect()
}

/// The hidden families, best match first, with the tools of each: the
/// families whose name, summary, tools' names or lines share a word with the
/// query, most shared first. A family no word matches is not listed.
pub fn matches(query: &str, hidden: &[HiddenTool]) -> Vec<(String, Vec<HiddenTool>)> {
    let q = tokens(query);
    let mut by: BTreeMap<String, Vec<HiddenTool>> = BTreeMap::new();
    for h in hidden {
        by.entry(h.family.clone()).or_default().push(h.clone());
    }
    let mut scored: Vec<(usize, String, Vec<HiddenTool>)> = by
        .into_iter()
        .filter_map(|(fam, tools)| {
            let mut text = fam.clone();
            text.push(' ');
            text.push_str(&family::summary(&fam).unwrap_or_default());
            for t in &tools {
                text.push(' ');
                text.push_str(&t.wire);
                text.push(' ');
                text.push_str(&t.line);
            }
            let have = tokens(&text);
            let score = q.iter().filter(|w| have.contains(w)).count();
            (score > 0).then_some((score, fam, tools))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, f, t)| (f, t)).collect()
}

/// The answer to a `tools_find`, and the families it asks for.
pub fn answer(query: &str, hidden: &[HiddenTool]) -> (String, Vec<String>, Vec<String>) {
    let found = matches(query, hidden);
    if found.is_empty() {
        let mut fams: Vec<String> = hidden.iter().map(|h| h.family.clone()).collect();
        fams.sort();
        fams.dedup();
        let list = fams
            .iter()
            .map(|f| format!("- {f}: {}", family::summary(f).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n");
        let text = if fams.is_empty() {
            format!("No tools are left out of this conversation, so none match {query:?}.")
        } else {
            format!("No left-out tool matches {query:?}. The families left out:\n{list}")
        };
        return (text, vec![], vec![]);
    }
    let mut text = format!(
        "Left-out tools matching {query:?}. They are loaded now: call the tool you need in your \
         next step.\n"
    );
    let (mut fams, mut tools) = (vec![], vec![]);
    for (fam, ts) in found.into_iter().take(3) {
        text.push_str(&format!(
            "\n{fam}: {}\n",
            family::summary(&fam).unwrap_or_default()
        ));
        for t in &ts {
            text.push_str(&format!("  - {}: {}\n", t.wire, t.line));
            tools.push(t.wire.clone());
        }
        fams.push(fam);
    }
    (text, fams, tools)
}

impl Board {
    /// A `tools_find` call for a conversation: what matches among the tools
    /// its request hides, and the families asked for.
    pub fn find(&self, session: &str, input: &Value) -> Result<ToolOutput, ToolFailure> {
        let query = query_of(input).map_err(ToolFailure::new)?;
        let hidden = self.hidden(session);
        let (text, fams, tools) = answer(&query, &hidden);
        if !fams.is_empty() {
            self.wants(session, &query, &fams, &tools);
        }
        Ok(ToolOutput {
            text,
            meta: Value::Null,
        })
    }
}

impl Board {
    /// `find` as the runtime runs an async tool.
    pub fn run(&self, session: &str, input: &Value) -> theseus_tools::AsyncMediaRun {
        let r = self.find(session, input).map(|o| (o, None, None));
        Box::pin(std::future::ready(r))
    }
}

/// The answer to a call to a tool the request does not load, which was not
/// run: it names the search, so the model's next move is to find the tool.
pub fn miss_words(h: &HiddenTool, mechanism: Option<Mechanism>) -> String {
    let fam = family::summary(&h.family).unwrap_or_default();
    match mechanism {
        Some(Mechanism::Find) => format!(
            "{} is not loaded in this conversation (the {} family: {fam}). Call {FIND} \
             with a few words for what you need; the family loads at once, and \
             you can call the tool in your next step",
            h.wire, h.family
        ),
        _ => format!(
            "{} is not loaded in this conversation (the {} family: {fam}). Search for \
             it first: call {SEARCH_TOOL} with a few words naming what you need ({}), then call \
             the tool it finds",
            h.wire, h.family, h.line
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(wire: &str, line: &str) -> HiddenTool {
        HiddenTool {
            wire: wire.into(),
            family: family::of_wire(wire),
            line: line.into(),
        }
    }

    fn hidden() -> Vec<HiddenTool> {
        vec![
            h(
                "aws_call",
                "Make any AWS operation, S3 buckets too: a read, a write, or a run.",
            ),
            h("aws_s3_list", "List the objects of a bucket."),
            h("term_open", "Open a terminal running a program."),
            h("mcp__docs__search", "Search the docs."),
        ]
    }

    #[test]
    fn the_best_matching_family_comes_first_with_its_tools() {
        let m = matches("list my aws bucket", &hidden());
        // The family that holds the bucket tools, then the generic one (theseus-3msy0).
        assert_eq!(m[0].0, "aws-s3");
        assert_eq!(m[1].0, "aws");
        assert_eq!(m[0].1.len() + m[1].1.len(), 2);
        let m = matches("open a terminal", &hidden());
        assert_eq!(m[0].0, "terminals");
        assert!(matches("zzz", &hidden()).is_empty());
    }

    #[test]
    fn an_answer_says_when_the_tools_load_and_asks_for_the_family() {
        let (text, fams, tools) = answer("s3 bucket", &hidden());
        assert_eq!(fams, ["aws-s3", "aws"]);
        assert_eq!(tools, ["aws_s3_list", "aws_call"]);
        assert!(
            text.contains("loaded now") && !text.contains("compaction"),
            "{text}"
        );
        assert!(text.contains("aws_s3_list"));
    }

    #[test]
    fn no_match_lists_the_families_left_out() {
        let (text, fams, _) = answer("zzz", &hidden());
        assert!(fams.is_empty());
        assert!(
            text.contains("- aws:") && text.contains("- terminals:"),
            "{text}"
        );
        let (none, _, _) = answer("aws", &[]);
        assert!(none.contains("No tools are left out"));
    }

    #[test]
    fn the_words_of_a_miss_name_the_search_of_the_mechanism() {
        let t = h("aws_call", "Make any AWS operation.");
        let native = miss_words(&t, Some(Mechanism::Search));
        assert!(
            native.contains("is not loaded") && native.contains(SEARCH_TOOL),
            "{native}"
        );
        let find = miss_words(&t, Some(Mechanism::Find));
        assert!(find.contains(FIND) && !find.contains(SEARCH_TOOL), "{find}");
    }

    #[test]
    fn a_find_through_the_board_records_the_wish() {
        let b = Board::default();
        b.note_request(
            "s",
            family::PlaceKind::Cli,
            Some(Mechanism::Find),
            Default::default(),
            hidden(),
        );
        let out = b.find("s", &json!({"query": "aws bucket"})).unwrap();
        assert!(out.text.contains("aws_call"));
        let known = ["files", "aws"].iter().map(|s| s.to_string()).collect();
        let sel = b.select(
            "s",
            family::PlaceKind::Cli,
            &crate::config::defer::DeferConfig::default(),
            0,
            &crate::tools_defer::board::Offer {
                known: &known,
                extra: &Default::default(),
                fit: None,
            },
        );
        assert!(sel.loaded().contains("aws"));
        assert!(b.find("s", &json!({})).is_err());
    }

    #[test]
    fn a_find_leaves_its_families_for_the_next_loop_to_take_once() {
        let b = Board::default();
        b.note_request(
            "s",
            family::PlaceKind::Cli,
            Some(Mechanism::Find),
            Default::default(),
            hidden(),
        );
        assert!(b.take_found("s").is_empty());
        b.find("s", &json!({"query": "open a terminal"})).unwrap();
        assert_eq!(
            b.take_found("s").into_iter().collect::<Vec<_>>(),
            ["terminals"]
        );
        // Taken: the loop after it finds nothing more, and another conversation never had any.
        assert!(b.take_found("s").is_empty());
        assert!(b.take_found("other").is_empty());
    }
}
