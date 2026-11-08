//! Every stop, refusal and skip tells the owner (theseus-3i6d1; the owner:
//! "for all stops: let's ALSO notify the user"). One table of the rules that
//! stop, refuse or skip, one text, and one post: a site calls
//! [`Outbox::notify_rule`] with the rule, what happened, the limit and the
//! figure that tripped it. It adds to what each rule does; a refusal still
//! refuses, a skip still skips.
//!
//! The list is kept complete by `every_rule_is_routed`: it reads the crate's
//! source and fails a rule that no site names, unless the rule is in
//! [`EXEMPT`] with its reason.

use serde_json::json;

use crate::outbox::Outbox;
use crate::owner_note::{note, Kind};

/// A rule of the spending table that stops, refuses or skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The hard multiple of a session's limit.
    A4,
    /// Jev's shadow day budget.
    D1,
    /// Self-improvement, per branch.
    D2,
    /// Self-improvement, per day.
    D3,
    /// Minting a tool.
    D4,
    /// Sealing an item.
    D5,
    /// Initiative, per day.
    D6,
    /// Consolidation, per day.
    D7,
    /// The people sweep, per day.
    D8,
    /// The ontology sweep, per day.
    D9,
    /// The judge's learning writer, per day.
    D10,
    /// One judge replay or backfill (an estimate).
    D11,
    /// One judge audit.
    D12,
    /// An MCP-opened session's limit.
    D13,
    /// Routines' run and month.
    D14,
    /// Project reviews' run and month.
    D15,
    /// The owner card job, per day (an estimate).
    D16,
    /// Thread import, per day.
    D17,
    /// Route-bench replay, per day and whole.
    D18,
    /// The AWS month.
    F1,
}

/// What the rule did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Stopped,
    Refused,
    Skipped,
}

impl Outcome {
    fn past(self) -> &'static str {
        match self {
            Outcome::Stopped => "stopped",
            Outcome::Refused => "refused",
            Outcome::Skipped => "skipped",
        }
    }
}

/// One rule's identity: its id, what it limits, its config key.
pub struct Spec {
    pub rule: Rule,
    pub id: &'static str,
    pub what: &'static str,
    pub key: &'static str,
}

const fn s(rule: Rule, id: &'static str, what: &'static str, key: &'static str) -> Spec {
    Spec {
        rule,
        id,
        what,
        key,
    }
}

/// Every rule, once.
pub const RULES: &[Spec] = &[
    s(
        Rule::A4,
        "A4",
        "a session's hard stop",
        "[kernel] spend_limit_hard_multiple",
    ),
    s(
        Rule::D1,
        "D1",
        "Jev's shadow day budget",
        "[judge] shadow_limit_usd_per_day",
    ),
    s(
        Rule::D2,
        "D2",
        "self-improvement's branch budget",
        "[self.budget] branch_usd",
    ),
    s(
        Rule::D3,
        "D3",
        "self-improvement's day budget",
        "[self.budget] day_usd",
    ),
    s(Rule::D4, "D4", "minting a tool", "[self.mint] usd"),
    s(Rule::D5, "D5", "sealing an item", "[self.sealed] item_usd"),
    s(
        Rule::D6,
        "D6",
        "initiative's day budget",
        "[initiative] day_usd",
    ),
    s(
        Rule::D7,
        "D7",
        "consolidation's day budget",
        "[memory] synth_limit_usd_per_day",
    ),
    s(
        Rule::D8,
        "D8",
        "the people sweep's day budget",
        "[people] sweep_usd_per_day",
    ),
    s(
        Rule::D9,
        "D9",
        "the ontology sweep's day budget",
        "[ontology] sweep_usd_per_day",
    ),
    s(
        Rule::D10,
        "D10",
        "the judge's learning writer",
        "[judge.learn] writer_limit_usd_per_day",
    ),
    s(
        Rule::D11,
        "D11",
        "one judge replay or backfill",
        "[judge] replay_limit_usd",
    ),
    s(
        Rule::D12,
        "D12",
        "one judge audit",
        "[judge] audit_limit_usd",
    ),
    s(
        Rule::D13,
        "D13",
        "an MCP-opened session's limit",
        "[mcp_server] spend_limit_usd",
    ),
    s(
        Rule::D14,
        "D14",
        "a routine's budget",
        "[routines] default_run_usd / default_month_usd",
    ),
    s(
        Rule::D15,
        "D15",
        "a project review's budget",
        "[projects] review_run_usd / review_month_usd",
    ),
    s(
        Rule::D16,
        "D16",
        "the owner card job's day budget",
        "[card] day_usd",
    ),
    s(
        Rule::D17,
        "D17",
        "thread import's day budget",
        "[threads] import_day_usd",
    ),
    s(
        Rule::D18,
        "D18",
        "route-bench replay",
        "[route_bench] replay_day_usd / whole_usd",
    ),
    s(
        Rule::F1,
        "F1",
        "the AWS month",
        "[aws.accounts.<id>] monthly_budget_usd",
    ),
];

