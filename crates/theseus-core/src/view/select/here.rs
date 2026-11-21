//! Slot G, here: the window and the ladder of the place, through the ladder's read (design §5.2). Four
//! bands: the floor (the words of the newest three exchanges), the rest of the window's words, the window's
//! tool results, and the ladder's nodes. A window exchange is one range: its words are asked for spoken and
//! its tool results whole, so rule 3 merges them and the cut takes the tool traffic away at the tool
//! results' place in the order, never the words of the floor.

use super::super::fold::{FoldNode, WINDOW_N};
use super::super::timeline::ExchangeRef;
use super::super::{Anchor, Form, Piece, PointOfView, Present, Reason, SourceRange, Space};
use super::make::{self, Cut};
use super::sources::Sources;
use super::{Band, Selector, Snapshot};

/// The floor: the words of the newest this-many exchanges of the window are never cut.
pub const FLOOR_EXCHANGES: usize = 3;

/// The place a point of view's here-slots read, if it is anchored to one and may see anything.
pub(super) fn place_of(pov: &PointOfView) -> Option<&str> {
    match (&pov.anchor, pov.present) {
        (_, Present::Unknown) | (Anchor::Task(_), _) => None,
        (Anchor::Place(p), _) => Some(p),
    }
}

/// The window's exchanges, oldest first, with their facts; and the ladder's nodes.
fn read(src: &Sources<'_>, place: &str) -> anyhow::Result<(Vec<Window>, Vec<FoldNode>)> {
    let since = src.places.new_mark(place)?;
    let read = src.ladder.read(place, WINDOW_N, since.as_ref())?;
    let mut window = Vec::with_capacity(read.window.len());
    for e in read.window {
        if let Some(f) = src.exchanges.facts(&e.turn_id)? {
            window.push(Window { e, f });
        }
    }
    Ok((window, read.nodes))
}

struct Window {
    e: ExchangeRef,
    f: super::sources::ExchangeFacts,
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

    fn window(&self, window: &[Window]) -> Vec<Piece> {
        let floor_from = window.len().saturating_sub(FLOOR_EXCHANGES);
        window
            .iter()
            .enumerate()
            .filter_map(|(i, w)| {
                let (reason, form) = match self.band {
                    Band::Floor if i >= floor_from => (Reason::Floor, Form::Spoken),
                    Band::WindowWords if i < floor_from => (Reason::Window, Form::Spoken),
                    Band::WindowTools if w.f.tools > 0 => (Reason::Window, Form::Whole),
                    _ => return None,
                };
                Some(make::exchange(
                    &w.e,
                    &w.f,
                    self.band,
                    reason,
                    form,
                    Cut::Window,
                ))
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
        let Some(place) = place_of(pov) else {
            return Ok(Vec::new());
        };
        let (window, nodes) = read(&self.src, place)?;
        if self.band == Band::Ladder {
            self.ladder(nodes)
        } else {
            Ok(self.window(&window))
        }
    }
}
