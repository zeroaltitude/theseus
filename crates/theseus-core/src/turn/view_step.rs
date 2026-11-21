//! A turn in a place set to `view` (issue 44, `[context] assembly`; `compiler::view`): which assembly the
//! turn's place has, and its view, assembled once on its first call and frozen for the rest of the turn.
//! A task's turn has no place and is always the transcript's.

use std::collections::{HashMap, HashSet};

use super::{compile, CompileInput, Compiled, RequestSpec, Turn, TurnRunner};
use crate::compiler::view::Frozen;
use crate::config::assembly::Assembly;
use crate::exchange::{place_of, OWNER};
use crate::ladder::view::StoreLadder;
use crate::places::PlaceClass;
use crate::session::SessionRecord;
use crate::store::Transcript;
use crate::stub::Kind;
use crate::view::budget::Budget;
use crate::view::fold::FoldNode;
use crate::view::permit::Permit;
use crate::view::render::MarkedBlock;
use crate::view::select::sources::Sources;
use crate::view::select::{table, Part};
use crate::view::sources_store::StoreSources;
use crate::view::text::tokens;
use crate::view::wire::{assemble, Assembled, NotAssembled, Parts, StoreSnapshot, StoreTimeline};
use crate::view::wire_text::TextRenderer;
use crate::view::{Anchor, Arrival, Audience, Form, PointOfView, Pos, Slot, Space};

impl TurnRunner {
    /// The turn's place, as the ledger writes it: where its message arrived, else its session's own.
    fn place_here(&self, t: &Turn<'_>) -> String {
        t.exchange.place().map_or_else(
            || {
                let target = self.outbox.try_target(t.tc.session_id).ok().flatten();
                place_of(target.as_deref(), t.tc.dir)
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
    /// else the transcript's, with a summary where the ring would cut (30c).
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
        let Some(f) = t.view.as_mut() else {
            anyhow::bail!("the turn's view was not assembled");
        };
        let compiled = crate::compiler::view::compile(input, f);
        if compiled.new_compilation {
            f.compilation_id = Some(compiled.compilation.id.clone());
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
        let place = self.place_here(t);
        let snap = StoreSnapshot {
            now_ms: self.kernel.now_ms(),
            timeline: StoreTimeline(store),
            at_hand: None,
        };
        let pov = PointOfView::compute(&Self::arrival(t, &place), &snap);
        let since = self.new_mark(&place);
        let message = message_of(store, sid, turn)?;
        let sources = StoreSources {
            store,
            mark: since.clone().map(|m| (place.as_str(), m)),
        };
        let ladder = StoreLadder(store);
        let src = Sources {
            exchanges: &sources,
            ladder: &ladder,
            folds: &sources,
            places: &sources,
            waiting: &sources,
            things: &sources,
            names: &sources,
            tasks: &sources,
            recall: &sources,
            people: &sources,
            message: &message,
        };
        let renderer = TextRenderer {
            store,
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
            gate: &Permit,
            renderer: &renderer,
            ladder: &ladder,
            since,
        };
        let a = match assemble(&pov, &snap, &parts, &self.view_budget(spec)) {
            Ok(a) => a,
            Err(NotAssembled::Read(e)) => return Err(e.context("the view's read")),
            Err(NotAssembled::Overflow(o)) => anyhow::bail!("the view's floor does not fit: {o:?}"),
        };
        let (window, spoken) = window_of(store, &a, turn)?;
        let ladder = renderer.ladder(&pov, &shown_nodes(&a))?;
        let block = |b: &MarkedBlock| Frozen::block(sid, turn, &b.heading, &b.text);
        let front = (a.parts.fixed.iter()).chain(&a.parts.ladder).chain(&ladder);
        t.view = Some(Frozen {
            turn_id: turn.into(),
            compilation_id: None,
            front: front.map(block).collect(),
            window,
            spoken,
            beside: a.parts.beside.as_ref().map(block),
            memory_page: a.memory_page.1.to_string(),
        });
        Ok(())
    }

    /// What the turn's input brings to its point of view.
    fn arrival(t: &Turn<'_>, place: &str) -> Arrival {
        let (person, marked) = t.exchange.person();
        Arrival {
            anchor: Anchor::Place(place.to_string()),
            owner_marked: marked && person.as_deref() == Some(OWNER),
            person,
            audience: match t.tc.class {
                PlaceClass::Private => Audience::Private,
                PlaceClass::Shared => Audience::Shared,
            },
        }
    }

    /// The view's one budget: the provider's window, less the reply's room.
    fn view_budget(&self, spec: &RequestSpec) -> Budget {
        let provider_window = (self.working_window(spec))
            .or_else(|| self.catalog.get(&spec.model).map(|e| e.context_window))
            .unwrap_or(u64::MAX);
        Budget::new(provider_window, u64::from(spec.max_tokens))
    }

    /// The place's newest `/new` mark: when the session its binding runs on now began (the ladder's own
    /// reading, `ladder::build`); none for a place no binding moves (the terminal).
    fn new_mark(&self, place: &str) -> Option<Pos> {
        let bare = place.strip_prefix("discord:").unwrap_or(place);
        let sid = self.outbox.place_session(bare).ok().flatten()?;
        let rec = self
            .store
            .get_session::<SessionRecord>(&sid)
            .ok()
            .flatten()?;
        Some(Pos {
            at_ms: rec.created_at_unix_ms,
            id: String::new(),
        })
    }
}

/// The person's message this turn answers; empty for a turn no message opened.
fn message_of(store: &crate::store::Store, sid: &str, turn: &str) -> anyhow::Result<String> {
    Ok(store
        .transcript(sid)?
        .iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default())
}

/// The window as the view shows it: the nodes of each exchange the budgeter kept in slot G, oldest first,
/// from every session that ran them (this turn's own is its tail; a recall note is the transcript's); and
/// the results of those it kept spoken, which render as receipts.
fn window_of(
    store: &crate::store::Store,
    a: &Assembled,
    turn: &str,
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
        for (p, n) in &read[&e.session_id] {
            if n.turn_id.as_deref() != Some(e.turn_id.as_str()) || n.kind == Kind::Recall {
                continue;
            }
            if *form != Form::Whole && n.kind == Kind::ToolResult {
                spoken.push(n.id.clone());
            }
            window.push((*p, n.clone()));
        }
    }
    Ok((window, spoken))
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
