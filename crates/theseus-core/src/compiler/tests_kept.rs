//! The kept render's rule (theseus-0l3g): over generated sessions, every
//! request a compile makes with the session's render kept across loops is
//! byte for byte the one it makes rendering every node. The sessions take
//! tool loops, late results, answers cut at the window and replaced, calls
//! left unanswered and answered later, recall notes, a person's message
//! landing inside a running turn, summaries, operator recompiles, and windows
//! small enough to clear old results and ring.

use proptest::prelude::*;
use serde_json::{json, Value};
use theseus_protocol::Usage;

use super::*;
use crate::node::{Body, ResultStatus};

fn spec() -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: "claude-opus-5".into(),
        max_tokens: 1000,
        system_text: "S".into(),
        context_text: String::new(),
        session_text: String::new(),
        persona_text: String::new(),
        card: None,
        cold: None,
        context_files: vec![],
        persona: None,
        tools: vec![
            json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
        ]
        .into(),
        effort: Some(Effort::High),
        turn_effort: None,
        effort_refused: vec![],
        thinking_display: ThinkingDisplay::Summarized,
        refusal_fallbacks: true,
        fallback: None,
        first_party: true,
        cache_ttl: CacheTtl::FiveMinutes,
        conversation_ttl: CacheTtl::FiveMinutes,
        walk: None,
        defer: None,
        memberships: vec![],
        guidance: vec![],
        prefix: None,
        prefix_used: None,
    }
}

fn answer(turn: &str, blocks: Vec<Value>, stop: Option<&str>) -> Node {
    Node::assistant(
        "s",
        turn,
        0,
        Body::AssistantMessage {
            blocks,
            model: "claude-opus-5".into(),
            provider: "anthropic".into(),
            stop_reason: stop.map(str::to_string),
            usage: Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    )
}

fn result(id: &str, content: String, late: bool) -> Node {
    Node::tool_result(
        "s",
        None,
        None,
        Body::ToolResult {
            tool_use_id: id.into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content,
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late,
            meta: json!({}),
            image: None,
            external: None,
        },
    )
}

/// One thing that happens to the session, then a compile.
#[derive(Debug, Clone)]
enum Step {
    /// A person's message: a new turn, or (`false`) one inside the turn.
    Said(usize, bool),
    /// An answer with `n` calls, each answered with a result of its size,
    /// thinking or not.
    Calls(Vec<usize>, bool),
    /// An answer with a call left unanswered, its result after a later node.
    Unanswered(usize),
    /// The answer that ends the turn.
    Done(usize),
    /// An answer cut at the window, then the one that replaces it.
    Cut,
    /// A late result of a background call.
    Late(usize),
    /// A recall note.
    Recalled,
    /// A summary of the nodes so far.
    Summarized,
    /// The operator's recompile of the transcript.
    Recompiled,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (1usize..300, prop::bool::weighted(0.85)).prop_map(|(n, t)| Step::Said(n, t)),
        4 => (prop::collection::vec(1usize..3_000, 1..3), any::<bool>())
            .prop_map(|(r, t)| Step::Calls(r, t)),
        1 => (1usize..500).prop_map(Step::Unanswered),
        3 => (1usize..400).prop_map(Step::Done),
        1 => Just(Step::Cut),
        1 => (1usize..300).prop_map(Step::Late),
        1 => Just(Step::Recalled),
        1 => Just(Step::Summarized),
        1 => Just(Step::Recompiled),
    ]
}

struct Session {
    nodes: Vec<(u64, Node)>,
    turn: usize,
    calls: usize,
    /// A call whose result waits for the next node.
    owed: Option<(String, usize)>,
}

impl Session {
    fn push(&mut self, n: Node) {
        let p = self.nodes.last().map_or(0, |(p, _)| *p) + 1;
        self.nodes.push((p, n));
        if let Some((id, size)) = self.owed.take() {
            let p = p + 1;
            self.nodes.push((p, result(&id, "o".repeat(size), false)));
        }
    }

