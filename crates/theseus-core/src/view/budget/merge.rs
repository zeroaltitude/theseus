//! Rules 3 and 4 of the budgeter: overlapping ranges merge into one item, and items that share a unit are
//! grouped. Everything here is order-independent: the input is put in one canonical order first, so the
//! order the selectors ran in cannot change an item, its representative or its group. Every pass is linear
//! after one sort: the overlap grouping keeps a running maximum end, and units are joined by union-find.

use std::collections::{BTreeMap, BTreeSet};

use super::super::select::Band;
use super::super::{Form, Piece, Pos, Sizes, Slot, Space};
use super::{ops, unit_of};

/// One merged range: the piece it is shown as, and every band that asked for it with the fullest form each
/// asked for.
#[derive(Debug, Clone)]
pub(super) struct Item {
    /// The representative, carrying the merged range, every reason, the fullest form asked for, the
    /// sizes of the merged coverage and the grown marks.
    pub piece: Piece,
    /// Priority of each band that asked, with the fullest form asked at it.
    pub asks: BTreeMap<usize, Form>,
    /// The other pieces merged into it (dropped as `Merged`).
    pub merged: Vec<Piece>,
    pub units: BTreeSet<String>,
    /// The highest priority at which a never-cut row asked (computed once).
    pub fixed_at: Option<usize>,
}

impl Item {
    /// Counted against slot G's allowance rather than the ceiling.
    pub fn here(&self) -> bool {
        self.piece.slot == Slot::Here
    }

    /// Counted against the topic block's own cap as well as the ceiling.
    pub fn topic(&self) -> bool {
        self.piece.slot == Slot::Called
    }
}

/// Whether the row at this priority is never cut: the frame (the compiler's, counted first) or a floor row
/// (rule 8).
pub(super) fn fixed_priority(priority: usize) -> bool {
    super::super::select::ADMISSION
        .get(priority)
        .is_some_and(|r| r.floor || r.band == Band::Frame)
}

/// A total order over pieces: band, newest first, handle, then the whole piece as a last resort, so two
/// pieces that differ anywhere never tie.
fn canon(p: &Piece) -> (usize, std::cmp::Reverse<Pos>, String, String) {
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
    let mut end: Option<(Space, Pos)> = None;
    for p in pieces {
        ops::tick(1);
        let joins = end
            .as_ref()
            .is_some_and(|(s, e)| *s == p.range.space && *e >= p.range.first);
        match groups.last_mut() {
            Some(g) if joins => {
                if let Some((_, e)) = end.as_mut() {
                    if p.range.last > *e {
                        *e = p.range.last.clone();
                    }
                }
                g.push(p);
            }
            _ => {
                end = Some((p.range.space.clone(), p.range.last.clone()));
                groups.push(vec![p]);
            }
        }
    }
    groups.into_iter().map(item).collect()
}

/// The sizes of a group's coverage: pieces of the same range count once at their largest size; pieces of
/// different ranges are summed, so a partial overlap is never charged less than its union (rule 5 errs
/// high, never low). Sums saturate: an impossible size cannot wrap to a small one.
fn coverage(group: &[Piece]) -> Sizes {
    let mut by_range: BTreeMap<(&Pos, &Pos), Sizes> = BTreeMap::new();
    for p in group {
        ops::tick(1);
        let s = by_range.entry((&p.range.first, &p.range.last)).or_default();
        s.whole = s.whole.max(p.sizes.whole);
        s.spoken = s.spoken.max(p.sizes.spoken);
        s.line = s.line.max(p.sizes.line);
    }
    by_range.values().fold(Sizes::default(), |a, s| Sizes {
        whole: a.whole.saturating_add(s.whole),
        spoken: a.spoken.saturating_add(s.spoken),
        line: a.line.saturating_add(s.line),
    })
}

/// One group of overlapping pieces as one item.
fn item(mut group: Vec<Piece>) -> Item {
    let sizes = coverage(&group);
    group.sort_by_cached_key(|p| (p.slot != Slot::Here, canon(p)));
    let mut rest = group.split_off(1);
    let mut piece = group.remove(0);
    let mut asks = BTreeMap::from([(piece.band.priority(), piece.form)]);
    let mut units: BTreeSet<String> = unit_of(&piece).map(str::to_owned).into_iter().collect();
    for q in &rest {
        ops::tick(1);
        let ask = asks.entry(q.band.priority()).or_insert(q.form);
        *ask = (*ask).max(q.form);
        units.extend(unit_of(q).map(str::to_owned));
        piece.reasons.extend(q.reasons.iter().copied());
        piece.form = piece.form.max(q.form);
        piece.marks.grow(&q.marks);
        piece.range.first = piece.range.first.clone().min(q.range.first.clone());
        piece.range.last = piece.range.last.clone().max(q.range.last.clone());
    }
    piece.sizes = sizes;
    rest.sort_by_cached_key(canon);
    let fixed_at = asks.keys().copied().filter(|p| fixed_priority(*p)).max();
    Item {
        piece,
        asks,
        merged: rest,
        units,
        fixed_at,
    }
}

/// Rule 4: items that share a unit (transitively) form one group. Returns each item's group (the smallest
/// index in it) and each group's members.
pub(super) fn unit_groups(items: &[Item]) -> (Vec<usize>, BTreeMap<usize, Vec<usize>>) {
    let mut parent: Vec<usize> = (0..items.len()).collect();
    let mut first_of: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, it) in items.iter().enumerate() {
        for u in &it.units {
            ops::tick(1);
            match first_of.get(u.as_str()) {
                Some(&j) => union(&mut parent, i, j),
                None => {
                    first_of.insert(u, i);
                }
            }
        }
    }
    let group: Vec<usize> = (0..items.len()).map(|i| find(&mut parent, i)).collect();
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, g) in group.iter().enumerate() {
        ops::tick(1);
        members.entry(*g).or_default().push(i);
    }
    (group, members)
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        ops::tick(1);
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Join two sets, the smaller root becoming the root, so a group's name is its smallest member.
fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    let (lo, hi) = (ra.min(rb), ra.max(rb));
    parent[hi] = lo;
}
