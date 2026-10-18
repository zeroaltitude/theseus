//! The families a message names, loaded up front for a model without tool
//! search (theseus-jpjcd).
//!
//! A model whose catalog entry has no `tool_search` is offered `tools_find`
//! for the families a request leaves out, and install #29 saw Haiku 5.5 never
//! call it: asked for `aws_whoami` it answered that the tool was not loaded.
//! A fallback the model ignores is no fallback, so for that mechanism the
//! turn's own words decide: a hidden family whose tool the message names, or
//! whose name it says, loads with the request, as far as the place's tool
//! budget allows. The same matching `tools_find` does for the model, applied
//! for it, once a turn (the loaded set still changes only where the
//! conversation recompiles). A model with tool search never reaches this: its
//! first request is as it was.

use std::collections::BTreeSet;

use serde_json::Value;

use super::family::{self, PlaceKind};
use super::find::tokens;
use super::{resolve, tools_bytes, Deferral, HiddenTool, Mechanism};

/// How much of a message is read for the names of tools.
pub const ASK_CHARS: usize = 4000;

/// Family names that are everyday words: saying one is no ask for the tools.
const PLAIN: &[&str] = &[
    "code",
    "self",
    "tasks",
    "reminders",
    "threads",
    "web",
    "memory",
    "files",
    "commands",
    "channels",
    "extensions",
    "plan",
];

/// The hidden families `ask` names, best first: one point for the family's
/// name (a plain word does not count, nor an MCP server's under four
/// letters), two for each tool whose whole wire name (two words or more, as
/// `aws_whoami`) the message says.
pub fn named(ask: &str, hidden: &[HiddenTool]) -> Vec<String> {
    let said: BTreeSet<String> = tokens(ask).into_iter().collect();
    let mut score: std::collections::BTreeMap<&str, u32> = Default::default();
    let mut name_counted: BTreeSet<&str> = BTreeSet::new();
    for h in hidden {
        let own = h.family.strip_prefix("mcp:").unwrap_or(&h.family);
        // A family named by several words (`aws-logs`) is said when each word is.
        let name_says = !PLAIN.contains(&own)
            && own.len() >= 3
            && own
                .split('-')
                .all(|w| said.contains(w) || said.contains(w.trim_end_matches('s')));
        let wire: Vec<String> = tokens(h.wire.strip_prefix("mcp__").unwrap_or(&h.wire));
        let tool_says = wire.len() >= 2 && wire.iter().all(|w| said.contains(w));
        // The name's point is the family's, once, however many tools it holds.
        let by = u32::from(name_says && name_counted.insert(h.family.as_str()))
            + 2 * u32::from(tool_says);
        if by > 0 {
            *score.entry(h.family.as_str()).or_default() += by;
        }
    }
    let mut out: Vec<(&str, u32)> = score.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    out.into_iter().map(|(f, _)| f.to_string()).collect()
}

