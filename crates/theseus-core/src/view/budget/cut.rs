//! Rules 5, 7 and 8 of the budgeter: the cut. Every way an item can get smaller is one step, and the steps
//! run from the end of the admission order. A step belongs to the band that asked for the form it takes
//! away: an exchange the floor asks for in spoken form and the window's tool results ask for whole loses its
//! tool traffic at the tool results' place in the order, and is never cut below spoken. Within one band,
//! every piece steps down a form (oldest first) before any piece steps down again, and only then are pieces
//! dropped, oldest first.
//!
//! What is charged is what will be rendered: each shown piece's form plus [`HEADING_TOKENS`], and
//! [`RESIDUE_LINE_TOKENS`] for each band that leaves a residue line. Sums are kept wide (`u128`), so no
//! size, however impossible, wraps to a small one.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use super::super::select::Band;
use super::super::{Form, Pos};
use super::merge::{fixed_priority, Item};
use super::{ops, Budget, HEADING_TOKENS, RESIDUE_LINE_TOKENS};

/// What a step does to its item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum To {
    Spoken,
    Line,
    Drop,
}

/// One step, in the order steps run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Step {
    priority: Reverse<usize>,
    to: To,
    /// Oldest first within a band.
    at: Pos,
    handle: String,
    item: usize,
}

/// Each item's shown form after the cut, or why it was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fate {
    Shown(Form),
    OverBudget,
    UnitCut,
}

/// The cut's result: each item's fate, and everything charged.
pub(super) struct Cut {
    pub fate: Vec<Fate>,
    pub tokens: u128,
}

/// Run the cut: start every item at the fullest form asked, then apply steps from the end of the order while
/// a pool the item is charged to is over. Text that is never cut (the frame, the floor) may stand over its
/// own pool; everything else is held to its pool and to the whole (see [`super::fit`]).
pub(super) fn cut(
    items: &[Item],
    groups: &[usize],
    members: &BTreeMap<usize, Vec<usize>>,
    budget: &Budget,
) -> Cut {
    let mut fate: Vec<Fate> = items.iter().map(|it| Fate::Shown(it.piece.form)).collect();
    let fixed_group: BTreeSet<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.fixed_at.is_some())
        .map(|(i, _)| groups[i])
        .collect();
    let mut steps: Vec<Step> = items
        .iter()
        .enumerate()
        .flat_map(|(i, it)| steps_of(i, it, fixed_group.contains(&groups[i])))
        .collect();
    steps.sort();
    let mut pools = Pools::of(items, &fate);
    for s in steps {
        ops::tick(1);
        let Fate::Shown(form) = fate[s.item] else {
            continue;
        };
        if !pools.over(&items[s.item], budget) {
            continue;
        }
        match s.to {
            To::Spoken | To::Line => {
                let to = if s.to == To::Spoken {
                    Form::Spoken
                } else {
                    Form::Line
                };
                if to < form {
                    pools.step(&items[s.item], form, to);
                    fate[s.item] = Fate::Shown(to);
                }
            }
            To::Drop => {
                for &j in members.get(&groups[s.item]).into_iter().flatten() {
                    ops::tick(1);
                    if let Fate::Shown(f) = fate[j] {
                        pools.drop(&items[j], f);
                        fate[j] = if j == s.item {
                            Fate::OverBudget
                        } else {
                            Fate::UnitCut
                        };
                    }
                }
            }
        }
    }
    Cut {
        tokens: pools.total(),
        fate,
    }
}

/// The steps of one item. Asks sorted from the first row to the last; at each ask's row the item may step
/// down to the fullest form asked by an earlier row, and at the first row it may step down to a line and
/// then be dropped. Nothing at or before a never-cut row; no drop for an item whose unit has one.
fn steps_of(i: usize, it: &Item, fixed_group: bool) -> Vec<Step> {
    let asks: Vec<(usize, Form)> = it.asks.iter().map(|(p, f)| (*p, *f)).collect();
    let mut cum = Vec::with_capacity(asks.len());
    for (k, (_, f)) in asks.iter().enumerate() {
        cum.push(if k == 0 { *f } else { (*f).max(cum[k - 1]) });
    }
    let mut out = Vec::new();
    for k in (0..asks.len()).rev() {
        let priority = asks[k].0;
        if fixed_priority(priority) || it.fixed_at.is_some_and(|p| priority <= p) {
            break;
        }
        let floor = if k == 0 { None } else { Some(cum[k - 1]) };
        let mut push = |to: To| {
            out.push(Step {
                priority: Reverse(priority),
                to,
                at: it.piece.range.last.clone(),
                handle: it.piece.handle.0.clone(),
                item: i,
            });
        };
        if cum[k] == Form::Whole && floor.is_none_or(|f| f < Form::Whole) {
            push(To::Spoken);
        }
        if cum[k] >= Form::Spoken && floor.is_none_or(|f| f < Form::Spoken) {
            push(To::Line);
        }
        if floor.is_none() && !fixed_group {
            push(To::Drop);
        }
    }
    out
}

/// What an item costs shown at a form: the form's size and its heading.
fn shown_cost(it: &Item, form: Form) -> u128 {
    u128::from(it.piece.sizes.at(form)) + u128::from(HEADING_TOKENS)
}

/// The sums rule 5 holds: slot G against its allowance, everything else against the ceiling, the topic block
/// (a part of everything else) against its own cap, and the whole against the one budget. A band's residue
/// line is charged to the pool of the first piece it counts.
struct Pools {
    here: u128,
    rest: u128,
    topic: u128,
    residue: BTreeSet<Band>,
}

impl Pools {
    fn of(items: &[Item], fate: &[Fate]) -> Self {
        let mut p = Self {
            here: 0,
            rest: 0,
            topic: 0,
            residue: BTreeSet::new(),
        };
        for (it, f) in items.iter().zip(fate) {
            if let Fate::Shown(f) = f {
                p.charge(it, 0, shown_cost(it, *f));
            }
        }
        p
    }

    /// Take `from` off the pools `it` is charged to and put `to` on them.
    fn charge(&mut self, it: &Item, from: u128, to: u128) {
        let pool = if it.here() {
            &mut self.here
        } else {
            &mut self.rest
        };
        *pool = *pool - from + to;
        if it.topic() {
            self.topic = self.topic - from + to;
        }
    }

    fn total(&self) -> u128 {
        self.here + self.rest
    }

    /// Whether cutting this item would bring an over-full pool down.
    fn over(&self, it: &Item, b: &Budget) -> bool {
        let mine = if it.here() {
            self.here > u128::from(b.here_allowance)
        } else {
            self.rest > u128::from(b.ceiling)
        };
        let topic = it.topic() && self.topic > u128::from(b.topic);
        mine || topic || self.total() > u128::from(b.total())
    }

    fn step(&mut self, it: &Item, from: Form, to: Form) {
        self.charge(it, shown_cost(it, from), shown_cost(it, to));
    }

    fn drop(&mut self, it: &Item, from: Form) {
        let cost = shown_cost(it, from);
        let line = if self.residue.insert(it.piece.band) {
            u128::from(RESIDUE_LINE_TOKENS)
        } else {
            0
        };
        self.charge(it, cost, line);
    }
}
