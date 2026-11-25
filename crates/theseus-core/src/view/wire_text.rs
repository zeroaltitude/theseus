//! The renderer over the store (`super::render::Renderer`), with the view's text (`super::text`): the memory
//! page; the budgeter's pieces as marked blocks, those of who and where and his older words in the fixed
//! front, those that move every turn (every place's newest exchanges, waiting, in play, called) in the one
//! block beside the message; and the ladder's nodes as fold blocks. The window is not the renderer's: the
//! compiler renders the window's exchanges as real turns (`compiler::view`).
//!
//! A piece is read by its range: an exchange of a place from the store, in the piece's form, each node with
//! what has been said about it since (`copies::since`, as the window lays it); a named thing's card, a
//! waiting line and the facts about the person as the turn's sources read them
//! (`super::sources_store::Sessions::texts`). A piece with no text read is left out of the text; it stays in
//! the view, so the view's record still says it was chosen.

use std::collections::{BTreeSet, HashMap};

use super::budget::View;
use super::fold::FoldNode;
use super::render::{MarkedBlock, Renderer, RequestParts};
use super::select::{Band, RECENT_LINE_TOKENS};
use super::sources_recall;
use super::sources_store::author_of;
use super::text::forms::{self, Said, Voice};
use super::text::labels::{self, Origin};
use super::text::parts::{self, Seen};
use super::text::{blocks, page};
use super::{Anchor, Form, Handle, Label, Piece, PointOfView, Slot, Snapshot, Space};
use crate::copies::since::Since;
use crate::exchange;
use crate::ladder::did;
use crate::node::Node;
use crate::recall::stamp::ZoneAt;
use crate::store::Store;
use crate::stub::Stub;

/// The renderer over `store`, its times said in `zone`.
pub struct TextRenderer<'a> {
    pub store: &'a Store,
    /// Where a task's card reads its jobs; none renders no task card.
    pub kernel: Option<&'a theseus_kernel::Kernel>,
    /// The assembly's one read of each session; none reads the store each time.
    pub sessions: Option<&'a super::sources_store::Sessions>,
    pub zone: ZoneAt<'a>,
    /// What has been said about an exchange's nodes since, laid with its words as the window lays them
    /// (`copies::since`); none lays them as stored.
    pub since: Option<&'a Since<'a>>,
}

/// Where a slot's pieces go on the wire.
enum Part {
    Fixed,
    Beside,
    /// The frame, the window and the message are laid by the compiler.
    Elsewhere,
}

fn part(p: &Piece) -> Part {
    match p.slot {
        // Who am I talking to: the people in the front; their recent lines elsewhere move every turn.
        Slot::WhoAndWhere if p.band == Band::PeopleRecent => Part::Beside,
        Slot::WhoAndWhere => Part::Fixed,
        Slot::Waiting | Slot::InPlay | Slot::Elsewhere | Slot::Called => Part::Beside,
        // A retired slot is only read from an old record; no view lays it.
        Slot::Frame | Slot::Here | Slot::Now | Slot::Retired => Part::Elsewhere,
    }
}

