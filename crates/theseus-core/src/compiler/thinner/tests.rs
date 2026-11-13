//! `thinner`'s own tests (theseus-3nk): every cause of the table in its
//! header places its drop, and a drop no cause places is named in the
//! report. Pure: nodes are built in memory, and no clock is read.

use std::sync::Arc;

use theseus_protocol::memory::{BudgetDrop, BudgetRange, BudgetReport};

use super::*;
use crate::compiler::{Compilation, Manifest, COMPILATION_SCHEMA};
use crate::node::{Body, Node};

fn user(text: &str) -> Node {
    Node::user("ses_1", Some("turn_1"), "cli", text)
}

fn summary(first: u64, last: u64) -> Node {
    Node::summary(
        "ses_1",
        "turn_9",
        Body::Summary {
            first,
            last,
            nodes: (last.saturating_sub(first) + 1) as u32,
            text: "the gist".into(),
            profile: "glm".into(),
            model: "glm-5.3-flash".into(),
            cost_usd: None,
            header: "[Summary of 2 earlier messages]".into(),
        },
    )
}

fn moved(of: &[&str], first: u64, last: u64) -> Node {
    Node::harness_body(
        "mov",
        "ses_1",
        "harness",
        Body::Moved {
            split: "spl_1".into(),
            undoes: None,
            thread: "gauges".into(),
            to_session: "ses_g".into(),
            nodes: of.iter().map(|s| s.to_string()).collect(),
            first,
            last,
            gist: "tides".into(),
            quiet: false,
        },
    )
}

fn stubbed(nodes: &[Node]) -> Vec<(u64, Stub)> {
    nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let p = i as u64 + 1;
            (p, Stub::hydrated(p, Arc::new(n.clone())))
        })
        .collect()
}

/// A compilation holding `includes`, with `strategy`.
fn compilation(strategy: &str, includes: &[&str]) -> Compilation {
    Compilation {
        id: format!("cmp_{strategy}_{}", includes.len()),
        schema: COMPILATION_SCHEMA,
        session_id: "ses_1".into(),
        created_at_ms: 0,
        trigger: "new_session".into(),
        strategy: strategy.into(),
        as_of: 0,
        includes: includes.iter().map(|s| s.to_string()).collect(),
        derived_from: None,
        manifest: Manifest {
            compiler_version: 1,
            renderer_version: 1,
            profile: "p".into(),
            provider: "anthropic".into(),
            model: "m".into(),
            system_digest: "d".into(),
            tools_digest: "t".into(),
            tools: Vec::new(),
            catalog_version: "c".into(),
            context_window: None,
            strip_thinking: false,
            context_files: Vec::new(),
            cache: None,
            memberships: Vec::new(),
            guidance: Vec::new(),
            standing: Vec::new(),
            loaded: None,
            prefix: None,
        },
        budget: None,
        recall_id: None,
        situation: None,
        inherits: None,
    }
}

fn ids(nodes: &[Node]) -> Vec<&str> {
    nodes.iter().map(|n| n.id.as_str()).collect()
}

fn report(dropped: Vec<BudgetDrop>) -> BudgetReport {
    BudgetReport {
        limit_tokens: 1000,
        used_tokens: 100,
        dropped,
        overage: None,
    }
}

fn node_drop(id: &str, reason: &str, tier: &str) -> BudgetDrop {
    BudgetDrop {
        node_id: Some(id.into()),
        range: None,
        reason: reason.into(),
        tokens: 7,
        tier: tier.into(),
    }
}

/// A prefix that holds everything its predecessor held is no thinning, and
/// the check costs nothing: `read` answers `None`.
#[test]
fn a_prefix_that_dropped_nothing_says_nothing() {
    let ns = vec![user("a"), user("b"), user("c")];
    let nodes = stubbed(&ns);
    let all = ids(&ns);
    let before = compilation("transcript", &all);
    let now = compilation("transcript", &all);
    let moves = crate::moved::read(&nodes);
    assert_eq!(read(&before, &now, &report(vec![]), &nodes, &moves), None);
}

/// The ring's cut places every node of its range, and the recall's own drop
/// places the node it names: a report that accounts for its whole cut adds
/// nothing to itself.
#[test]
fn the_rings_range_and_a_recalls_node_place_their_drops() {
    let ns = vec![user("a"), user("b"), user("c"), user("d")];
    let nodes = stubbed(&ns);
    let all = ids(&ns);
    let before = compilation("transcript", &all);
    // The ring cut a and b; the recall's own item, d, did not fit.
    let now = compilation("transcript", &[all[2]]);
    let cut = BudgetDrop {
        node_id: None,
        range: Some(BudgetRange {
            first: all[0].into(),
            last: all[1].into(),
            nodes: 2,
        }),
        reason: "overflow".into(),
        tokens: 40,
        tier: "ring".into(),
    };
    let mut r = report(vec![cut, node_drop(all[3], "budget", "recall")]);
    let moves = crate::moved::read(&nodes);
    let t = read(&before, &now, &r, &nodes, &moves).expect("three were dropped");
    assert_eq!((t.dropped, t.accounted, t.unaccounted), (3, 3, 0));
    assert!(t.whole() && t.names.is_empty());
    let was = r.dropped.len();
    say(&mut r, &t, &now.id);
    assert_eq!(r.dropped.len(), was, "a whole accounting writes no drop");
    assert_eq!(unaccounted(&r), 0);
}

