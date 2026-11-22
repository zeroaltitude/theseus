//! The strategy `view` (issue 44, `[context] assembly = "view"`; design §5.1 "The order on the wire" and
//! "Frozen within a turn"): a turn's request built from its view instead of its session's transcript.
//!
//! On the wire: the system part as the transcript's, with the memory page after the session's block; then,
//! as harness blocks heading the first user message, the fixed blocks (who and where, his older words) and
//! the ladder's nodes; then the window, the place's newest exchanges as REAL turns, read from the store
//! across the sessions that ran them and rendered by the compiler's own renderer (calls paired with their
//! results; their thinking not carried); then this turn: one block beside the new message, the message, and
//! the turn's own calls. The window's end carries a cache mark, so the front is read from the cache on the
//! next turn; what moves every turn comes after it.
//!
//! Frozen within a turn: the turn's first call builds a [`Frozen`] once (`turn::view_step`) and every later
//! call renders the same front, window and beside block, so each request of the turn begins with the bytes
//! of the one before (the thinking binding, `docs/design/thinking-binding.md`). The next turn builds afresh.
//! Nothing of the view is written into the session: the beside block lives in the turn alone.
//! `docs/design/view-compile.md` has the whole of it.

use std::collections::HashSet;

use serde_json::Value;

use super::*;
use crate::stub::Stub;

/// The compilation's `strategy`.
pub const STRATEGY: &str = "view";
/// The compilation's `trigger`: every turn in a view place builds its own.
pub const TRIGGER: &str = "view_turn";
/// The reason a window result renders as a receipt because the budgeter kept its exchange spoken.
const SPOKEN: &str = "spoken";
/// The provider's most cache breakpoints in one request.
const MAX_BREAKPOINTS: usize = 4;
/// Tool output in the window that sets off a clearing pass, in tokens of results not yet cleared (design
/// §5.2: the build's rule, with its own high mark for a window counted in exchanges).
pub const CLEAR_AT_TOKENS: u64 = 80_000;
/// The newest results a pass keeps whole, in tokens.
pub const KEEP_TOKENS: u64 = 40_000;

/// What a turn in a view place sends besides its own nodes, frozen at its first call.
#[derive(Debug, Clone, Default)]
pub struct Frozen {
    pub turn_id: String,
    /// The compilation its first call made; the turn's later calls append to it.
    pub compilation_id: Option<String>,
    /// The fixed blocks and the ladder's nodes, each one harness node, in wire order.
    pub front: Vec<Node>,
    /// The window's nodes, oldest first, from every session that ran its exchanges.
    pub window: Vec<(u64, Stub)>,
    /// The results of the window's exchanges the budgeter kept spoken: they render as receipts.
    pub spoken: Vec<String>,
    /// The block beside the new message (waiting, in play, elsewhere lately, called).
    pub beside: Option<Node>,
    /// The memory page, at the end of the system part; empty for none.
    pub memory_page: String,
    /// The view the turn was built from, until its record is kept with the turn's compilation.
    pub record: Option<crate::view::budget::View>,
}

impl Frozen {
    /// The marked blocks' text as a harness node of this turn.
    pub fn block(session_id: &str, turn_id: &str, heading: &str, text: &str) -> Node {
        let body = match heading.is_empty() {
            true => text.to_string(),
            false => format!("{heading}\n\n{text}"),
        };
        Node::user(session_id, Some(turn_id), "harness", &body)
    }
}

/// The spec with the memory page after the session's block.
fn with_page(spec: &RequestSpec, page: &str) -> Option<RequestSpec> {
    (!page.is_empty()).then(|| {
        let mut s = spec.clone();
        s.session_text = match s.session_text.is_empty() {
            true => page.to_string(),
            false => format!("{}\n\n{page}", s.session_text),
        };
        s
    })
}

