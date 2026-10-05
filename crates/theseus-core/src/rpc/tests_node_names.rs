//! A node named by its id's end (theseus-glyw): `node.reach` takes a node's
//! last 6 or more characters, or the cockpit's `msg·a1b2c3`, when one node
//! ends so, and refuses, in words, an end that names two, none, or one too
//! short to be an end; a whole id answers as it always did.

use theseus_protocol::{error_code, NodeReachParams, SessionKind};

use super::tests::test_core;
use super::Core;
use crate::node::Node;
use crate::reach::{resolve, Named};
use crate::session::SessionRecord;

fn open(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// A message in `session` with the id `id`.
fn put(core: &Core, session: &str, id: &str) {
    let mut n = Node::user(session, None, "operator", &format!("I am {id}"));
    n.id = id.into();
    core.store.append(&[n.record().unwrap()]).unwrap();
}

fn reach(core: &Core, name: &str) -> Result<theseus_protocol::NodeReachResult, (i64, String)> {
    core.node_reach(NodeReachParams {
        node_id: name.into(),
        max_generations: None,
    })
    .map_err(|e| (e.code, e.message))
}

/// Two sessions whose nodes end alike: `msg_…7c41ab` in one and
/// `tcl_…7c41ab` in the other, beside nodes of fresh ids.
fn rig() -> (std::sync::Arc<Core>, String, String) {
    let core = test_core("ok");
    let (a, b) = (open(&core), open(&core));
    for _ in 0..20 {
        put(&core, &a, &crate::new_id("msg"));
        put(&core, &b, &crate::new_id("msg"));
    }
    put(&core, &a, "msg_0198d3a0b1c2d3e4f5a6b7c8d97c41ab");
    put(&core, &b, "tcl_0198d3a0ffffffffffffffffff7c41ab");
    put(&core, &a, "msg_0198d3a0b1c2d3e4f5a6b7c8d9e0f1a2");
    (core, a, b)
}

/// One node ends so: its last 6, more of its end, `…` before it, or the
/// cockpit's form, each answers as its whole id does.
#[tokio::test]
async fn an_end_that_names_one_node_answers_as_its_whole_id() {
    let (core, a, _) = rig();
    let whole = reach(&core, "msg_0198d3a0b1c2d3e4f5a6b7c8d9e0f1a2").unwrap();
    assert_eq!(whole.session_id, a);
    for name in ["e0f1a2", "d9e0f1a2", "…e0f1a2", "msg·e0f1a2", " e0f1a2 "] {
        let got = reach(&core, name).unwrap();
        assert_eq!(
            serde_json::to_value(&got).unwrap(),
            serde_json::to_value(&whole).unwrap(),
            "{name}"
        );
    }
    // The cockpit's prefix tells two alike apart.
    assert_eq!(
        reach(&core, "tcl·7c41ab").unwrap().node_id,
        "tcl_0198d3a0ffffffffffffffffff7c41ab"
    );
}

/// An end two nodes share is refused, naming each with its session, and
/// never answered as the first of them.
#[tokio::test]
async fn an_end_that_names_two_nodes_is_refused_naming_each() {
    let (core, a, b) = rig();
    let (code, why) = reach(&core, "7c41ab").unwrap_err();
    assert_eq!(code, error_code::INVALID_PARAMS);
    assert!(why.starts_with("`7c41ab` names 2 nodes ("), "{why}");
    for (id, s) in [
        ("msg_0198d3a0b1c2d3e4f5a6b7c8d97c41ab", &a),
        ("tcl_0198d3a0ffffffffffffffffff7c41ab", &b),
    ] {
        let named = format!("{id} in {}", theseus_protocol::short_id(s));
        assert!(why.contains(&named), "{why} names {named}");
    }
    assert!(why.ends_with("give more of its id"), "{why}");
    assert!(
        matches!(resolve(&core.store, "d97c41ab").unwrap(), Named::One(id) if id.starts_with("msg_")),
        "more of the end names one"
    );
}

/// An end under 6 characters, or one no node has, is not found, in words.
#[tokio::test]
async fn an_end_too_short_or_unknown_is_not_found() {
    let (core, _, _) = rig();
    let (code, why) = reach(&core, "0f1a2").unwrap_err();
    assert_eq!(code, error_code::NOT_FOUND);
    assert!(why.contains("at least its id's last 6 characters"), "{why}");
    let (code, why) = reach(&core, "msg·f1a2").unwrap_err();
    assert_eq!(code, error_code::NOT_FOUND, "{why}");
    let (code, why) = reach(&core, "abcdef").unwrap_err();
    assert_eq!(code, error_code::NOT_FOUND);
    assert_eq!(why, "no node's id ends with `abcdef`");
    let (code, _) = reach(&core, "res·e0f1a2").unwrap_err();
    assert_eq!(code, error_code::NOT_FOUND, "the prefix is held too");
}

/// The resolve walks the index's keys and decodes no node: its time over a
/// store of 100,000 nodes (`--run-ignored only`, in a release build, for the
/// report; the suite's runs are under any load).
#[tokio::test]
#[ignore]
async fn the_resolve_at_a_hundred_thousand_nodes() {
    let core = test_core("ok");
    let s = open(&core);
    let mut last = String::new();
    for _ in 0..100 {
        let batch: Vec<_> = (0..1000)
            .map(|_| {
                let n = Node::user(&s, None, "operator", "x");
                last = n.id.clone();
                n.record().unwrap()
            })
            .collect();
        core.store.append(&batch).unwrap();
    }
    let end = &last[last.len() - 6..];
    let mut times = Vec::new();
    for _ in 0..5 {
        let t = std::time::Instant::now();
        let got = resolve(&core.store, end).unwrap();
        times.push(t.elapsed());
        assert_eq!(got, Named::One(last.clone()));
    }
    times.sort();
    eprintln!(
        "resolve at 100,000 nodes: {times:?} (median {:?})",
        times[2]
    );
}
