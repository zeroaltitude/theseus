//! A thread's card and its cold resume (theseus-h4a7): pure compiles over
//! invented threads.

use serde_json::json;

use super::card::Card;
use super::cold::Cold;
use super::tests_ring::{answer, call, closed, result, spec};
use super::*;

const WORKING: u64 = 20_000;

/// `turns` exchanges of a person's message, a call, its result of `size`
/// characters and the answer: `(position, node)` from 1.
fn thread(turns: usize, size: usize) -> Vec<(u64, Node)> {
    let mut nodes = Vec::new();
    for k in 0..turns {
        nodes.push(Node::user("s", None, "cli", &format!("question {k}")));
        let id = format!("c{k}");
        nodes.push(answer(vec![
            json!({"type": "text", "text": format!("looking at gauge {k}")}),
            call(&id, &format!("log/g{k}.txt")),
        ]));
        nodes.push(result(&id, &format!("g{k} {}", "x".repeat(size))));
        nodes.push(answer(vec![
            json!({"type": "text", "text": format!("gauge {k} is fine")}),
        ]));
    }
    nodes
        .into_iter()
        .enumerate()
        .map(|(i, n)| (i as u64 + 1, n))
        .collect()
}

fn stubs_of(nodes: &[(u64, Node)]) -> Vec<(u64, crate::stub::Stub)> {
    nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect()
}

fn run(
    nodes: &[(u64, Node)],
    current: Option<&Compilation>,
    spec: &RequestSpec,
    window: Option<u64>,
) -> Compiled {
    let nodes = stubs_of(nodes);
    let last = nodes.last().map_or(0, |(p, _)| *p);
    compile(CompileInput {
        session_id: "s",
        current,
        nodes: &nodes,
        last_position: last,
        spec,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: window,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
    })
}

fn carded(now: &str, recorded: Option<&str>) -> RequestSpec {
    let mut s = spec();
    s.card = Some(Card::new(now.into(), recorded.map(str::to_string)));
    s
}

fn cold(s: &mut RequestSpec, keep: u32, working: u64) {
    s.cold = Some(Cold {
        keep_turns: keep,
        working_tokens: working,
    });
}

/// The person's messages a request carries, as text.
fn people(c: &Compiled) -> Vec<String> {
    c.request
        .messages
        .iter()
        .filter(|m| m["role"] == "user")
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|b| b["text"].as_str().map(str::to_string))
        .filter(|t| t.starts_with("question "))
        .collect()
}

fn stubbed(c: &Compiled) -> usize {
    c.request
        .messages
        .iter()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter(|b| b["type"] == "tool_result")
        .filter(|b| {
            b["content"]
                .as_str()
                .is_some_and(|t| t.starts_with("[Result cleared to save context:"))
        })
        .count()
}

/// The thread's card is the first block of the first message, and an append
/// renders the snapshot its compilation took even when the thread has moved
/// on; a recompile takes the card as it stands.
#[test]
fn the_card_heads_the_messages_and_changes_only_at_a_recompile() {
    let nodes = thread(3, 100);
    let first = run(&nodes, None, &carded("CARD one", None), None);
    assert!(first.new_compilation);
    assert_eq!(first.card.as_deref(), Some("CARD one"));
    assert_eq!(first.request.messages[0]["content"][0]["text"], "CARD one");
    assert_eq!(
        first.request.messages[0]["content"][1]["text"],
        "question 0"
    );

    // The thread's card moved; its compilation's snapshot did not.
    let mut more = nodes;
    more.extend(thread(1, 100).into_iter().map(|(p, n)| (p + 12, n)));
    let append = run(
        &more,
        Some(&first.compilation),
        &carded("CARD two", Some("CARD one")),
        None,
    );
    assert!(!append.new_compilation, "{:?}", append.trigger);
    assert_eq!(append.request.messages[0]["content"][0]["text"], "CARD one");
    assert_eq!(append.card.as_deref(), Some("CARD one"));
    assert_eq!(
        serde_json::to_string(&append.request.messages[..first.request.messages.len()]).unwrap(),
        serde_json::to_string(&first.request.messages).unwrap(),
        "the prefix is the bytes it was"
    );

    // A recompile (a changed system here) takes the card as it stands.
    let mut changed = carded("CARD two", Some("CARD one"));
    changed.system_text.push_str(" and more");
    let recompile = run(&more, Some(&first.compilation), &changed, None);
    assert!(recompile.new_compilation);
    assert_eq!(
        recompile.request.messages[0]["content"][0]["text"],
        "CARD two"
    );
    assert_eq!(recompile.card.as_deref(), Some("CARD two"));
}

