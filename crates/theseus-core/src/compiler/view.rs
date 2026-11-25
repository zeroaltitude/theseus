//! The strategy `view` (issue 44; design §5.1 "The order on the wire" and
//! "Frozen within a turn"): a turn's request built from its view, the only way a request is built since the
//! session's transcript was removed (2026-11-22).
//!
//! On the wire: the system part as the transcript's, with the memory page after the session's block; then,
//! as harness blocks heading the first user message, the fixed blocks (who and where, his older words) and
//! the ladder's nodes; then the window, the place's newest exchanges as REAL turns, read from the store
//! across the sessions that ran them and rendered by the compiler's own renderer (calls paired with their
//! results; their thinking not carried); then this turn: one block beside the new message, the message, and
//! the turn's own calls, after the notes written into its session since its previous turn
//! (`between`). The window's end carries a cache mark, so the front is read from the cache on the
//! next turn; what moves every turn comes after it.
//!
//! Frozen within a turn: the turn's first call builds a [`Frozen`] once (`turn::view_step`) and every later
//! call renders the same front, window and beside block, so each request of the turn begins with the bytes
//! of the one before (the thinking binding, `docs/design/thinking-binding.md`). The next turn builds afresh.
//! Nothing of the view is written into the session: the beside block lives in the turn alone.
//! `docs/design/view-compile.md` has the whole of it.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::*;
use crate::stub::Stub;

pub(crate) mod between;
pub mod kept;

/// What a render reads besides the nodes: the blobs, what the session does not show, the cut answer a call
/// replaces, and the recall's sources.
type RenderMedia<'a> = (
    Option<&'a crate::blobs::Blobs>,
    &'a [crate::session::NotShown],
    Option<&'a str>,
    &'a crate::recall::render::Sources,
);

/// The compilation's `strategy`.
pub const STRATEGY: &str = "view";
/// The compilation's `trigger`: every turn in a view place builds its own.
pub const TRIGGER: &str = "view_turn";
/// The provider's most cache breakpoints in one request.
const MAX_BREAKPOINTS: usize = 4;
/// Tool output in the window that sets off a clearing pass, in tokens of results not yet cleared (design
/// §5.2: the build's rule, with its own high mark for a window counted in exchanges).
pub const CLEAR_AT_TOKENS: u64 = 80_000;
/// The newest results a pass keeps whole, in tokens.
pub const KEEP_TOKENS: u64 = 40_000;
/// The meta key of a window result laid as its receipt ([`receipted`]): its content is already the stub a
/// clearing pass would make of it.
pub const RECEIPT: &str = "view_receipt";

/// Whether `r` is a window result laid as its receipt.
pub fn is_receipt(r: &Node) -> bool {
    matches!(&r.body, Body::ToolResult { meta, .. } if meta.get(RECEIPT) == Some(&Value::Bool(true)))
}

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
    /// The turn's own answers whose thinking a reset retired (a seal since, a refused image under it, a
    /// forced recompile): sent without it from then on.
    pub stripped: HashSet<String>,
    /// The sessions whose exchanges the view lays, the ladder's fold nodes' included: re-checked against
    /// their seals before each call.
    pub sessions: Vec<String>,
    /// The exchanges it lays or folds (the window's, the shown pieces' and the fold nodes' constituents):
    /// their messages' node seals are re-checked before each call, as the sessions' seals are.
    pub exchanges: Vec<String>,
    /// The verified point of view it was assembled under; a different reading before a later call (a steer
    /// narrowed it) reassembles under the new permit.
    pub verified: Option<theseus_memory::gate::PointOfView>,
    /// The block beside the new message (waiting, in play, elsewhere lately, called).
    pub beside: Option<Node>,
    /// The memory page, at the end of the system part; empty for none.
    pub memory_page: String,
    /// The view the turn was built from, until its record is kept with the turn's compilation.
    pub record: Option<crate::view::budget::View>,
    /// The room the turn made when a request passed the budget (`turn::view_step::room`): its clearing
    /// holds for every new chain of the turn, and every reassembly carries it.
    pub room: crate::view::record::Room,
    /// The floor did not fit (rule 9): nothing is laid, and every call is over by the floor's numbers.
    pub floor: Option<crate::view::budget::Overflow>,
    /// The re-admitted candidates of today's recall (what recall's admission let through, before the view's
    /// gate and budget chose what to lay), admitted again by a reassembly and re-asked before every call
    /// (`turn::view_step::recall`).
    pub recalled: Vec<crate::node::RecalledRef>,
    /// The turn's sizes as its requests are finished and sent (`turn::view_step::room`), carried by every
    /// reassembly.
    pub fit: Fit,
    /// The turn's own results laid as their head, by node id and characters shown: the last room before an
    /// overage (`turn::view_step::room`), carried by every reassembly.
    pub cut: HashMap<String, usize>,
}