/// Compile one call of a turn in a view place: the turn's first call makes the compilation (the window's
/// node ids its `includes`), a later call of the same turn appends to it. `input.nodes` is the session's
/// transcript; the turn's own nodes are read from it by turn id.
pub fn compile(input: CompileInput<'_>, frozen: &Frozen) -> Compiled {
    let page = with_page(input.spec, &frozen.memory_page);
    let spec = page.as_ref().unwrap_or(input.spec);
    let entry = input.catalog.get(&spec.model);
    let window = input.window_override.or(entry.map(|e| e.context_window));
    let current = input
        .current
        .filter(|c| frozen.compilation_id.as_deref() == Some(c.id.as_str()));
    let new_compilation = current.is_none();
    let mut compilation = current
        .cloned()
        .unwrap_or_else(|| made_now(&input, frozen, spec, window));
    let laid = laid(input.nodes, frozen);
    let rates = entry.map_or_else(|| TokenRates::of(&spec.model), |e| e.bytes_per_token);
    let made = |c: &Compilation| {
        let split = split(c, &laid, &frozen.spoken);
        let rendered = render(spec, input.catalog, &split, &laid.0, &input);
        let counted = counted_part(&laid.0, &split, &rendered.request);
        let est = estimate(&rendered.request, rates, counted);
        (split, rendered, est)
    };
    let (split, mut rendered, mut est) = made(&compilation);
    // Fixed between clearings: a pass runs only as a turn begins, and its stubs hold after.
    let mut cleared = None;
    if let Some(pass) = new_compilation
        .then(|| clear(&split, &laid.0, &rendered, &est))
        .flatten()
    {
        let (drops, results, chars) = pass;
        let before = est.tokens;
        let b = compilation.budget.get_or_insert_with(Default::default);
        let kept = b.dropped.len() as u64;
        b.dropped.extend(drops);
        (_, rendered, est) = made(&compilation);
        cleared = Some(Pass {
            results,
            chars,
            kept,
            tokens_before: before,
            tokens_after: est.tokens,
        });
    }
    let Rendered {
        request,
        prefix_nodes,
        tail_nodes,
        repairs,
        ..
    } = rendered;
    est.bytes = request.json_bytes();
    let limit = window.map(|w| request_budget(w, spec));
    let drops = clearing::drops(compilation.budget.as_ref()).cloned();
    let budget = budget_report(limit, &est, drops.collect::<Vec<_>>());
    let trigger = new_compilation.then(|| TRIGGER.to_string());
    let situation = situation::settle(
        input.situation,
        input.current.is_none(),
        new_compilation,
        trigger.as_deref(),
    );
    if new_compilation {
        compilation.budget = Some(budget.clone());
        compilation.situation = Some(situation.clone());
    }
    Compiled {
        budget,
        messages: request.messages.len(),
        digest: request.digest(),
        request,
        compilation,
        new_compilation,
        trigger,
        prefix_nodes,
        tail_nodes,
        est_tokens: est.tokens,
        estimate: est,
        repairs,
        cache: cache_layout(spec, input.catalog),
        withheld: spec
            .context_files
            .iter()
            .filter(|f| f.withheld.is_some())
            .count() as u64,
        signals: Default::default(),
        situation,
        cleared,
        card: None,
        prefix: None,
    }
}

/// The turn's compilation, made at its first call: the window's node ids its `includes`, its thinking
/// stripped, and what the turn before cleared still cleared.
fn made_now(
    input: &CompileInput<'_>,
    frozen: &Frozen,
    spec: &RequestSpec,
    window: Option<u64>,
) -> Compilation {
    let includes: Vec<String> = frozen.window.iter().map(|(_, n)| n.id.clone()).collect();
    Compilation {
        id: crate::new_id("cmp"),
        schema: COMPILATION_SCHEMA,
        session_id: input.session_id.into(),
        created_at_ms: theseus_protocol::now_unix_ms(),
        trigger: TRIGGER.into(),
        strategy: STRATEGY.into(),
        as_of: input.last_position,
        budget: carried(input.current, &includes),
        includes,
        derived_from: input.current.map(|c| c.id.clone()),
        manifest: manifest_for(spec, input.catalog, window, true),
        recall_id: None,
        situation: None,
        inherits: None,
    }
}

/// The results the turn before cleared that are still in the window: a clearing holds.
fn carried(current: Option<&Compilation>, includes: &[String]) -> Option<BudgetReport> {
    let ids: HashSet<&str> = includes.iter().map(String::as_str).collect();
    let dropped: Vec<BudgetDrop> = clearing::drops(current.and_then(|c| c.budget.as_ref()))
        .filter(|d| d.node_id.as_deref().is_some_and(|id| ids.contains(id)))
        .cloned()
        .collect();
    (!dropped.is_empty()).then(|| BudgetReport {
        dropped,
        ..Default::default()
    })
}

/// The build's clearing pass over the laid nodes, with this strategy's numbers: when the results not yet
/// cleared pass [`CLEAR_AT_TOKENS`], every result over `MIN_CHARS` older than the newest [`KEEP_TOKENS`]
/// becomes its receipt (the newest exchange's stay whole). The drops, the results and their characters.
fn clear(
    split: &Compilation,
    nodes: &[(u64, Stub)],
    rendered: &Rendered,
    est: &Estimate,
) -> Option<(Vec<BudgetDrop>, u64, u64)> {
    if !clearing::any_old(nodes) {
        return None;
    }
    let shown: Vec<(u64, &Node)> = nodes.iter().map(|(p, n)| (*p, &**n)).collect();
    let already = clearing::of(split);
    let per_byte = est.tokens as f64 / rendered.request.json_bytes().max(1) as f64;
    if clearing::live_tokens(&shown, &already, per_byte) <= CLEAR_AT_TOKENS {
        return None;
    }
    let (drops, chars) = clearing::pass(&shown, &already, KEEP_TOKENS, per_byte);
    let results = drops.len() as u64;
    (!drops.is_empty()).then_some((drops, results, chars))
}

