//! Rules 3 and 4 of the budgeter: overlapping ranges merge into one item, and items that share a unit are
//! grouped. Everything here is order-independent: the input is put in one canonical order first, so the
//! order the selectors ran in cannot change an item, its representative or its group.

use std::collections::{BTreeMap, BTreeSet};

use super::super::select::Band;
use super::super::{Form, Piece, Slot};

/// One merged range: the piece it is shown as, and every band that asked for it with the fullest form each
/// asked for.
#[derive(Debug, Clone)]
pub(super) struct Item {
    /// The representative, carrying the merged range, every reason, the fullest form asked for, the
    /// largest size of each form and the grown marks.
    pub piece: Piece,
    /// Priority of each band that asked, with the fullest form asked at it.
    pub asks: BTreeMap<usize, Form>,
    /// The other pieces merged into it (dropped as `Merged`).
    pub merged: Vec<Piece>,
    pub units: BTreeSet<String>,
}

impl Item {
    /// Counted against slot G's allowance rather than the ceiling.
    pub fn here(&self) -> bool {
        self.piece.slot == Slot::Here
    }

    /// The highest priority (smallest index) at which a never-cut row asked, if any: the frame (it is the
    /// compiler's, counted first and never cut) or a floor row (rule 8).
    pub fn fixed_at(&self) -> Option<usize> {
        self.asks
            .keys()
            .copied()
            .filter(|p| fixed_priority(*p))
            .max()
    }
}

/// Whether the row at this priority is never cut.
pub(super) fn fixed_priority(priority: usize) -> bool {
    super::super::select::ADMISSION
        .get(priority)
        .is_some_and(|r| r.floor || r.band == Band::Frame)
}

/// A total order over pieces: band, newest first, handle, then the whole piece as a last resort, so two
/// pieces that differ anywhere never tie.
fn canon(p: &Piece) -> (usize, std::cmp::Reverse<super::super::Pos>, String, String) {
    (
        p.band.priority(),
        std::cmp::Reverse(p.range.last.clone()),
        p.handle.0.clone(),
        serde_json::to_string(p).unwrap_or_default(),
    )
}

/// Rule 3: merge identical or overlapping ranges. The item is shown in slot G if any member is the window's
/// or the ladder's, otherwise in the first row of the admission table that asked.
pub(super) fn merge(mut pieces: Vec<Piece>) -> Vec<Item> {
    pieces.sort_by_cached_key(|p| {
        (
            p.range.space.clone(),
            p.range.first.clone(),
            p.range.last.clone(),
            canon(p),
        )
    });
    let mut groups: Vec<Vec<Piece>> = Vec::new();
    for p in pieces {
        let joins = groups.last().is_some_and(|g| {
            g[0].range.space == p.range.space && g.iter().any(|q| q.range.last >= p.range.first)
        });
        match groups.last_mut() {
            Some(g) if joins => g.push(p),
            _ => groups.push(vec![p]),
        }
    }
    groups.into_iter().map(item).collect()
}

/// One group of overlapping pieces as one item.
fn item(mut group: Vec<Piece>) -> Item {
    group.sort_by_cached_key(|p| (p.slot != Slot::Here, canon(p)));
    let mut rest = group.split_off(1);
    let mut piece = group.remove(0);
    let mut asks = BTreeMap::from([(piece.band.priority(), piece.form)]);
    let mut units: BTreeSet<String> = piece.unit.iter().cloned().collect();
    for q in &rest {
        let ask = asks.entry(q.band.priority()).or_insert(q.form);
        *ask = (*ask).max(q.form);
        units.extend(q.unit.iter().cloned());
        piece.reasons.extend(q.reasons.iter().copied());
        piece.form = piece.form.max(q.form);
        piece.marks.grow(&q.marks);
        piece.sizes.whole = piece.sizes.whole.max(q.sizes.whole);
        piece.sizes.spoken = piece.sizes.spoken.max(q.sizes.spoken);
        piece.sizes.line = piece.sizes.line.max(q.sizes.line);
        piece.range.first = piece.range.first.clone().min(q.range.first.clone());
        piece.range.last = piece.range.last.clone().max(q.range.last.clone());
    }
    rest.sort_by_cached_key(canon);
    Item {
        piece,
        asks,
        merged: rest,
        units,
    }
}

/// Rule 4: items that share a unit (transitively) form one group; returns each item's group number.
pub(super) fn unit_groups(items: &[Item]) -> Vec<usize> {
    let mut group: Vec<usize> = (0..items.len()).collect();
    let mut first_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        for u in &it.units {
            match first_of.get(u.as_str()) {
                Some(&j) => {
                    let (from, to) = (group[i], group[j]);
                    relabel(&mut group, from, to);
                }
                None => {
                    first_of.insert(u, i);
                }
            }
        }
    }
    group
}

fn relabel(group: &mut [usize], from: usize, to: usize) {
    if from == to {
        return;
    }
    let (from, to) = (from.max(to), from.min(to));
    for g in group.iter_mut().filter(|g| **g == from) {
        *g = to;
    }
}