    fn turn(&self) -> String {
        format!("t{}", self.turn)
    }

    fn call(&mut self) -> String {
        self.calls += 1;
        format!("c{}", self.calls)
    }

    fn apply(&mut self, s: &Step) -> Option<Recompile> {
        match s {
            Step::Said(n, new) => {
                self.turn += usize::from(*new);
                let turn = self.turn();
                let m = Node::user("s", Some(&turn), "web", &"w".repeat(*n));
                self.push(m);
            }
            Step::Calls(sizes, thinks) => {
                let turn = self.turn();
                for size in sizes {
                    let id = self.call();
                    let mut blocks = vec![];
                    if *thinks {
                        blocks.push(json!({"type": "thinking", "thinking": "t".repeat(200), "signature": "g".repeat(80)}));
                    }
                    blocks.push(json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": format!("f{id}")}}));
                    self.push(answer(&turn, blocks, None));
                    self.push(result(&id, "r".repeat(*size), false));
                }
            }
            Step::Unanswered(size) => {
                let turn = self.turn();
                let id = self.call();
                let blocks =
                    vec![json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {}})];
                self.push(answer(&turn, blocks, None));
                self.owed = Some((id, *size));
            }
            Step::Done(n) => {
                let turn = self.turn();
                self.push(answer(
                    &turn,
                    vec![json!({"type": "text", "text": "d".repeat(*n)})],
                    None,
                ));
            }
            Step::Cut => {
                let turn = self.turn();
                let cut = vec![json!({"type": "text", "text": "partial"})];
                self.push(answer(&turn, cut, Some(crate::provider::WINDOW_EXCEEDED)));
                self.push(answer(
                    &turn,
                    vec![json!({"type": "text", "text": "again"})],
                    None,
                ));
            }
            Step::Late(n) => {
                let id = self.call();
                self.push(result(&id, "l".repeat(*n), true));
            }
            Step::Recalled => {
                let turn = self.turn();
                let mut n = Node::user("s", Some(&turn), "harness", "");
                n.body = Body::Recall {
                    recall_id: format!("rcl{}", self.nodes.len()),
                    arm: "baseline".into(),
                    items: vec![],
                };
                self.push(n);
            }
            Step::Summarized => {
                let last = self.nodes.last().map_or(0, |(p, _)| *p);
                let first = self.nodes.first().map_or(0, |(p, _)| *p);
                if last > first + 4 {
                    let mut n = Node::user("s", None, "harness", "");
                    n.body = Body::Summary {
                        first,
                        last: last - 2,
                        nodes: 3,
                        text: "what came before".into(),
                        profile: "p".into(),
                        model: "m".into(),
                        cost_usd: None,
                        header: "[Summary]".into(),
                    };
                    self.push(n);
                }
            }
            Step::Recompiled => return Some(Recompile::Transcript),
        }
        None
    }
}

fn compile_with_kept(
    nodes: &[(u64, crate::stub::Stub)],
    current: Option<&Compilation>,
    window: u64,
    force: Option<Recompile>,
    kept: Option<&kept::Kept>,
) -> Compiled {
    compile_on(&spec(), nodes, current, (window, force), kept)
}