/// Rules no site notifies from, with why. A rule listed here is argued in
/// the step's report; `every_rule_is_routed` holds each reason non-empty.
pub const EXEMPT: &[(Rule, &str)] = &[
    (
        Rule::D13,
        "its budget is pinned, so the session asks (a question card) instead of notifying; asking is louder",
    ),
    (
        Rule::D5,
        "its stop site, rsi/sealed/relay.rs, is a keel-held file: wiring it waits for the owner's Keel ack",
    ),
    (
        Rule::F1,
        "the stop happens inside AWS Budgets, not in this process; no code path here sees it",
    ),
];

impl Rule {
    pub fn spec(self) -> &'static Spec {
        RULES
            .iter()
            .find(|r| r.rule == self)
            .expect("every rule is in RULES")
    }
}

/// The local day, a scope for a rule that speaks once a day.
pub fn today() -> String {
    crate::owner_note::day(theseus_protocol::now_unix_ms())
}

fn usd(v: f64) -> String {
    format!("${v:.2}")
}

/// The notice: the rule's id and name, what it did, the limit, the figure
/// that tripped it, and the config key to change. `detail` names the thing
/// stopped (a branch, a run), or is empty.
pub fn text(rule: Rule, outcome: Outcome, limit_usd: f64, figure_usd: f64, detail: &str) -> String {
    let sp = rule.spec();
    let title = format!("Rule {} {} {}", sp.id, outcome.past(), sp.what);
    let trigger = format!(
        "{} against a limit of {} (`{}`)",
        usd(figure_usd),
        usd(limit_usd),
        sp.key
    );
    let body = if detail.is_empty() {
        "Nothing else changed: the rule did what it always does, and this tells you.".to_string()
    } else {
        format!("{detail}\nThe rule did what it always does; this tells you.")
    };
    note(Kind::Alert, &title, &trigger, &body)
}

impl Outbox {
    /// Tell the owner a rule stopped, refused or skipped, once for `scope`
    /// (a day, a branch, a session: the caller's word), so a rule that trips
    /// on every call speaks once. Returns whether a post was written. A post
    /// that cannot be written is logged and never changes what the rule did.
    #[allow(clippy::too_many_arguments)]
    pub fn notify_rule(
        &self,
        session: Option<&str>,
        rule: Rule,
        outcome: Outcome,
        limit_usd: f64,
        figure_usd: f64,
        scope: &str,
        detail: &str,
    ) -> bool {
        if !self.rule_first(rule, scope) {
            return false;
        }
        let body = json!({
            "kind": "spend_ceiling",
            "text": text(rule, outcome, limit_usd, figure_usd, detail),
        });
        match self.to_operator(session, body) {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!(rule = rule.spec().id, error = %format!("{e:#}"), "rule notice was not written");
                false
            }
        }
    }
}

impl Outbox {
    /// A judge replay or backfill refused on its estimate (D11): told every
    /// time, since each is an operator's own act.
    pub fn notify_estimate_refused(&self, what: &str, limit_usd: f64, estimate_usd: f64) {
        let scope = format!("{what}:{}", theseus_protocol::now_unix_ms());
        let detail = format!("The {what} was refused on its estimate before anything was sent.");
        self.notify_rule(
            None,
            Rule::D11,
            Outcome::Refused,
            limit_usd,
            estimate_usd,
            &scope,
            &detail,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> Vec<(std::path::PathBuf, String)> {
        let mut out = Vec::new();
        let mut todo = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    todo.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push((p.clone(), std::fs::read_to_string(&p).unwrap()));
                }
            }
        }
        out
    }

    #[test]
    fn the_text_names_the_rule_and_the_figure() {
        let t = text(Rule::D1, Outcome::Skipped, 2.0, 2.31, "a judgment");
        assert!(t.contains("Rule D1 skipped"), "{t}");
        assert!(t.contains("$2.31") && t.contains("$2.00"), "{t}");
        assert!(t.contains("shadow_limit_usd_per_day"), "{t}");
        assert!(crate::owner_note::is_headed(&t), "{t}");
    }

    #[test]
    fn every_rule_is_routed() {
        assert_eq!(
            RULES.len(),
            20,
            "the table has every id of the spending table"
        );
        let files = sources();
        let mut routed = 0;
        for sp in RULES {
            if let Some((_, why)) = EXEMPT.iter().find(|(r, _)| *r == sp.rule) {
                assert!(why.len() > 20, "{} is exempt without a reason", sp.id);
                continue;
            }
            let needle = format!("Rule::{:?}", sp.rule);
            let sites = files
                .iter()
                .filter(|(p, _)| !p.ends_with("rule_notice.rs"))
                .filter(|(p, t)| {
                    let _ = p;
                    // D11 goes through its one helper, which names the rule once.
                    (t.contains("notify_rule(") && t.contains(&needle))
                        || (sp.rule == Rule::D11 && t.contains("notify_estimate_refused("))
                })
                .count();
            assert!(
                sites > 0,
                "rule {} ({}) has no notify_rule site",
                sp.id,
                sp.what
            );
            routed += 1;
        }
        assert_eq!(routed, RULES.len() - EXEMPT.len());
    }
}
