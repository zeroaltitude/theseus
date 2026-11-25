use super::*;
use crate::node::{Body, ResultStatus};
use theseus_protocol::Usage;

fn spec(model: &str, system: &str) -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: model.into(),
        max_tokens: 1000,
        system_text: system.into(),
        context_text: String::new(),
        session_text: String::new(),
        persona_text: String::new(),
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
    }
}

/// The tools digest the replaced sorted-key serializer gave (at 8a1e41d).
/// If it changed, every stored manifest would read as a tool change, and
/// each session's next turn would recompile and strip its thinking.
#[test]
fn the_tools_digest_matches_the_one_stored_manifests_carry() {
    let mut s = spec("m", "sys");
    s.tools = vec![
        json!({"name": "fs_read", "description": "Read a file", "input_schema": {"type": "object", "required": ["path"], "properties": {"path": {"type": "string"}, "offset": {"type": "integer", "minimum": 0}}}}),
        json!({"name": "proc_run", "input_schema": {"properties": {"argv": {"items": {"type": "string"}, "type": "array"}}, "type": "object"}, "description": "Run é ✓"}),
    ]
    .into();
    let m = manifest_for(&s, &Catalog::builtin(), None, false);
    assert_eq!(m.tools_digest, "46f4e6a0c66ccbb0");
}

