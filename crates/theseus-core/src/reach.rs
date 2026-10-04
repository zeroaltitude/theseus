//! `node.reach` (theseus-n4m, step 12a; design stage2 §2.11): where a node
//! went, derived when it is asked from records that already say so.
//!
//! - **Direct exposure**, read from the node's own session: each compilation
//!   whose prefix (`includes`) holds it, and each loop after it whose context
//!   held it. A loop's reply names its compilation, which holds the node or
//!   left it in the tail (after `as_of`), as `render_request` builds the
//!   request. Every compilation today admits only its own session's nodes, so
//!   no index is needed.
//! - **Descendants**: the edges into the node (the scope `in:<node>`), each a
//!   copy in another session, with its own exposure, generation by
//!   generation.
//!
//! **Why derive, and not store.** §6.1 writes the reverse `includes` edges in
//! the compilation's frame. Every `includes` today is a run of the session's
//! own nodes, and the compilation record already holds it: about 300 reverse
//! edges per recompile would repeat it in every recompile's frame. The first
//! route that admits a node from outside its session (M6's recall, M7's
//! borrowing) brings its own reverse entries, and this reads them then.
//!
//! **FAST.** Nothing runs until asked, and nothing at startup. One scan per
//! session reached, from its earliest node on, and one scope scan per node
//! for its copies.

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use serde::Deserialize;
use theseus_protocol::{
    NodeReachResult, ReachCompilation, ReachDescendant, ReachExposure, ReachTotals,
};
use theseus_store::kinds;

use crate::compiler::Compilation;
use crate::graph::{Edge, EdgeKind};
use crate::node::Node;
use crate::store::Store;

/// Generations of copies followed when the request names none.
pub const DEFAULT_GENERATIONS: u32 = 3;
/// The most a request may ask for.
pub const MAX_GENERATIONS: u32 = 16;
/// The most copies one walk follows; past it the answer is partial.
pub const MAX_DESCENDANTS: usize = 256;

/// A node the walk reached: the root, or a copy.
struct Reached {
    node: Node,
    position: u64,
    generation: u32,
    /// The edge that reached it, from the node it copies (`to`).
    by: Option<Edge>,
}

/// Where `node_id` went; None when there is no such node.
pub fn reach(
    store: &Store,
    node_id: &str,
    max_generations: Option<u32>,
) -> Result<Option<NodeReachResult>> {
    let max = max_generations
        .unwrap_or(DEFAULT_GENERATIONS)
        .min(MAX_GENERATIONS);
    let Some((position, node)) = store.get_node(node_id)? else {
        return Ok(None);
    };
    let mut reached = vec![Reached {
        node,
        position,
        generation: 0,
        by: None,
    }];
    let mut seen: BTreeSet<String> = BTreeSet::from([node_id.to_string()]);
    let mut partial = false;
    let mut i = 0;
    'walk: while i < reached.len() {
        let (id, generation) = (reached[i].node.id.clone(), reached[i].generation);
        i += 1;
        let copies = copies_of(store, &id)?;
        if copies.is_empty() {
            continue;
        }
        if generation >= max {
            partial = true;
            continue;
        }
        for e in copies {
            if !seen.insert(e.from.clone()) {
                continue;
            }
            if reached.len() > MAX_DESCENDANTS {
                partial = true;
                break 'walk;
            }
            let Some((position, node)) = store.get_node(&e.from)? else {
                continue;
            };
            reached.push(Reached {
                node,
                position,
                generation: generation + 1,
                by: Some(e),
            });
        }
    }

    // Each session reached, read once.
    let mut by_session: HashMap<&str, Vec<usize>> = HashMap::new();
    for (k, r) in reached.iter().enumerate() {
        by_session
            .entry(r.node.session_id.as_str())
            .or_default()
            .push(k);
    }
    let mut exposure: Vec<ReachExposure> = vec![ReachExposure::default(); reached.len()];
    for (session, ks) in &by_session {
        let nodes: Vec<(u64, &Node)> = ks
            .iter()
            .map(|k| (reached[*k].position, &reached[*k].node))
            .collect();
        for (k, e) in ks.iter().zip(exposures(store, session, &nodes)?) {
            exposure[*k] = e;
        }
    }

    let totals = ReachTotals {
        contexts: exposure
            .iter()
            .map(|e| e.compilations.len() as u64 + e.loops)
            .sum(),
        sessions: by_session.len() as u32,
    };
    let mut exposure = exposure.into_iter();
    let direct = exposure.next().unwrap_or_default();
    let mut walked = reached.into_iter();
    let root = walked.next().expect("the root");
    let descendants = walked
        .zip(exposure)
        .filter_map(|(r, exposure)| {
            let edge = r.by?;
            Some(ReachDescendant {
                node_id: r.node.id,
                session_id: r.node.session_id,
                position: r.position,
                generation: r.generation,
                via: edge.kind,
                route: edge.via,
                from: edge.to,
                exposure,
            })
        })
        .collect();
    Ok(Some(NodeReachResult {
        node_id: root.node.id,
        session_id: root.node.session_id,
        position: root.position,
        direct,
        descendants,
        totals,
        partial,
    }))
}