/// A view turn's sizes beyond what its view charges, measured on its requests as they are sent: the room
/// it makes reserves them, and the request that goes out is held to them (`turn::view_step::room`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fit {
    /// The last compile's estimate before the request was finished: its upper bound and its tokens.
    pub base: (u64, u64),
    /// What finishing (its tails) added to the last request of the current chain, upper bound and tokens.
    pub tail_on: (u64, u64),
    /// What finishing added to this call's own request that the size check sent back (`Size::Again`): the
    /// repair of that same call is charged it, and it is cleared once the call fits or fails. Never a
    /// tail of an earlier request.
    pub finish: (u64, u64),
    /// What the view's assembly charged, last assembled.
    pub charged: u64,
    /// The request's tokens beyond what the view charged (the turn's own nodes, the tools, the tails).
    pub own: u64,
    /// The estimate of a request the provider refused for its size: every later one is strictly smaller.
    pub under: Option<u64>,
    /// Tokens more to reserve, measured by this room pass's last step (not carried to the next pass).
    pub squeeze: u64,
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
    let (spec, current) = chain_spec(&input, frozen);
    let spec = &spec;
    let new_compilation = current.is_none();
    let entry = input.catalog.get(&spec.model);
    let window = input.window_override.or(entry.map(|e| e.context_window));
    let compilation = current
        .cloned()
        .unwrap_or_else(|| made_now(&input, frozen, spec, window));
    // A new chain in the middle of a turn: the turn's answers so far go without their thinking.
    let mut stripped = frozen.stripped.clone();
    if new_compilation && frozen.compilation_id.is_some() {
        stripped.extend(own_answers(input.nodes, &frozen.turn_id));
    }
    let laid = laid(input.nodes, frozen, &stripped);
    let at = (compilation, new_compilation, frozen.room.keep);
    compiled_from(&input, spec, at, &laid)
}

/// The spec a call renders, and the chain's compilation it appends to (none: a new chain).
fn chain_spec<'c>(
    input: &CompileInput<'c>,
    frozen: &Frozen,
) -> (RequestSpec, Option<&'c Compilation>) {
    let window_of = |s: &RequestSpec| {
        let entry = input.catalog.get(&s.model);
        input.window_override.or(entry.map(|e| e.context_window))
    };
    // The transcript's spec preparation (`super::prepared`): a chain's first call renders the recompile's
    // spec (the walk's guidance, the resolved tools); its later calls the append's. An effective spec that
    // changed since the chain began (a
    // family `tools_find` loaded, a changed system, tools or model) starts a new chain, as the transcript
    // recompiles on a changed digest.
    let (appended, recompiled) = prepared(input);
    let fresh = || effective(&recompiled, &frozen.memory_page);
    let chained = input
        .current
        .filter(|c| frozen.compilation_id.as_deref() == Some(c.id.as_str()));
    let (spec, current) = match chained {
        None => (fresh(), None),
        Some(c) => {
            let base = appended.as_deref().unwrap_or(&*recompiled);
            let s = effective(base, &frozen.memory_page);
            let m = manifest_for(&s, input.catalog, window_of(&s), true);
            let same = (m.system_digest == c.manifest.system_digest)
                && m.tools_digest == c.manifest.tools_digest
                && m.model == c.manifest.model
                && m.provider == c.manifest.provider;
            match same {
                true => (s, Some(c)),
                false => (fresh(), None),
            }
        }
    };
    (spec, current)
}

