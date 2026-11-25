//! A turn's view (issue 44; `compiler::view`): every turn's request is assembled by the view, once on the
//! turn's first call and frozen for the rest of the turn. A task's turns are at the task's own place,
//! `task:<task id>` (`view_step::task`).
//!
//! Every place uses the same view: presence, seals and moves withhold nothing. The verified anchor
//! (`crate::view_gate::Verified`) and the scoped permit (`view::permit::Permit::scoped`, made by
//! `fork::window::Fork::permit`) retain the self-loop's own-place scope. Removed sources
//! are checked again before later calls; authority remains attached to each input's provenance.

use std::collections::{HashMap, HashSet};

mod failed;
pub(crate) mod kept;
pub(crate) mod occasion;
pub(super) mod preview;
mod recall;
mod room;
#[cfg(test)]
pub(crate) use room::force_resize;
pub(super) mod task;

#[cfg(test)]
pub(crate) use super::exchange_step::{route_readable, route_unreadable};
#[cfg(test)]
pub(crate) use failed::fail_next_assembly;
pub(in crate::turn) use failed::ViewFailed;

use super::{CompileInput, Compiled, RequestSpec, Turn, TurnRunner};
use crate::compiler::view::Frozen;
use crate::node::{Body, Node};
use crate::store::{Store, Transcript};
use crate::stub::{Kind, Stub};
use crate::task_graph::joined::{judged_task, StoreCards};
use crate::view::budget::{Budget, Overflow};
use crate::view::fold::FoldNode;
use crate::view::permit::Permit;
use crate::view::render::MarkedBlock;
use crate::view::select::sources::{Names, Sources, Things};
use crate::view::select::{table, Part};
use crate::view::sources_names::{Called, ViewAdmits};
use crate::view::sources_people::StorePeople;
use crate::view::sources_store::{author_of, Sessions, StoreSources};
use crate::view::sources_waiting::StoreWaiting;
use crate::view::text::forms::{self, Voice};
use crate::view::text::labels::Origin;
use crate::view::text::page::memory_page_at;
use crate::view::text::parts::{self, Seen};
use crate::view::text::tokens;
use crate::view::wire::OnceLadder;
use crate::view::wire::{assemble, Assembled, NotAssembled, Parts, StoreSnapshot, StoreTimeline};
use crate::view::wire_text::TextRenderer;
use crate::view::{Anchor, Form, Handle, Label, PointOfView, Pos, Present, Reasons, Slot, Space};
use crate::view_gate::Verified;

impl TurnRunner {
    /// This loop's compile, from the turn's view (assembled on its first call). A view that cannot be
    /// assembled soundly fails the turn plainly (`ViewFailed`): never a partial view. The view's record is
    /// kept with the compilation it was built for, once.
    pub(super) async fn compile_here(
        &self,
        t: &mut Turn<'_>,
        input: CompileInput<'_>,
    ) -> anyhow::Result<Compiled> {
        // The message's numbered items, named off the path since the exchange opened, are what its view's
        // names are matched with.
        t.exchange.await_items().await;
        match self.view_compile(t, input) {
            Ok(c) => Ok(c),
            Err(e) if e.is::<ViewFailed>() => Err(e),
            // Never a partial view: the turn fails with the error's words, and nothing is sent.
            Err(e) => Err(ViewFailed::Assembly(format!("{e:#}")).into()),
        }
    }

    /// One call of a view turn; a turn whose place could not be read fails (`ViewFailed::Place`).
    fn view_compile(&self, t: &mut Turn<'_>, input: CompileInput<'_>) -> anyhow::Result<Compiled> {
        let turn = t.tc.turn_id;
        let held = t.view.as_ref().filter(|f| f.turn_id == turn);
        if held.is_none() {
            if t.exchange.place_unread() {
                return Err(ViewFailed::Place.into());
            }
        } else if self.view_gone(t) {
            // Something it laid was removed or erased since (absence is not proof): reassembled, on a new
            // chain, with the room the turn made; a part of the harness's block whose records went is
            // rendered again first (`now_step::now_refreshed`).
            self.now_refreshed(t)?;
            self.view_refrozen(t, input.spec, None)?;
            Self::view_reset(t, input.nodes);
        }
        self.view_freeze(t, input.spec)?;
        // A refused image under the turn's thinking, or a block the provider dropped: the same view, a new
        // chain.
        if input.strip.is_some() || input.force {
            Self::view_reset(t, input.nodes);
        }
        // Over the budget as rendered, or refused by the provider for its size: the view makes its own room
        // (`room`), and a request it cannot fit fails on its overage.
        let compiled = self.view_roomed(t, input)?;
        let Some(f) = t.view.as_mut() else {
            anyhow::bail!("the turn's view was not assembled");
        };
        if compiled.new_compilation {
            if f.compilation_id.is_some() {
                // A new chain in the middle of the turn (an effective spec that changed).
                let own: Vec<String> =
                    crate::compiler::view::own_answers(input.nodes, turn).collect();
                f.stripped.extend(own);
            }
            f.compilation_id = Some(compiled.compilation.id.clone());
            // Routing's first compile is persisted only if the call uses it (`route_step::keep_first`).
            if !t.route.defer_persist {
                kept::record_kept(t, &compiled);
            }
        }
        Ok(compiled)
    }