/// The nodes as the request lays them, renumbered so the compiler's split reads the front and the window as
/// the prefix and the beside block and the turn's own nodes as the tail; and where the prefix ends.
fn laid(nodes: &[(u64, Stub)], frozen: &Frozen) -> (Vec<(u64, Stub)>, u64) {
    let turn = frozen.turn_id.as_str();
    let front = frozen.front.iter().map(|n| Stub::from(n.clone()));
    let window = frozen.window.iter().map(|(_, n)| n.clone());
    let prefix: Vec<Stub> = front.chain(window).collect();
    // A recall note is the transcript's: in a view, recall is a selector's piece beside the message.
    let own = nodes
        .iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn) && n.kind != crate::stub::Kind::Recall)
        .map(|(_, n)| n.clone());
    let tail = frozen.beside.iter().cloned().map(Stub::from).chain(own);
    let as_of = prefix.len() as u64;
    let all = prefix.into_iter().chain(tail).enumerate();
    (all.map(|(i, n)| (i as u64 + 1, n)).collect(), as_of)
}

/// The compilation the laid nodes are split under: `c`, its prefix the first `as_of` of them, its thinking
/// stripped (earlier turns' thinking is not carried).
fn split(
    c: &Compilation,
    (nodes, as_of): &(Vec<(u64, Stub)>, u64),
    spoken: &[String],
) -> Compilation {
    let includes = nodes
        .iter()
        .take(*as_of as usize)
        .map(|(_, n)| n.id.clone())
        .collect();
    // An exchange kept spoken shows its results as receipts, in this render only.
    let mut budget = c.budget.clone();
    if !spoken.is_empty() {
        let b = budget.get_or_insert_with(Default::default);
        b.dropped.extend(spoken.iter().map(|id| BudgetDrop {
            node_id: Some(id.clone()),
            range: None,
            reason: SPOKEN.into(),
            tokens: 0,
            tier: clearing::TIER.into(),
        }));
    }
    Compilation {
        as_of: *as_of,
        includes,
        budget,
        recall_id: None,
        inherits: None,
        manifest: Manifest {
            strip_thinking: true,
            ..c.manifest.clone()
        },
        ..c.clone()
    }
}

/// Render the laid nodes under their split, with the cache mark at the window's end and no more breakpoints
/// than the provider takes.
fn render(
    spec: &RequestSpec,
    catalog: &Catalog,
    split: &Compilation,
    nodes: &[(u64, Stub)],
    input: &CompileInput<'_>,
) -> Rendered {
    let media = (input.blobs, input.hidden, None, input.sources);
    let mut rendered = render_kept(spec, catalog, split, nodes, media, None);
    let first_tail = nodes
        .iter()
        .find(|(p, _)| *p > split.as_of)
        .and_then(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        });
    mark_front(&mut rendered, first_tail.as_deref(), spec);
    rendered
}

/// The cache mark where the fixed front ends: on the last block before the tail's first. The conversation's
/// automatic mark stays at the end; a system layer's mark gives way when the request would carry more
/// breakpoints than the provider takes.
fn mark_front(rendered: &mut Rendered, first_tail: Option<&str>, spec: &RequestSpec) {
    let req = &mut rendered.request;
    if req.cache_control.is_none() || rendered.prefix_nodes == 0 {
        return;
    }
    let Some(at) = rendered.tail_from.checked_sub(1) else {
        return;
    };
    let marker = spec.conversation_ttl.min(spec.cache_ttl).marker();
    let Some(blocks) = req.messages[at]["content"].as_array_mut() else {
        return;
    };
    let is_first = |b: &Value| first_tail.is_some_and(|t| b["text"].as_str() == Some(t));
    let end = blocks.iter().position(is_first).unwrap_or(blocks.len());
    let Some(b) = end.checked_sub(1).and_then(|i| blocks.get_mut(i)) else {
        return;
    };
    b["cache_control"] = marker;
    // The window's mark and the conversation's: the system keeps what is left, its first layer first.
    let room = MAX_BREAKPOINTS.saturating_sub(2);
    let marked: Vec<usize> = (req.system.iter().enumerate())
        .filter(|(_, b)| b.get("cache_control").is_some())
        .map(|(i, _)| i)
        .collect();
    let over = marked.len().saturating_sub(room);
    let drop: HashSet<usize> = marked.iter().skip(1).take(over).copied().collect();
    for i in drop {
        if let Some(o) = req.system[i].as_object_mut() {
            o.remove("cache_control");
        }
    }
}
