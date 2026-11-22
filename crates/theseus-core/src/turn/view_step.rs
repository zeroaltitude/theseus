//! A turn in a place set to `view` (issue 44, `[context] assembly`; `compiler::view`): which assembly the
//! turn's place has, and its view, assembled once on its first call and frozen for the rest of the turn.
//! A task's turns are at the task's own place, `task:<task id>` (`view_step::task`).
//!
//! Who is present is never the view's to say: the point of view's presence and anchor are the turn's
//! verified ones (`crate::view_gate::Verified`, computed by `turn::gate_step` from the input's mark and the
//! place read on its own), and the gate is the turn's permit (`view::permit::Permit::for_turn`). A channel
//! bound private, or an agent's process at the terminal, is never the owner alone (decision 5).

use std::collections::{HashMap, HashSet};

mod failed;
pub(crate) mod kept;
pub(in crate::turn) mod occasion;
pub(super) mod preview;
mod recall;
mod room;
pub(super) mod task;

#[cfg(test)]
pub(crate) use super::exchange_step::route_unreadable;
#[cfg(test)]
pub(crate) use failed::fail_next_assembly;
pub(in crate::turn) use failed::ViewFailed;

use super::{compile, CompileInput, Compiled, RequestSpec, Turn, TurnRunner};
use crate::compiler::view::Frozen;
use crate::config::assembly::Assembly;
use crate::node::{Body, Node};
use crate::store::{Store, Transcript};
use crate::stub::{Kind, Stub};
use crate::task_graph::joined::{judged_task, StoreCards};
use crate::view::budget::{Budget, Overflow};
use crate::view::fold::FoldNode;
use crate::view::permit::Permit;
use crate::view::render::{MarkedBlock, Renderer};
use crate::view::select::sources::{Names, Sources, Things};
use crate::view::select::{table, Part};
use crate::view::sources_names::{Called, ViewAdmits};
use crate::view::sources_people::StorePeople;
use crate::view::sources_store::{author_of, Sessions, StoreSources};
use crate::view::sources_waiting::StoreWaiting;
use crate::view::text::forms::{self, Voice};
use crate::view::text::labels::Origin;
use crate::view::text::parts::{self, Seen};
use crate::view::text::tokens;
use crate::view::wire::OnceLadder;
use crate::view::wire::{assemble, Assembled, NotAssembled, Parts, StoreSnapshot, StoreTimeline};
use crate::view::wire_text::TextRenderer;
use crate::view::{Anchor, Form, Handle, Label, PointOfView, Pos, Present, Reasons, Slot, Space};
use crate::view_gate::Verified;
use theseus_memory::gate::Seal;

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

    /// How this turn's requests are assembled: `[context] assembly` for its place (a task's, its own). With
    /// no such setting, the view (the transcript where the ledger is off: `ContextConfig::assembly_for`).
    pub(super) fn assembly_here(&self, t: &Turn<'_>) -> Assembly {
        self.cfg.context.assembly_for(&self.place_here(t))
    }

    /// This loop's compile: in a place set to `view`, from the turn's view (assembled on its first call);
    /// else, and for a view turn over its budget (`Fallback`), the transcript's. A view that cannot be
    /// assembled soundly fails the turn plainly (`ViewFailed`): never a partial view. The view's record is
    /// kept with the compilation it was built for, once.
    pub(super) async fn compile_here(
        &self,
        t: &mut Turn<'_>,
        input: CompileInput<'_>,
    ) -> anyhow::Result<Compiled> {
        if self.assembly_here(t) == Assembly::View {
            // The message's numbered items, named off the path since the exchange opened, are what its
            // view's names are matched with.
            t.exchange.await_items().await;
            match self.view_compile(t, input) {
                Ok(Ok(c)) => return Ok(c),
                Ok(Err(why)) => Self::view_declined(t, why),
                Err(e) if e.is::<ViewFailed>() => return Err(e),
                // Never a partial view: the turn fails with the error's words, and nothing is sent.
                Err(e) => return Err(ViewFailed::Assembly(format!("{e:#}")).into()),
            }
        }
        // A view turn that declined is the transcript's in full: the recall this call withheld for the
        // view, or a view call before it kept, is carried now, as on any transcript turn, whatever the
        // withheld flag says.
        let recarried;
        let mut input: CompileInput<'_> = input;
        let kept = t.recall.withheld.is_some() || t.recall.kept_for_view.is_some();
        if kept {
            t.recall.withheld = None;
            // After a view call was dispatched, the recall it withheld comes back once.
            self.recall_readmitted(t);
            recarried = self.recall_view(t, input.nodes.to_vec());
            (input.nodes, input.sources) = (&recarried.0, &recarried.1);
        }
        // After a view's compilation the transcript starts a new chain: the view's prefix is not its own.
        if input
            .current
            .is_some_and(|c| c.strategy == crate::compiler::view::STRATEGY)
        {
            input.force = input.force.or(Some(crate::compiler::Recompile::Transcript));
        }
        Ok(compile(input))
    }

    /// One call of a view turn, or why this turn is the transcript's (`None`: decided before); a turn whose
    /// place could not be read fails (`ViewFailed::Place`).
    fn view_compile(
        &self,
        t: &mut Turn<'_>,
        input: CompileInput<'_>,
    ) -> anyhow::Result<Result<Compiled, Option<Fallback>>> {
        let turn = t.tc.turn_id;
        let held = t.view.as_ref().filter(|f| f.turn_id == turn);
        if held.is_some_and(|f| f.declined) {
            return Ok(Err(None));
        }
        if held.is_none() {
            if t.exchange.place_unread() {
                return Err(ViewFailed::Place.into());
            }
        } else if self.view_revoked(t) || self.view_moved(t) {
            // A source sealed since, or a presence that changed (a steer narrowed it): reassembled under
            // the new permit, on a new chain, with the room the turn made.
            self.view_refrozen(t, input.spec, None)?;
            Self::view_reset(t, input.nodes);
        }
        self.view_freeze(t, input.spec)?;
        // A refused image under the turn's thinking, or a forced recompile: the same view, a new chain.
        if input.strip.is_some() || input.force.is_some() {
            Self::view_reset(t, input.nodes);
        }
        let store = t.tc.store;
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
            f.card.clone_from(&compiled.card);
            if let Some(view) = f.record.take() {
                let room = f.room.made(compiled.cleared.map_or(0, |p| p.results));
                let replay = kept::chain_kept(store, &mut t.keeping, f);
                if let Err(e) = crate::view::record::keep_made(
                    store,
                    turn,
                    &compiled.compilation.id,
                    &view,
                    (room, replay),
                ) {
                    tracing::warn!(error = %format!("{e:#}"), "the view's record was not kept");
                }
            }
        }
        Ok(Ok(compiled))
    }

    /// After every await of a call's compile and before the provider (issue 44): the frozen view's verified
    /// point of view and its sessions' seals against the gate's reading now. A difference compiles the call
    /// again (the compile step reassembles under the new permit, on a new chain), at most twice; a third
    /// difference fails the call plainly (`ViewFailed::Moved`), and nothing is sent. The request that goes out is the last one
    /// compiled, its tail kept as the turn loop keeps one.
    pub(super) async fn view_readmitted(
        &self,
        t: &mut Turn<'_>,
        session: &mut crate::session::SessionRecord,
        spec: &RequestSpec,
        compiled: Compiled,
        i: u32,
    ) -> anyhow::Result<Result<Compiled, super::Failure>> {
        let mut compiled = compiled;
        for again in 0..=VIEW_READMISSIONS {
            if !(self.view_revoked(t) || self.view_moved(t)) {
                return Ok(Ok(compiled));
            }
            if again == VIEW_READMISSIONS {
                return Ok(Err(Self::view_failed(t, &ViewFailed::Moved)));
            }
            compiled =
                match Box::pin(self.compile_step(t, session, spec, None, None, None, i)).await? {
                    Ok(mut c) => {
                        self.tails.finish(t.tc.session_id, &mut c, t.tail.take());
                        c
                    }
                    Err(f) => return Ok(Err(f)),
                };
        }
        Ok(Ok(compiled))
    }

    /// The turn is the transcript's from now on, and the ledger says why once.
    fn view_declined(t: &mut Turn<'_>, why: Option<Fallback>) {
        let Some(why) = why else {
            return;
        };
        t.tc.ledger(
            theseus_protocol::LedgerKind::ContextViewFallback,
            serde_json::json!({"turn_id": t.tc.turn_id, "why": why.as_str()}),
        );
        t.view = Some(Frozen {
            turn_id: t.tc.turn_id.into(),
            declined: true,
            ..Frozen::default()
        });
    }

    /// Whether the gate reads the turn's point of view differently from when its view was assembled.
    pub(super) fn view_moved(&self, t: &Turn<'_>) -> bool {
        // A declined turn has no view to move (it is the transcript's).
        let Some(f) = (t.view.as_ref()).filter(|f| f.turn_id == t.tc.turn_id && !f.declined) else {
            return false;
        };
        let now = Verified::of_session(self, t.tc.session_id);
        f.verified.as_ref() != Some(now.pov())
    }

    /// Whether a session the frozen view lays has been sealed since it was assembled (the gate's own
    /// reading, for this turn's session).
    pub(super) fn view_revoked(&self, t: &Turn<'_>) -> bool {
        let Some(f) = t.view.as_ref().filter(|f| f.turn_id == t.tc.turn_id) else {
            return false;
        };
        // A session sealed, or a message it shows moved into a thread or that thread sealed, since it was
        // assembled; or a recalled item it lays held out now (`recall::view_recall_revoked`).
        kept::held_now(self, t.tc.session_id, (&f.sessions, &f.exchanges, &[]))
            || self.view_recall_revoked(t.tc.session_id, &f.recalled)
    }

    /// A new chain for the turn: its next call makes a new compilation (the sent tails let go), and the
    /// turn's own answers so far are sent without their thinking, which the change invalidated.
    fn view_reset(t: &mut Turn<'_>, nodes: &[(u64, Stub)]) {
        let turn = t.tc.turn_id;
        let Some(f) = t.view.as_mut() else {
            return;
        };
        f.compilation_id = None;
        f.card = None;
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
        let (fork, permit) = crate::fork::window::Fork::permit(store, sid, turn, verified);
        let read = Sessions::default();
        let held = |s: &str| crate::view_gate::seal_of(self, Some(sid), s) != Seal::Clear;
        let moved_read = crate::view_gate::Moved::read(self, Some(sid));
        let moved = |e: &crate::exchange::Exchange| moved_read.holds(e);
        let sources = StoreSources {
            store,
            mark: since.clone().map(|m| (place.as_str(), m)),
            recalled,
            sealed: &held,
            moved: &moved,
            sessions: &read,
        };
        // One read of the ladder for the whole assembly: every selector, the text and the window see it.
        let store_ladder = fork.ladder(store);
        let gated = GatedLadder::new(&store_ladder, store, &held, &moved);
        let ladder = OnceLadder::new(&gated);
        let moving = self.moving(store, (&permit, &pov, &moved_read), &items, &read)?;
        let cards = StoreCards {
            store,
            kernel: &self.kernel,
            names: &sources,
            homes: self,
            judged: asked.and_then(|n| judged_task(store, sid, &n)),
        };
        let src = sources_of(&permit, (&sources, &moving), &ladder, &cards, &message);
        let renderer = TextRenderer {
            store,
            kernel: Some(&self.kernel),
            sessions: Some(&read),
            zone: &crate::recall::stamp::system,
        };
        let (frame, now) = frame_and_now(spec, renderer.memory_page().1, &message);
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
        let a = match assemble(&pov, &snap, &parts, budget) {
            Ok(a) => a,
            Err(NotAssembled::Read(e)) => return Err(e.context("the view's read")),
            // Rule 9: the floor did not fit. The assembler says so by its numbers; a turn fails on it
            // (`room`), a preview reports it.
            Err(NotAssembled::Overflow(o)) => return Ok(Built::overflowed(turn, o, memory_page)),
        };
        let (window, mut sessions) = window_of((store, &read), &a, &pov, turn, renderer.zone)?;
        sessions.extend(gated.sessions.borrow().iter().cloned());
        let exchanges = laid_exchanges(&window, turn, &gated.exchanges.borrow(), &a);
        let folds = renderer.ladder(&pov, &shown_nodes(&a))?;
        let front = (a.parts.fixed.iter()).chain(&a.parts.ladder).chain(&folds);
        let frozen = Frozen {
            turn_id: turn.into(),
            front: front.map(block).collect(),
            window,
            sessions: sorted(sessions),
            exchanges,
            verified: Some(verified_pov),
            beside: fork.note(a.parts.beside.as_ref().map(block)),
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
        (permit, pov, moved): (&'a Permit, &'a PointOfView, &'a crate::view_gate::Moved<'a>),
        items: &[String],
        read: &'a Sessions,
    ) -> anyhow::Result<Moving<'a>> {
        let (texts, depends) = (&read.texts, &read.depends);
        let called = match self.threads.core() {
            Some(core) => {
                let tasks = crate::task_graph::all(store)?;
                let admits = ViewAdmits::new((permit, pov), (store, self, moved), tasks, depends);
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
                owner: self.cfg.places.owner.clone().unwrap_or_default(),
                now_ms: pov.now_ms,
                texts,
            },
        })
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

