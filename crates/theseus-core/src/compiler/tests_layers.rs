//! The layered request (theseus-h4a7): the header, the persona and the project
//! layer, each with its breakpoint, the messages under the automatic one, and
//! the session's block after them with none.

use super::layers::{layer_digests, HEADER, PERSONA, PROJECT};
use super::tests_ring::spec;
use super::*;

/// A text of about `n` bytes.
fn big(c: char, n: usize) -> String {
    c.to_string().repeat(n)
}

fn layered() -> RequestSpec {
    let mut s = spec();
    s.system_text = big('h', 12_000);
    s.persona_text = big('p', 3_000);
    s.context_text = big('c', 3_000);
    s.session_text = "Directory: /work/tide-gauges".into();
    s
}

fn marks(c: &Compiled) -> Vec<bool> {
    c.request
        .system
        .iter()
        .map(|b| b.get("cache_control").is_some())
        .collect()
}

fn compiled(spec: &RequestSpec) -> Compiled {
    let nodes = [(1u64, Node::user("s", None, "cli", "hello").into())];
    compile(CompileInput {
        session_id: "s",
        current: None,
        nodes: &nodes,
        last_position: 1,
        spec,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::ConversationStart,
        kept: None,
    })
}

/// Three layers, three breakpoints, then the messages' automatic one: the
/// provider's four. The session's block follows the layers with none.
#[test]
fn the_request_is_three_layers_a_session_block_and_the_messages() {
    let s = layered();
    let c = compiled(&s);
    let texts: Vec<&str> = c
        .request
        .system
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        [
            s.system_text.as_str(),
            s.persona_text.as_str(),
            s.context_text.as_str(),
            "Directory: /work/tide-gauges"
        ]
    );
    assert_eq!(marks(&c), [true, true, true, false]);
    assert_eq!(
        c.cache.breakpoints(),
        [HEADER, PERSONA, PROJECT, "conversation"]
    );
    assert!(
        c.request.cache_control.is_some(),
        "the automatic breakpoint"
    );
    // Four breakpoints in all: the provider's limit.
    assert_eq!(marks(&c).iter().filter(|m| **m).count() + 1, 4);
}

/// No persona configured: no second layer, nothing marked for it, as the
/// owner's daemon has it today.
#[test]
fn without_a_persona_the_second_layer_is_left_out_and_unmarked() {
    let mut s = layered();
    s.persona_text.clear();
    let c = compiled(&s);
    assert_eq!(c.request.system.len(), 3);
    assert_eq!(c.cache.breakpoints(), [HEADER, PROJECT, "conversation"]);
    assert_eq!(marks(&c), [true, true, false]);
}

/// A layer is marked only when its prefix can reach the model's minimum.
#[test]
fn a_layer_under_the_minimum_is_not_marked() {
    let mut s = layered();
    s.system_text = "short".into();
    s.persona_text.clear();
    s.context_text = "small".into();
    let c = compiled(&s);
    assert_eq!(c.cache.breakpoints(), ["conversation"]);
    assert_eq!(marks(&c), [false, false, false]);
}

/// Each layer has a digest, and two requests that share a layer share it.
#[test]
fn each_layer_has_a_digest_that_follows_its_text() {
    let a = layered();
    let mut b = layered();
    b.context_text.push_str(" and a topic's guidance");
    b.session_text = "Directory: /work/moons".into();
    let (da, db) = (layer_digests(&a), layer_digests(&b));
    let names: Vec<&str> = da.iter().map(|d| d.0.as_str()).collect();
    assert_eq!(names, [HEADER, PERSONA, PROJECT, "session"]);
    assert_eq!(da[0], db[0]);
    assert_eq!(da[1], db[1]);
    assert_ne!(da[2], db[2]);
    assert_ne!(da[3], db[3]);
}
