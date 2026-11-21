//! A turn in a place set to `view` (issue 44, `[context] assembly`; `compiler::view`): which assembly the
//! turn's place has, and its view, assembled once on its first call and frozen for the rest of the turn.
//! A task's turn has no place and is always the transcript's.

use std::collections::HashMap;

use super::{compile, CompileInput, Compiled, RequestSpec, Turn, TurnRunner};
use crate::compiler::view::Frozen;
use crate::config::assembly::Assembly;
use crate::exchange::{place_of, OWNER};
use crate::ladder::view::StoreLadder;
use crate::places::PlaceClass;
use crate::store::Transcript;
use crate::view::budget::Budget;
use crate::view::permit::Permit;
use crate::view::render::MarkedBlock;
use crate::view::wire::{assemble, NotAssembled, Parts, StoreSnapshot, StoreTimeline};
use crate::view::wire_text::TextRenderer;
use crate::view::{Anchor, Arrival, Audience, PointOfView};

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
        let (sid, turn) = (t.tc.session_id, t.tc.turn_id);
        let place = self.place_here(t);
        let snap = StoreSnapshot {
            now_ms: self.kernel.now_ms(),
            timeline: StoreTimeline(t.tc.store),
            at_hand: None,
        };
        let (person, marked) = t.exchange.person();
        let arrival = Arrival {
            anchor: Anchor::Place(place),
            owner_marked: marked && person.as_deref() == Some(OWNER),
            person,
            audience: match t.tc.class {
                PlaceClass::Private => Audience::Private,
                PlaceClass::Shared => Audience::Shared,
            },
        };
        let pov = PointOfView::compute(&arrival, &snap);
        let provider_window = (self.working_window(spec))
            .or_else(|| self.catalog.get(&spec.model).map(|e| e.context_window))
            .unwrap_or(u64::MAX);
        let budget = Budget::new(provider_window, u64::from(spec.max_tokens));
        let renderer = TextRenderer {
            store: t.tc.store,
            zone: &crate::recall::stamp::system,
        };
        let parts = Parts {
            selectors: &[],
            gate: &Permit,
            renderer: &renderer,
            ladder: &StoreLadder(t.tc.store),
        };
        let a = match assemble(&pov, &snap, &parts, &budget) {
            Ok(a) => a,
            Err(NotAssembled::Read(e)) => return Err(e.context("the view's read")),
            Err(NotAssembled::Overflow(o)) => anyhow::bail!("the view's floor does not fit: {o:?}"),
        };
        // The window's exchanges, from every session that ran them; this turn's own is its tail.
        let mut read: HashMap<String, Transcript> = HashMap::new();
        let mut window = Vec::new();
        for e in a.ladder.window.iter().filter(|e| e.turn_id != turn) {
            if !read.contains_key(&e.session_id) {
                read.insert(e.session_id.clone(), t.tc.store.transcript(&e.session_id)?);
            }
            let nodes = read[&e.session_id].iter();
            window.extend(
                nodes
                    .filter(|(_, n)| n.turn_id.as_deref() == Some(e.turn_id.as_str()))
                    .filter(|(_, n)| n.kind != crate::stub::Kind::Recall)
                    .cloned(),
            );
        }
        let ladder = renderer.ladder(&pov, &a.ladder.nodes)?;
        let block = |b: &MarkedBlock| Frozen::block(sid, turn, &b.heading, &b.text);
        let front = (a.parts.fixed.iter()).chain(&a.parts.ladder).chain(&ladder);
        t.view = Some(Frozen {
            turn_id: turn.into(),
            compilation_id: None,
            front: front.map(block).collect(),
            window,
            beside: a.parts.beside.as_ref().map(block),
            memory_page: a.memory_page.1.to_string(),
        });
        Ok(())
    }
}
