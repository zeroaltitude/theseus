//! Slot G, here: the window and the ladder of the place, through the ladder's read (design §5.2). Four
//! bands: the floor (the words of the newest three exchanges), the rest of the window's words, the window's
//! tool results, and the ladder's nodes. A window exchange is one range: its words are asked for spoken and
//! its tool results whole, so rule 3 merges them and the cut takes the tool traffic away at the tool
//! results' place in the order, never the words of the floor. The newest exchange's tool results are the
//! window's like any other's: laid whole when there is room, as their receipts when there is not. Which
//! exchanges are the floor is chosen over those the gate admits, so a denied exchange never pushes a
//! permitted one out of it.

use super::super::fold::{FoldNode, FOLD_STEP};
use super::super::timeline::ExchangeRef;
use super::super::{Anchor, Form, Piece, PointOfView, Reason, SourceRange, Space};
use super::make::{self, Cut};
use super::sources::Sources;
use super::{Band, Selector, Snapshot};

/// The floor: the words of the newest this-many exchanges of the window are never cut.
pub const FLOOR_EXCHANGES: usize = 3;

/// The place a point of view's here-slots read, if it is anchored to one.
pub(super) fn anchor_place(pov: &PointOfView) -> Option<&str> {
    match &pov.anchor {
        Anchor::Task(_) => None,
        Anchor::Place(p) => Some(p),
    }
}

/// The window's exchanges, oldest first, with their facts; and the ladder's nodes.
fn read(src: &Sources<'_>, place: &str) -> anyhow::Result<(Vec<Window>, Vec<FoldNode>)> {
    let since = src.places.new_mark(place)?;
    let read = src.ladder.read(place, FOLD_STEP, since.as_ref())?;
    let mut window = Vec::with_capacity(read.window.len());
    for e in read.window {
        if let Some(f) = src.exchanges.facts(&e.turn_id)? {
            window.push(Window { e, f });
        }
    }
    Ok((window, read.nodes))
}

pub(super) struct Window {
    pub(super) e: ExchangeRef,
    pub(super) f: super::sources::ExchangeFacts,
}

/// Whether the gate admits window exchange `w` (as the floor asks for it): only those are laid here, and every
/// choice over the window (the floor's newest three, the people in it) is made over those.
fn laid(src: &Sources<'_>, pov: &PointOfView, w: &Window) -> bool {
    let p = make::exchange(
        &w.e,
        &w.f,
        Band::Floor,
        Reason::Floor,
        Form::Spoken,
        Cut::Window,
    );
    make::admits(src, pov, &p)
}

/// The window's exchanges the gate admits, oldest first: the exchanges this place's window lays (block 4).
/// None for a point of view anchored to no place.
pub(super) fn shown(src: &Sources<'_>, pov: &PointOfView) -> anyhow::Result<Vec<Window>> {
    let Some(place) = anchor_place(pov) else {
        return Ok(Vec::new());
    };
    let (window, _) = read(src, place)?;
    Ok(window.into_iter().filter(|w| laid(src, pov, w)).collect())
}

/// One selector of slot G.
pub struct Here<'a> {
    band: Band,
    src: Sources<'a>,
}

impl<'a> Here<'a> {
    /// `band` is one of `Floor`, `WindowWords`, `WindowTools`, `Ladder`.
    pub fn new(band: Band, src: Sources<'a>) -> Self {
        debug_assert!(matches!(
            band,
            Band::Floor | Band::WindowWords | Band::WindowTools | Band::Ladder
        ));
        Self { band, src }
    }

    fn window(&self, pov: &PointOfView, window: &[Window]) -> Vec<Piece> {
        let piece = |w: &Window, band, reason, form| {
            make::exchange(&w.e, &w.f, band, reason, form, Cut::Window)
        };
        let admitted: Vec<&Window> = window.iter().filter(|w| laid(&self.src, pov, w)).collect();
        let floor_from = admitted.len().saturating_sub(FLOOR_EXCHANGES);
        admitted
            .into_iter()
            .enumerate()
            .filter_map(|(i, w)| {
                let (reason, form) = match self.band {
                    Band::Floor if i >= floor_from => (Reason::Floor, Form::Spoken),
                    Band::WindowWords if i < floor_from => (Reason::Window, Form::Spoken),
                    Band::WindowTools if w.f.tools > 0 => (Reason::Window, Form::Whole),
                    _ => return None,
                };
                Some(piece(w, self.band, reason, form))
            })
            .collect()
    }

    fn ladder(&self, nodes: Vec<FoldNode>) -> anyhow::Result<Vec<Piece>> {
        nodes
            .into_iter()
            .map(|n| {
                let sizes = self.src.folds.sizes(&n)?;
                Ok(make::of_ref(
                    super::sources::Ref {
                        range: SourceRange {
                            space: Space::Folds {
                                place: n.place,
                                level: n.level,
                            },
                            first: n.first,
                            last: n.last,
                        },
                        marks: n.marks,
                        sizes,
                        handle: n.handle,
                        form: Form::Whole,
                        unit: None,
                    },
                    Band::Ladder,
                    Reason::Ladder,
                ))
            })
            .collect()
    }
}

impl Selector for Here<'_> {
    fn band(&self) -> Band {
        self.band
    }

    fn select(&self, pov: &PointOfView, _: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let Some(place) = anchor_place(pov) else {
            return Ok(Vec::new());
        };
        let (window, nodes) = read(&self.src, place)?;
        if self.band == Band::Ladder {
            self.ladder(nodes)
        } else {
            Ok(self.window(pov, &window))
        }
    }
}