/// The request of a call from its spec, its compilation (whether it is new, and the turn's room-making keep)
/// and the laid nodes: the clearing pass at a chain's first call, the render, the estimate and the report.
fn compiled_from(
    input: &CompileInput<'_>,
    spec: &RequestSpec,
    (mut compilation, new_compilation, keep): (Compilation, bool, Option<u64>),
    laid: &(Vec<(u64, Stub)>, u64),
) -> Compiled {
    let entry = input.catalog.get(&spec.model);
    let window = input.window_override.or(entry.map(|e| e.context_window));
    let rates = entry.map_or_else(|| TokenRates::of(&spec.model), |e| e.bytes_per_token);
    // A cut answer this call replaces is left out, as the transcript leaves it (theseus-9p88).
    let retrying = input.overflowed.and_then(|o| o.retrying.as_deref());
    let media = (input.blobs, input.hidden, retrying, input.sources);
    let made = |c: &Compilation| {
        let split = split(c, laid);
        let rendered = render(spec, input.catalog, &split, &laid.0, media);
        let counted = counted_part(&laid.0, &split, &rendered.request);
        let est = estimate(&rendered.request, rates, counted);
        (split, rendered, est)
    };
    let (split, mut rendered, mut est) = made(&compilation);
    // Fixed between clearings: a pass runs only as a turn begins, and its stubs hold after.
    let mut cleared = None;
    if let Some(pass) = new_compilation
        .then(|| clear(&split, &laid.0, (&rendered, &est), keep))
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
        situation,
        cleared,
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
/// becomes its receipt (the newest exchange's stay whole). A turn that made room (`keep`) runs it whatever
/// the line, keeping that many tokens of the newest whole. The drops, the results and their characters.
fn clear(
    split: &Compilation,
    nodes: &[(u64, Stub)],
    (rendered, est): (&Rendered, &Estimate),
    keep: Option<u64>,
) -> Option<(Vec<BudgetDrop>, u64, u64)> {
    if !clearing::any_old(nodes) {
        return None;
    }
    let shown: Vec<(u64, &Node)> = nodes.iter().map(|(p, n)| (*p, &**n)).collect();
    let already = clearing::of(split);
    let per_byte = est.tokens as f64 / rendered.request.json_bytes().max(1) as f64;
    if keep.is_none() && clearing::live_tokens(&shown, &already, per_byte) <= CLEAR_AT_TOKENS {
        return None;
    }
    let (drops, chars) = clearing::pass(&shown, &already, keep.unwrap_or(KEEP_TOKENS), per_byte);
    let results = drops.len() as u64;
    (!drops.is_empty()).then_some((drops, results, chars))
}

/// The nodes as the request lays them, renumbered so the compiler's split reads the front and the window as
/// the prefix and the beside block and the turn's own nodes as the tail; and where the prefix ends.
fn laid(
    nodes: &[(u64, Stub)],
    frozen: &Frozen,
    stripped: &HashSet<String>,
) -> (Vec<(u64, Stub)>, u64) {
    let turn = frozen.turn_id.as_str();
    let front = frozen.front.iter().map(|n| Stub::from(n.clone()));
    let window = frozen.window.iter().map(|(_, n)| n.clone());
    let prefix: Vec<Stub> = front.chain(window).collect();
    // A recall note is the transcript's: in a view, recall is a selector's piece beside the message.
    let laid: HashSet<&str> = (frozen.window.iter())
        .filter_map(|(_, n)| n.turn_id.as_deref())
        .collect();
    let notes = between::notes(nodes, turn, &laid);
    let own = nodes
        .iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn) || notes.contains(&n.id))
        .filter(|(_, n)| n.kind != crate::stub::Kind::Recall)
        .map(|(_, n)| match stripped.contains(&n.id) {
            true => unthought(n),
            false => n.clone(),
        })
        .map(|n| match frozen.cut.get(&n.id) {
            Some(shown) => headed(&n, *shown),
            None => n,
        });
    let tail = frozen.beside.iter().cloned().map(Stub::from).chain(own);
    let as_of = prefix.len() as u64;
    let all = prefix.into_iter().chain(tail).enumerate();
    (all.map(|(i, n)| (i as u64 + 1, n)).collect(), as_of)
}