fn compile_on(
    spec: &RequestSpec,
    nodes: &[(u64, crate::stub::Stub)],
    current: Option<&Compilation>,
    (window, force): (u64, Option<Recompile>),
    kept: Option<&kept::Kept>,
) -> Compiled {
    compile(CompileInput {
        session_id: "s",
        current,
        nodes,
        last_position: nodes.last().map_or(0, |(p, _)| *p),
        spec,
        catalog: &Catalog::builtin(),
        force,
        window_override: Some(window),
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: kept.map(|k| (k, 0)),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn a_kept_render_makes_every_request_a_whole_render_makes(
        steps in prop::collection::vec(step(), 1..60),
        window in prop::sample::select(vec![4_000u64, 9_000, 30_000, 1_000_000]),
    ) {
        let kept = kept::Kept::new(64 << 20);
        let mut s = Session { nodes: vec![], turn: 0, calls: 0, owed: None };
        s.apply(&Step::Said(5, true));
        let mut current: Option<Compilation> = None;
        for (i, st) in steps.iter().enumerate() {
            let force = s.apply(st);
            let nodes: Vec<(u64, crate::stub::Stub)> =
                s.nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect();
            let whole = compile_with_kept(&nodes, current.as_ref(), window, force, None);
            let held = compile_with_kept(&nodes, current.as_ref(), window, force, Some(&kept));
            prop_assert_eq!(
                serde_json::to_string(&held.request.body()).unwrap(),
                serde_json::to_string(&whole.request.body()).unwrap(),
                "step {} ({:?})", i, st
            );
            prop_assert_eq!(&held.digest, &whole.digest);
            prop_assert_eq!(held.estimate, whole.estimate);
            prop_assert_eq!(&held.repairs, &whole.repairs);
            prop_assert_eq!((held.prefix_nodes, held.tail_nodes), (whole.prefix_nodes, whole.tail_nodes));
            prop_assert_eq!(&held.budget, &whole.budget);
            prop_assert_eq!(held.new_compilation, whole.new_compilation);
            // The compile step keeps what it made: an append keeps the one
            // it appended to.
            current = Some(held.compilation.clone());
        }
    }
}

/// Across a long session's turns the render is taken up, not made again:
/// every append after the first renders only the nodes written since.
#[test]
fn appends_take_up_the_kept_render() {
    let kept = kept::Kept::new(64 << 20);
    let mut s = Session {
        nodes: vec![],
        turn: 0,
        calls: 0,
        owed: None,
    };
    let mut current: Option<Compilation> = None;
    for _ in 0..300 {
        s.apply(&Step::Said(40, true));
        s.apply(&Step::Calls(vec![200], false));
        s.apply(&Step::Done(60));
        let nodes: Vec<(u64, crate::stub::Stub)> = s
            .nodes
            .iter()
            .map(|(p, n)| (*p, n.clone().into()))
            .collect();
        let c = compile_with_kept(&nodes, current.as_ref(), 1_000_000, None, Some(&kept));
        current = Some(c.compilation);
    }
    let (hits, misses) = kept.counts();
    assert_eq!(
        (hits, misses),
        (299, 1),
        "every append after the first took up the kept render"
    );
}

/// A fallback a timeout made leaves nothing out, so the requests naming it
/// still take up the kept render: only a refused answer to leave out
/// (`RequestSpec::fallback`'s id) puts the render aside (theseus-3cf5c).
#[test]
fn a_timeout_fallback_keeps_the_kept_render() {
    let on = |refused: Option<&str>| {
        let kept = kept::Kept::new(64 << 20);
        let spec = RequestSpec {
            fallback: Some(("claude-sonnet-5".into(), refused.map(str::to_string))),
            ..spec()
        };
        let mut s = Session {
            nodes: vec![],
            turn: 0,
            calls: 0,
            owed: None,
        };
        let mut current: Option<Compilation> = None;
        for _ in 0..20 {
            s.apply(&Step::Said(40, true));
            s.apply(&Step::Calls(vec![200], false));
            s.apply(&Step::Done(60));
            let nodes: Vec<(u64, crate::stub::Stub)> = s
                .nodes
                .iter()
                .map(|(p, n)| (*p, n.clone().into()))
                .collect();
            let c = compile_on(
                &spec,
                &nodes,
                current.as_ref(),
                (1_000_000, None),
                Some(&kept),
            );
            current = Some(c.compilation);
        }
        kept.counts()
    };
    assert_eq!(
        on(None),
        (19, 1),
        "no refused answer: the render is taken up"
    );
    assert_eq!(
        on(Some("msg_refused")),
        (0, 0),
        "a refused answer sets it aside"
    );
}
