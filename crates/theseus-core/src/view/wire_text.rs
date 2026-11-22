//! The renderer over the store (`super::render::Renderer`), with the view's text (`super::text`): the memory
//! page; the budgeter's pieces as marked blocks, those of who and where and his older words in the fixed
//! front, those that move every turn (waiting, in play, elsewhere lately, called) in the one block beside the
//! message; and the ladder's nodes as fold blocks. The window is not the renderer's: the compiler renders
//! the window's exchanges as real turns (`compiler::view`).
//!
//! A piece is read by its range: an exchange of a place from the store, in the piece's form; a named
//! thing's card, a waiting line and the facts about the person as the turn's sources read them
//! (`super::sources_store::Sessions::texts`). A piece with no text read is left out of the text; it stays in
//! the view, so the view's record still says it was chosen.

use super::budget::View;
use super::fold::FoldNode;
use super::render::{MarkedBlock, Renderer, RequestParts};
use super::sources_store::author_of;
use super::text::forms::{self, Said, Voice};
use super::text::labels::{self, Origin};
use super::text::parts::{self, Seen};
use super::text::{blocks, page};
use super::{Anchor, Form, Handle, Label, Piece, PointOfView, Slot, Snapshot, Space};
use crate::exchange;
use crate::ladder::did;
use crate::node::Node;
use crate::recall::stamp::ZoneAt;
use crate::store::Store;

/// The renderer over `store`, its times said in `zone`.
pub struct TextRenderer<'a> {
    pub store: &'a Store,
    /// Where a task's card reads its jobs; none renders no task card.
    pub kernel: Option<&'a theseus_kernel::Kernel>,
    /// The assembly's one read of each session; none reads the store each time.
    pub sessions: Option<&'a super::sources_store::Sessions>,
    pub zone: ZoneAt<'a>,
}

/// Where a slot's pieces go on the wire.
enum Part {
    Fixed,
    Beside,
    /// The frame, the window and the message are laid by the compiler.
    Elsewhere,
}

fn part(slot: Slot) -> Part {
    match slot {
        Slot::WhoAndWhere | Slot::Past => Part::Fixed,
        Slot::Waiting | Slot::InPlay | Slot::Elsewhere | Slot::Called => Part::Beside,
        Slot::Frame | Slot::Here | Slot::Now => Part::Elsewhere,
    }
}

impl TextRenderer<'_> {
    /// The nodes of exchange `turn`, in order, from the session that ran it.
    fn exchange_nodes(&self, turn: &str) -> anyhow::Result<Vec<Node>> {
        let Some(ex) = exchange::exchange(self.store, turn)? else {
            return Ok(Vec::new());
        };
        let nodes = match self.sessions {
            Some(s) => s.exchange(self.store, &ex.session_id, turn)?,
            None => (self.store.transcript(&ex.session_id)?.into_iter())
                .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
                .collect(),
        };
        Ok(nodes.into_iter().map(|(_, n)| (*n).clone()).collect())
    }

    /// A piece's marked blocks: an exchange's parts each labelled by its own provenance
    /// (`parts::exchange_blocks`); a piece of a space not read here, none.
    fn blocks(&self, p: &Piece, form: Form, pov: &PointOfView) -> anyhow::Result<Vec<MarkedBlock>> {
        match &p.range.space {
            Space::Exchanges { .. } => {}
            Space::Task { task } => return self.task_card(p, task, pov),
            Space::Card { .. } | Space::Waiting { .. } | Space::Facts { .. } => {
                return Ok(self.read_text(p, form, pov))
            }
            // A recalled item is laid whole, as its `Recall` note renders it, or not at all.
            Space::Recalled { .. } => return Ok(self.read_text(p, Form::Whole, pov)),
            _ => return Ok(Vec::new()),
        }
        let place = p.marks.place.as_deref().unwrap_or("an unknown place");
        let here = matches!(&pov.anchor, Anchor::Place(a) if a == place);
        let mut ids = vec![p.range.first.id.as_str()];
        if p.range.last.id != p.range.first.id {
            ids.push(p.range.last.id.as_str());
        }
        let mut out = Vec::new();
        for turn in ids {
            let nodes = self.exchange_nodes(turn)?;
            let handle = Handle(turn.to_string());
            let said = match form {
                Form::Whole => forms::whole(&nodes),
                Form::Spoken => forms::spoken(&nodes),
                Form::Line => line(
                    &nodes,
                    did::did(self.store, turn)?.map(|d| did::shown_line(self.store, &d)),
                    &handle,
                ),
            };
            let seen = Seen {
                origin: Origin {
                    place,
                    at_ms: p.range.last.at_ms,
                    source: None,
                },
                here,
                live: false,
                reasons: &p.reasons,
                range: p.range.clone(),
                handle,
            };
            let author = |s: &Said| author_of(self.store, &s.node);
            out.extend(parts::exchange_blocks(&said, &author, &seen, self.zone));
        }
        Ok(out)
    }

    /// A joined task's card, one block of one provenance (`labels::try_block`).
    fn task_card(
        &self,
        p: &Piece,
        task: &str,
        pov: &PointOfView,
    ) -> anyhow::Result<Vec<MarkedBlock>> {
        let Some(kernel) = self.kernel else {
            return Ok(Vec::new());
        };
        let Some(card) = crate::task_graph::joined::read(self.store, kernel, task, pov.now_ms)?
        else {
            return Ok(Vec::new());
        };
        let origin = Origin {
            place: p.marks.place.as_deref().unwrap_or("an unknown place"),
            at_ms: p.range.last.at_ms,
            source: None,
        };
        let text = card.text();
        let block = labels::try_block(p, pov, &origin, &text, self.zone)
            .unwrap_or_else(|_| labels::block(p, pov, &origin, &text, self.zone));
        Ok(vec![block])
    }

    /// A piece whose text the turn's sources read (a card, a waiting line, the facts about the person), as
    /// one block of its own provenance: whole, or its first line alone in the line form.
    fn read_text(&self, p: &Piece, form: Form, pov: &PointOfView) -> Vec<MarkedBlock> {
        let read = self
            .sessions
            .and_then(|s| s.texts.borrow().get(&p.range.space).cloned());
        let Some(text) = read else {
            return Vec::new();
        };
        let text = match form {
            Form::Line => text.lines().next().unwrap_or_default().to_string(),
            Form::Whole | Form::Spoken => text,
        };
        let origin = Origin {
            place: p.marks.place.as_deref().unwrap_or("an unknown place"),
            at_ms: p.range.last.at_ms,
            source: None,
        };
        vec![labels::block(p, pov, &origin, &text, self.zone)]
    }

    /// The ladder's nodes as fold blocks, oldest first, the person's words read from the store.
    pub fn ladder(
        &self,
        pov: &PointOfView,
        nodes: &[FoldNode],
    ) -> anyhow::Result<Vec<MarkedBlock>> {
        let mut out = Vec::new();
        for n in nodes {
            let mut words = Vec::new();
            for id in &n.his_words {
                let text = self.store.get_node(id)?.and_then(|(_, n)| did::words(&n));
                words.push((author_of(self.store, id), text));
            }
            out.push(blocks::fold_block_by(n, &words, pov, self.zone));
        }
        Ok(out)
    }
}