/// The compilation the laid nodes are split under: `c`, its prefix the first `as_of` of them, its thinking
/// stripped (earlier turns' thinking is not carried).
fn split(c: &Compilation, (nodes, as_of): &(Vec<(u64, Stub)>, u64)) -> Compilation {
    let includes = nodes
        .iter()
        .take(*as_of as usize)
        .map(|(_, n)| n.id.clone())
        .collect();
    Compilation {
        as_of: *as_of,
        includes,
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
    media: RenderMedia<'_>,
) -> Rendered {
    let mut rendered = render_request(spec, catalog, split, nodes, media);
    let first_tail = nodes
        .iter()
        .find(|(p, _)| *p > split.as_of)
        .and_then(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        });
    // Who am I alone (a fresh conversation's front, `view::who`) takes no mark of its own: the conversation's
    // mark at the end holds it, and the system keeps its marks as they were before the block was laid.
    let only_who = split.as_of == 1
        && nodes.first().is_some_and(|(_, n)| {
            matches!(&n.body, Body::UserMessage { text, .. } if text.starts_with(crate::view::who::HEAD))
        });
    if !only_who {
        mark_front(&mut rendered, first_tail.as_deref(), spec);
    }
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

/// A view turn's request rendered again from its frozen view (`kept::frozen`), the compilation it was
/// stored as, and the session's nodes before its answer: the bytes [`compile`] sent, for a replay
/// (`own_bench::rebuild`). `spec` is the request's own, its system blocks read back from their blobs.
pub fn rendered_again(
    (spec, catalog): (&RequestSpec, &Catalog),
    c: &Compilation,
    frozen: &Frozen,
    nodes: &[(u64, Stub)],
    media: RenderMedia<'_>,
) -> ProviderRequest {
    let laid = laid(nodes, frozen, &frozen.stripped);
    render(spec, catalog, &split(c, &laid), &laid.0, media).request
}

/// The turn's own answers so far, by node id.
pub fn own_answers<'a>(
    nodes: &'a [(u64, Stub)],
    turn: &'a str,
) -> impl Iterator<Item = String> + 'a {
    (nodes.iter())
        .filter(move |(_, n)| n.turn_id.as_deref() == Some(turn))
        .filter(|(_, n)| n.kind == crate::stub::Kind::AssistantMessage)
        .map(|(_, n)| n.id.clone())
}

/// The spec a call renders: `s` with the memory page.
fn effective(s: &RequestSpec, page: &str) -> RequestSpec {
    let s = s.clone();
    match with_page(&s, page) {
        Some(p) => p,
        None => s,
    }
}

/// The spec a turn's first call would render from `s` today, the memory page in it: what
/// `context.explain` digests to say whether a turn's system is unchanged.
pub(crate) fn first_call_spec(s: &RequestSpec) -> RequestSpec {
    effective(s, crate::view::text::page::memory_page().1)
}

/// A result of the turn's own laid as its first `shown` characters, ending before any known value they
/// would split, with one line saying how many are shown.
fn headed(n: &Stub, shown: usize) -> Stub {
    let mut node = (**n).clone();
    if let Body::ToolResult { content, .. } = &mut node.body {
        let all = content.chars().count();
        if shown < all {
            let (head, _) = crate::scrub::safe::head(content, shown);
            let n = crate::narrative::thousands;
            *content = format!(
                "{head}\n[Only the first {} of {} characters of this result are shown, to fit the \
                 window; read the rest in parts with the same tool.]",
                n(head.chars().count() as u64),
                n(all as u64)
            );
        }
    }
    Stub::from(node)
}

