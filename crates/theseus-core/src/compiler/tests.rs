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
        card: None,
        cold: None,
        context_files: vec![],
        persona: None,
        tools: vec![
            json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
        ],
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
    ];
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
        },
    )
}

fn thinking() -> Value {
    json!({"type": "thinking", "thinking": "", "signature": "sig"})
}

fn run(
    nodes: &[(u64, Node)],
    current: Option<&Compilation>,
    spec: &RequestSpec,
    last: u64,
) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> =
        nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect();
    compile(CompileInput {
        session_id: "s",
        current,
        nodes: &nodes,
        last_position: last,
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
        situation: &situation::Situation::Continuation,
        kept: None,
    })
}

#[test]
fn appending_keeps_every_earlier_byte_and_tool_results_follow_their_call() {
    let sp = spec("claude-opus-5", "You are Theseus.");
    let mut nodes = vec![(1, Node::user("s", None, "web", "read /x"))];
    let c1 = run(&nodes, None, &sp, 1);
    assert!(c1.new_compilation);
    assert_eq!(c1.trigger.as_deref(), Some("new_session"));
    assert_eq!(c1.request.thinking.as_ref().unwrap()["type"], "adaptive");
    assert_eq!(c1.request.output_config.as_ref().unwrap()["effort"], "high");
    assert_eq!(c1.request.extra["fallbacks"], "default");
    assert!(c1.request.betas.contains(&BETA_FALLBACKS.to_string()));
    // A header this short is under Opus 5's caching minimum, so the only
    // breakpoint is the conversation's (theseus-ev1).
    assert!(c1.request.system[0].get("cache_control").is_none());
    assert_eq!(c1.request.cache_control, Some(json!({"type": "ephemeral"})));

    nodes.push((
        2,
        assistant(vec![
            thinking(),
            json!({"type": "text", "text": "Reading."}),
            json!({"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "/x"}}),
            json!({"type": "tool_use", "id": "t2", "name": "fs_read", "input": {"path": "/y"}}),
        ]),
    ));
    nodes.push((3, result("t2", "Y", ResultStatus::Ok, false)));
    nodes.push((4, result("t1", "X", ResultStatus::Error, false)));
    let c2 = run(&nodes, Some(&c1.compilation), &sp, 4);
    assert!(!c2.new_compilation);
    assert_eq!(c2.decision(), "append");
    assert_eq!(&c2.request.messages[..1], &c1.request.messages[..]);
    assert_eq!(c2.request.messages.len(), 3);
    // Results in one user message, in tool_use order, error flagged.
    let rb = &c2.request.messages[2]["content"];
    assert_eq!(rb[0]["tool_use_id"], "t1");
    assert_eq!(rb[0]["is_error"], true);
    assert_eq!(rb[1]["tool_use_id"], "t2");
    assert!(rb[1].get("is_error").is_none());
    // The thinking block replays verbatim.
    assert_eq!(c2.request.messages[1]["content"][0], thinking());

    nodes.push((5, assistant(vec![json!({"type": "text", "text": "Done."})])));
    nodes.push((6, Node::user("s", None, "web", "thanks")));
    let c3 = run(&nodes, Some(&c1.compilation), &sp, 6);
    assert_eq!(&c3.request.messages[..3], &c2.request.messages[..]);
    assert_eq!(c3.request.system, c2.request.system);
    assert_eq!(c3.request.tools, c2.request.tools);
    assert!(c3.repairs.is_empty());
}

#[test]
fn a_system_or_model_change_recompiles_and_strips_the_prefixs_thinking() {
    let sp = spec("claude-opus-5", "A");
    let nodes = vec![
        (1, Node::user("s", None, "web", "hi")),
        (
            2,
            assistant(vec![thinking(), json!({"type": "text", "text": "hello"})]),
        ),
        (3, Node::user("s", None, "web", "again")),
    ];
    let c1 = run(&nodes[..1], None, &sp, 1);
    let sp2 = spec("claude-opus-5", "B");
    let c2 = run(&nodes, Some(&c1.compilation), &sp2, 3);
    assert!(c2.new_compilation);
    assert_eq!(c2.trigger.as_deref(), Some("system_changed"));
    assert!(c2.compilation.manifest.strip_thinking);
    assert_eq!(
        c2.compilation.derived_from.as_deref(),
        Some(c1.compilation.id.as_str())
    );
    assert_eq!(
        c2.request.messages[1]["content"].as_array().unwrap().len(),
        1
    );

    // A thinking block's signature is for the model that wrote it
    // (theseus-kol): a model change strips them as a system change does.
    let sp3 = spec("claude-sonnet-5", "A");
    let c3 = run(&nodes, Some(&c1.compilation), &sp3, 3);
    assert_eq!(c3.trigger.as_deref(), Some("model_changed"));
    assert!(c3.compilation.manifest.strip_thinking);
    assert_eq!(
        c3.request.messages[1]["content"],
        json!([{"type": "text", "text": "hello"}])
    );
    assert!(
        !c3.request.extra.contains_key("fallbacks"),
        "sonnet 5 has no fallbacks row"
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

    // A new compilation, made for Anthropic: nothing strips its prefix,
    // and still GLM's thinking stays home.
    let sonnet = spec("claude-sonnet-5-5", "A");
    let c = run(&nodes, None, &sonnet, 3);
    assert!(!c.compilation.manifest.strip_thinking);
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
    assert_eq!(switched.trigger.as_deref(), Some("model_changed"));
    assert!(switched.compilation.manifest.strip_thinking);
    assert!(no_thinking(&switched));
}

#[test]
fn late_results_render_as_text_and_missing_results_are_repaired() {
    let sp = spec("glm-5.3-flash", "");
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
    assert!(c.request.thinking.is_none(), "GLM takes no thinking param");
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

#[test]
fn fresh_drops_the_past_and_overflow_rings_at_a_user_boundary() {
    let sp = spec("claude-opus-5", "S");
    let big = "word ".repeat(2_000);
    let mut nodes = Vec::new();
    for i in 0..6u64 {
        nodes.push((
            i * 2 + 1,
            Node::user("s", None, "web", &format!("q{i} {big}")),
        ));
        nodes.push((
            i * 2 + 2,
            assistant(vec![
                thinking(),
                json!({"type": "text", "text": format!("a{i}")}),
            ]),
        ));
    }
    let nodes: Vec<(u64, crate::stub::Stub)> =
        nodes.into_iter().map(|(p, n)| (p, n.into())).collect();
    let (catalog, sources) = (Catalog::builtin(), Default::default());
    let input = CompileInput {
        session_id: "s",
        current: None,
        nodes: &nodes,
        last_position: 12,
        spec: &sp,
        catalog: &catalog,
        force: Some(Recompile::Fresh),
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &sources,
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
    };
    let c = compile(input);
    assert_eq!(c.compilation.strategy, "fresh");
    assert!(c.compilation.manifest.strip_thinking);
    assert_eq!(
        c.request.messages.len(),
        2,
        "only the latest exchange survives"
    );
    assert!(c.request.messages[0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("q5"));
    assert!(c.request.messages[1]["content"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| !is_thinking(b)));

    let c = compile(CompileInput {
        force: None,
        window_override: Some(13_000),
        ..input
    });
    assert_eq!(c.trigger.as_deref(), Some("overflow"));
    assert_eq!(c.compilation.strategy, "ring");
    assert!(c.compilation.manifest.strip_thinking);
    assert_eq!(
        c.request.messages[0]["role"], "user",
        "ring cuts before a user message"
    );
    assert!(c.request.messages.len() < 12);
    assert!(c.request.messages.iter().all(|m| m["content"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| !is_thinking(b))));
    // The ring's cut, in the report (M6 30b): the leading nodes it left
    // out as a range, first to last, under a limit the request fits.
    let b = &c.budget;
    assert_eq!(c.compilation.budget.as_ref(), Some(b), "stored with it");
    assert_eq!(b.limit_tokens, 13_000 - sp.max_tokens as u64 - 4_096);
    assert!(
        b.used_tokens <= b.limit_tokens && b.overage.is_none(),
        "{b:?}"
    );
    let cut = &b.dropped[0];
    assert_eq!(
        (cut.tier.as_str(), cut.reason.as_str()),
        ("ring", "overflow")
    );
    let range = cut.range.as_ref().unwrap();
    let kept = &c.compilation.includes[0];
    let first_kept = nodes.iter().position(|(_, n)| &n.id == kept).unwrap();
    assert_eq!(range.first, nodes[0].1.id);
    assert_eq!(range.last, nodes[first_kept - 1].1.id);
    assert_eq!(range.nodes, first_kept as u64);
    assert!(cut.tokens > 0);
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
    let tools = len(&Value::Array(sp.tools.clone()));
    let header = len(&json!({"type": "text", "text": sp.system_text}));
    let context = len(&json!({"type": "text", "text": sp.context_text}));
    assert_eq!(m.blocks[0].prefix_bytes, tools + header);
    assert_eq!(m.blocks[1].prefix_bytes, tools + header + context);

    // An edited context file recompiles; the header's bytes do not move.
    let mut edited = sp;
    edited.context_text.push_str(" and one more line");
    let c2 = run(&hi(), Some(&c.compilation), &edited, 1);
    assert_eq!(c2.trigger.as_deref(), Some("system_changed"));
    assert_eq!(
        serde_json::to_string(&c2.request.system[0]).unwrap(),
        serde_json::to_string(&c.request.system[0]).unwrap()
    );
    assert_ne!(c2.request.system[1], c.request.system[1]);
    assert_eq!(c2.request.tools, c.request.tools);
}

/// Without context files the system is one block, with the digest and
/// the bytes it had before the split, so those sessions do not recompile.
/// With them the digest is new, so a compilation from before the split
/// recompiles once and drops its prefix's thinking, whose signatures were
/// made over the old single block (theseus-ev1).
#[test]
fn one_block_keeps_its_old_digest_and_two_recompile_once() {
    // A header over the minimum, as every real one is but on Haiku 4.5.
    let header = format!("You are Theseus. {}", "h".repeat(4_000));
    let one = spec("claude-opus-5", &header);
    let m = manifest_for(&one, &Catalog::builtin(), None, false);
    assert_eq!(m.system_digest, sha(&header));
    let c = run(&hi(), None, &one, 1);
    assert_eq!(
        c.request.system,
        vec![json!({"type": "text", "text": header, "cache_control": {"type": "ephemeral"}})]
    );
    // The old layout: one block, the files after a blank line.
    let mut two = one;
    two.context_text = "## A context file".into();
    let mut old = c.compilation;
    old.manifest.system_digest = sha(&format!("{}\n\n{}", two.system_text, two.context_text));
    old.manifest.cache = None;
    let nodes = vec![
        (1, Node::user("s", None, "web", "hi")),
        (
            2,
            assistant(vec![thinking(), json!({"type": "text", "text": "hello"})]),
        ),
        (3, Node::user("s", None, "web", "again")),
    ];
    let c2 = run(&nodes, Some(&old), &two, 3);
    assert_eq!(c2.trigger.as_deref(), Some("system_changed"));
    assert!(c2.compilation.manifest.strip_thinking);
    assert_eq!(c2.request.system.len(), 2);
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
    let base = len(&Value::Array(sp.tools.clone())) + len(&json!({"type": "text", "text": ""}));
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
    let c = compile(CompileInput {
        session_id: "s",
        current: None,
        nodes: &nodes,
        last_position: 1,
        spec: &sp,
        catalog: &catalog,
        force: None,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
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

// ------------------------------------- a request past the window (theseus-9p88)

/// `run` on a 1M-token model, after the provider said a request passed
/// the window.
fn run_over(
    nodes: &[(u64, Node)],
    current: Option<&Compilation>,
    spec: &RequestSpec,
    last: u64,
    overflowed: Option<&Overflowed>,
) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> =
        nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect();
    compile(CompileInput {
        session_id: "s",
        current,
        nodes: &nodes,
        last_position: last,
        spec,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
    })
}

/// Six exchanges of about 3,000 tokens each, every answer with thinking.
fn six_exchanges() -> Vec<(u64, Node)> {
    let big = "word ".repeat(2_000);
    let mut nodes = Vec::new();
    for i in 0..6u64 {
        nodes.push((
            i * 2 + 1,
            Node::user("s", None, "web", &format!("q{i} {big}")),
        ));
        nodes.push((
            i * 2 + 2,
            assistant(vec![
                thinking(),
                json!({"type": "text", "text": format!("a{i}")}),
            ]),
        ));
    }
    nodes
}

/// The exchanges a request keeps: its user messages' first words.
fn kept(c: &Compiled) -> Vec<String> {
    c.request
        .messages
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| {
            let text = m["content"][0]["text"].as_str().unwrap_or_default();
            text.split(' ').next().unwrap_or_default().to_string()
        })
        .collect()
}

/// theseus-9p88: the provider refused a request its estimate and Opus 5's
/// 1M window said fit. The next compilation rings anyway, by the window
/// the provider named, and reads its candidates at the provider's count:
/// a count twice the estimate keeps fewer exchanges than the estimate
/// alone would. Without the provider's word, nothing rings.
#[test]
fn a_request_the_provider_refused_rings_by_its_window_and_its_count() {
    let sp = spec("claude-opus-5", "S");
    let nodes = six_exchanges();
    let c0 = run_over(&nodes, None, &sp, 12, None);
    assert_eq!(c0.compilation.strategy, "transcript", "1M fits it all");
    let mut more = nodes;
    more.push((13, Node::user("s", None, "web", "q6 and now?")));
    let plain = run_over(&more, Some(&c0.compilation), &sp, 13, None);
    assert_eq!(plain.decision(), "append", "the estimate alone fits");
    assert_eq!(kept(&plain).len(), 7);

    // The provider's window, 20,000; its count of the request unknown.
    let by_window = Overflowed {
        counted: None,
        estimated: plain.est_tokens,
        maximum: Some(20_000),
        retrying: None,
    };
    let w = run_over(&more, Some(&c0.compilation), &sp, 13, Some(&by_window));
    assert_eq!(w.trigger.as_deref(), Some("overflow"));
    assert_eq!(w.compilation.strategy, "ring");
    assert_eq!(w.compilation.manifest.context_window, Some(20_000));
    let budget = 20_000 - 1_000 - 4_096;
    assert!(w.est_tokens <= budget * 6 / 10, "{}", w.est_tokens);
    assert_eq!(w.request.messages[0]["role"], "user");
    // The provider counted twice the estimate: fewer exchanges fit.
    let counted = Overflowed {
        counted: Some(plain.est_tokens * 2),
        ..by_window
    };
    let n = run_over(&more, Some(&c0.compilation), &sp, 13, Some(&counted));
    assert_eq!(n.trigger.as_deref(), Some("overflow"));
    assert!(
        kept(&n).len() < kept(&w).len(),
        "{:?} against {:?}",
        kept(&n),
        kept(&w)
    );
    assert!(
        n.est_tokens * 2 <= budget * 6 / 10 || kept(&n) == ["q6"],
        "the count's reading fits, or only the last exchange is left: {:?}",
        kept(&n)
    );
    // A window the estimate fits: only the provider's word rings it, and
    // its count, ten times the estimate, reads every candidate so.
    let forced = Overflowed {
        counted: Some(plain.est_tokens * 10),
        maximum: Some(200_000),
        ..counted.clone()
    };
    let budget = 200_000 - 1_000 - 4_096;
    assert!(plain.estimate.upper <= budget, "the estimate fits");
    let f = run_over(&more, Some(&c0.compilation), &sp, 13, Some(&forced));
    assert_eq!(f.trigger.as_deref(), Some("overflow"), "rung anyway");
    assert!(kept(&f).len() < kept(&plain).len(), "{:?}", kept(&f));
    assert!(f.est_tokens * 10 <= budget * 6 / 10, "{}", f.est_tokens);
    // A catalog window smaller than the provider's stays.
    let bigger = Overflowed {
        maximum: Some(5_000_000),
        ..counted
    };
    let b = run_over(&more, Some(&c0.compilation), &sp, 13, Some(&bigger));
    assert_eq!(b.compilation.manifest.context_window, Some(1_000_000));
}

/// theseus-9p88: a forced ring with nothing earlier to drop (the session's
/// only message passed the window, and nothing came after it) renders the
/// same request, which the turn reads as "the ring could drop nothing".
/// Since theseus-3ki0 the ring also cuts inside the message's turn: once
/// its loops follow it, the forced ring drops the early ones, the message
/// pinned at the top, and renders a new request.
#[test]
fn a_forced_ring_with_one_message_renders_the_same_request() {
    let sp = spec("claude-opus-5", "S");
    let mut nodes = vec![(1, Node::user("s", None, "web", "one long message"))];
    let c0 = run_over(&nodes, None, &sp, 1, None);
    let o = Overflowed {
        counted: Some(2_000_000),
        estimated: c0.est_tokens,
        maximum: Some(1_000_000),
        retrying: None,
    };
    let c1 = run_over(&nodes, Some(&c0.compilation), &sp, 1, Some(&o));
    assert!(!c1.new_compilation, "{:?}", c1.trigger);
    assert_eq!(c1.digest, c0.digest);
    // Its turn's loops after it: the forced ring cuts between them.
    for k in 0..6u64 {
        let id = format!("t{k}");
        nodes.push((2 + k * 2, assistant(vec![call(&id)])));
        nodes.push((
            3 + k * 2,
            result(&id, &"x".repeat(9_000), ResultStatus::Ok, false),
        ));
    }
    let c2 = run_over(&nodes, Some(&c0.compilation), &sp, 13, None);
    let o = Overflowed {
        counted: Some(c2.est_tokens * 10),
        estimated: c2.est_tokens,
        ..o
    };
    let c3 = run_over(&nodes, Some(&c2.compilation), &sp, 13, Some(&o));
    assert!(c3.new_compilation, "{:?}", c3.trigger);
    assert_ne!(c3.digest, c2.digest);
    assert_eq!(
        c3.request.messages[0]["content"][0]["text"],
        "one long message"
    );
    assert!(c3.request.messages.len() < c2.request.messages.len());
}

/// An answer of turn `turn`'s, cut at the window, with a call.
fn cut_answer(turn: &str, loop_index: u32, text: &str, call_id: &str) -> Node {
    let mut n = assistant(vec![
        thinking(),
        json!({"type": "text", "text": text}),
        call(call_id),
    ]);
    n.turn_id = Some(turn.into());
    n.loop_index = Some(loop_index);
    if let Body::AssistantMessage { stop_reason, .. } = &mut n.body {
        *stop_reason = Some(crate::provider::WINDOW_EXCEEDED.into());
    }
    n
}

fn of_turn(mut n: Node, turn: &str, loop_index: u32) -> Node {
    n.turn_id = Some(turn.into());
    n.loop_index = Some(loop_index);
    n
}

/// theseus-9p88: the retry of an answer cut at the window leaves it out,
/// with its call and the call's "not run" result, so the request ends
/// with the operator's message, not the cut answer (a prefill the newer
/// models refuse). Once the retry answers, every later render leaves it
/// out too, by the rule alone, so each request begins with the one
/// before. A cut answer no later answer of its turn followed stays.
#[test]
fn an_answer_cut_at_the_window_is_left_out_once_a_retry_replaces_it() {
    let sp = spec("claude-opus-5", "S");
    let mut nodes = vec![
        (1, of_turn(Node::user("s", None, "web", "q0"), "t0", 0)),
        (
            2,
            of_turn(
                assistant(vec![json!({"type": "text", "text": "a0"})]),
                "t0",
                0,
            ),
        ),
        (3, of_turn(Node::user("s", None, "web", "q1"), "t1", 0)),
    ];
    let c0 = run_over(&nodes, None, &sp, 3, None);
    let cut = cut_answer("t1", 0, "the first half", "c1");
    let cut_id = cut.id.clone();
    nodes.push((4, cut));
    nodes.push((5, result("c1", "not run", ResultStatus::Error, false)));
    let o = Overflowed {
        counted: Some(900_000),
        estimated: c0.est_tokens,
        maximum: Some(1_000_000),
        retrying: Some(cut_id),
    };
    let retry = run_over(&nodes, Some(&c0.compilation), &sp, 5, Some(&o));
    assert_eq!(retry.trigger.as_deref(), Some("overflow"));
    let last = retry.request.messages.last().unwrap();
    assert_eq!(
        (&last["role"], &last["content"][0]["text"]),
        (&json!("user"), &json!("q1")),
        "{:?}",
        retry.request.messages
    );
    let text = serde_json::to_string(&retry.request.messages).unwrap();
    assert!(!text.contains("the first half") && !text.contains("\"c1\""));

    // The retry's answer replaces it in every later render.
    nodes.push((
        6,
        of_turn(
            assistant(vec![json!({"type": "text", "text": "whole"})]),
            "t1",
            1,
        ),
    ));
    nodes.push((7, of_turn(Node::user("s", None, "web", "q2"), "t2", 0)));
    let next = run_over(&nodes, Some(&retry.compilation), &sp, 7, None);
    assert_eq!(next.decision(), "append");
    assert_eq!(
        &next.request.messages[..retry.request.messages.len()],
        &retry.request.messages[..],
        "the next request begins with the retry's"
    );
    let text = serde_json::to_string(&next.request.messages).unwrap();
    assert!(!text.contains("the first half") && text.contains("whole"));

    // A cut answer nothing in its turn followed is rendered.
    nodes.push((8, cut_answer("t2", 0, "cut and left", "c2")));
    nodes.push((9, result("c2", "not run", ResultStatus::Error, false)));
    nodes.push((10, of_turn(Node::user("s", None, "web", "q3"), "t3", 0)));
    let later = run_over(&nodes, Some(&retry.compilation), &sp, 10, None);
    let text = serde_json::to_string(&later.request.messages).unwrap();
    assert!(text.contains("cut and left") && text.contains("\"c2\""));
    assert!(!text.contains("the first half"));
}

// ------------------------------------------------- the estimate (theseus-f5hf)

/// `run` with a window.
fn run_in(
    nodes: &[(u64, Node)],
    current: Option<&Compilation>,
    spec: &RequestSpec,
    last: u64,
    window: u64,
) -> Compiled {
    let nodes: Vec<(u64, crate::stub::Stub)> =
        nodes.iter().map(|(p, n)| (*p, n.clone().into())).collect();
    compile(CompileInput {
        session_id: "s",
        current,
        nodes: &nodes,
        last_position: last,
        spec,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: Some(window),
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
    })
}

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

/// A conversation heavy in tool results on Sonnet 5.5. chars/4 puts it
/// under the ring's budget, while its count at the recorded ratio for
/// Theseus's JSON (×1.52 of chars/4, the cache2 lane's first request)
/// passes the window itself: the provider would refuse it. The estimate
/// reads the JSON at the catalog's figure, and the ring rings.
#[test]
fn the_ring_rings_for_json_that_chars4_reads_as_fitting() {
    let sp = spec("claude-sonnet-5-5", "S");
    let out = json_output(88_000);
    let mut nodes = Vec::new();
    for i in 0..3u64 {
        let id = format!("t{i}");
        nodes.push((3 * i + 1, Node::user("s", None, "web", "read the next")));
        nodes.push((3 * i + 2, assistant(vec![call(&id)])));
        nodes.push((3 * i + 3, result(&id, &out, ResultStatus::Ok, false)));
    }
    let window = 100_000;
    let budget = window - 1_000 - 4_096;
    // The request as it stands, with no window to ring at.
    let whole = run(&nodes, None, &sp, 9);
    let chars4 = whole.estimate.bytes / 4;
    assert!(chars4 < budget, "chars/4 {chars4} rings by itself");
    assert!(
        chars4 * 152 / 100 > window,
        "the recorded ratio's count fits"
    );
    assert_eq!(whole.estimate.method, EstimateMethod::Bytes);
    assert!(whole.est_tokens > chars4, "{:?}", whole.estimate);
    assert!(
        whole.estimate.upper > chars4 * 152 / 100,
        "{:?}",
        whole.estimate
    );

    let c = run_in(&nodes, None, &sp, 9, window);
    assert_eq!(c.trigger.as_deref(), Some("overflow"));
    assert_eq!(c.compilation.strategy, "ring");
    assert!(c.est_tokens <= budget * 6 / 10, "{:?}", c.estimate);
    assert!(
        c.estimate.upper <= budget,
        "the kept turns do not ring again"
    );
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

/// GLM's tokenizer reads Theseus's requests at about four bytes a token
/// (×1.02 of chars/4, the cache2 lane), so its figures leave a GLM
/// conversation unrung where Sonnet 5.5's would ring the same bytes.
#[test]
fn a_glm_conversation_is_not_rung_early() {
    let window = 200_000;
    let mut glm = spec("glm-5.3-flash", "S");
    glm.provider = "zai".into();
    // A conversation that chars/4 puts at 65 % of the budget.
    let budget = window - 1_000 - 4_096;
    let text = "word ".repeat((budget * 65 / 100 * 4 / 6) as usize / 5);
    let mut nodes = Vec::new();
    for i in 0..6u64 {
        nodes.push((i * 2 + 1, Node::user("s", None, "web", &text)));
        nodes.push((
            i * 2 + 2,
            assistant(vec![json!({"type": "text", "text": "ok"})]),
        ));
    }
    nodes.pop();
    let c = run_in(&nodes, None, &glm, 11, window);
    let chars4 = c.estimate.bytes / 4;
    assert!(chars4 > budget * 64 / 100, "{chars4}");
    assert_eq!(
        c.trigger.as_deref(),
        Some("new_session"),
        "{:?}",
        c.estimate
    );
    assert!(c.est_tokens < chars4 * 115 / 100, "{:?}", c.estimate);
    // Counted, GLM's own count of 90 % of the budget does not ring.
    let first = run_in(&hi(), None, &glm, 1, window);
    let mut counted = hi();
    counted.push((
        2,
        answered(
            vec![json!({"type": "text", "text": "ok"})],
            &first.compilation.id,
            budget * 90 / 100,
            2,
        ),
    ));
    counted.push((3, Node::user("s", None, "web", "and then?")));
    let c = run_in(&counted, Some(&first.compilation), &glm, 3, window);
    assert_eq!(c.estimate.method, EstimateMethod::Counted);
    assert!(!c.new_compilation, "{:?}", c.estimate);
}
