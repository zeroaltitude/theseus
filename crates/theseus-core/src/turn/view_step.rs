//! A turn in a place set to `view` (issue 44, `[context] assembly`; `compiler::view`): which assembly the
//! turn's place has, and its view, assembled once on its first call and frozen for the rest of the turn.
//! A task's turn has no place and is always the transcript's.
//!
//! Who is present is never the view's to say: the point of view's presence and anchor are the turn's
//! verified ones (`crate::view_gate::Verified`, computed by `turn::gate_step` from the input's mark and the
//! place read on its own), and the gate is the turn's permit (`view::permit::Permit::for_turn`). A channel
//! bound private, or an agent's process at the terminal, is never the owner alone (decision 5).

use std::collections::{HashMap, HashSet};

use super::{compile, CompileInput, Compiled, RequestSpec, Turn, TurnRunner};
use crate::compiler::view::Frozen;
use crate::config::assembly::Assembly;
use crate::ladder::view::StoreLadder;
use crate::node::{Body, Node};
use crate::store::{Store, Transcript};
use crate::stub::{Kind, Stub};
use crate::task_graph::joined::{judged_task, StoreCards};
use crate::view::budget::{Budget, Overflow};
use crate::view::fold::FoldNode;
use crate::view::permit::Permit;
use crate::view::render::{MarkedBlock, Renderer};
use crate::view::select::sources::Sources;
use crate::view::select::{table, Part};
use crate::view::sources_store::{author_of, StoreSources};
use crate::view::text::forms::{self, Voice};
use crate::view::text::labels::Origin;
use crate::view::text::parts::{self, Seen};
use crate::view::text::tokens;
use crate::view::wire::{assemble, Assembled, NotAssembled, Parts, StoreSnapshot, StoreTimeline};
use crate::view::wire_text::TextRenderer;
use crate::view::{Anchor, Form, Handle, Label, PointOfView, Pos, Present, Reasons, Slot, Space};
use crate::view_gate::Verified;

impl TurnRunner {
    /// The turn's place, as the ledger writes it: where its message arrived, else its session's own.
    fn place_here(&self, t: &Turn<'_>) -> String {
        t.exchange.place().map_or_else(
            || {
                let target = self.outbox.try_target(t.tc.session_id).ok().flatten();
                crate::exchange::place_of(target.as_deref(), t.tc.dir)
            },
            str::to_string,
        )
    }

    /// How this turn's requests are assembled: `[context] assembly` for its place. With no such setting,
    /// the transcript, and nothing is read.
    pub(super) fn assembly_here(&self, t: &Turn<'_>) -> Assembly {
        if self.cfg.context.assembly.is_none() || t.tc.task.is_some() {
            return Assembly::Transcript;
        }
        self.cfg.context.assembly_for(&self.place_here(t))
    }

    /// This loop's compile: in a place set to `view`, from the turn's view (assembled on its first call);
    /// else the transcript's, with a summary where the ring would cut (30c). The view's record is kept with
    /// the compilation it was built for, once.
    pub(super) async fn compile_here(
        &self,
        t: &mut Turn<'_>,
        input: CompileInput<'_>,
        i: u32,
    ) -> anyhow::Result<Compiled> {
        if self.assembly_here(t) != Assembly::View {
            return Box::pin(self.compact(t, compile(input), input, i)).await;
        }
        self.view_freeze(t, input.spec)?;
        let (store, turn) = (t.tc.store, t.tc.turn_id);
        let Some(f) = t.view.as_mut() else {
            anyhow::bail!("the turn's view was not assembled");
        };
        let compiled = crate::compiler::view::compile(input, f);
        if compiled.new_compilation {
            f.compilation_id = Some(compiled.compilation.id.clone());
            if let Some(view) = f.record.take() {
                if let Err(e) =
                    crate::view::record::keep(store, turn, &compiled.compilation.id, &view)
                {
                    tracing::warn!(error = %format!("{e:#}"), "the view's record was not kept");
                }
            }
        }
        Ok(compiled)
    }

