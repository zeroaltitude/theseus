//! The families a message names, loaded up front for a model without tool
//! search (theseus-jpjcd).
//!
//! A model whose catalog entry has no `tool_search` is offered `tools_find`
//! for the families a request leaves out, and install #29 saw Haiku 5.5 never
//! call it: asked for `aws_whoami` it answered that the tool was not loaded.
//! A fallback the model ignores is no fallback, so for that mechanism the
//! turn's own words decide: a hidden family whose tool the message names, or
//! whose name it says, loads with the request, as far as the place's budget
//! for the whole first request allows: the system, what deferring costs, the
//! families already loaded and this one (`Fit`, theseus-3bs4e), and since
//! theseus-xlri5 past it by exactly the admitted families' size, under the
//! ceiling; Jev's judgment of the prompt (`families.v1`) admits the families it
//! finds relevant after the named ones. The same matching `tools_find` does for the model, applied
//! for it, once a turn (the loaded set still changes only where the
//! conversation recompiles). A model with tool search never reaches this: its
//! first request is as it was.

use std::collections::BTreeSet;

use serde_json::Value;

use super::family;
use super::find::tokens;
use super::fit::Fit;
use super::HiddenTool;

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

/// Words that start or make a task: "start two background tasks", not "my tasks today".
const MAKES: &[&str] = &[
    "start", "create", "make", "launch", "spawn", "begin", "kick", "new",
];

/// How many words may stand between the verb and "task".
const GAP: usize = 4;

/// Whether the message asks for a task to be made (theseus-gg78p): "task" or "tasks" within a few
/// words after a verb that starts one, or "background task(s)". The `tasks` family is the DM's
/// deferred one (the owner's rule), so a model that never searches takes `proc.run`'s background
/// job for it. Both kinds of model pass through this (`extra`; the deferral's search path).
pub fn asks_for_task(ask: &str) -> bool {
    let words = tokens(&ask.chars().take(ASK_CHARS).collect::<String>());
    let is_task = |w: &String| w == "task" || w == "tasks";
    words.iter().enumerate().any(|(i, w)| {
        is_task(w)
            && (i > 0 && words[i - 1] == "background"
                || words[i.saturating_sub(GAP + 1)..i]
                    .iter()
                    .any(|v| MAKES.contains(&v.as_str())))
    })
}

/// Who admitted a family past the fresh choice (theseus-xlri5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    /// The message asks for a task (`asks_for_task`).
    Asked,
    /// The message names it (`named`).
    Named,
    /// Jev judged it relevant to the message (`families.v1`).
    Jev,
    /// A static cue admitted it (theseus-z9mze.4: the fan-out cue, `turn/route_step/fan_out_cue.rs`).
    Cue,
}

impl By {
    pub fn as_str(self) -> &'static str {
        match self {
            By::Asked => "asked",
            By::Named => "named",
            By::Jev => "jev",
            By::Cue => "cue",
        }
    }
}

/// A family admitted for a message: what its tools cost, and who admitted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub family: String,
    pub tokens: u64,
    pub by: By,
}

/// The families to add to `loaded` for a message, in order: those `ask` names
/// (best first), then those Jev judged relevant (`jev`, most confident first),
/// each hidden and hideable. Each is kept while the whole first request stays
/// at or under `fit.ceiling` (theseus-xlri5): the cap is soft by exactly the
/// admitted families' size. A family that does not fit is skipped and a
/// smaller one after it is still tried. Nothing when the request without them
/// already exceeds the place's budget (`fit.budget`), or nothing is asked.
pub fn extra(
    ask: &str,
    jev: &[(String, f64)],
    catalog: &[Value],
    loaded: &BTreeSet<String>,
    fit: &Fit,
) -> Vec<Admitted> {
    let mut out = Vec::new();
    if fit.total(loaded) > fit.budget {
        return out;
    }
    let hidden: Vec<HiddenTool> = super::hidden_of(catalog, &[])
        .into_iter()
        .filter(|h| family::hideable(&h.family) && !loaded.contains(&h.family))
        .collect();
    let mut judged: Vec<&(String, f64)> = jev
        .iter()
        .filter(|(f, _)| hidden.iter().any(|h| &h.family == f))
        .collect();
    judged.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let named = match ask.trim().is_empty() {
        true => Vec::new(),
        false => named(ask, &hidden),
    };
    let asked = asks_for_task(ask)
        .then(|| family::TASKS.to_string())
        .filter(|f| hidden.iter().any(|h| &h.family == f));
    let order = asked
        .into_iter()
        .map(|f| (f, By::Asked))
        .chain(named.into_iter().map(|f| (f, By::Named)))
        .chain(judged.into_iter().map(|(f, _)| (f.clone(), By::Jev)));
    let mut now = loaded.clone();
    for (fam, by) in order {
        if now.contains(&fam) || !fits(fit, &now, &fam) {
            continue;
        }
        now.insert(fam.clone());
        let tokens = fit.tokens(&fam);
        out.push(Admitted {
            family: fam,
            tokens,
            by,
        });
    }
    out
}

