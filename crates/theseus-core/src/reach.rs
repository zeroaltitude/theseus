//! `node.reach` (theseus-n4m, step 12a; design stage2 §2.11): where a node
//! went, derived when it is asked from records that already say so.
//!
//! - **Direct exposure**, read from the node's own session: each compilation
//!   whose prefix (`includes`) holds it, and each loop after it whose context
//!   held it. A loop's reply names its compilation, which holds the node or
//!   left it in the tail (after `as_of`), as `render_request` builds the
//!   request. Every compilation today admits only its own session's nodes, so
//!   no index is needed.
//! - **A view turn** (issue 44): its compilation's `includes` is the frozen window alone. What else its
//!   requests laid is read when asked, from the view record kept for it (`view::record`, the one `view
//!   show` reads): each recalled item, and each exchange laid with the form it was laid in (a person's words
//!   in every form, a reply's in the spoken and whole, tool traffic in the whole alone). And each call lays
//!   the turn's own nodes written before it. A node is exposed in every request it was laid in, whatever slot
//!   laid it; a fold lays its account, not its sources, so a fold's sources are not counted. A view record
//!   that cannot be read leaves the answer `partial`: never a silent "nowhere".
//! - **Descendants**: the edges into the node (the scope `in:<node>`), each a
//!   copy in another session, with its own exposure, generation by
//!   generation.
//!
//! **Why derive, and not store.** §6.1 writes the reverse `includes` edges in
//! the compilation's frame. Every `includes` today is a run of the session's
//! own nodes, and the compilation record already holds it: about 300 reverse
//! edges per recompile would repeat it in every recompile's frame. The first
//! route that admits a node from outside its session is a fork's
//! (theseus-2qzx): its frame writes one reverse entry for the whole run (an
//! `includes` edge from its first compilation into the source's session),
//! and `forks_of` reads them, generation by generation. M6's recall and M7's
//! borrowing bring their own when they admit by reference.
//!
//! **FAST.** Nothing runs until asked, and nothing at startup. One scan per
//! session reached, from its earliest node on, and one scope scan per node
//! for its copies.

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use serde::Deserialize;
use theseus_protocol::{
    NodeReachResult, ReachCompilation, ReachDescendant, ReachExposure, ReachFork, ReachTotals,
};
use theseus_store::kinds;

use crate::compiler::Compilation;
use crate::graph::{Edge, EdgeKind};
use crate::node::{Body, Node};
use crate::store::Store;
use crate::view::{Form, Pos, Space};

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
        let (got, unread) = exposures(store, session, &nodes)?;
        partial |= unread;
        for (k, e) in ks.iter().zip(got) {
            exposure[*k] = e;
        }
    }

    // The forks that took it by reference (theseus-2qzx).
    let (forks, forks_partial) = forks_of(store, &reached[0], max)?;
    partial |= forks_partial;
    let totals = totals_of(&exposure, &forks, by_session.len());
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
        forks,
        totals,
        partial,
    }))
}

/// What a walk reached in all: every context of the node, its copies and its
/// forks, and the sessions they are in.
fn totals_of(exposure: &[ReachExposure], forks: &[ReachFork], sessions: usize) -> ReachTotals {
    let contexts = |e: &ReachExposure| e.compilations.len() as u64 + e.loops;
    ReachTotals {
        contexts: exposure.iter().map(contexts).sum::<u64>()
            + forks.iter().map(|f| contexts(&f.exposure)).sum::<u64>(),
        sessions: (sessions + forks.len()) as u32,
    }
}