/// How many times one call is compiled again when the gate's reading moved under its view.
const VIEW_READMISSIONS: u32 = 2;

/// A test's seam: what runs at the start of the next `env_tail` of a session (after the view's assembly,
/// before the call is sent), once.
#[cfg(test)]
type EnvHook = Box<dyn FnOnce() + Send>;

#[cfg(test)]
static AT_ENV_TAIL: std::sync::Mutex<Vec<(String, EnvHook)>> = std::sync::Mutex::new(Vec::new());

/// Run `f` at the next `env_tail` of `session`.
#[cfg(test)]
pub(crate) fn env_tail_hook(session: &str, f: EnvHook) {
    AT_ENV_TAIL.lock().unwrap().push((session.to_string(), f));
}

#[cfg(test)]
pub(super) fn at_env_tail(session: &str) {
    let hook = {
        let mut hooks = AT_ENV_TAIL.lock().unwrap();
        (hooks.iter().position(|(s, _)| s == session)).map(|i| hooks.remove(i).1)
    };
    if let Some(f) = hook {
        f();
    }
}

/// What one view is assembled for: a turn's (`view_freeze`), or a preview's of a place's next turn.
pub(super) struct Asked<'a> {
    pub store: &'a Store,
    /// The session whose turn it is: the reader a seal of its own thread does not hold out.
    pub sid: &'a str,
    pub turn: &'a str,
    pub person: Option<String>,
    /// The gate's point of view, never the caller's word: a preview's; none reads the turn's
    /// (`Verified::of_session`) where a turn always read it, after the clock.
    pub verified: Option<Verified>,
    /// The items today's recall found and admitted, read for slot H (none for a preview, which runs no
    /// recall).
    pub recalled: Vec<crate::view::sources_recall::Recalled>,
    /// The message given (a preview's); none reads the turn's own, by node and text, at the point a turn
    /// always read it (after the clock and the `/new` mark).
    pub message: Option<String>,
    /// The message's numbered items, as the exchange's naming told them (none for a preview).
    pub items: Vec<String>,
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