    /// After every await of a call's compile and before the provider (issue 44): whether anything the frozen
    /// view laid was removed or erased since ([`Self::view_gone`]: erase stays, absence is not proof), which
    /// compiles the call again at most twice and fails it plainly a third time (`ViewFailed::Gone`); then the
    /// size check (`room::view_size`) on the finished request, a recompile for size coming back to both (at
    /// most `room::SIZINGS` of them). The request that goes out is the last one compiled. Anything goes (2026-11-22): no presence, seal or move is asked again.
    pub(super) async fn view_readmitted(
        &self,
        t: &mut Turn<'_>,
        session: &mut crate::session::SessionRecord,
        spec: &RequestSpec,
        compiled: Compiled,
        i: u32,
    ) -> anyhow::Result<Result<Compiled, super::Failure>> {
        let (mut compiled, mut gone, mut sized) = (compiled, 0, 0);
        loop {
            if self.view_gone(t) {
                if gone == VIEW_REASSEMBLIES {
                    return Ok(Err(Self::view_failed(t, &ViewFailed::Gone)));
                }
                gone += 1;
            } else {
                match room::view_size(t, &mut compiled, sized == room::SIZINGS) {
                    room::Size::Fits => return Ok(Ok(compiled)),
                    room::Size::Over => {
                        return Ok(Self::overage(t, &compiled, i).map_or(Ok(compiled), Err));
                    }
                    room::Size::Again => sized += 1,
                }
            }
            compiled =
                match Box::pin(self.compile_step(t, session, spec, false, None, None, i)).await? {
                    Ok(c) => c,
                    Err(f) => return Ok(Err(f)),
                };
        }
    }

    /// After the turn's route decided who answers and what kind of turn it is (issue 46): the parts the
    /// route decides are set in the harness's block, and a block that changed is laid again with the view
    /// re-frozen, on a new chain, before any request of the turn is sent. Whether it was.
    pub(super) fn now_relaid(&self, t: &mut Turn<'_>, spec: &RequestSpec) -> anyhow::Result<bool> {
        let mut r = t.now.take().unwrap_or_default();
        let changed = Self::now_routed(t, &mut r);
        t.now = Some(r);
        if !changed || !t.view.as_ref().is_some_and(|f| f.turn_id == t.tc.turn_id) {
            return Ok(false);
        }
        self.view_refrozen(t, spec, None)?;
        Self::view_reset(t, &[]);
        Ok(true)
    }

    /// Whether an exchange the frozen view lays is gone now, or a node its recall laid is gone or erased
    /// (`kept::held_now`), or a record a part of the harness's block was rendered from (`now_step::now_stale`).
    pub(super) fn view_gone(&self, t: &Turn<'_>) -> bool {
        let Some(f) = t.view.as_ref().filter(|f| f.turn_id == t.tc.turn_id) else {
            return false;
        };
        let recalled: Vec<String> = (f.recalled.iter()).map(|r| r.node_id.clone()).collect();
        kept::held_now(self, t.tc.session_id, (&[], &f.exchanges, &recalled))
            || !self.now_stale(t).is_empty()
    }

    /// A new chain for the turn: its next call makes a new compilation, and the
    /// turn's own answers so far are sent without their thinking, which the change invalidated.
    fn view_reset(t: &mut Turn<'_>, nodes: &[(u64, Stub)]) {
        let turn = t.tc.turn_id;
        let Some(f) = t.view.as_mut() else {
            return;
        };
        f.compilation_id = None;
        f.stripped.extend(
            (nodes.iter())
                .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
                .filter(|(_, n)| n.kind == Kind::AssistantMessage)
                .map(|(_, n)| n.id.clone()),
        );
    }

    /// The turn's view, assembled on its first call and kept in `t.view` for its later ones (frozen within
    /// a turn); a new turn assembles afresh. A floor too large for the budget (rule 9) lays nothing: its
    /// calls fail on the floor's overage (`room`), never a small view.
    fn view_freeze(&self, t: &mut Turn<'_>, spec: &RequestSpec) -> anyhow::Result<()> {
        if t.view.as_ref().is_some_and(|f| f.turn_id == t.tc.turn_id) {
            return Ok(());
        }
        self.view_refrozen(t, spec, None)
    }

