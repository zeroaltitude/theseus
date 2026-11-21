//! The renderer over the store (`super::render::Renderer`), with the view's text (`super::text`): the memory
//! page; the budgeter's pieces as marked blocks, those of who and where and his older words in the fixed
//! front, those that move every turn (waiting, in play, elsewhere lately, called) in the one block beside the
//! message; and the ladder's nodes as fold blocks. The window is not the renderer's: the compiler renders
//! the window's exchanges as real turns (`compiler::view`).
//!
//! A piece is read by its range: an exchange of a place from the store, in the piece's form. A piece of a
//! space this renderer does not read yet (a card, a fact, a task) is left out of the text; it stays in the
//! view, so the view's record still says it was chosen.

use super::budget::View;
use super::fold::FoldNode;
use super::render::{MarkedBlock, Renderer, RequestParts};
use super::text::labels::{self, Origin};
use super::text::{blocks, forms, page};
use super::{Handle, Label, Piece, PointOfView, Slot, Snapshot, Space};
use crate::exchange;
use crate::ladder::did;
use crate::node::Node;
use crate::recall::stamp::ZoneAt;
use crate::store::Store;

/// The renderer over `store`, its times said in `zone`.
pub struct TextRenderer<'a> {
    pub store: &'a Store,
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
        Ok(self
            .store
            .transcript(&ex.session_id)?
            .into_iter()
            .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
            .map(|(_, n)| (*n).clone())
            .collect())
    }

    /// A piece's text in its form, or none for a space not read here.
    fn text(&self, p: &Piece, form: super::Form) -> anyhow::Result<Option<String>> {
        let Space::Exchanges { .. } = &p.range.space else {
            return Ok(None);
        };
        let mut ids = vec![p.range.first.id.as_str()];
        if p.range.last.id != p.range.first.id {
            ids.push(p.range.last.id.as_str());
        }
        let mut out = Vec::new();
        for turn in ids {
            let nodes = self.exchange_nodes(turn)?;
            let line = did::did(self.store, turn)?.map(|d| d.line);
            let handle = Handle(turn.to_string());
            out.push(forms::render(form, &nodes, line.as_deref(), &handle));
        }
        Ok(Some(out.join("\n\n")))
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
                words.push(self.store.get_node(id)?.and_then(|(_, n)| did::words(&n)));
            }
            out.push(blocks::fold_block(n, &words, pov, self.zone));
        }
        Ok(out)
    }
}

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
        heading: "beside your message: what is waiting, in play, said elsewhere lately, and called"
            .into(),
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
            let Some(text) = self.text(p, s.form)? else {
                continue;
            };
            let origin = Origin {
                place: p.marks.place.as_deref().unwrap_or("an unknown place"),
                at_ms: p.range.last.at_ms,
                source: None,
            };
            to.push(labels::block(p, &view.pov, &origin, &text, self.zone));
        }
        let (version, _) = page::memory_page();
        Ok(RequestParts {
            memory_page: version,
            fixed,
            beside: beside(moving),
            ..RequestParts::default()
        })
    }
}