/// Why a turn in a view place is the transcript's: a closed list, said in `context.view_fallback`. It is
/// empty: a turn no message opened, a turn's place, history the ledger holds no exchange for, a failed read,
/// a gate that kept moving (issue 44) and a request too large (`room`) are each the view's own to answer, and
/// none hands a turn back. The plumbing goes with the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fallback {}

impl Fallback {
    pub(super) fn as_str(self) -> &'static str {
        match self {}
    }
}

/// The ladder as the gate reads it today: a fold node is kept only when every exchange it stands for
/// resolves to a session, none of those sessions is sealed, seal pending or unreadable now, and none of
/// those exchanges holds a message or reply moved into a thread not proven clear (`view_gate::Moved`; a
/// fold built before the move is held so); its words AND its account go together. The sessions and the
/// exchanges of those kept are recorded, so a seal or a move before a later call revokes them too.
struct GatedLadder<'a> {
    inner: &'a dyn crate::view::fold::Ladder,
    store: &'a Store,
    held: &'a dyn Fn(&str) -> bool,
    moved: &'a dyn Fn(&crate::exchange::Exchange) -> bool,
    sessions: std::cell::RefCell<Vec<String>>,
    exchanges: std::cell::RefCell<Vec<String>>,
}

impl<'a> GatedLadder<'a> {
    fn new(
        inner: &'a dyn crate::view::fold::Ladder,
        store: &'a Store,
        held: &'a dyn Fn(&str) -> bool,
        moved: &'a dyn Fn(&crate::exchange::Exchange) -> bool,
    ) -> Self {
        Self {
            inner,
            store,
            held,
            moved,
            sessions: Default::default(),
            exchanges: Default::default(),
        }
    }