    /// A view assembled for `asked` under `budget`: what a turn freezes, and the assembly it was made from
    /// (the floor's overflow, rule 9, when it could not fit). The one assembler: a turn's and a preview's
    /// (`view_step::preview`).
    pub(super) fn view_built(
        &self,
        asked: Asked<'_>,
        spec: &RequestSpec,
        budget: &Budget,
    ) -> anyhow::Result<Built> {
        let Asked {
            store,
            sid,
            turn,
            person,
            verified,
            recalled,
            message: given,
            items,
            harness,
        } = asked;
        let snap = StoreSnapshot {
            now_ms: self.kernel.now_ms(),
            timeline: StoreTimeline(store),
            at_hand: None,
        };
        let verified = verified.unwrap_or_else(|| Verified::of_session(self, sid));
        let pov = verified_pov(&verified, person, snap.now_ms);
        let Anchor::Place(place) = pov.anchor.clone() else {
            anyhow::bail!("a view turn is anchored to a place");
        };
        let since = crate::ladder::new_mark(store, &place)?;
        // Read where a turn has always read it: after the clock and the `/new` mark.
        let (asked, message) =
            given.map_or_else(|| message_of(store, sid, turn), |text| Ok((None, text)))?;
        let verified_pov = verified.pov().clone();
        if verified.scope_unread() {
            anyhow::bail!(
                "this session's execution cannot be read, so whether the self loop opened it cannot be decided"
            );
        }
        let (fork, permit) = crate::fork::window::Fork::permit(store, sid, turn, verified)?;
        let read = fork.sessions();
        let sources = StoreSources {
            store,
            mark: since.clone().map(|m| (place.as_str(), m)),
            recalled,
            sessions: &read,
            own: Some(turn),
        };
        // One read of the ladder for the whole assembly: every selector, the text and the window see it.
        let store_ladder = fork.ladder(store);
        let resolved = ResolvedLadder::new(&store_ladder, store);
        let ladder = OnceLadder::new(&resolved);
        // What has been said since about the nodes it lays, read once: beside the message and in the window.
        let wrong = self.memory.labeled(store)?;
        let said = crate::copies::since::Since::new(store, &wrong);
        let moving = self.moving(store, (&permit, &pov), &items, (&read, &said))?;
        let cards = StoreCards {
            store,
            kernel: &self.kernel,
            names: &sources,
            homes: self,
            judged: asked.and_then(|n| judged_task(store, sid, &n)),
        };
        let src = sources_of(&permit, (&sources, &moving), &ladder, &cards, &message);
        let renderer = self.renderer(store, &read, &said);
        let who = self.who_am_i(spec);
        let memory_page = memory_page_at(src.sizes.here).into_owned();
        let (frame, now) = frame_and_now(spec, (&memory_page, &who), &message, harness);
        let selectors = table(src, frame, now);
        let parts = Parts {
            selectors: &selectors,
            gate: &permit,
            renderer: &renderer,
            ladder: &ladder,
            since,
        };
        let block = |b: &MarkedBlock| Frozen::block(sid, turn, &b.heading, &b.text);
        let a = match assemble(&pov, &snap, &parts, budget) {
            Ok(a) => a,
            Err(NotAssembled::Read(e)) => return Err(e.context("the view's read")),
            // Rule 9: the floor did not fit. The assembler says so by its numbers; a turn fails on it
            // (`room`), a preview reports it.
            Err(NotAssembled::Overflow(o)) => return Ok(Built::overflowed(turn, o, memory_page)),
        };
        let (window, mut sessions) =
            window_of((store, &read, sid), &a, &pov, (turn, &said), renderer.zone)?;
        sessions.extend(resolved.sessions.borrow().iter().cloned());
        let exchanges = laid_exchanges(&window, turn, &resolved.exchanges.borrow(), &a);
        let folds = renderer.ladder(&pov, &shown_nodes(&a))?;
        let front = (a.parts.fixed.iter()).chain(&a.parts.ladder).chain(&folds);
        let frozen = Frozen {
            turn_id: turn.into(),
            // Who am I first: code's own lines, the same bytes until the profile or the persona changes.
            front: std::iter::once(Frozen::block(sid, turn, "", &who))
                .chain(front.map(block))
                .collect(),
            window,
            sessions: sorted(sessions),
            exchanges,
            verified: Some(verified_pov),
            beside: a.parts.beside.as_ref().map(block),
            memory_page,
            record: Some(a.view.clone()),
            ..Frozen::default()
        };
        Ok(Built {
            frozen,
            assembled: Ok(a),
        })
    }

    /// The sources of what is beside the message and who is speaking, read once before assembly: the
    /// names table through the turn's permit (none outside a core, which holds the stores it reads), what
    /// waits, and the facts about the person.
    fn moving<'a>(
        &'a self,
        store: &'a Store,
        (permit, pov): (&'a Permit, &'a PointOfView),
        items: &[String],
        (read, said): (&'a Sessions, &'a crate::copies::since::Since<'a>),
    ) -> anyhow::Result<Moving<'a>> {
        let (texts, depends) = (&read.texts, &read.depends);
        let called = match self.threads.core() {
            Some(core) => {
                let tasks = crate::task_graph::all(store)?;
                let admits = ViewAdmits::new((permit, pov), (store, self), tasks, depends);
                Some(Called::build(core, admits, items.to_vec(), texts)?)
            }
            None => None,
        };
        Ok(Moving {
            called,
            waiting: StoreWaiting {
                gate: permit,
                pov,
                store,
                kernel: &self.kernel,
                homes: self,
                texts,
                depends,
            },
            people: StorePeople {
                ontology: self.ontology.snapshot(store)?,
                // Every effective owner handle, those the bindings make among them: who the owner is.
                owner: self.place_rule.owners(&self.cfg).into_iter().collect(),
                now_ms: pov.now_ms,
                store,
                sessions: read,
                // The known values as they stand for this assembly: where a stored text it lays may be cut.
                cuts: self.tools.scrubber.cuts(),
                since: Some(said),
            },
            sizes: self.cfg.context.sizes(),
        })
    }

    /// The turn's renderer over `store`: its kernel, the assembly's one read of each session, and what has been
    /// said since about the nodes it lays, its times said in the system's zone.
    fn renderer<'a>(
        &'a self,
        store: &'a Store,
        read: &'a Sessions,
        said: &'a crate::copies::since::Since<'a>,
    ) -> TextRenderer<'a> {
        TextRenderer {
            store,
            kernel: Some(&self.kernel),
            sessions: Some(read),
            zone: &crate::recall::stamp::system,
            since: Some(said),
        }
    }

    /// The view's one budget: the provider's window, less the reply's room.
    fn view_budget(&self, spec: &RequestSpec) -> Budget {
        let provider_window = (self.working_window(spec))
            .or_else(|| self.catalog.get(&spec.model).map(|e| e.context_window))
            .unwrap_or(u64::MAX);
        // The reply's room and the headroom the compiler keeps for the turn's own calls
        // (`compiler::budget::request_budget`), so a view that fits is one the compiler sends.
        Budget::new(provider_window, u64::from(spec.max_tokens) + CALL_HEADROOM)
    }
}