    /// The turn's view, assembled on its first call and kept in `t.view` for its later ones (frozen within
    /// a turn); a new turn assembles afresh.
    fn view_freeze(&self, t: &mut Turn<'_>, spec: &RequestSpec) -> anyhow::Result<()> {
        if t.view.as_ref().is_some_and(|f| f.turn_id == t.tc.turn_id) {
            return Ok(());
        }
        let (sid, turn, store) = (t.tc.session_id, t.tc.turn_id, t.tc.store);
        let snap = StoreSnapshot {
            now_ms: self.kernel.now_ms(),
            timeline: StoreTimeline(store),
            at_hand: None,
        };
        let verified = Verified::of_session(self, sid);
        let pov = verified_pov(&verified, t.exchange.person().0, snap.now_ms);
        let Anchor::Place(place) = pov.anchor.clone() else {
            anyhow::bail!("a view turn is anchored to a place");
        };
        let since = crate::ladder::new_mark(store, &place)?;
        let (asked, message) = message_of(store, sid, turn)?;
        let permit = Permit::of(verified);
        let sources = StoreSources {
            store,
            mark: since.clone().map(|m| (place.as_str(), m)),
            recalled: recalled(t),
        };
        let ladder = StoreLadder(store);
        let cards = StoreCards {
            store,
            kernel: &self.kernel,
            names: &sources,
            homes: self,
            judged: asked.and_then(|n| judged_task(store, sid, &n)),
        };
        let src = Sources {
            gate: &permit,
            exchanges: &sources,
            ladder: &ladder,
            folds: &sources,
            places: &sources,
            waiting: &sources,
            things: &sources,
            names: &sources,
            tasks: &cards,
            recall: &sources,
            people: &sources,
            message: &message,
        };
        let renderer = TextRenderer {
            store,
            kernel: Some(&self.kernel),
            zone: &crate::recall::stamp::system,
        };
        let system = [
            &spec.system_text,
            &spec.persona_text,
            &spec.context_text,
            &spec.session_text,
        ];
        let frame = vec![Part {
            name: "system".into(),
            tokens: system.iter().map(|s| tokens(s)).sum::<u64>()
                + tokens(renderer.memory_page().1),
        }];
        let now = vec![Part {
            name: "message".into(),
            tokens: tokens(&message),
        }];
        let selectors = table(src, frame, now);
        let parts = Parts {
            selectors: &selectors,
            gate: &permit,
            renderer: &renderer,
            ladder: &ladder,
            since,
        };
        let block = |b: &MarkedBlock| Frozen::block(sid, turn, &b.heading, &b.text);
        let memory_page = renderer.memory_page().1.to_string();
        let a = match assemble(&pov, &snap, &parts, &self.view_budget(spec)) {
            Ok(a) => a,
            Err(NotAssembled::Read(e)) => return Err(e.context("the view's read")),
            Err(NotAssembled::Overflow(o)) => {
                // Rule 9: the turn is told what did not fit and asked for a narrower question.
                t.view = Some(Frozen {
                    turn_id: turn.into(),
                    beside: Some(Frozen::block(sid, turn, "", &overflow_note(&o))),
                    memory_page,
                    ..Frozen::default()
                });
                return Ok(());
            }
        };
        let (window, spoken) = window_of(store, &a, &pov, turn, renderer.zone)?;
        let folds = renderer.ladder(&pov, &shown_nodes(&a))?;
        let front = (a.parts.fixed.iter()).chain(&a.parts.ladder).chain(&folds);
        t.view = Some(Frozen {
            turn_id: turn.into(),
            compilation_id: None,
            front: front.map(block).collect(),
            window,
            spoken,
            beside: a.parts.beside.as_ref().map(block),
            memory_page,
            record: Some(a.view),
        });
        Ok(())
    }

    /// The view's one budget: the provider's window, less the reply's room.
    fn view_budget(&self, spec: &RequestSpec) -> Budget {
        let provider_window = (self.working_window(spec))
            .or_else(|| self.catalog.get(&spec.model).map(|e| e.context_window))
            .unwrap_or(u64::MAX);
        Budget::new(provider_window, u64::from(spec.max_tokens))
    }
}

/// The view's point of view from the turn's verified one: its anchor and its presence are the gate's, never
/// computed here from the place's class and a mark.
fn verified_pov(verified: &Verified, person: Option<String>, now_ms: u64) -> PointOfView {
    use theseus_memory::gate::Present as Gate;
    let v = verified.pov();
    PointOfView {
        anchor: Anchor::Place(v.anchor.clone()),
        person,
        present: match v.present {
            Gate::OwnerAlone => Present::OwnerAlone,
            Gate::PrivateChannel => Present::Private,
            Gate::Shared => Present::Shared,
            Gate::Open => Present::Open,
            Gate::Unknown => Present::Unknown,
        },
        at_hand: None,
        now_ms,
    }
}

/// Rule 9's words for the turn: the floor's own text cannot fit the model's window.
fn overflow_note(o: &Overflow) -> String {
    let Overflow::Floor {
        handle,
        tokens,
        room,
    } = o;
    format!(
        "Nothing of this place's earlier conversation could be laid out for this turn: its newest exchange \
         alone is about {tokens} tokens, and the model's window has room for {room}. Do not answer as if you \
         had seen it. Tell the person what did not fit (it opens with {}), and ask for a narrower question, \
         or read it in parts with the history tool.",
        handle.0
    )
}

/// The nodes today's recall found for this turn (its pending note, never written in a view place).
fn recalled(t: &Turn<'_>) -> Vec<String> {
    match t.recall.pending.as_ref().map(|n| &n.body) {
        Some(Body::Recall { items, .. }) => items.iter().map(|i| i.node_id.clone()).collect(),
        _ => Vec::new(),
    }
}