/// An exchange in the line form, as parts: the person's words exactly, then the `did` line (or "reply not
/// summarised") and the handle, as code's own mark.
fn line(nodes: &[Node], did: Option<String>, handle: &Handle) -> Vec<Said> {
    let mut out: Vec<Said> = forms::whole(nodes)
        .into_iter()
        .filter(|s| s.voice == Voice::Person)
        .take(1)
        .collect();
    let did = did.filter(|d| !d.trim().is_empty());
    out.push(Said {
        voice: Voice::Mark,
        text: format!(
            "→ {} (open {})",
            did.as_deref().map_or("reply not summarised", str::trim),
            handle.0
        ),
        node: String::new(),
        call: None,
        outside: None,
    });
    out
}

/// The heading of the block beside the message: the harness's, never the person's words
/// (`task_graph::view::is_head` lists it for the stand-in models that read those).
pub const BESIDE_HEAD: &str =
    "beside your message: what is waiting, in play, said elsewhere lately, and called";

/// The marked blocks beside the message as one: each keeps its own heading.
fn beside(blocks: Vec<MarkedBlock>) -> Option<MarkedBlock> {
    let label = blocks.first()?.label;
    let text = blocks
        .iter()
        .map(|b| match b.heading.is_empty() {
            true => b.text.clone(),
            false => format!("{}\n{}", b.heading, b.text),
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    Some(MarkedBlock {
        label: match blocks.iter().all(|b| b.label == label) {
            true => label,
            false => Label::Testimony,
        },
        heading: BESIDE_HEAD.into(),
        text,
        sources: blocks.iter().flat_map(|b| b.sources.clone()).collect(),
        handle: None,
    })
}

impl Renderer for TextRenderer<'_> {
    fn memory_page(&self) -> (u32, &'static str) {
        page::memory_page()
    }

    fn render(&self, view: &View, _: &dyn Snapshot) -> anyhow::Result<RequestParts> {
        let (mut fixed, mut moving) = (Vec::new(), Vec::new());
        for s in &view.shown {
            let p = &s.piece;
            let to = match part(p.slot) {
                Part::Fixed => &mut fixed,
                Part::Beside => &mut moving,
                Part::Elsewhere => continue,
            };
            to.extend(self.blocks(p, s.form, &view.pov)?);
        }
        recall_preamble(&mut moving);
        let (version, _) = page::memory_page();
        Ok(RequestParts {
            memory_page: version,
            fixed,
            beside: beside(moving),
            ..RequestParts::default()
        })
    }
}

/// The recall note's own first line, once, before the first recalled item laid (issue 44): the same words a
/// `Recall` note opens with, its count the items laid, so the boundary is said whatever each item's label.
fn recall_preamble(moving: &mut Vec<MarkedBlock>) {
    let recalled =
        |b: &MarkedBlock| (b.sources.iter()).any(|r| matches!(r.space, Space::Recalled { .. }));
    let Some(at) = moving.iter().position(recalled) else {
        return;
    };
    let n = moving.iter().filter(|b| recalled(b)).count();
    moving.insert(
        at,
        MarkedBlock {
            label: Label::Testimony,
            heading: String::new(),
            text: crate::recall::render::preamble(n),
            sources: Vec::new(),
            handle: None,
        },
    );
}