impl TextRenderer<'_> {
    /// The nodes of exchange `turn`, in order, from the session that ran it, each with what has been said
    /// about it since.
    fn exchange_nodes(&self, turn: &str) -> anyhow::Result<Vec<Node>> {
        let mut nodes = self.exchange_stubs(turn)?;
        if let Some(since) = self.since {
            nodes = since.lay(nodes)?;
        }
        Ok(nodes.into_iter().map(|(_, n)| (*n).clone()).collect())
    }

    /// Exchange `turn`'s `did` line as shown, unless a fork's point falls in or before it (`fork::cut`).
    fn summary(&self, turn: &str) -> anyhow::Result<Option<String>> {
        let summarised = (self.sessions)
            .map(|s| s.summarised(self.store, turn))
            .transpose()?;
        if summarised == Some(false) {
            return Ok(None);
        }
        Ok(did::did(self.store, turn)?.map(|d| did::shown_line(self.store, &d)))
    }

    /// Exchange `turn`'s nodes, in order, each as its newest record ([`newest`]).
    fn exchange_stubs(&self, turn: &str) -> anyhow::Result<Vec<(u64, Stub)>> {
        let Some(ex) = exchange::exchange(self.store, turn)? else {
            return Ok(Vec::new());
        };
        Ok(newest(match self.sessions {
            Some(s) => s.exchange(self.store, &ex.session_id, turn)?,
            None => (self.store.transcript(&ex.session_id)?.into_iter())
                .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
                .collect(),
        }))
    }

    /// A piece's marked blocks: an exchange's parts each labelled by its own provenance
    /// (`parts::exchange_blocks`); a piece of a space not read here, none.
    fn blocks(&self, p: &Piece, form: Form, pov: &PointOfView) -> anyhow::Result<Vec<MarkedBlock>> {
        match &p.range.space {
            Space::Exchanges { .. } => {}
            Space::Task { task } => return self.task_card(p, task, pov),
            Space::Card { .. } | Space::Waiting { .. } => {
                return Ok(self.read_text(p, form, pov, labels::block))
            }
            // The owner's filing about a person: headed with no time, so the front holds.
            Space::Facts { .. } => return Ok(self.read_text(p, form, pov, labels::filing_block)),
            // A recalled item is laid whole, as its `Recall` note renders it, or not at all.
            Space::Recalled { .. } => {
                return Ok(self.read_text(p, Form::Whole, pov, sources_recall::block))
            }
            _ => return Ok(Vec::new()),
        }
        let place = p.marks.place.as_deref().unwrap_or("an unknown place");
        let here = matches!(&pov.anchor, Anchor::Place(a) if a == place);
        let mut ids = vec![p.range.first.id.as_str()];
        if p.range.last.id != p.range.first.id {
            ids.push(p.range.last.id.as_str());
        }
        // A person's recent line elsewhere, or a line from every place: within its own bound, cut with its
        // handle, its words headed with the person's name.
        let lined = matches!(p.band, Band::PeopleRecent | Band::Everywhere);
        let mut out = Vec::new();
        for turn in ids {
            let nodes = self.exchange_nodes(turn)?;
            let handle = Handle(turn.to_string());
            let said = match form {
                Form::Whole => forms::whole(&nodes),
                Form::Spoken => forms::spoken(&nodes),
                Form::Line if lined => forms::fit(
                    line(&nodes, self.summary(turn)?, &handle),
                    RECENT_LINE_TOKENS,
                    &handle,
                ),
                Form::Line => line(&nodes, self.summary(turn)?, &handle),
            };
            // Its words are headed with the person's name, as the owner's records hold it.
            let person = match lined {
                true => self.person_name(turn)?,
                false => None,
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
                person: person.as_deref(),
            };
            let author = |s: &Said| author_of(self.store, &s.node);
            out.extend(parts::exchange_blocks(&said, &author, &seen, self.zone));
        }
        Ok(out)
    }

    /// A person's recent line elsewhere (`Band::PeopleRecent`) as it is laid beside the message: its blocks,
    /// each heading and wrapper with the name in it, and the blank line after it, in tokens by today's estimate.
    /// The measure the line's bound is kept by (`select::people`), read the way the request lays it.
    pub fn recent_tokens(&self, p: &Piece, pov: &PointOfView) -> anyhow::Result<u64> {
        let blocks = self.blocks(p, Form::Line, pov)?;
        let text: String = blocks.iter().map(|b| laid(b) + "\n\n").collect();
        Ok(super::text::tokens(&text))
    }

    /// The name of the person who opened exchange `turn`, as the turn's sources found it in the owner's
    /// records (`super::sources_store::Sessions::names`); none when they found none.
    fn person_name(&self, turn: &str) -> anyhow::Result<Option<String>> {
        let Some(person) = exchange::exchange(self.store, turn)?.and_then(|ex| ex.person) else {
            return Ok(None);
        };
        Ok(self
            .sessions
            .and_then(|s| s.names.borrow().get(&person).cloned()))
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
    fn read_text(&self, p: &Piece, form: Form, pov: &PointOfView, lay: Lay) -> Vec<MarkedBlock> {
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
        vec![lay(p, pov, &origin, &text, self.zone)]
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

/// A session's records of an exchange, each node once, in its place, as its newest record: a transcript holds
/// every record of a node, and one written again (its tombstone, when it was erased) reads as the history tool
/// reads it (`Store::get_node`), so an erased message lays none of its words.
fn newest(nodes: Vec<(u64, Stub)>) -> Vec<(u64, Stub)> {
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<(u64, Stub)> = Vec::new();
    for (p, n) in nodes {
        match at.get(&n.id) {
            Some(&i) => out[i].1 = n,
            None => {
                at.insert(n.id.clone(), out.len());
                out.push((p, n));
            }
        }
    }
    out
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

/// The heading of the block beside the message: the harness's own line
/// (`task_graph::view::is_head` lists it for the stand-in models that read those). It names no topic: on
/// build 190 its earlier words, "what is waiting, in play, said elsewhere lately, and called", read to a small
/// model as an agenda, and a one-line message was answered with a status report and a digest of what was said
/// elsewhere (X190). Clause by clause:
/// - "Harness-supplied context, not a new request": who laid the block, and that its presence asks for nothing.
/// - "Each item's own framing still applies": an order, the owner's words or an outside-text warning inside it
///   keeps its force; the heading disowns none of them (some items ARE the person's words, said elsewhere).
/// - "Address the current request or reason for this turn": true for a message, and for a continuation or a
///   wake, whose actionable line stands BEFORE the block and is followed by nothing.
/// - "do not summarize this block merely because it is present": the fault itself, named.
pub const BESIDE_HEAD: &str = "[Harness-supplied context, not a new request. Each item's own framing still \
                               applies. Address the current request or reason for this turn; do not summarize \
                               this block merely because it is present.]";

/// One block as it is laid beside the message: its heading, then its text.
fn laid(b: &MarkedBlock) -> String {
    match b.heading.is_empty() {
        true => b.text.clone(),
        false => format!("{}\n{}", b.heading, b.text),
    }
}

/// The marked blocks beside the message as one: each keeps its own heading.
fn beside(blocks: Vec<MarkedBlock>) -> Option<MarkedBlock> {
    let label = blocks.first()?.label;
    let text = blocks.iter().map(laid).collect::<Vec<_>>().join("\n\n");
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

/// How a piece of one provenance becomes its block: `labels::block`, or a recalled item's own.
type Lay = fn(&Piece, &PointOfView, &Origin<'_>, &str, ZoneAt<'_>) -> MarkedBlock;

impl Renderer for TextRenderer<'_> {
    fn memory_page(&self) -> (u32, &'static str) {
        page::memory_page()
    }

    /// Every node an exchange shown here lays with its words, in the form it is laid
    /// (`sources_recall::carries`): a window's, a fork's inherited one, or another slot's.
    fn carried(&self, view: &View) -> anyhow::Result<BTreeSet<String>> {
        let mut out = BTreeSet::new();
        for s in
            (view.shown.iter()).filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
        {
            for turn in [&s.piece.range.first.id, &s.piece.range.last.id] {
                let laid = self.exchange_stubs(turn)?.into_iter();
                out.extend(
                    laid.filter(|(_, n)| sources_recall::carries(s.form, n.kind))
                        .map(|(_, n)| n.id),
                );
            }
        }
        Ok(out)
    }

    fn render(&self, view: &View, _: &dyn Snapshot) -> anyhow::Result<RequestParts> {
        let (mut fixed, mut moving) = (Vec::new(), Vec::new());
        for s in &view.shown {
            let p = &s.piece;
            let to = match part(p) {
                Part::Fixed => &mut fixed,
                Part::Beside => &mut moving,
                Part::Elsewhere => continue,
            };
            to.extend(self.blocks(p, s.form, &view.pov)?);
        }
        recall_preamble(&mut moving);
        recall_left_out(&mut moving, view);
        floor_left_out(&mut moving, view);
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

/// What a bare floor cut of the newest exchanges here, said first beside the message (after a provider refused a
/// request of the turn for its size, `turn::view_step::room`): `[N more of the newest exchanges here, about T
/// tokens, left out; open HANDLE]` (`blocks::residue_line`), the handle the newest exchange cut. A floor is cut
/// only there, so an ordinary view has no such line. A line that would cost as much as what it stands for is not
/// said: the bare floor is there to send a smaller request than the refused one.
fn floor_left_out(moving: &mut Vec<MarkedBlock>, view: &View) {
    use super::budget::{DropWhy, Residue};
    let cut: Vec<&super::budget::Dropped> = (view.dropped.iter())
        .filter(|d| d.reasons.contains(&super::Reason::Floor))
        .filter(|d| matches!(d.why, DropWhy::OverBudget | DropWhy::UnitCut))
        .collect();
    let Some(newest) = cut.iter().max_by(|a, b| a.range.last.cmp(&b.range.last)) else {
        return;
    };
    let r = Residue {
        band: Band::Floor,
        pieces: u32::try_from(cut.len()).unwrap_or(u32::MAX),
        tokens: cut.iter().map(|d| d.tokens).sum(),
    };
    let Some(text) = blocks::residue_line(&r, &newest.handle) else {
        return;
    };
    if super::text::tokens(&text) >= r.tokens {
        return;
    }
    moving.insert(
        0,
        MarkedBlock {
            label: Label::Testimony,
            heading: String::new(),
            text,
            sources: Vec::new(),
            handle: None,
        },
    );
}

/// What the budget cut of slot H, said where its items end (issue 44): `[N more of what search recalled,
/// about T tokens, left out; open HANDLE]` (`blocks::residue_line`), the handle the newest item cut. With
/// every item cut, the line alone, and no preamble. A passage set aside as already in the window is not
/// counted: it is there.
fn recall_left_out(moving: &mut Vec<MarkedBlock>, view: &View) {
    use super::budget::DropWhy;
    let Some(r) = view.residue.iter().find(|r| r.band == Band::Recall) else {
        return;
    };
    let Some(cut) = (view.dropped.iter())
        .find(|d| d.band == Band::Recall && !matches!(d.why, DropWhy::Merged | DropWhy::InWindow))
    else {
        return;
    };
    let Some(text) = blocks::residue_line(r, &cut.handle) else {
        return;
    };
    let recalled =
        |b: &MarkedBlock| (b.sources.iter()).any(|r| matches!(r.space, Space::Recalled { .. }));
    let at = moving
        .iter()
        .rposition(recalled)
        .map_or(moving.len(), |i| i + 1);
    moving.insert(
        at,
        MarkedBlock {
            label: Label::Testimony,
            heading: String::new(),
            text,
            sources: Vec::new(),
            handle: None,
        },
    );
}