/// The families to add to `loaded` for a message: those it names that are
/// hidden and hideable, best first, each kept only while the system (`system`
/// tokens) and the request's tools stay within the place kind's budget (the
/// tools estimated as the request does, bytes at 2.4 to the token). Nothing
/// when `ask` names nothing, or the request already exceeds the budget.
pub fn extra(
    ask: &str,
    kind: PlaceKind,
    catalog: &[Value],
    loaded: &BTreeSet<String>,
    system: u64,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if ask.trim().is_empty() {
        return out;
    }
    let hidden: Vec<HiddenTool> = super::hidden_of(catalog, &[])
        .into_iter()
        .filter(|h| family::hideable(&h.family) && !loaded.contains(&h.family))
        .collect();
    let budget = kind.tools_budget_tokens().saturating_sub(system);
    let mut now = loaded.clone();
    for fam in named(ask, &hidden) {
        now.insert(fam.clone());
        let d = Deferral {
            mechanism: Mechanism::Find,
            loaded: now.clone(),
            resolved: false,
        };
        if tools_bytes(&resolve(catalog, &d)) * 10 / 24 <= budget {
            out.insert(fam);
        } else {
            now.remove(&fam);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn h(wire: &str) -> HiddenTool {
        HiddenTool {
            wire: wire.into(),
            family: family::of_wire(wire),
            line: String::new(),
        }
    }

    fn hidden() -> Vec<HiddenTool> {
        vec![
            h("aws_whoami"),
            h("aws_call"),
            h("term_open"),
            h("task_create"),
            h("mcp__docs__search"),
        ]
    }

    #[test]
    fn a_tool_or_a_family_the_message_says_is_named() {
        assert_eq!(named("run aws_whoami for me", &hidden()), ["aws"]);
        assert_eq!(named("who am I on AWS?", &hidden()), ["aws"]);
        assert_eq!(named("open a terminal", &hidden()), ["terminals"]);
        assert_eq!(named("search the docs server", &hidden()), ["mcp:docs"]);
    }

    #[test]
    fn everyday_words_and_one_word_of_a_tool_name_name_nothing() {
        assert!(named("write the code and open the file", &hidden()).is_empty());
        assert!(named("create my tasks and the web page", &hidden()).is_empty());
        // `plan` is an everyday word too: saying it loads nothing, where a
        // model has no tool search (theseus-lyxy).
        let plan = [h("plan_present"), h("plan_step")];
        assert!(named("make a plan for the code", &plan).is_empty());
        assert_eq!(named("plan present the steps", &plan), ["plan"]);
        assert!(named("", &hidden()).is_empty());
    }

    /// The AWS tools are five families (theseus-3msy0): a tool named whole loads its own family first,
    /// and a family's own word with `aws` loads it; saying only `AWS` loads the generic one.
    #[test]
    fn an_aws_tool_or_a_part_of_aws_names_its_own_family() {
        let aws: Vec<HiddenTool> = [
            "aws_call",
            "aws_whoami",
            "aws_s3_list",
            "aws_logs_tail",
            "aws_stack_plan",
            "aws_cost",
            "aws_hands_run",
        ]
        .into_iter()
        .map(h)
        .collect();
        assert_eq!(
            named("read aws_logs_tail for the api group", &aws)[0],
            "aws-logs"
        );
        assert_eq!(named("plan it with aws_stack_plan", &aws)[0], "aws-stacks");
        assert_eq!(named("run aws_hands_run twice", &aws)[0], "aws-hands");
        assert_eq!(named("aws_cost for this month", &aws)[0], "aws-review");
        assert_eq!(named("who am I on AWS?", &aws), ["aws"]);
        // (`aws_s3_list`'s words are all said: its family leads.)
        assert_eq!(named("list my AWS s3 bucket", &aws), ["aws-s3", "aws"]);
        assert_eq!(named("show the stacks on aws", &aws), ["aws", "aws-stacks"]);
        assert!(named("open the s3 file", &aws).is_empty());
    }

    #[test]
    fn the_best_named_family_comes_first() {
        let said = "aws whoami, and open a terminal";
        assert_eq!(named(said, &hidden()), ["aws", "terminals"]);
    }

    fn def(name: &str, pad: usize) -> Value {
        json!({"name": name, "description": "x".repeat(pad), "input_schema": {"type": "object"}})
    }

    #[test]
    fn a_family_that_would_pass_the_places_budget_is_not_added() {
        // The aws tool is about 25 KB, over 8,000 tokens (19 KB) with the rest,
        // under a direct message's 15,000.
        let catalog = vec![def("fs_read", 100), def("aws_call", 25_000)];
        let loaded: BTreeSet<String> = ["files".to_string()].into();
        let ask = "who am I on aws";
        assert!(extra(ask, PlaceKind::Cli, &catalog, &loaded, 0).is_empty());
        assert!(extra(ask, PlaceKind::Task, &catalog, &loaded, 0).is_empty());
        assert_eq!(
            extra(ask, PlaceKind::Dm, &catalog, &loaded, 0),
            ["aws".to_string()].into()
        );
        // A small one fits anywhere, and an already loaded family adds nothing.
        let small = vec![def("fs_read", 100), def("aws_call", 500)];
        assert_eq!(extra(ask, PlaceKind::Cli, &small, &loaded, 0).len(), 1);
        let both: BTreeSet<String> = ["files".to_string(), "aws".to_string()].into();
        assert!(extra(ask, PlaceKind::Cli, &small, &both, 0).is_empty());
    }

    /// The budget is the whole request's: the system's tokens come off it before the tools are counted.
    #[test]
    fn the_systems_tokens_come_off_the_budget() {
        // About 7,300 tokens of tools in all: under 8,000 alone, over it beside a 1,300-token system.
        let catalog = vec![def("fs_read", 100), def("aws_call", 17_300)];
        let loaded: BTreeSet<String> = ["files".to_string()].into();
        let ask = "who am I on aws";
        assert_eq!(extra(ask, PlaceKind::Cli, &catalog, &loaded, 0).len(), 1);
        assert!(extra(ask, PlaceKind::Cli, &catalog, &loaded, 1_300).is_empty());
    }
}