/// The headroom the compiler keeps beside the reply for the turn's own calls, in tokens.
const CALL_HEADROOM: u64 = 4_096;

/// How many times one call is compiled again when something its view laid was removed under it.
const VIEW_REASSEMBLIES: u32 = 2;

/// A test's seam: what runs after the next compile of a session has assembled its view, before the call
/// is sent, once (where the request's tail once read git).
#[cfg(test)]
type AssembledHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
static AT_ASSEMBLED: std::sync::Mutex<Vec<(String, AssembledHook)>> =
    std::sync::Mutex::new(Vec::new());

/// Run `f` after the next compile of `session` has assembled its view.
#[cfg(test)]
pub(crate) fn assembled_hook(session: &str, f: AssembledHook) {
    AT_ASSEMBLED.lock().unwrap().push((session.to_string(), f));
}

#[cfg(test)]
pub(super) fn at_assembled(session: &str) {
    let hook = {
        let mut hooks = AT_ASSEMBLED.lock().unwrap();
        (hooks.iter().position(|(s, _)| s == session)).map(|i| hooks.remove(i).1)
    };
    if let Some(f) = hook {
        f();
    }
}

/// What one view is assembled for: a turn's (`view_freeze`), or a preview's of a place's next turn.
pub(super) struct Asked<'a> {
    pub store: &'a Store,
    /// The session whose turn is being assembled.
    pub sid: &'a str,
    pub turn: &'a str,
    pub person: Option<String>,
    /// The gate's point of view, never the caller's word: a turn's at its own place
    /// (`Verified::of_turn`), or a preview's; none reads the session's (`Verified::of_session`).
    pub verified: Option<Verified>,
    /// The items today's recall found and admitted, read for slot H (none for a preview, which runs no
    /// recall).
    pub recalled: Vec<crate::view::sources_recall::Recalled>,
    /// The message given (a preview's); none reads the turn's own, by node and text, at the point a turn
    /// always read it (after the clock and the `/new` mark).
    pub message: Option<String>,
    /// The message's numbered items, as the exchange's naming told them (none for a preview).
    pub items: Vec<String>,
    /// The tokens of the harness's part of the block beside the message (`now_step`), with what the
    /// route may add to it, counted in slot I before the other bands (none for a preview).
    pub harness: u64,
}

/// A view as assembled: what a turn freezes, and the assembly (or the floor's overflow).
pub(super) struct Built {
    pub frozen: Frozen,
    pub assembled: Result<Assembled, Overflow>,
}

impl Built {
    /// Rule 9: the floor did not fit. Nothing is laid: a turn fails on the floor's overage (`room`), and a
    /// preview says the floor's numbers.
    fn overflowed(turn: &str, o: Overflow, memory_page: String) -> Self {
        let frozen = Frozen {
            turn_id: turn.into(),
            memory_page,
            ..Frozen::default()
        };
        Built {
            frozen,
            assembled: Err(o),
        }
    }
}

/// The ladder as the model's assembly reads it: a fold node is kept only when every exchange it stands for
/// resolves now (a removed thing is not sent again); the sessions and the exchanges of those kept are
/// recorded, so a removal before a later call or a replay revokes them too (`view_gone`, `kept::held_now`).
/// No presence, seal or move holds a fold.
struct ResolvedLadder<'a> {
    inner: &'a dyn crate::view::fold::Ladder,
    store: &'a Store,
    sessions: std::cell::RefCell<Vec<String>>,
    exchanges: std::cell::RefCell<Vec<String>>,
}

impl<'a> ResolvedLadder<'a> {
    fn new(inner: &'a dyn crate::view::fold::Ladder, store: &'a Store) -> Self {
        Self {
            inner,
            store,
            sessions: Default::default(),
            exchanges: Default::default(),
        }
    }
}

impl crate::view::fold::Ladder for ResolvedLadder<'_> {
    fn read(
        &self,
        place: &str,
        n: usize,
        since: Option<&Pos>,
    ) -> anyhow::Result<crate::view::fold::LadderRead> {
        let mut read = self.inner.read(place, n, since)?;
        let mut kept = Vec::new();
        for node in read.nodes {
            let Some(exchanges) = crate::ladder::fold::constituents(self.store, &node.handle.0)
            else {
                continue;
            };
            for e in exchanges {
                self.sessions.borrow_mut().push(e.session_id);
                self.exchanges.borrow_mut().push(e.turn_id);
            }
            kept.push(node);
        }
        read.nodes = kept;
        Ok(read)
    }
}

/// `v` in order, each once.
fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// Every exchange a view lays or folds, once each: the window's (but `turn`'s own), the kept folds'
/// constituents, and the ends of every shown exchange piece's range. Each is read again before each call
/// (`view_gone`): one removed since revokes the view.
fn laid_exchanges(
    window: &[(u64, Stub)],
    turn: &str,
    folded: &[String],
    a: &Assembled,
) -> Vec<String> {
    let shown = (a.view.shown.iter())
        .filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
        .flat_map(|s| {
            [
                s.piece.range.first.id.clone(),
                s.piece.range.last.id.clone(),
            ]
        });
    // The exchanges a shown card read its excerpts from (its provenance): a removal since revokes it too.
    let read = (a.view.shown.iter()).flat_map(|s| match &s.piece.range.space {
        Space::Card { from: Some(p), .. } => p.exchanges.clone(),
        _ => Vec::new(),
    });
    let mut out: Vec<String> = (window.iter())
        .filter_map(|(_, n)| n.turn_id.clone())
        .filter(|t| t != turn)
        .chain(folded.iter().cloned())
        .chain(shown)
        .chain(read)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// What [`TurnRunner::moving`] reads.
struct Moving<'a> {
    called: Option<Called<'a>>,
    waiting: StoreWaiting<'a>,
    people: StorePeople<'a>,
    /// The look-backs' sizes, `[context]`'s.
    sizes: crate::config::context::Sizes,
}