fn assistant(blocks: Vec<Value>) -> Node {
    Node::assistant(
        "s",
        "t",
        0,
        Body::AssistantMessage {
            blocks,
            // The model these tests' requests mostly name: a message
            // another model wrote carries no thinking back (25e).
            model: "claude-opus-5".into(),
            provider: "anthropic".into(),
            stop_reason: None,
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

fn result(id: &str, content: &str, status: ResultStatus, late: bool) -> Node {
    Node::tool_result(
        "s",
        None,
        None,
        Body::ToolResult {
            tool_use_id: id.into(),
            tool: "fs.read".into(),
            status,
            is_error: matches!(status, ResultStatus::Error | ResultStatus::Declined),
            content: content.into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late,
            meta: json!({"exit_code": 0}),
            image: None,
            external: None,
            payload: None,
            parts: Vec::new(),
        },
    )
}

fn run(
    nodes: &[(u64, Node)],
    current: Option<&Compilation>,
    spec: &RequestSpec,
    last: u64,
) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> =
        nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect();
    super::tests_parts::on_view(CompileInput {
        session_id: "s",
        current,
        nodes: &nodes,
        last_position: last,
        spec,
        catalog: &Catalog::builtin(),
        force: false,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        situation: &situation::Situation::Continuation,
    })
}

/// A request that ends in tool results, mid-way through a tool loop, carries
/// the effort-only message its turn's first answer recorded, so it must carry
/// the beta that allows it too. Without the beta Anthropic answers a 400
/// (`messages.N.output_config: Extra inputs are not permitted`) to every call
/// after the first of a turn that ran at another level.
#[test]
fn a_request_ending_in_tool_results_carries_the_per_message_effort_beta() {
    let mut sp = spec("claude-fable-5-1", "You are Theseus.");
    sp.turn_effort = Some(Effort::Low);
    let answer = Node::assistant(
        "s",
        "t",
        0,
        Body::AssistantMessage {
            blocks: vec![
                json!({"type": "text", "text": "Reading."}),
                json!({"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "/x"}}),
            ],
            model: "claude-fable-5-1".into(),
            provider: "anthropic".into(),
            stop_reason: None,
            usage: Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: Some(Effort::Low),
        },
    );
    let nodes = vec![
        (1, Node::user("s", None, "web", "read /x")),
        (2, answer),
        (3, result("t1", "X", ResultStatus::Ok, false)),
    ];
    let c = run(&nodes, None, &sp, 3);
    let msgs = &c.request.messages;
    assert_eq!(msgs.last().unwrap()["content"][0]["type"], "tool_result");
    assert!(
        msgs.iter().any(|m| m["role"] == "system"),
        "the recorded effort renders before its turn: {msgs:?}"
    );
    assert!(
        c.request.betas.iter().any(|b| b == BETA_TURN_EFFORT),
        "a message that carries output_config needs its beta: {:?}",
        c.request.betas
    );
}

/// A cassette of a response as the Messages API documents it: a signed
/// `thinking` block, a `redacted_thinking` block with opaque data, then text.
/// Hand-written to the documented shape, NOT a live recording (theseus-9bggi:
/// no API key in the VM that built it); the bytes are odd on purpose.
fn cassette_blocks() -> Vec<Value> {
    vec![
        json!({"type": "thinking", "thinking": "step one", "signature": "EqQBCkYIAxgC+/=="}),
        json!({"type": "redacted_thinking", "data": "EmwKAhgBEgy3va3pzix/LafPsn4aDFIT90cYlmL9\n+/AA=="}),
        json!({"type": "text", "text": "done"}),
    ]
}

#[test]
fn a_redacted_thinking_block_goes_back_verbatim_and_strips_with_the_rest() {
    let nodes = vec![
        (1, Node::user("s", None, "web", "hi")),
        (2, assistant(cassette_blocks())),
        (3, Node::user("s", None, "web", "again")),
    ];
    let sp = spec("claude-opus-5", "A");
    let c1 = run(&nodes[..1], None, &sp, 1);
    // Append: the next request carries the answer's blocks byte for byte.
    let c2 = run(&nodes, Some(&c1.compilation), &sp, 3);
    assert_eq!(c2.decision(), "append");
    assert_eq!(
        c2.request.messages[1]["content"],
        Value::Array(cassette_blocks())
    );
    // A system change recompiles and strips both kinds, keeping the text.
    let c3 = run(
        &nodes,
        Some(&c1.compilation),
        &spec("claude-opus-5", "B"),
        3,
    );
    assert_eq!(
        c3.request.messages[1]["content"],
        json!([{"type": "text", "text": "done"}])
    );
}

/// An assistant message another provider wrote: GLM's, with z.ai's
/// signature, as the history a turn on Anthropic replays (theseus-kol).
fn glm_said(blocks: Vec<Value>) -> Node {
    let mut n = assistant(blocks);
    if let Body::AssistantMessage {
        model, provider, ..
    } = &mut n.body
    {
        *model = "glm-5.3-flash".into();
        *provider = "zai".into();
    }
    n
}

fn glm_thinking() -> Value {
    json!({"type": "thinking", "thinking": "I should run it.", "signature": "zai-signature"})
}

/// The history's assistant message came from provider X (GLM, `zai`), and
/// the request goes to Y (Anthropic): it carries none of X's thinking
/// blocks with their signatures, wherever the message sits, in a new
/// compilation's prefix or in an old one's tail, a tool loop's last
/// message included (the 400 of theseus-kol). The same provider's keep
/// theirs, byte for byte.
#[test]
fn a_thinking_block_goes_back_only_to_the_provider_that_wrote_it() {
    let call = json!({"type": "tool_use", "id": "t1", "name": "proc_run", "input": {"argv": ["sleep", "25"]}});
    let nodes = vec![
        (1, Node::user("s", None, "cli", "run sleep 25")),
        (2, glm_said(vec![glm_thinking(), call.clone()])),
        (
            3,
            result("t1", "Still running", ResultStatus::Background, false),
        ),
    ];
    let no_thinking = |c: &Compiled| {
        c.request.messages.iter().all(|m| {
            m["content"]
                .as_array()
                .unwrap()
                .iter()
                .all(|b| !is_thinking(b))
        }) && !serde_json::to_string(&c.request.messages)
            .unwrap()
            .contains("zai-signature")
    };

    // A new chain, made for Anthropic: a view chain's manifest marks its
    // prefix stripped, and still GLM's thinking in the turn stays home.
    let sonnet = spec("claude-sonnet-5-5", "A");
    let c = run(&nodes, None, &sonnet, 3);
    assert!(c.compilation.manifest.strip_thinking);
    assert!(no_thinking(&c), "{:?}", c.request.messages);
    assert_eq!(c.request.messages[1]["content"], json!([call]));

    // The same history, on GLM's own provider: its thinking goes back.
    let mut glm = spec("glm-5.3-flash", "A");
    glm.provider = "zai".into();
    let g = run(&nodes, None, &glm, 3);
    assert_eq!(g.request.messages[1]["content"][0], glm_thinking());

    // A GLM message in the tail of a compilation made for Anthropic
    // (one that was never replaced): stripped there too.
    let c1 = run(&nodes[..1], None, &sonnet, 1);
    let c2 = run(&nodes, Some(&c1.compilation), &sonnet, 3);
    assert_eq!(c2.decision(), "append");
    assert!(no_thinking(&c2), "{:?}", c2.request.messages);

    // And when the operator switches the live profile mid-session, the
    // next turn's recompile strips every earlier thinking block.
    let s1 = run(&nodes, None, &glm, 3);
    let switched = run(&nodes, Some(&s1.compilation), &sonnet, 3);
    assert_eq!(switched.trigger.as_deref(), Some("view_turn"));
    assert!(switched.compilation.manifest.strip_thinking);
    assert!(no_thinking(&switched));
}

#[test]
fn late_results_render_as_text_and_missing_results_are_repaired() {
    let mut sp = spec("glm-5.3-flash", "");
    // No effort asked, none sent: GLM's row takes one (theseus-a1wvq.1).
    sp.effort = None;
    let nodes = vec![
        (1, Node::user("s", None, "web", "build it")),
        (
            2,
            assistant(vec![
                json!({"type": "tool_use", "id": "t1", "name": "proc_run", "input": {}}),
            ]),
        ),
        (
            3,
            result("t1", "Still running", ResultStatus::Background, false),
        ),
        (
            4,
            assistant(vec![json!({"type": "text", "text": "Started."})]),
        ),
        (5, result("t1", "BUILD OK", ResultStatus::Ok, true)),
        (
            6,
            assistant(vec![
                json!({"type": "tool_use", "id": "t9", "name": "fs_read", "input": {}}),
            ]),
        ),
    ];
    let c = run(&nodes, None, &sp, 6);
    assert!(
        c.request.thinking.is_none(),
        "GLM thinks implicitly: no thinking param"
    );
    assert!(c.request.output_config.is_none());
    assert!(c.request.system.is_empty());
    let m = &c.request.messages;
    assert_eq!(m[2]["content"][0]["content"], "Still running");
    let late = m[4]["content"][0]["text"].as_str().unwrap();
    assert!(
        late.contains("Background result")
            && late.contains("BUILD OK")
            && late.contains("exit code 0")
    );
    assert_eq!(c.repairs, vec!["t9".to_string()]);
    assert_eq!(m.last().unwrap()["content"][0]["is_error"], true);
}

/// A spec whose header and context blocks have these many characters.
fn two_blocks(model: &str, header: usize, context: usize) -> RequestSpec {
    let mut s = spec(model, &"h".repeat(header));
    s.context_text = "c".repeat(context);
    s
}

fn hi() -> Vec<(u64, Node)> {
    vec![(1, Node::user("s", None, "web", "hi"))]
}

/// Each system block's `cache_control`, then the top-level one.
fn marks(c: &Compiled) -> Vec<Value> {
    c.request
        .system
        .iter()
        .map(|b| b.get("cache_control").cloned().unwrap_or(Value::Null))
        .chain([c.request.cache_control.clone().unwrap_or(Value::Null)])
        .collect()
}

/// The system goes out in two blocks, the header and the context files,
/// each with its breakpoint, and the conversation's is the top-level one:
/// 3 of the provider's 4 (theseus-ev1). An edit to a context file changes
/// the second block only: the header's bytes, and with the tools before
/// them its cache entry, stay.
#[test]
fn the_system_is_two_blocks_each_with_a_breakpoint() {
    let sp = two_blocks("claude-opus-5", 4_000, 2_000);
    let c = run(&hi(), None, &sp, 1);
    let sys = &c.request.system;
    assert_eq!(sys.len(), 2);
    assert_eq!(sys[0]["text"], sp.system_text);
    assert_eq!(sys[1]["text"], sp.context_text);
    let five = json!({"type": "ephemeral"});
    assert_eq!(marks(&c), [five.clone(), five.clone(), five]);
    assert_eq!(c.cache.breakpoints(), ["header", "project", "conversation"]);
    let m = c.compilation.manifest.cache.clone().unwrap();
    assert_eq!(m, c.cache);
    assert!(m.caches && m.blocks.iter().all(|b| b.marked));
    assert_eq!(m.min_tokens, 512);
    // Each prefix is the tools and the system through its block, in bytes.
    let len = |v: &Value| serde_json::to_string(v).unwrap().len() as u64;
    let tools = len(&Value::Array(sp.tools.to_vec()));
    let header = len(&json!({"type": "text", "text": sp.system_text}));
    let context = len(&json!({"type": "text", "text": sp.context_text}));
    assert_eq!(m.blocks[0].prefix_bytes, tools + header);
    assert_eq!(m.blocks[1].prefix_bytes, tools + header + context);

    // An edited context file recompiles; the header's bytes do not move.
    let mut edited = sp;
    edited.context_text.push_str(" and one more line");
    let c2 = run(&hi(), Some(&c.compilation), &edited, 1);
    assert_eq!(c2.trigger.as_deref(), Some("view_turn"));
    assert_eq!(
        serde_json::to_string(&c2.request.system[0]).unwrap(),
        serde_json::to_string(&c.request.system[0]).unwrap()
    );
    assert_ne!(c2.request.system[1], c.request.system[1]);
    assert_eq!(c2.request.tools, c.request.tools);
}

/// A breakpoint whose prefix can never reach the model's minimum gets
/// none: the provider would not cache it, and it would take a slot
/// (theseus-ev1). A token takes 2 bytes at the fewest, so on Haiku 4.5
/// (4,096 tokens) a prefix under 8,192 bytes is surely short: this
/// ~4,100-byte header gets none, and the ~24,000-byte context block one.
/// The manifest says which is which. On Opus 5, whose minimum is 512
/// (1,024 bytes), both are marked.
#[test]
fn a_prefix_under_the_models_minimum_gets_no_breakpoint() {
    let sp = two_blocks("claude-haiku-4-5", 4_000, 20_000);
    let c = run(&hi(), None, &sp, 1);
    let five = json!({"type": "ephemeral"});
    assert_eq!(marks(&c), [Value::Null, five.clone(), five.clone()]);
    let m = c.compilation.manifest.cache.clone().unwrap();
    assert_eq!(m.min_tokens, 4_096);
    let header = &m.blocks[0];
    assert_eq!((header.block.as_str(), header.marked), ("header", false));
    assert!((4_000..8_192).contains(&header.prefix_bytes), "{header:?}");
    let context = &m.blocks[1];
    assert_eq!((context.block.as_str(), context.marked), ("project", true));
    assert!(context.prefix_bytes >= 24_000, "{context:?}");
    assert_eq!(c.cache.breakpoints(), ["project", "conversation"]);
    let stored = serde_json::to_value(&c.compilation.manifest).unwrap();
    assert_eq!(stored["cache"]["blocks"][0]["marked"], false, "{stored}");

    // The bound exactly: a header prefix of 8,192 bytes is marked on
    // Haiku, and one of 8,191 is not.
    let len = |v: &Value| serde_json::to_string(v).unwrap().len();
    let base = len(&Value::Array(sp.tools.to_vec())) + len(&json!({"type": "text", "text": ""}));
    for (bytes, marked) in [(8_192, true), (8_191, false)] {
        let at = spec("claude-haiku-4-5", &"h".repeat(bytes - base));
        let b = cache_layout(&at, &Catalog::builtin()).blocks[0].clone();
        assert_eq!((b.prefix_bytes, b.marked), (bytes as u64, marked));
    }

    // Without context files, Haiku's only breakpoint is the conversation's.
    let alone = spec("claude-haiku-4-5", &"h".repeat(4_000));
    let c = run(&hi(), None, &alone, 1);
    assert_eq!(marks(&c), [Value::Null, five.clone()]);

    let mut opus = sp;
    opus.model = "claude-opus-5".into();
    let c = run(&hi(), None, &opus, 1);
    assert_eq!(marks(&c), [five.clone(), five.clone(), five]);
}

/// The TTL on the wire (theseus-ev1). `5m` is the API's default, so its
/// markers name none: the bytes of every request before 13c. `1h` is on
/// every breakpoint, the top-level automatic one included. A task keeps
/// its own conversation at 5 minutes after the header's 1-hour entries,
/// the order the provider allows, and no spec puts a longer-lived entry
/// after a shorter one. A TTL is in no digest, so changing it does not
/// recompile.
#[test]
fn the_ttl_goes_on_every_breakpoint_longest_first() {
    let five = json!({"type": "ephemeral"});
    let hour = json!({"type": "ephemeral", "ttl": "1h"});
    let mut sp = two_blocks("claude-opus-5", 4_000, 2_000);
    let c = run(&hi(), None, &sp, 1);
    assert_eq!(marks(&c), [five.clone(), five.clone(), five.clone()]);
    assert_eq!(c.request.body()["cache_control"], five);

    sp.cache_ttl = CacheTtl::OneHour;
    sp.conversation_ttl = CacheTtl::OneHour;
    let c = run(&hi(), None, &sp, 1);
    assert_eq!(marks(&c), [hour.clone(), hour.clone(), hour.clone()]);
    assert_eq!(c.request.body()["cache_control"], hour);
    assert_eq!(c.request.body()["system"][0]["cache_control"]["ttl"], "1h");

    // A task on a 1-hour profile: the header's entries for an hour, its
    // own conversation's for 5 minutes.
    sp.conversation_ttl = CacheTtl::FiveMinutes;
    let c = run(&hi(), None, &sp, 1);
    assert_eq!(marks(&c), [hour.clone(), hour, five.clone()]);

    // Never a 1-hour entry after a 5-minute one.
    sp.cache_ttl = CacheTtl::FiveMinutes;
    sp.conversation_ttl = CacheTtl::OneHour;
    let c = run(&hi(), None, &sp, 1);
    assert_eq!(marks(&c), [five.clone(), five.clone(), five]);

    // A TTL change keeps the compilation, and so the prefix's thinking.
    let mut longer = sp;
    longer.cache_ttl = CacheTtl::OneHour;
    longer.conversation_ttl = CacheTtl::OneHour;
    let again = run(&hi(), Some(&c.compilation), &longer, 1);
    assert!(!again.new_compilation);
}

/// A model whose provider does not cache gets no breakpoints at all, and
/// its manifest says so (theseus-ev1). Every built-in model caches, so
/// it takes a config's `caches = false`.
#[test]
fn a_model_that_does_not_cache_gets_no_breakpoints() {
    let mut rows = std::collections::BTreeMap::new();
    rows.insert(
        "claude-opus-5".to_string(),
        crate::catalog::CatalogRow {
            caches: Some(false),
            ..Default::default()
        },
    );
    let catalog = Catalog::with_overrides(&rows);
    let sp = two_blocks("claude-opus-5", 4_000, 2_000);
    let nodes: Vec<(u64, crate::stub::Stub)> =
        hi().into_iter().map(|(p, n)| (p, n.into())).collect();
    let c = super::tests_parts::on_view(CompileInput {
        session_id: "s",
        current: None,
        nodes: &nodes,
        last_position: 1,
        spec: &sp,
        catalog: &catalog,
        force: false,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        situation: &situation::Situation::Continuation,
    });
    assert_eq!(c.request.system.len(), 2);
    assert_eq!(marks(&c), [Value::Null, Value::Null, Value::Null]);
    assert!(c.request.body().get("cache_control").is_none());
    let m = c.compilation.manifest.cache.unwrap();
    assert!(!m.caches && m.blocks.iter().all(|b| !b.marked));
    assert!(c.cache.breakpoints().is_empty());
}

/// A compilation stored before 13c has no `cache` in its manifest. It
/// reads as none, and with the same one-block system the next turn
/// appends to it, rendering the same bytes.
#[test]
fn a_manifest_from_before_the_cache_layout_reads_as_none() {
    let sp = spec("claude-opus-5", "You are Theseus.");
    let c = run(&hi(), None, &sp, 1);
    let mut v = serde_json::to_value(&c.compilation).unwrap();
    assert!(v["manifest"]["cache"].is_object());
    v["manifest"].as_object_mut().unwrap().remove("cache");
    let old: Compilation = serde_json::from_value(v).unwrap();
    assert!(old.manifest.cache.is_none());
    let mut nodes = hi();
    nodes.push((2, Node::user("s", None, "web", "again")));
    let c2 = run(&nodes, Some(&old), &sp, 2);
    assert!(!c2.new_compilation, "{:?}", c2.trigger);
    assert_eq!(c2.request.system, c.request.system);
}

// ------------------------------------------------- the estimate (theseus-f5hf)

/// An answer a call of compilation `cmp` returned, which the provider
/// counted: `input` tokens in (a cache write, as a first call's is),
/// `output` out.
fn answered(blocks: Vec<Value>, cmp: &str, input: u64, output: u64) -> Node {
    let mut n = assistant(blocks);
    if let Body::AssistantMessage {
        usage,
        compilation_id,
        ..
    } = &mut n.body
    {
        *usage = Usage {
            input_tokens: 4,
            cache_creation_input_tokens: input - 4,
            output_tokens: output,
            ..Default::default()
        };
        *compilation_id = Some(cmp.into());
    }
    n
}

fn call(id: &str) -> Value {
    json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": "/x"}})
}

/// `n` bytes of a tool's JSON output.
fn json_output(n: usize) -> String {
    let row = r#"{"id":1234,"name":"node_17","kind":"file","size":4096,"tags":["a","b"]},"#;
    row.repeat(n / row.len() + 1)[..n].to_string()
}

/// From a compilation's second call on, the estimate adds what is new
/// to the provider's own count of the last request and its answer.
#[test]
fn a_compilations_second_call_adds_to_the_providers_count() {
    let sp = spec("claude-sonnet-5-5", "S");
    let mut nodes = vec![(1, Node::user("s", None, "web", "read /x"))];
    let c1 = run(&nodes, None, &sp, 1);
    assert_eq!(c1.estimate.method, EstimateMethod::Bytes);
    assert_eq!(c1.estimate.counted, 0);
    let id = c1.compilation.id.clone();
    // The provider counted 5,204 tokens in and 88 out; then 11,142 bytes
    // of tool output.
    nodes.push((2, answered(vec![call("t1")], &id, 5_204, 88)));
    nodes.push((
        3,
        result("t1", &json_output(11_142), ResultStatus::Ok, false),
    ));
    let c2 = run(&nodes, Some(&c1.compilation), &sp, 3);
    assert!(!c2.new_compilation);
    let e = c2.estimate;
    assert_eq!(e.method, EstimateMethod::Counted);
    assert_eq!(e.counted, 5_204 + 88);
    // The new part: the output at 2.4 bytes a token, the result's
    // message and block, two ids (the call's and the result's), and the
    // answer's message.
    assert_eq!(e.estimated, 4_643 + 3 + 1 + 2 * 15 + 3);
    assert_eq!(e.tokens, e.counted + e.estimated);
    assert_eq!(
        e.upper,
        e.counted + e.estimated + (e.estimated * 40).div_ceil(100)
    );
    assert_eq!(c2.est_tokens, e.tokens);

    // An answer of another compilation, or one with no count, is not
    // counted on: the estimate is all bytes again.
    let mut other = nodes.clone();
    other[1].1 = answered(vec![call("t1")], "cmp_other", 5_204, 88);
    let c = run(&other, Some(&c1.compilation), &sp, 3);
    assert_eq!(c.estimate.method, EstimateMethod::Bytes);
    let mut uncounted = nodes.clone();
    uncounted[1].1 = assistant(vec![call("t1")]);
    let c = run(&uncounted, Some(&c1.compilation), &sp, 3);
    assert_eq!(c.estimate.method, EstimateMethod::Bytes);
    // A recompile makes a new compilation, which no answer is of yet.
    let mut sp = sp;
    sp.system_text = "S2".into();
    let c = run(&nodes, Some(&c1.compilation), &sp, 3);
    assert!(c.new_compilation);
    assert_eq!(c.estimate.method, EstimateMethod::Bytes);
}

/// Recorded counts: each loop of the owner's DM on Sonnet 5.5 (2026-10-01),
/// as the provider counted it, against the estimate from the loop
/// before's count. Each row: the last request's count and its answer's
/// output tokens and calls; then the bytes of tool output, or of a
/// message, the request added; and the provider's count of it. The
/// estimate is never under the count by more than its margin, so the
/// bound the ring checks is never under it.
#[test]
fn the_estimate_is_within_its_margin_of_recorded_counts() {
    // (count, output, calls, tool output bytes, message bytes, next count)
    let dm: [(u64, u64, usize, usize, usize, u64); 19] = [
        (4_633, 88, 1, 11_142, 0, 11_092),
        (11_092, 695, 0, 0, 84, 11_819),
        (11_819, 125, 2, 18_848, 0, 19_914),
        (19_914, 287, 3, 38_499, 0, 34_542),
        (34_542, 4_176, 0, 0, 105, 38_754),
        (38_754, 432, 3, 228, 0, 39_380),
        (39_380, 233, 1, 2_141, 0, 40_527),
        (40_527, 336, 3, 3_421, 0, 42_789),
        (42_789, 425, 2, 3_555, 0, 44_868),
        (44_868, 505, 1, 7_515, 0, 48_200),
        (48_200, 1_060, 0, 0, 66, 49_286),
        (49_286, 126, 1, 2_141, 0, 50_383),
        (50_383, 310, 1, 526, 0, 51_001),
        (51_001, 197, 1, 2_437, 0, 52_114),
        (52_114, 282, 1, 1_811, 0, 53_070),
        (53_070, 558, 3, 4_658, 0, 55_728),
        (55_728, 429, 1, 2_416, 0, 57_283),
        (57_283, 722, 0, 0, 92, 58_036),
        (58_036, 343, 1, 833, 0, 58_726),
    ];
    let sp = spec("claude-sonnet-5-5", "S");
    let first = run(&hi(), None, &sp, 1);
    let mut worst: f64 = 1.0;
    for (count, output, calls, out_bytes, msg_bytes, next) in dm {
        let ids: Vec<String> = (0..calls).map(|k| format!("t{k}")).collect();
        let mut nodes = hi();
        let blocks: Vec<Value> = if calls == 0 {
            vec![json!({"type": "text", "text": "a"})]
        } else {
            ids.iter().map(|id| call(id)).collect()
        };
        nodes.push((2, answered(blocks, &first.compilation.id, count, output)));
        let mut p = 3;
        for id in &ids {
            let share = out_bytes / calls;
            nodes.push((p, result(id, &json_output(share), ResultStatus::Ok, false)));
            p += 1;
        }
        if msg_bytes > 0 {
            nodes.push((p, Node::user("s", None, "web", &"m".repeat(msg_bytes))));
            p += 1;
        }
        let c = run(&nodes, Some(&first.compilation), &sp, p);
        let e = c.estimate;
        assert_eq!(e.method, EstimateMethod::Counted);
        assert!(e.upper >= next, "{e:?} against {next}");
        worst = worst.min(e.tokens as f64 / next as f64);
    }
    // At its worst, an 11 KB tool output read at 1.76 bytes a token, the
    // estimate is 15 % low; the ring's bound covers it.
    assert!(worst > 0.84, "{worst}");
}

/// GLM and DeepSeek think without being asked (theseus-a1wvq.1): their rows
/// say `Implicit`, the request carries no `thinking` field whatever the
/// display, and an effort a row takes goes as one of the three levels the
/// endpoints document (medium and xhigh as high, DeepSeek's own mapping).
#[test]
fn glm_rows_think_implicitly_and_take_three_effort_levels() {
    let catalog = Catalog::builtin();
    for id in [
        "glm-5.3",
        "glm-5.2",
        "glm-5.3-flash",
        "glm-5.3-flashx",
        "deepseek-flash",
        "deepseek-v4-pro",
    ] {
        let e = catalog.get(id).unwrap();
        assert_eq!(
            (e.thinking, e.effort),
            (ThinkingMode::Implicit, true),
            "{id}"
        );
    }
    for (asked, sent) in [
        (Effort::Low, "low"),
        (Effort::Medium, "high"),
        (Effort::High, "high"),
        (Effort::Xhigh, "high"),
        (Effort::Max, "max"),
    ] {
        let mut sp = spec("glm-5.3-flash", "sys");
        sp.effort = Some(asked);
        let c = run(&[(1, Node::user("s", None, "cli", "hi"))], None, &sp, 1);
        assert_eq!(
            c.request.output_config.unwrap()["effort"],
            sent,
            "{asked:?}"
        );
    }
    // Claude's levels go as asked.
    let mut sp = spec("claude-opus-5", "sys");
    sp.effort = Some(Effort::Xhigh);
    let c = run(&[(1, Node::user("s", None, "cli", "hi"))], None, &sp, 1);
    assert_eq!(c.request.output_config.unwrap()["effort"], "xhigh");
}

#[test]
fn implicit_thinking_sends_no_thinking_field() {
    for (model, display) in [
        ("deepseek-flash", ThinkingDisplay::Summarized),
        ("deepseek-v4-pro", ThinkingDisplay::Omitted),
        ("glm-5.3", ThinkingDisplay::Updates),
    ] {
        let mut sp = spec(model, "sys");
        sp.thinking_display = display;
        let c = run(&[(1, Node::user("s", None, "cli", "hi"))], None, &sp, 1);
        assert!(c.request.thinking.is_none(), "{model}");
        assert!(c.request.betas.is_empty(), "{model}: {:?}", c.request.betas);
    }
}

/// A response that calls a tool of ours and the tool search together can stop
/// at our call with the search unanswered (`stop_reason: tool_use`, no
/// `tool_search_tool_result`). Replayed, Anthropic answers every later request
/// with a 400 (`messages.1: … srvtoolu_… was found without a corresponding
/// tool_search_tool_result block`), so the turn and its retry fail. The render
/// leaves the open server call out; an answered one stays.
#[test]
fn a_server_call_the_provider_never_answered_is_not_replayed() {
    // A request with the tool search tool: the answered search stays (theseus-it6th).
    let mut sp = spec("claude-opus-5", "You are Theseus.");
    sp.tools = vec![
        json!({"type": "tool_search_tool_bm25_20251119", "name": "tool_search_tool_bm25"}),
        json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
    ]
    .into();
    let search = |id: &str| {
        json!({"type": "server_tool_use", "id": id, "name": "tool_search_tool_bm25",
               "input": {"query": "ask the owner"}})
    };
    let found = |id: &str| {
        json!({"type": "tool_search_tool_result", "tool_use_id": id,
               "content": {"type": "tool_search_tool_search_result", "tool_references": []}})
    };
    let call = json!({"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "/x"}});
    let nodes = vec![
        (1, Node::user("s", None, "web", "go")),
        (2, assistant(vec![call.clone(), search("srvtoolu_open")])),
        (3, result("t1", "X", ResultStatus::Ok, false)),
        (
            4,
            assistant(vec![
                search("srvtoolu_done"),
                found("srvtoolu_done"),
                call.clone(),
            ]),
        ),
        (5, result("t1", "X", ResultStatus::Ok, false)),
    ];
    let c = run(&nodes, None, &sp, 5);
    let sent = serde_json::to_string(&c.request.messages).unwrap();
    assert!(!sent.contains("srvtoolu_open"), "{sent}");
    assert!(sent.contains("srvtoolu_done"), "{sent}");
    // The answer keeps its tool call, and the call's result still follows it.
    assert_eq!(c.request.messages[1]["content"], json!([call]));
    assert_eq!(c.request.messages[2]["content"][0]["tool_use_id"], "t1");
}