    /// Every exchange fold node `id` stands for; none when any cannot be resolved, or any holds a moved
    /// message the reader may not read.
    fn constituents(&self, id: &str) -> Option<Vec<crate::exchange::Exchange>> {
        fold_constituents(self.store, id, self.moved)
    }
}

impl crate::view::fold::Ladder for GatedLadder<'_> {
    fn read(
        &self,
        place: &str,
        n: usize,
        since: Option<&Pos>,
    ) -> anyhow::Result<crate::view::fold::LadderRead> {
        let mut read = self.inner.read(place, n, since)?;
        let mut kept = Vec::new();
        for node in read.nodes {
            let Some(exchanges) = self.constituents(&node.handle.0) else {
                continue;
            };
            if exchanges.iter().any(|e| (self.held)(&e.session_id)) {
                continue;
            }
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

/// Every exchange fold node `id` stands for; none when any cannot be resolved, or any holds a moved message
/// the reader may not read (`moved`, `view_gate::Moved::holds`). The gated ladder's rule, and a preview's
/// listing of a fold.
pub(super) fn fold_constituents(
    store: &Store,
    id: &str,
    moved: &dyn Fn(&crate::exchange::Exchange) -> bool,
) -> Option<Vec<crate::exchange::Exchange>> {
    let node = crate::ladder::fold::node(store, id).ok()??;
    let read = crate::ladder::fold::open(store, &node).ok()?;
    let whole = !node.exchanges.is_empty() && read.len() == node.exchanges.len();
    (whole && !read.iter().any(moved)).then_some(read)
}

/// `v` in order, each once.
fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// Every exchange a view lays or folds, once each: the window's (but `turn`'s own), the kept folds'
/// constituents, and the ends of every shown exchange piece's range. Their messages' node seals are asked
/// again before each call.
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
    // The exchanges a shown card read its excerpts from (its provenance): a move since revokes it too.
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
}

/// Every source the selectors read: the store's, the turn's permit as their gate, the ladder read once,
/// the named things (none outside a core), what waits, the facts about the person, the joined task's
/// card, and the message.
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
    }
}