/// Every source the selectors read: the store's, the turn's permit as their gate, the ladder read once,
/// the named things (none outside a core), what waits, the facts about the person, the joined task's
/// card, the message, and the look-backs' sizes.
fn sources_of<'a, 'b>(
    gate: &'a Permit,
    (store, moving): (&'a StoreSources<'b>, &'a Moving<'b>),
    ladder: &'a dyn crate::view::fold::Ladder,
    cards: &'a StoreCards<'b>,
    message: &'a str,
) -> Sources<'a> {
    let (things, names): (&dyn Things, &dyn Names) = match &moving.called {
        Some(c) => (c, c),
        None => (store, store),
    };
    Sources {
        gate,
        exchanges: store,
        ladder,
        folds: store,
        places: store,
        waiting: &moving.waiting,
        things,
        names,
        tasks: cards,
        recall: store,
        people: &moving.people,
        message,
        sizes: moving.sizes,
    }
}

/// Slots A and I as code lays them: the system part with the memory page, and who am I at the head of the
/// first user message; the harness's part of the block beside the message (`now_step`), counted here before
/// the other bands are admitted, and the message.
fn frame_and_now(
    spec: &RequestSpec,
    (page, who): (&str, &str),
    message: &str,
    harness: u64,
) -> (Vec<Part>, Vec<Part>) {
    let system = [
        &spec.system_text,
        &spec.persona_text,
        &spec.context_text,
        &spec.session_text,
    ];
    let frame = Part {
        name: "system".into(),
        tokens: system.iter().map(|s| tokens(s)).sum::<u64>() + tokens(page),
    };
    let who = Part {
        name: "who_am_i".into(),
        tokens: tokens(who),
    };
    let mut now = Vec::new();
    if harness > 0 {
        now.push(Part {
            name: "harness".into(),
            tokens: harness,
        });
    }
    now.push(Part {
        name: "message".into(),
        tokens: tokens(message),
    });
    (vec![frame, who], now)
}

impl TurnRunner {
    /// The who-am-I block (`view::who`) for a request of `spec`: its profile and model, and the persona in
    /// play by its name (the built-in one when none is configured or found).
    fn who_am_i(&self, spec: &RequestSpec) -> String {
        let book = self.personas();
        let persona = (spec.persona.as_deref())
            .and_then(|k| book.get(k))
            .cloned()
            .unwrap_or_else(crate::config::PersonaView::built_in);
        crate::view::who::text(&crate::view::who::Who {
            profile: &spec.profile,
            model: &spec.model,
            persona: &persona.name,
        })
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

/// The items today's recall found for this turn (its pending note), before the view admits them
/// (`recall::view_recall`).
///
/// Read once, at the turn's first freeze, and that is enough: recall is settled before a turn's first
/// call, so nothing lands after the freeze. The pending note is written only by the first loop's recall
/// (`recall_first`), which the turn loop awaits before that loop's compile, where the view is frozen. It
/// answers within its deadline or not at all (a late pack is dropped), the rerank's wait and refill run
/// inside it, a later loop asks nothing, and a late verdict only marks the pack already held. Held by
/// `tests_route::a_quick_picks_later_loop_keeps_its_recall` and
/// `tests_route::a_routed_turn_still_writes_and_sends_its_recall`: every call of the turn, the switched
/// model's included, carries the note once.
fn recalled(t: &Turn<'_>) -> Vec<crate::node::RecalledRef> {
    match t.recall.pending.as_ref().map(|n| &n.body) {
        Some(Body::Recall { items, .. }) => items.clone(),
        _ => Vec::new(),
    }
}

/// The person's message this turn answers, by node and text (a task's first turn, its brief); none and
/// empty for a turn no message opened.
fn message_of(store: &Store, sid: &str, turn: &str) -> anyhow::Result<(Option<String>, String)> {
    let own = (store.transcript(sid)?.iter())
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
        .find_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some((Some(n.id.clone()), text.clone())),
            _ => None,
        });
    match own {
        Some(m) => Ok(m),
        None => Ok((task::brief_of(store, sid, turn)?)
            .map_or_else(Default::default, |(id, text)| (Some(id), text))),
    }
}