/// The family a cue admits (theseus-z9mze.4), held to the same ceiling as every other admission:
/// nothing when it is not hideable, is already loaded, is not in the request's tools, or does not fit.
pub fn cued(
    family: &str,
    catalog: &[Value],
    loaded: &BTreeSet<String>,
    fit: &Fit,
) -> Option<Admitted> {
    let hidden = super::hidden_of(catalog, &[])
        .into_iter()
        .any(|h| h.family == family);
    (family::hideable(family) && hidden && !loaded.contains(family) && fits(fit, loaded, family))
        .then(|| Admitted {
            family: family.to_string(),
            tokens: fit.tokens(family),
            by: By::Cue,
        })
}

/// Whether `family` joins `loaded` with the whole first request still at or
/// under the ceiling: the one check of every family a message admits
/// (theseus-s48k6).
pub fn fits(fit: &Fit, loaded: &BTreeSet<String>, family: &str) -> bool {
    let mut with = loaded.clone();
    with.insert(family.to_string());
    fit.total(&with) <= fit.ceiling
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools_defer::PlaceKind;
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
    fn a_message_asks_for_a_task_only_with_a_verb_that_makes_one() {
        for yes in [
            "Start two background tasks, $1 each: A) bash x.sh; B) y.sh",
            "please create a task to tidy the notes",
            "make me a new task for the release",
            "can you launch a background task?",
            "spawn 3 tasks",
        ] {
            assert!(asks_for_task(yes), "{yes}");
        }
        for no in [
            "what tasks are open?",
            "my tasks today are mostly email",
            "list the tasks you did yesterday",
            "start the server, then check the task board later on in the afternoon",
            "run a background job",
            "",
        ] {
            assert!(!asks_for_task(no), "{no}");
        }
    }

    /// theseus-z9mze.4: the cue is a fourth way in, and the other three admit as before.
    #[test]
    fn a_cue_is_a_fourth_way_in_and_the_other_three_admit_as_before() {
        let catalog = vec![
            def("fs_read", 100),
            def("aws_whoami", 600),
            def("task_create", 600),
            def("memory_lookup", 300),
            def("task_fan_out", 600),
        ];
        let loaded: BTreeSet<String> = ["files".to_string()].into();
        let mut fit = fit_of(PlaceKind::Dm, &catalog, 0);
        fit.ceiling = 15_000;
        let got = super::extra(
            "start a background task, then run aws_whoami",
            &[("memory".to_string(), 0.9)],
            &catalog,
            &loaded,
            &fit,
        );
        let by: Vec<(&str, By)> = got.iter().map(|a| (a.family.as_str(), a.by)).collect();
        assert_eq!(
            by,
            [
                ("tasks", By::Asked),
                ("aws", By::Named),
                ("memory", By::Jev)
            ]
        );
        assert_eq!(
            [By::Asked, By::Named, By::Jev, By::Cue].map(By::as_str),
            ["asked", "named", "jev", "cue"]
        );
        // The cue admits the helpers family, by its own word, under the same ceiling.
        let c = cued(family::HELPERS, &catalog, &loaded, &fit).unwrap();
        assert_eq!((c.family.as_str(), c.by), ("helpers", By::Cue));
        assert_eq!(c.tokens, fit.tokens("helpers"));
        // Nothing for a family already loaded, one the request lacks, one never hidden, or one
        // that does not fit.
        let with: BTreeSet<String> = ["files".to_string(), "helpers".to_string()].into();
        assert!(cued(family::HELPERS, &catalog, &with, &fit).is_none());
        assert!(cued(family::HELPERS, &catalog[..4], &loaded, &fit).is_none());
        assert!(cued(family::OTHER, &catalog, &loaded, &fit).is_none());
        fit.ceiling = 1;
        assert!(cued(family::HELPERS, &catalog, &loaded, &fit).is_none());
    }

    #[test]
    fn the_best_named_family_comes_first() {
        let said = "aws whoami, and open a terminal";
        assert_eq!(named(said, &hidden()), ["aws", "terminals"]);
    }

    fn def(name: &str, pad: usize) -> Value {
        json!({"name": name, "description": "x".repeat(pad), "input_schema": {"type": "object"}})
    }

    /// The families the named rule alone admits (no Jev), as a set: the tests before theseus-xlri5.
    fn extra(
        ask: &str,
        catalog: &[Value],
        loaded: &BTreeSet<String>,
        fit: &Fit,
    ) -> BTreeSet<String> {
        super::extra(ask, &[], catalog, loaded, fit)
            .into_iter()
            .map(|a| a.family)
            .collect()
    }

    /// The fit of a request whose tools are `catalog`, at the request's own figures (2.4 bytes to the
    /// token), beside a system of `system` tokens. Its ceiling is the budget (`admit_ceiling = 1.0`,
    /// the cap before theseus-xlri5), which the tests written before it hold.
    fn fit_of(kind: PlaceKind, catalog: &[Value], system: u64) -> Fit {
        let mut cost = std::collections::BTreeMap::new();
        for t in catalog {
            let bytes = serde_json::to_string(t).unwrap().len() as u64;
            *cost
                .entry(family::of_wire(t["name"].as_str().unwrap()))
                .or_default() += (bytes * 10).div_ceil(24);
        }
        Fit {
            budget: kind.tools_budget_tokens(),
            ceiling: kind.tools_budget_tokens(),
            raised: 0,
            system,
            held: 0,
            cost,
        }
    }

    #[test]
    fn a_family_that_would_pass_the_places_budget_is_not_added() {
        // The aws tool is about 25 KB, over 8,000 tokens (19 KB) with the rest,
        // under a direct message's 15,000.
        let catalog = vec![def("fs_read", 100), def("aws_call", 25_000)];
        let loaded: BTreeSet<String> = ["files".to_string()].into();
        let ask = "who am I on aws";
        let at = |kind| extra(ask, &catalog, &loaded, &fit_of(kind, &catalog, 0));
        assert!(at(PlaceKind::Cli).is_empty());
        assert!(at(PlaceKind::Task).is_empty());
        assert_eq!(at(PlaceKind::Dm), ["aws".to_string()].into());
        // A small one fits anywhere, and an already loaded family adds nothing.
        let small = vec![def("fs_read", 100), def("aws_call", 500)];
        let fit = fit_of(PlaceKind::Cli, &small, 0);
        assert_eq!(extra(ask, &small, &loaded, &fit).len(), 1);
        let both: BTreeSet<String> = ["files".to_string(), "aws".to_string()].into();
        assert!(extra(ask, &small, &both, &fit).is_empty());
    }

    /// The budget is the whole request's: the system's tokens come off it before the tools are counted.
    #[test]
    fn the_systems_tokens_come_off_the_budget() {
        // About 7,300 tokens of tools in all: under 8,000 alone, over it beside a 1,300-token system.
        let catalog = vec![def("fs_read", 100), def("aws_call", 17_300)];
        let loaded: BTreeSet<String> = ["files".to_string()].into();
        let ask = "who am I on aws";
        let at = |system| {
            extra(
                ask,
                &catalog,
                &loaded,
                &fit_of(PlaceKind::Cli, &catalog, system),
            )
        };
        assert_eq!(at(0).len(), 1);
        assert!(at(1_300).is_empty());
    }

    /// theseus-3bs4e: the budget is the whole first request's. A family whose tools fit the budget alone
    /// is not added beside the system and the families already loaded; a smaller named one that still
    /// fits is, whatever its rank.
    #[test]
    fn a_named_family_is_counted_on_the_whole_request() {
        // The aws family is about 7,250 tokens and the terminals' about 410; `files` is loaded (100).
        let catalog = vec![
            def("fs_read", 100),
            def("aws_call", 17_300),
            def("term_open", 900),
        ];
        let loaded: BTreeSet<String> = ["files".to_string()].into();
        let ask = "who am I on aws, and open a terminal";
        // With no system, both fit the cli's 8,000.
        let alone = fit_of(PlaceKind::Cli, &catalog, 0);
        let both = ["aws".to_string(), "terminals".to_string()].into();
        assert_eq!(extra(ask, &catalog, &loaded, &alone), both);
        // The tools alone fit; beside a 1,300-token system the best-ranked family does not, so it is
        // skipped and the smaller one is tried and kept.
        let beside = fit_of(PlaceKind::Cli, &catalog, 1_300);
        assert_eq!(
            extra(ask, &catalog, &loaded, &beside),
            ["terminals".to_string()].into()
        );
        // With a system that leaves room for neither, nothing is added.
        let full = fit_of(PlaceKind::Cli, &catalog, 7_800);
        assert!(extra(ask, &catalog, &loaded, &full).is_empty());
    }

    /// theseus-xlri5: named families first, then Jev's, most confident first; each is kept while
    /// the whole stays under the ceiling, so one past it is skipped and a smaller one after it still
    /// loads; a base already over the budget admits nothing, and a family the message names and Jev
    /// judged is admitted once, as named.
    #[test]
    fn admission_stops_at_the_ceiling() {
        // files ~60, aws ~2,550, terminals ~3,800, tasks ~6,300, the mcp server's ~420 tokens.
        let catalog = vec![
            def("fs_read", 100),
            def("aws_whoami", 6_000),
            def("term_open", 9_000),
            def("task_create", 15_000),
            def("mcp__docs__search", 900),
        ];
        let loaded: BTreeSet<String> = ["files".to_string()].into();
        let mut fit = fit_of(PlaceKind::Cli, &catalog, 6_000);
        fit.ceiling = 16_000;
        let jev = vec![
            ("tasks".to_string(), 0.95),
            ("mcp:docs".to_string(), 0.70),
            ("aws".to_string(), 0.80),
        ];
        let got = super::extra(
            "run aws_whoami and open a terminal",
            &jev,
            &catalog,
            &loaded,
            &fit,
        );
        let fams: Vec<(&str, By)> = got.iter().map(|a| (a.family.as_str(), a.by)).collect();
        // 6,060 + aws 2,550 + terminals 3,800 is 12,400: tasks (6,300) would pass 16,000 and is
        // skipped, the docs server's after it fits; aws, named, is not admitted again as Jev's.
        assert_eq!(
            fams,
            [
                ("aws", By::Named),
                ("terminals", By::Named),
                ("mcp:docs", By::Jev)
            ]
        );
        assert_eq!(got[0].tokens, fit.tokens("aws"));
        let mut all = loaded.clone();
        all.extend(got.iter().map(|a| a.family.clone()));
        assert!(fit.total(&all) <= fit.ceiling);
        // Jev's alone, with nothing said: by confidence, under the same ceiling.
        let judged = super::extra("", &jev, &catalog, &loaded, &fit);
        assert_eq!(judged[0].family, "tasks");
        assert!(judged.iter().all(|a| a.by == By::Jev));
        // A family Jev judged that the catalog does not hide is no admission.
        let loaded_aws: BTreeSet<String> = ["files".to_string(), "aws".to_string()].into();
        let none = super::extra("", &[("aws".into(), 0.9)], &catalog, &loaded_aws, &fit);
        assert!(none.is_empty());
        // A base over the budget admits nothing, however high the ceiling.
        let over = fit_of(PlaceKind::Cli, &catalog, 8_100);
        let over = Fit {
            ceiling: 16_000,
            ..over
        };
        assert!(super::extra("run aws_whoami", &jev, &catalog, &loaded, &over).is_empty());
    }
}