/// The forks that took the node by reference, generation by generation: the
/// `includes` edges into its session (one per fork, for the run), each kept
/// when its run reaches the node, and then the forks of each fork. The walk
/// is `max` generations deep; true when a fork beyond it was left.
fn forks_of(store: &Store, root: &Reached, max: u32) -> Result<(Vec<ReachFork>, bool)> {
    let mut out: Vec<ReachFork> = Vec::new();
    let mut partial = false;
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut level = vec![root.node.session_id.clone()];
    let mut generation = 0;
    while !level.is_empty() {
        let mut next = Vec::new();
        for session in &level {
            for r in store.scope_after(&Edge::scope_into(session), 0)? {
                if r.kind != kinds::EDGE {
                    continue;
                }
                let e: Edge = r.decode()?;
                if EdgeKind::named(&e.kind) != Some(EdgeKind::Includes) {
                    continue;
                }
                let Some(c) = store.get_compilation(&e.from)? else {
                    continue;
                };
                let reaches = c
                    .inherits
                    .as_ref()
                    .is_some_and(|i| i.through_position >= root.position);
                if !reaches || !seen.insert(c.session_id.clone()) {
                    continue;
                }
                if generation >= max {
                    partial = true;
                    continue;
                }
                let at = c.inherits.as_ref().map_or(0, |i| i.through_position);
                let nodes = [(root.position, &root.node)];
                let (got, unread) = exposures(store, &c.session_id, &nodes)?;
                partial |= unread;
                let exposure = got.into_iter().next().unwrap_or_default();
                next.push(c.session_id.clone());
                out.push(ReachFork {
                    session_id: c.session_id,
                    compilation_id: c.id,
                    generation: generation + 1,
                    from: session.clone(),
                    at_position: at,
                    exposure,
                });
            }
        }
        level = next;
        generation += 1;
    }
    Ok((out, partial))
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
            // A fork's run into a session, which no node id names.
            Some(EdgeKind::Includes) => {}
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
    /// The turn whose call this reply answers: a view turn's calls lay the turn's own nodes before them.
    #[serde(default)]
    turn_id: Option<String>,
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
/// its id. True as well when a view turn's record could not be read: what it
/// laid beyond its window is then unknown, and the answer says so.
fn exposures(
    store: &Store,
    session: &str,
    nodes: &[(u64, &Node)],
) -> Result<(Vec<ReachExposure>, bool)> {
    let from = nodes.iter().map(|(p, _)| *p).min().unwrap_or(0);
    let Scanned {
        compilations,
        made,
        loops,
    } = scan(store, session, from)?;
    // What each view turn laid beyond its window, read once a compilation.
    let mut laid: HashMap<&str, Option<ViewLaid>> = HashMap::new();
    for (id, (_, c)) in &compilations {
        if c.strategy == crate::compiler::view::STRATEGY {
            laid.insert(id.as_str(), ViewLaid::read(store, id));
        }
    }
    let unread = laid.values().any(Option::is_none);
    let mut out = Vec::with_capacity(nodes.len());
    for (p, n) in nodes {
        let at_exchange = exchange_at(store, n)?;
        let holds = |c: &Compilation| {
            c.includes.contains(&n.id)
                || crate::fork::inherited(c, *p)
                || laid
                    .get(c.id.as_str())
                    .and_then(Option::as_ref)
                    .is_some_and(|v| v.lays(n, at_exchange.as_ref()))
        };
        let mut e = ReachExposure::default();
        let seen = |at: u64, e: &mut ReachExposure| {
            e.first_ms = Some(e.first_ms.map_or(at, |f| f.min(at)));
            e.last_ms = Some(e.last_ms.map_or(at, |l| l.max(at)));
        };
        let in_tail = crate::compiler::renderable(n);
        // The loops that laid it, and the compilations they answer.
        let mut by_loop: BTreeSet<&str> = BTreeSet::new();
        for (at, ms, c, turn) in &loops {
            if at <= p {
                continue;
            }
            let Some((_, c)) = compilations.get(c) else {
                continue;
            };
            let tail = match c.strategy == crate::compiler::view::STRATEGY {
                // A view turn's call lays the turn's own nodes written before it.
                true => turn.is_some() && n.turn_id == *turn,
                false => in_tail && *p > c.as_of,
            };
            if holds(c) || tail {
                e.loops += 1;
                by_loop.insert(c.id.as_str());
                seen(*ms, &mut e);
            }
        }
        for id in &made {
            let (at, c) = &compilations[id];
            if (at > p && holds(c)) || by_loop.contains(id.as_str()) {
                e.compilations.push(ReachCompilation {
                    compilation_id: c.id.clone(),
                    strategy: c.strategy.clone(),
                    created_at_ms: c.created_at_ms,
                });
                seen(c.created_at_ms, &mut e);
            }
        }
        out.push(e);
    }
    Ok((out, unread))
}

/// A loop: its reply's position, its time, its compilation and its turn.
type Loop = (u64, u64, String, Option<String>);

/// One scan of a session from `from`: its compilations (each with its position; a loop's made before `from`
/// read by its id, at 0), the ids of those made in the scan, in order, and its loops.
struct Scanned {
    compilations: HashMap<String, (u64, Compilation)>,
    made: Vec<String>,
    loops: Vec<Loop>,
}

fn scan(store: &Store, session: &str, from: u64) -> Result<Scanned> {
    let mut compilations: HashMap<String, (u64, Compilation)> = HashMap::new();
    let mut made: Vec<String> = Vec::new();
    let mut loops: Vec<Loop> = Vec::new();
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
                    loops.push((r.position, reply.created_at_ms, c, reply.turn_id));
                }
            }
            _ => {}
        }
    }
    for (_, _, c, _) in &loops {
        if !compilations.contains_key(c) {
            if let Some(x) = store.get_compilation(c)? {
                compilations.insert(c.clone(), (0, x));
            }
        }
    }
    Ok(Scanned {
        compilations,
        made,
        loops,
    })
}