/// The copies of `id`: the edges into it (the scope `in:<id>`) of a kind
/// this build follows, in WAL order.
fn copies_of(store: &Store, id: &str) -> Result<Vec<Edge>> {
    let mut out = Vec::new();
    for r in store.scope_after(&Edge::scope_into(id), 0)? {
        if r.kind != kinds::EDGE {
            continue;
        }
        let e: Edge = r.decode()?;
        match EdgeKind::named(&e.kind) {
            Some(EdgeKind::DerivedFrom) => out.push(e),
            // The memory pass's (M6 31a): a likeness, not a copy.
            Some(EdgeKind::SameEntity | EdgeKind::Supersedes) => {}
            None => {
                // A kind a newer build wrote: no copy this build follows.
            }
        }
    }
    Ok(out)
}

/// What a reply says of the loop that wrote it, read without its blocks.
#[derive(Deserialize)]
struct Reply {
    created_at_ms: u64,
    body: ReplyBody,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReplyBody {
    AssistantMessage {
        #[serde(default)]
        compilation_id: Option<String>,
    },
    #[serde(other)]
    Other,
}

/// Each node's exposure in `session`, from one scan of it after the
/// earliest of `nodes`. A loop's compilation made before that is read by
/// its id.
fn exposures(store: &Store, session: &str, nodes: &[(u64, &Node)]) -> Result<Vec<ReachExposure>> {
    let from = nodes.iter().map(|(p, _)| *p).min().unwrap_or(0);
    let mut compilations: HashMap<String, (u64, Compilation)> = HashMap::new();
    let mut made: Vec<String> = Vec::new();
    // Each loop: its reply's position, its time, and its compilation.
    let mut loops: Vec<(u64, u64, String)> = Vec::new();
    for r in store.scope_after(session, from)? {
        match r.kind {
            kinds::COMPILATION => {
                let c: Compilation = r.decode()?;
                let id = c.id.clone();
                // A compilation written twice counts once, as its latest
                // record.
                if compilations.insert(id.clone(), (r.position, c)).is_none() {
                    made.push(id);
                }
            }
            kinds::NODE => {
                let reply: Reply = r.decode()?;
                if let ReplyBody::AssistantMessage {
                    compilation_id: Some(c),
                } = reply.body
                {
                    loops.push((r.position, reply.created_at_ms, c));
                }
            }
            _ => {}
        }
    }
    for (_, _, c) in &loops {
        if !compilations.contains_key(c) {
            if let Some(x) = store.get_compilation(c)? {
                compilations.insert(c.clone(), (0, x));
            }
        }
    }
    let mut out = Vec::with_capacity(nodes.len());
    for (p, n) in nodes {
        let holds = |c: &Compilation| c.includes.contains(&n.id);
        let mut e = ReachExposure::default();
        let seen = |at: u64, e: &mut ReachExposure| {
            e.first_ms = Some(e.first_ms.map_or(at, |f| f.min(at)));
            e.last_ms = Some(e.last_ms.map_or(at, |l| l.max(at)));
        };
        for id in &made {
            let (at, c) = &compilations[id];
            if at > p && holds(c) {
                e.compilations.push(ReachCompilation {
                    compilation_id: c.id.clone(),
                    strategy: c.strategy.clone(),
                    created_at_ms: c.created_at_ms,
                });
                seen(c.created_at_ms, &mut e);
            }
        }
        let in_tail = crate::compiler::renderable(n);
        for (at, ms, c) in &loops {
            if at <= p {
                continue;
            }
            let Some((_, c)) = compilations.get(c) else {
                continue;
            };
            if holds(c) || (in_tail && *p > c.as_of) {
                e.loops += 1;
                seen(*ms, &mut e);
            }
        }
        out.push(e);
    }
    Ok(out)
}