/// A drop nothing in the record places is named in the report, as reason
/// `unaccounted` of tier `thinner`, beside the drops that were placed: the
/// positive count is asserted too, so an empty predecessor cannot pass.
#[test]
fn a_drop_nothing_places_is_named_in_the_report() {
    let ns = vec![user("a"), user("b"), user("c")];
    let nodes = stubbed(&ns);
    let all = ids(&ns);
    let before = compilation("transcript", &all);
    // c is gone with nothing to show for it; b's drop is recorded.
    let now = compilation("transcript", &[all[0]]);
    let mut r = report(vec![node_drop(all[1], "budget", "recall")]);
    let moves = crate::moved::read(&nodes);
    let t = read(&before, &now, &r, &nodes, &moves).expect("two were dropped");
    assert_eq!((t.dropped, t.accounted, t.unaccounted), (2, 1, 1));
    assert!(!t.whole());
    assert_eq!(t.names, vec![all[2].to_string()]);
    say(&mut r, &t, &now.id);
    assert_eq!(unaccounted(&r), 1);
    let named = r
        .dropped
        .iter()
        .find(|d| d.reason == UNACCOUNTED)
        .expect("the drop is in the report");
    assert_eq!(named.node_id.as_deref(), Some(all[2]));
    assert_eq!(named.tier, TIER);
    // The recall's own drop is still there, unchanged.
    assert_eq!(r.dropped.iter().filter(|d| d.tier == "recall").count(), 1);
}

/// `fresh` is the operator's own start-over: every drop is placed by the
/// strategy, and the report says nothing extra.
#[test]
fn fresh_places_every_drop_by_the_operators_own_ask() {
    let ns = vec![user("a"), user("b"), user("c")];
    let nodes = stubbed(&ns);
    let all = ids(&ns);
    let before = compilation("transcript", &all);
    let now = compilation("fresh", &[all[2]]);
    let moves = crate::moved::read(&nodes);
    let t = read(&before, &now, &report(vec![]), &nodes, &moves).expect("two were dropped");
    assert_eq!((t.dropped, t.accounted, t.unaccounted), (2, 2, 0));
}

/// A compaction's summary stands for its range, and a split's `Moved`
/// record for what it moved: each places its own drops, and a node neither
/// covers is still named.
#[test]
fn a_summary_and_a_split_place_what_each_took_out() {
    let (a, b, c) = (user("a"), user("b"), user("c"));
    let m = moved(&[c.id.as_str()], 3, 3);
    let s = summary(1, 1);
    // a is behind the summary's range; c was moved to a thread; b is gone
    // with nothing to show for it.
    let ns = vec![a.clone(), b.clone(), c.clone(), m, s.clone()];
    let nodes = stubbed(&ns);
    let before = compilation("transcript", &[&a.id, &b.id, &c.id]);
    let now = compilation("transcript", &[&s.id]);
    let moves = crate::moved::read(&nodes);
    let t = read(&before, &now, &report(vec![]), &nodes, &moves).expect("three were dropped");
    assert_eq!((t.dropped, t.accounted, t.unaccounted), (3, 2, 1));
    assert_eq!(t.names, vec![b.id]);
}

/// A node this compile's read does not hold at all was not dropped by the
/// compile: the read was narrower than the one the predecessor compiled
/// over (a task's view, a page bound), and the compile has nothing to say.
#[test]
fn a_node_this_read_does_not_hold_is_not_this_compiles_thinning() {
    let ns = vec![user("a"), user("b")];
    let nodes = stubbed(&ns);
    let all = ids(&ns);
    let before = compilation("transcript", &[all[0], all[1], "nod_elsewhere"]);
    let now = compilation("transcript", &[all[0], all[1]]);
    let moves = crate::moved::read(&nodes);
    let t = read(&before, &now, &report(vec![]), &nodes, &moves).expect("one was dropped");
    assert_eq!((t.dropped, t.accounted, t.unaccounted), (1, 1, 0));
}

/// The report names at most `CAP` nodes one by one; the whole count is the
/// `Thinner`'s, which the log and the row say.
#[test]
fn the_report_names_at_most_the_cap_and_the_count_is_whole() {
    let ns: Vec<Node> = (0..CAP + 4).map(|i| user(&format!("m{i}"))).collect();
    let nodes = stubbed(&ns);
    let all = ids(&ns);
    let before = compilation("transcript", &all);
    let now = compilation("transcript", &[all[0]]);
    let mut r = report(vec![]);
    let moves = crate::moved::read(&nodes);
    let t = read(&before, &now, &r, &nodes, &moves).expect("the rest were dropped");
    assert_eq!(t.dropped, CAP as u64 + 3);
    assert_eq!(t.unaccounted, CAP as u64 + 3);
    assert_eq!(t.names.len(), CAP);
    say(&mut r, &t, &now.id);
    assert_eq!(unaccounted(&r), CAP);
}
