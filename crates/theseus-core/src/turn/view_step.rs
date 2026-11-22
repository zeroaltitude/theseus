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
use crate::view::sources_store::{author_of, Sessions, StoreSources};
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
        // Before every later call of the turn: a source sealed since ends the old chain (`view_reset`).
        if self.view_revoked(t) {
            t.view = None;
            self.view_freeze(t, input.spec)?;
            Self::view_reset(t, input.nodes);
        }
        self.view_freeze(t, input.spec)?;
        // A refused image under the turn's thinking, or a forced recompile: the same view, a new chain.
        if input.strip.is_some() || input.force.is_some() {
            Self::view_reset(t, input.nodes);
        }
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

    /// Whether a session the frozen view lays has been sealed since it was assembled (the gate's own
    /// reading, for this turn's session).
    fn view_revoked(&self, t: &Turn<'_>) -> bool {
        let Some(f) = t.view.as_ref().filter(|f| f.turn_id == t.tc.turn_id) else {
            return false;
        };
        let sid = t.tc.session_id;
        (f.sessions.iter()).any(|s| crate::view_gate::seal_of(self, Some(sid), s) != Seal::Clear)
    }

    /// A new chain for the turn: its next call makes a new compilation (the sent tails let go), and the
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
        let read = Sessions::default();
        let held = |s: &str| crate::view_gate::seal_of(self, Some(sid), s) != Seal::Clear;
        let sources = StoreSources {
            store,
            mark: since.clone().map(|m| (place.as_str(), m)),
            recalled: recalled(t),
            sealed: &held,
            sessions: &read,
        };
        // One read of the ladder for the whole assembly: every selector, the text and the window see it.
        let store_ladder = StoreLadder(store);
        let ladder = OnceLadder::new(&store_ladder);
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
        let (window, sessions) = window_of((store, &read), &a, &pov, turn, renderer.zone)?;
        let folds = renderer.ladder(&pov, &shown_nodes(&a))?;
        let front = (a.parts.fixed.iter()).chain(&a.parts.ladder).chain(&folds);
        t.view = Some(Frozen {
            turn_id: turn.into(),
            compilation_id: None,
            front: front.map(block).collect(),
            window,
            stripped: HashSet::new(),
            sessions,
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
/// part labelled by its own provenance (`labelled`), the results of those kept spoken as their receipts,
/// made now from the window alone (`compiler::view::receipted`); and every session the view lays.
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
    let (mut window, mut spoken) = (Vec::new(), HashSet::new());
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
    sessions.sort();
    sessions.dedup();
    Ok((crate::compiler::view::receipted(window, &spoken), sessions))
}

/// The most of a call's input a `Spoken` exchange sends, in characters; past it, a bounded receipt.
const CALL_INPUT_CHARS: usize = 400;

/// A `Spoken` exchange as the budgeter paid for it: its words, and each call's input bounded to a receipt
/// past [`CALL_INPUT_CHARS`] (its result is a receipt already); the calls stay paired with their results.
fn bounded(nodes: Vec<(u64, Stub)>, handle: &Handle) -> Vec<(u64, Stub)> {
    nodes
        .into_iter()
        .map(|(p, n)| {
            let Body::AssistantMessage { blocks, .. } = &n.body else {
                return (p, n);
            };
            let long = |b: &serde_json::Value| {
                b["type"] == "tool_use" && b["input"].to_string().chars().count() > CALL_INPUT_CHARS
            };
            if !blocks.iter().any(long) {
                return (p, n);
            }
            let mut node = (*n).clone();
            if let Body::AssistantMessage { blocks, .. } = &mut node.body {
                for b in blocks.iter_mut().filter(|b| long(b)) {
                    let input = b["input"].to_string();
                    let head: String = input.chars().take(CALL_INPUT_CHARS / 2).collect();
                    b["input"] = serde_json::json!({
                        "shown_as": format!(
                            "{head}… ({} characters of input; open {} for the whole call)",
                            input.chars().count(),
                            handle.0
                        )
                    });
                }
            }
            (p, Stub::from(node))
        })
        .collect()
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
    let did = crate::ladder::did::did(store, &handle.0)?.map(|d| d.line);
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
        let spoken = bounded(laid(vec![answer("t1", vec![call])]), &Handle("t1".into()));
        let Body::AssistantMessage { blocks, .. } = &spoken[0].1.body else {
            panic!("an answer");
        };
        assert_eq!(
            blocks[0]["id"], "w1",
            "the call stays, to pair with its result"
        );
        let sent = blocks[0]["input"].to_string();
        assert!(sent.contains("shown_as") && sent.len() < 600, "{sent}");
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