/// The person's message this turn answers, by node and text; none and empty for a turn no message opened.
fn message_of(store: &Store, sid: &str, turn: &str) -> anyhow::Result<(Option<String>, String)> {
    Ok(store
        .transcript(sid)?
        .iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
        .find_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some((Some(n.id.clone()), text.clone())),
            _ => None,
        })
        .unwrap_or_default())
}

/// The window as the view shows it: the nodes of each exchange the budgeter kept in slot G, oldest first,
/// from every session that ran them (this turn's own is its tail; a recall note is the transcript's), each
/// part labelled by its own provenance (`labelled`); and the results of those kept spoken, which render
/// as receipts.
fn window_of(
    store: &Store,
    a: &Assembled,
    pov: &PointOfView,
    turn: &str,
    zone: crate::recall::stamp::ZoneAt<'_>,
) -> anyhow::Result<(Transcript, Vec<String>)> {
    let here: HashMap<&str, Form> = (a.view.shown.iter())
        .filter(|s| s.piece.slot == Slot::Here)
        .filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
        .map(|s| (s.piece.range.first.id.as_str(), s.form))
        .collect();
    let mut read: HashMap<String, Transcript> = HashMap::new();
    let (mut window, mut spoken) = (Vec::new(), Vec::new());
    for e in a.ladder.window.iter().filter(|e| e.turn_id != turn) {
        let Some(form) = here.get(e.turn_id.as_str()) else {
            continue;
        };
        if !read.contains_key(&e.session_id) {
            read.insert(e.session_id.clone(), store.transcript(&e.session_id)?);
        }
        let nodes: Vec<(u64, Stub)> = read[&e.session_id]
            .iter()
            .filter(|(_, n)| n.turn_id.as_deref() == Some(e.turn_id.as_str()))
            .filter(|(_, n)| n.kind != Kind::Recall)
            .cloned()
            .collect();
        for (_, n) in &nodes {
            if *form != Form::Whole && n.kind == Kind::ToolResult {
                spoken.push(n.id.clone());
            }
        }
        let at = Pos {
            at_ms: e.at_ms,
            id: e.turn_id.clone(),
        };
        window.extend(labelled(store, nodes, pov, at, zone));
    }
    Ok((window, spoken))
}

/// An exchange's nodes as real turns, each part labelled by its own provenance (`parts::mark_parts`, live):
/// the owner's words and Theseus's side stay as stored; a message of unknown authorship is headed "author
/// unknown", another person's words are wrapped as theirs, and a result from outside is wrapped as outside
/// text, each on its own.
fn labelled(
    store: &Store,
    nodes: Vec<(u64, Stub)>,
    pov: &PointOfView,
    at: Pos,
    zone: crate::recall::stamp::ZoneAt<'_>,
) -> Vec<(u64, Stub)> {
    let Anchor::Place(place) = &pov.anchor else {
        return nodes;
    };
    let whole: Vec<Node> = nodes.iter().map(|(_, n)| (**n).clone()).collect();
    let reasons = Reasons::new();
    let seen = Seen {
        origin: Origin {
            place,
            at_ms: at.at_ms,
            source: None,
        },
        here: true,
        live: true,
        reasons: &reasons,
        range: crate::view::SourceRange {
            space: Space::Exchanges {
                place: place.clone(),
            },
            first: at.clone(),
            last: at.clone(),
        },
        handle: Handle(at.id),
    };
    let author = |s: &forms::Said| author_of(store, &s.node);
    let marked = parts::mark_parts(&forms::whole(&whole), &author, &seen, zone);
    let relabel: HashMap<&str, &parts::MarkedPart> = (marked.iter())
        .filter(|m| m.label != Label::Here && matches!(m.voice, Voice::Person | Voice::Result))
        .map(|m| (m.node.as_str(), m))
        .collect();
    if relabel.is_empty() {
        return nodes;
    }
    let ids: HashSet<&str> = relabel.keys().copied().collect();
    nodes
        .iter()
        .map(|(p, n)| {
            if !ids.contains(n.id.as_str()) {
                return (*p, n.clone());
            }
            let m = relabel[n.id.as_str()];
            let mut node = (**n).clone();
            match &mut node.body {
                Body::UserMessage { text, .. } => *text = m.turn_text(),
                Body::ToolResult { content, .. } => {
                    *content = forms::result_text(n).unwrap_or_default();
                }
                _ => {}
            }
            (*p, Stub::from(node))
        })
        .collect()
}

/// The ladder's nodes the budgeter kept, oldest first.
fn shown_nodes(a: &Assembled) -> Vec<FoldNode> {
    let kept: HashSet<&str> = (a.view.shown.iter())
        .filter(|s| matches!(s.piece.range.space, Space::Folds { .. }))
        .map(|s| s.piece.handle.0.as_str())
        .collect();
    (a.ladder.nodes.iter())
        .filter(|n| kept.contains(n.handle.0.as_str()))
        .cloned()
        .collect()
}