/// A session with no card renders the messages as before.
#[test]
fn a_session_without_a_card_has_none() {
    let c = run(&thread(2, 10), None, &spec(), None);
    assert_eq!(c.request.messages[0]["content"][0]["text"], "question 0");
    assert_eq!(c.card, None);
}

/// A cold resume of a 200-turn thread compiles under its working tokens with
/// its stubs: the newest turns whole, the older results stubbed, the older
/// turns gone, no call without its result.
#[test]
fn a_cold_resume_of_a_long_thread_fits_its_working_tokens_with_its_stubs() {
    let mut nodes = thread(200, 12_000);
    let first = run(&nodes, None, &carded("CARD", None), None);
    assert!(first.est_tokens > 300_000, "{}", first.est_tokens);
    nodes.push((
        nodes.len() as u64 + 1,
        Node::user("s", None, "cli", "question 200"),
    ));

    let mut s = carded("CARD", Some("CARD"));
    cold(&mut s, 6, WORKING);
    let c = run(&nodes, Some(&first.compilation), &s, None);
    assert!(c.new_compilation);
    assert_eq!(c.trigger.as_deref(), Some("cold_resume"));
    assert_eq!(c.compilation.strategy, "ring");
    assert!(c.est_tokens <= WORKING, "{} tokens", c.est_tokens);
    let kept = people(&c);
    assert_eq!(kept.last().map(String::as_str), Some("question 200"));
    assert!(kept.len() <= 6, "the newest turns only: {kept:?}");
    assert!(stubbed(&c) > 0, "the older results are stubs");
    assert_eq!(c.request.messages[0]["content"][0]["text"], "CARD");
    closed(&c).unwrap();
    let cut = c.budget.dropped.iter().find(|d| d.tier == "ring").unwrap();
    assert_eq!(cut.reason, "cold_resume");
    assert!(cut.range.as_ref().unwrap().nodes > 700);

    // The same thread, warm, is untouched for its size: the ring and
    // compaction guard the window as they always did.
    let w = run(
        &nodes,
        Some(&first.compilation),
        &carded("CARD", Some("CARD")),
        None,
    );
    assert!(!w.new_compilation, "{:?}", w.trigger);
    assert!(w.est_tokens > 300_000);
}

/// A small thread resumed cold is rewritten too, to take the card as it
/// stands; it keeps every turn when they fit.
#[test]
fn a_small_thread_resumed_cold_takes_its_fresh_card_and_keeps_its_turns() {
    let mut nodes = thread(4, 200);
    let first = run(&nodes, None, &carded("CARD one", None), None);
    nodes.push((
        nodes.len() as u64 + 1,
        Node::user("s", None, "cli", "question 4"),
    ));
    let mut s = carded("CARD two", Some("CARD one"));
    cold(&mut s, 6, WORKING);
    let c = run(&nodes, Some(&first.compilation), &s, None);
    assert_eq!(c.trigger.as_deref(), Some("cold_resume"));
    assert_eq!(c.compilation.strategy, "transcript");
    assert_eq!(people(&c).len(), 5, "every turn fits");
    assert_eq!(c.request.messages[0]["content"][0]["text"], "CARD two");
    assert_eq!(stubbed(&c), 0);
}

/// Few turns, but big results: the stubs make room before any turn goes.
#[test]
fn big_old_results_are_stubbed_before_a_turn_is_dropped() {
    let mut nodes = thread(5, 30_000);
    let first = run(&nodes, None, &carded("CARD", None), None);
    nodes.push((
        nodes.len() as u64 + 1,
        Node::user("s", None, "cli", "question 5"),
    ));
    let mut s = carded("CARD", Some("CARD"));
    cold(&mut s, 6, WORKING);
    let c = run(&nodes, Some(&first.compilation), &s, None);
    assert_eq!(c.trigger.as_deref(), Some("cold_resume"));
    assert!(c.est_tokens <= WORKING, "{}", c.est_tokens);
    assert_eq!(people(&c).len(), 6, "all six turns kept: {:?}", people(&c));
    assert_eq!(stubbed(&c), 5);
}