/// Where a node's exchange sits in its place's timeline: the place and the position a view piece names it
/// by. None for a node of no turn, or of a turn with no exchange.
fn exchange_at(store: &Store, n: &Node) -> Result<Option<(String, Pos)>> {
    let Some(turn) = n.turn_id.as_deref() else {
        return Ok(None);
    };
    Ok(crate::exchange::exchange(store, turn)?.map(|x| {
        let at = Pos {
            at_ms: x.start_ms,
            id: x.turn_id,
        };
        (x.place, at)
    }))
}

/// What one view turn laid beyond its compilation's window, from its kept record.
struct ViewLaid {
    /// The recalled items' nodes.
    recalled: BTreeSet<String>,
    /// Each exchange range laid, its place, and the form it was laid in.
    exchanges: Vec<(String, Pos, Pos, Form)>,
}

impl ViewLaid {
    /// The record kept for `compilation`; None when it cannot be read or was never kept.
    fn read(store: &Store, compilation: &str) -> Option<Self> {
        let v = match crate::view::record::read(store, compilation) {
            Ok(Some(v)) => v,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!(compilation, error = %format!("{e:#}"),
                    "a view record cannot be read: reach says partial");
                return None;
            }
        };
        let mut out = Self {
            recalled: BTreeSet::new(),
            exchanges: Vec::new(),
        };
        for s in v.shown {
            let r = s.piece.range;
            match r.space {
                Space::Recalled { node, .. } => {
                    out.recalled.insert(node);
                }
                Space::Exchanges { place } => out.exchanges.push((place, r.first, r.last, s.form)),
                // A fold lays its account, not its sources; a card, a fact, a task or what waits is no node.
                _ => {}
            }
        }
        Some(out)
    }

    /// Whether this view laid `n`'s text: as a recalled item, or in an exchange laid in a form that carries it.
    fn lays(&self, n: &Node, at: Option<&(String, Pos)>) -> bool {
        if self.recalled.contains(&n.id) {
            return true;
        }
        let Some((place, at)) = at else {
            return false;
        };
        let carried = |form: Form| match n.body {
            Body::UserMessage { .. } => true,
            Body::AssistantMessage { .. } => form != Form::Line,
            _ => form == Form::Whole,
        };
        self.exchanges.iter().any(|(p, first, last, form)| {
            p == place
                && (first.id == at.id || last.id == at.id || (first..=last).contains(&at))
                && carried(*form)
        })
    }
}

/// The fewest characters of an id's end that name a node (theseus-glyw): an
/// id's last six are a UUIDv7's random bits.
pub const MIN_ENDING: usize = 6;

/// How many nodes a refusal of an ending that names several lists.
const NAMED: usize = 8;

/// What a name that is no node's whole id names.
#[derive(Debug, PartialEq, Eq)]
pub enum Named {
    /// The one node whose id ends so: its whole id.
    One(String),
    /// No node's id ends so, or the name is too short to be an id's end:
    /// why, in words.
    Unknown(String),
    /// The end of more than one node's id: each, in words.
    Refused(String),
}

/// The node `name` means when it is no node's whole id (theseus-glyw): its
/// id's last 6 or more characters (`a1b2c3`, `…a1b2c3`), or the cockpit's
/// `msg·a1b2c3`, its prefix too, when exactly one node ends so. The store
/// walks its index's keys for the ending and reads no node but those that
/// match, to name each one's session when there are several.
pub fn resolve(store: &Store, name: &str) -> Result<Named> {
    use theseus_store::Store as _;
    let n = name.trim().trim_start_matches('…');
    let (prefix, ending) = match n.split_once('·') {
        Some((p, e)) => (Some(format!("{p}_")), e),
        None => (None, n),
    };
    if ending.chars().count() < MIN_ENDING {
        return Ok(Named::Unknown(format!(
            "no node is named `{name}`: give a node's whole id, or at least its id's last \
             {MIN_ENDING} characters"
        )));
    }
    let mut ids = store.inner().keys_ending(kinds::NODE, ending, 64)?;
    if let Some(p) = &prefix {
        ids.retain(|id| id.starts_with(p.as_str()));
    }
    match ids.as_slice() {
        [] => Ok(Named::Unknown(match &prefix {
            Some(p) => format!("no node's id starts `{p}` and ends with `{ending}`"),
            None => format!("no node's id ends with `{ending}`"),
        })),
        [one] => Ok(Named::One(one.clone())),
        many => {
            let mut named = Vec::new();
            for id in many.iter().take(NAMED) {
                let session = store.get_node(id)?.map(|(_, n)| n.session_id);
                named.push(match session {
                    Some(s) => format!("{id} in {}", theseus_protocol::short_id(&s)),
                    None => id.clone(),
                });
            }
            let more = many.len().saturating_sub(NAMED);
            let more = if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            };
            let count = match many.len() {
                64 => "at least 64".to_string(),
                n => n.to_string(),
            };
            Ok(Named::Refused(format!(
                "`{name}` names {count} nodes ({}{more}): give more of its id",
                named.join(", ")
            )))
        }
    }
}