/// An answer of the turn's own without its thinking: a reset retired it.
fn unthought(n: &Stub) -> Stub {
    let mut node = (**n).clone();
    if let Body::AssistantMessage { blocks, .. } = &mut node.body {
        blocks.retain(|b| !super::render::is_thinking(b));
    }
    Stub::from(node)
}

/// The window's nodes with the results of `spoken` exchanges as their receipts, made once, from the window
/// alone, when the view is frozen: a receipt's "after it you wrote" is the next answer of its OWN exchange,
/// or nothing, and never changes after it was first sent (a result with no answer after it never borrows
/// this turn's first answer).
pub fn receipted(window: Vec<(u64, Stub)>, spoken: &HashSet<String>) -> Vec<(u64, Stub)> {
    // A result's call is the one it provably answers (`pairing`, issue 44), as the render lays it: a window lays
    // many exchanges, and a provider that numbers its calls per response repeats ids between them.
    let mut by_call: std::collections::HashMap<(String, String), (String, Value)> =
        std::collections::HashMap::new();
    for (_, n) in &window {
        let Body::AssistantMessage { blocks, .. } = &n.body else {
            continue;
        };
        for b in blocks.iter().filter(|b| b["type"] == "tool_use") {
            let id = b["id"].as_str().unwrap_or_default().to_string();
            let call = (
                b["name"].as_str().unwrap_or_default().to_string(),
                b["input"].clone(),
            );
            by_call.insert((n.id.clone(), id), call);
        }
    }
    let paired: std::collections::HashMap<String, (String, Value)> = {
        let p = super::pairing::pairing(window.iter().map(|(_, n)| &**n));
        (window.iter())
            .filter_map(|(_, n)| {
                let (node, id) = p.call_of(&n.id)?;
                let call = by_call.get(&(node.to_string(), id.to_string()))?;
                Some((n.id.clone(), call.clone()))
            })
            .collect()
    };
    // The note a clearing pass writes (`clearing::notes`): so a receipt is, byte for byte, the stub that
    // clearing makes of the same result, and a cleared result reads the same whether its exchange is laid
    // whole or spoken (the cache's prefix holds when an exchange's form changes).
    let answer_after = |i: usize| -> Option<String> {
        let turn = window[i].1.turn_id.clone();
        (window[i + 1..].iter())
            .take_while(|(_, n)| n.turn_id == turn)
            .find_map(|(_, n)| match &n.body {
                Body::AssistantMessage { blocks, .. } => Some(blocks),
                _ => None,
            })
            .and_then(|blocks| super::clearing::remember::note_of(blocks))
    };
    let notes: Vec<Option<String>> = (0..window.len()).map(answer_after).collect();
    window
        .iter()
        .enumerate()
        .map(|(i, (p, n))| {
            let Body::ToolResult { tool, .. } = &n.body else {
                return (*p, n.clone());
            };
            if !spoken.contains(&n.id) {
                return (*p, n.clone());
            }
            // A result that answers no call here (a late one, one whose call is not established) is named by
            // its own tool, as the spoken form names it.
            let (name, input) =
                (paired.get(&n.id).cloned()).unwrap_or_else(|| (tool.clone(), Value::Null));
            let text = super::clearing::stub_noted(&name, &input, n, notes[i].as_deref());
            let mut node = (**n).clone();
            if let Body::ToolResult { content, meta, .. } = &mut node.body {
                *content = text;
                // Already its stub: a clearing pass that holds it lays it as it is (`render`).
                meta[RECEIPT] = Value::Bool(true);
            }
            (*p, Stub::from(node))
        })
        .collect()
}