/// The window as the view shows it: the nodes of each exchange the budgeter kept in slot G, oldest first,
/// from every session that ran them (this turn's own is its tail; a recall note is the transcript's), each
/// part labelled by its own provenance (`labelled`), the results of those kept spoken as their receipts,
/// made now from the window alone (`compiler::view::receipted`); an exchange laid whole with what has been
/// said about its nodes since (`copies::since`, in memory); and every session the view lays, those a
/// card, a waiting line or a fact was read from (`Sessions::depends`) among them, for source tracking.
fn window_of(
    (store, read, sid): (&Store, &Sessions, &str),
    a: &Assembled,
    pov: &PointOfView,
    (turn, said): (&str, &crate::copies::since::Since<'_>),
    zone: crate::recall::stamp::ZoneAt<'_>,
) -> anyhow::Result<(Transcript, Vec<String>)> {
    let here: HashMap<&str, Form> = (a.view.shown.iter())
        .filter(|s| s.piece.slot == Slot::Here)
        .filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
        .map(|s| (s.piece.range.first.id.as_str(), s.form))
        .collect();
    // A task's opening heads its window (`task::window_head`); any other place's window opens empty.
    let (mut window, mut spoken) = (task::window_head(store, sid, pov)?, HashSet::new());
    let mut laid = Vec::new();
    for e in a.ladder.window.iter().filter(|e| e.turn_id != turn) {
        let Some(form) = here.get(e.turn_id.as_str()) else {
            continue;
        };
        laid.push(e.session_id.clone());
        let nodes: Vec<(u64, Stub)> =
            (read.exchange(store, &e.session_id, &e.turn_id)?.into_iter())
                .filter(|(_, n)| n.kind != Kind::Recall)
                .collect();
        for (_, n) in &nodes {
            if *form != Form::Whole && n.kind == Kind::ToolResult {
                spoken.insert(n.id.clone());
            }
        }
        let at = Pos {
            at_ms: e.at_ms,
            id: e.turn_id.clone(),
        };
        let nodes = labelled(store, nodes, pov, at, zone);
        let handle = Handle(e.turn_id.clone());
        window.extend(match form {
            Form::Whole => said.lay(nodes)?,
            Form::Spoken => bounded(nodes, &handle),
            Form::Line => as_line((store, read), nodes, &handle)?,
        });
    }
    let mut sessions: Vec<String> = (a.view.shown.iter())
        .filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
        .filter_map(|s| {
            crate::exchange::exchange(store, &s.piece.range.first.id)
                .ok()
                .flatten()
        })
        .map(|e| e.session_id)
        .chain(laid)
        .collect();
    // A joined task card's home session is a source dependency too.
    let cards: Vec<&str> = (a.view.shown.iter())
        .filter_map(|s| match &s.piece.range.space {
            Space::Task { task } => Some(task.as_str()),
            _ => None,
        })
        .collect();
    if !cards.is_empty() {
        let all = crate::task_graph::all(store)?;
        for id in cards {
            // Fail closed: a shown card whose home cannot be read is no view (the turn declines).
            let Some(rec) = all.iter().find(|r| r.id == id) else {
                anyhow::bail!("a shown task card's home cannot be resolved");
            };
            sessions.push(crate::task_graph::home(&all, rec));
        }
    }
    sessions.extend(read.depends.borrow().iter().cloned());
    sessions.sort();
    sessions.dedup();
    Ok((crate::compiler::view::receipted(window, &spoken), sessions))
}

/// The most of a call's input a `Spoken` exchange sends, in characters; past it, a bounded receipt.
const CALL_INPUT_CHARS: usize = 400;

/// A `Spoken` exchange as the budgeter paid for it: its words, and each call whose input runs past
/// [`CALL_INPUT_CHARS`] as historical text (`[earlier call: <tool> <head>… (N characters of input)]`) with
/// its result folded into that text as its stub, instead of a tool use whose input no longer fits the tool's
/// schema; the other calls stay real, paired with their results (receipts already).
fn bounded(nodes: Vec<(u64, Stub)>, handle: &Handle) -> Vec<(u64, Stub)> {
    let long = |b: &serde_json::Value| {
        b["type"] == "tool_use" && b["input"].to_string().chars().count() > CALL_INPUT_CHARS
    };
    let (folds, folded) = folds(&nodes, long);
    let mut out = Vec::new();
    for (p, n) in nodes {
        match &n.body {
            Body::AssistantMessage { blocks, .. } if blocks.iter().any(long) => {
                let fold = to_fold(blocks, &long);
                let mut node = (*n).clone();
                if let Body::AssistantMessage { blocks, .. } = &mut node.body {
                    let uses = blocks.iter_mut().enumerate();
                    for (_, b) in uses.filter(|(i, _)| fold.contains(i)) {
                        let id = b["id"].as_str().unwrap_or_default().to_string();
                        let name = b["name"].as_str().unwrap_or_default().to_string();
                        let input = b["input"].to_string();
                        let head: String = input.chars().take(CALL_INPUT_CHARS / 2).collect();
                        let result = folds.get(&(n.id.clone(), id)).cloned().unwrap_or_default();
                        *b = serde_json::json!({"type": "text", "text": format!(
                            "[earlier call: {name} {head}… ({} characters of input); {result}; open {} for \
                             the whole call]",
                            input.chars().count(),
                            handle.0
                        )});
                    }
                }
                out.push((p, Stub::from(node)));
            }
            Body::ToolResult { .. } if folded.contains(&n.id) => {}
            _ => out.push((p, n)),
        }
    }
    out
}

/// Each long call's fold text, by the call (its assistant node and id), and the results folded.
type Folds = (HashMap<(String, String), String>, HashSet<String>);

/// For [`bounded`]: each long call's fold text (its stub with its own result), by the call, and the results
/// those folds hold, by node id. A call's result is its own by proven identity (`compiler::pairing`, issue 44):
/// a provider may repeat an id between the exchange's responses, so a fold describes its own call and only
/// its own result is dropped; a response that repeats an id folds its uses with "no result".
fn folds(nodes: &[(u64, Stub)], long: impl Fn(&serde_json::Value) -> bool) -> Folds {
    let p = crate::compiler::pairing::pairing(nodes.iter().map(|(_, n)| &**n));
    let (mut folds, mut folded) = (HashMap::new(), HashSet::new());
    for (_, n) in nodes {
        let Body::AssistantMessage { blocks, .. } = &n.body else {
            continue;
        };
        let fold = to_fold(blocks, &long);
        for (_, b) in blocks.iter().enumerate().filter(|(i, _)| fold.contains(i)) {
            let id = b["id"].as_str().unwrap_or_default();
            let r = p.result((n.id.as_str(), id));
            let name = b["name"].as_str().unwrap_or_default();
            let text = r.map_or_else(
                || "no result".into(),
                |r| forms::stub(name, &b["input"], Some(r)),
            );
            folded.extend(r.map(|r| r.id.clone()));
            folds.insert((n.id.clone(), id.to_string()), text);
        }
    }
    (folds, folded)
}

/// The uses of a response a `Spoken` exchange folds ([`bounded`]), by their place in its blocks: each long one,
/// and every use of an id the response gives to more than one use one of which is long (issue 44). Folding
/// only the long one would leave the other the id's only use, and a later pairing (the receipts, the render)
/// would give it the long one's result; folded, all of them read "no result" (`compiler::pairing`, rule 0).
fn to_fold(
    blocks: &[serde_json::Value],
    long: &impl Fn(&serde_json::Value) -> bool,
) -> HashSet<usize> {
    let id = |b: &serde_json::Value| b["id"].as_str().unwrap_or_default().to_string();
    let uses: Vec<(usize, &serde_json::Value)> = (blocks.iter().enumerate())
        .filter(|(_, b)| b["type"] == "tool_use")
        .collect();
    let mut count: HashMap<String, usize> = HashMap::new();
    for (_, b) in &uses {
        *count.entry(id(b)).or_default() += 1;
    }
    let tied: HashSet<String> = (uses.iter())
        .filter(|(_, b)| long(b) && count[&id(b)] > 1)
        .map(|(_, b)| id(b))
        .collect();
    (uses.iter())
        .filter(|(_, b)| long(b) || tied.contains(&id(b)))
        .map(|(i, _)| *i)
        .collect()
}

/// A `Line` exchange as the budgeter paid for it: the person's words (as labelled), then the `did` line
/// (or "reply not summarised") and the handle, as one message of the person's turn.
fn as_line(
    (store, read): (&Store, &Sessions),
    nodes: Vec<(u64, Stub)>,
    handle: &Handle,
) -> anyhow::Result<Vec<(u64, Stub)>> {
    let Some((p, ask)) = nodes.into_iter().find(|(_, n)| n.kind == Kind::UserMessage) else {
        return Ok(Vec::new());
    };
    // A fork's point in or before the exchange: its summary would tell the fork what came after (`fork::cut`).
    let did = match read.summarised(store, &handle.0)? {
        true => crate::ladder::did::did(store, &handle.0)?
            .map(|d| crate::ladder::did::shown_line(store, &d)),
        false => None,
    };
    let mut node = (*ask).clone();
    if let Body::UserMessage { text, .. } = &mut node.body {
        *text = forms::line(text, did.as_deref(), handle);
    }
    Ok(vec![(p, Stub::from(node))])
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
        person: None,
    };
    let author = |s: &forms::Said| author_of(store, &s.node);
    let marked = parts::mark_parts(&forms::whole(&whole), &author, &seen, zone);
    let relabel: HashMap<&str, &parts::MarkedPart> = (marked.iter())
        .filter(|m| {
            m.label != Label::Here
                && matches!(m.voice, Voice::Person | Voice::Harness | Voice::Result)
        })
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("store")).unwrap();
        (dir, s)
    }

    fn answer(turn: &str, blocks: Vec<serde_json::Value>) -> Node {
        let mut n = Node::user("s", Some(turn), "cli", "x");
        n.body = Body::AssistantMessage {
            blocks,
            model: "m".into(),
            provider: "anthropic".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        };
        n
    }

    fn laid(nodes: Vec<Node>) -> Vec<(u64, Stub)> {
        (nodes.into_iter().enumerate())
            .map(|(i, n)| (i as u64, Stub::from(n)))
            .collect()
    }

    /// What is sent is the form the budgeter paid for: an old long reply kept as `Line` goes as the person's
    /// words and the did line; a large `fs_write` kept `Spoken` goes with its input bounded, its call kept.
    #[test]
    fn a_kept_form_is_sent_as_that_form() {
        let (_d, s) = store();
        let long = "a long reply ".repeat(2_000);
        let read = Sessions::default();
        let line = as_line(
            (&s, &read),
            laid(vec![
                Node::user("s", Some("t0"), "cli", "what did the audit say?"),
                answer("t0", vec![json!({"type": "text", "text": long})]),
            ]),
            &Handle("t0".into()),
        )
        .unwrap();
        assert_eq!(line.len(), 1);
        let text = crate::recall::text_of(&line[0].1);
        assert!(text.starts_with("what did the audit say?"), "{text}");
        assert!(
            text.contains("reply not summarised") && text.len() < 200,
            "{text}"
        );

        let input = json!({"path": "big.rs", "content": "x".repeat(50_000)});
        let call = json!({"type": "tool_use", "id": "w1", "name": "fs_write", "input": input});
        let mut done = Node::user("s", Some("t1"), "cli", "x");
        done.body = Body::ToolResult {
            tool_use_id: "w1".into(),
            tool: "fs.write".into(),
            status: crate::node::ResultStatus::Ok,
            is_error: false,
            content: "wrote 50,000 bytes".into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: json!({}),
            image: None,
            external: None,
            payload: None,
        };
        let spoken = bounded(
            laid(vec![answer("t1", vec![call]), done]),
            &Handle("t1".into()),
        );
        assert_eq!(spoken.len(), 1, "the call's result is folded into its text");
        let Body::AssistantMessage { blocks, .. } = &spoken[0].1.body else {
            panic!("an answer");
        };
        assert_eq!(
            blocks[0]["type"], "text",
            "historical text, not a tool use with a broken input"
        );
        let sent = blocks[0]["text"].as_str().unwrap();
        assert!(
            sent.starts_with("[earlier call: fs_write") && sent.len() < 800,
            "{sent}"
        );
    }

    /// A long call and a short call with ONE id (a provider that numbers its calls per response) in a `Spoken`
    /// exchange (issue 44): the long one's fold describes its OWN result, and the short one keeps its own.
    #[test]
    fn a_long_and_a_short_call_with_one_id_each_keep_their_own_result() {
        let done = |content: &str| {
            let mut n = Node::user("s", Some("t1"), "cli", "x");
            n.body = Body::ToolResult {
                tool_use_id: "call_0".into(),
                tool: "fs.write".into(),
                status: crate::node::ResultStatus::Ok,
                is_error: false,
                content: content.into(),
                correlation_id: None,
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: json!({}),
                image: None,
                external: None,
                payload: None,
            };
            n
        };
        let input = json!({"path": "big.rs", "content": "x".repeat(50_000)});
        let long = json!({"type": "tool_use", "id": "call_0", "name": "fs_write", "input": input});
        let short = json!({"type": "tool_use", "id": "call_0", "name": "fs_read", "input": {"path": "a.rs"}});
        let spoken = bounded(
            laid(vec![
                answer("t1", vec![long]),
                done("wrote 50,000 bytes"),
                answer("t1", vec![short]),
                done("fn small() {}"),
            ]),
            &Handle("t1".into()),
        );
        let texts: Vec<String> = (spoken.iter())
            .map(|(_, n)| serde_json::to_string(&n.body).unwrap())
            .collect();
        assert_eq!(
            spoken.len(),
            3,
            "only the long call's result is folded: {texts:?}"
        );
        assert!(
            texts[0].contains("[earlier call: fs_write") && texts[0].contains("18 characters"),
            "the fold describes its own result: {}",
            texts[0]
        );
        assert!(
            texts[2].contains("fn small() {}"),
            "the short call keeps its own result: {texts:?}"
        );
    }

    /// **The bounded window keeps a repeated id repeated** (issue 44). A stored response gives one id to a long
    /// use that ran (its record and correlated result stored) and a short one that did not. Folding only the
    /// long use would leave the short one the id's sole use, and the compile would then lay the long use's
    /// result under it. Through the bounded window, the receipts and a compile, no use reads that result.
    #[test]
    fn a_fold_never_makes_a_repeated_id_unique() {
        use crate::compiler::tests_view::{frozen, result, run, said};
        let input = json!({"path": "big.rs", "content": "x".repeat(50_000)});
        let long = json!({"type": "tool_use", "id": "call_0", "name": "fs_write", "input": input});
        let short = json!({"type": "tool_use", "id": "call_0", "name": "fs_read", "input": {"path": "a.rs"}});
        let both = said("t0", vec![long, short]);
        let record = Node::tool_call(
            "s",
            Some("t0"),
            None,
            Body::ToolCall {
                tool_use_id: "call_0".into(),
                tool: "fs.write".into(),
                wire_name: "fs_write".into(),
                input: json!({"path": "big.rs"}),
                assistant_node: both.id.clone(),
                correlation_id: Some("act_long".into()),
                gate: None,
            },
        );
        let mut wrote = result("t0", "call_0", "wrote the long file");
        if let Body::ToolResult { correlation_id, .. } = &mut wrote.body {
            *correlation_id = Some("act_long".into());
        }
        let exchange = vec![
            Node::user("s", Some("t0"), "cli", "ask t0"),
            both,
            record,
            wrote,
            said("t0", vec![json!({"type": "text", "text": "answer t0"})]),
        ];
        let spoken = bounded(laid(exchange), &Handle("t0".into()));
        let window: Vec<Node> = spoken.into_iter().map(|(_, n)| (*n).clone()).collect();
        let mut f = frozen("t1", &window);
        f.window = crate::compiler::view::receipted(f.window, &HashSet::new());
        let first = run(&f, &[Node::user("s", Some("t1"), "cli", "now")], None);
        let all = serde_json::to_string(&first.request.messages).unwrap();
        assert!(
            !all.contains("wrote the long file"),
            "no use reads the long use's result: {all}"
        );
    }

    /// Each session an assembly reads is read from the store once, however many exchanges of it are asked.
    #[test]
    fn an_assembly_reads_each_session_once() {
        let (_d, s) = store();
        let mut records = Vec::new();
        for (sid, turn) in [("a", "t1"), ("b", "t2"), ("a", "t3"), ("b", "t4")] {
            records.push(Node::user(sid, Some(turn), "cli", turn).record().unwrap());
        }
        s.append(&records).unwrap();
        let read = Sessions::default();
        for (sid, turn) in [
            ("a", "t1"),
            ("b", "t2"),
            ("a", "t3"),
            ("b", "t4"),
            ("a", "t1"),
        ] {
            assert_eq!(read.exchange(&s, sid, turn).unwrap().len(), 1);
        }
        assert_eq!(read.reads.get(), 2);
    }
}