/// Slots A and I as code lays them: the system part with the memory page, and the message.
fn frame_and_now(spec: &RequestSpec, page: &str, message: &str) -> (Vec<Part>, Vec<Part>) {
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
    let now = Part {
        name: "message".into(),
        tokens: tokens(message),
    };
    (vec![frame], vec![now])
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
/// made now from the window alone (`compiler::view::receipted`); and every session the view lays, those a
/// card, a waiting line or a fact was read from (`Sessions::depends`) among them, so a seal of any before a
/// later call revokes the view.
fn window_of(
    (store, read): (&Store, &Sessions),
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
    // A task's opening heads its window (`task::window_head`); any other place's window opens empty.
    let (mut window, mut spoken) = (task::window_head(store, pov)?, HashSet::new());
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
            Form::Whole => nodes,
            Form::Spoken => bounded(nodes, &handle),
            Form::Line => as_line(store, nodes, &handle)?,
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
    // A joined task card's home session is a dependency too: its seal before a later call revokes the card.
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
    let results: HashMap<String, Node> = (nodes.iter())
        .filter_map(|(_, n)| match &n.body {
            Body::ToolResult { tool_use_id, .. } => Some((tool_use_id.clone(), (**n).clone())),
            _ => None,
        })
        .collect();
    let mut folded: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (p, n) in nodes {
        match &n.body {
            Body::AssistantMessage { blocks, .. } if blocks.iter().any(long) => {
                let mut node = (*n).clone();
                if let Body::AssistantMessage { blocks, .. } = &mut node.body {
                    for b in blocks.iter_mut().filter(|b| long(b)) {
                        let id = b["id"].as_str().unwrap_or_default().to_string();
                        let name = b["name"].as_str().unwrap_or_default().to_string();
                        let input = b["input"].to_string();
                        let head: String = input.chars().take(CALL_INPUT_CHARS / 2).collect();
                        let result = results.get(&id).map_or_else(
                            || "no result".into(),
                            |r| forms::stub(&name, &b["input"], Some(r)),
                        );
                        *b = serde_json::json!({"type": "text", "text": format!(
                            "[earlier call: {name} {head}… ({} characters of input); {result}; open {} for \
                             the whole call]",
                            input.chars().count(),
                            handle.0
                        )});
                        folded.insert(id);
                    }
                }
                out.push((p, Stub::from(node)));
            }
            Body::ToolResult { tool_use_id, .. } if folded.contains(tool_use_id) => {}
            _ => out.push((p, n)),
        }
    }
    out
}

/// A `Line` exchange as the budgeter paid for it: the person's words (as labelled), then the `did` line
/// (or "reply not summarised") and the handle, as one message of the person's turn.
fn as_line(
    store: &Store,
    nodes: Vec<(u64, Stub)>,
    handle: &Handle,
) -> anyhow::Result<Vec<(u64, Stub)>> {
    let Some((p, ask)) = nodes.into_iter().find(|(_, n)| n.kind == Kind::UserMessage) else {
        return Ok(Vec::new());
    };
    let did = crate::ladder::did::did(store, &handle.0)?
        .map(|d| crate::ladder::did::shown_line(store, &d));
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
        let line = as_line(
            &s,
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

#[cfg(test)]
#[path = "view_step_moved.rs"]
mod moved_tests;
