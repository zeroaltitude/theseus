//! Rules 5, 7 and 8 of the budgeter: the cut. Every way an item can get smaller is one step, and the steps
//! run from the end of the admission order. A step belongs to the band that asked for the form it takes
//! away: an exchange the floor asks for in spoken form and the window's tool results ask for whole loses its
//! tool traffic at the tool results' place in the order, and is never cut below spoken. Within one band,
//! every piece steps down a form (oldest first) before any piece steps down again, and only then are pieces
//! dropped, oldest first.

use std::cmp::Reverse;

use super::super::{Form, Pos};
use super::merge::{fixed_priority, Item};
use super::Budget;

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

/// Each item's shown form after the cut (`None`: dropped) and why it was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fate {
    Shown(Form),
    OverBudget,
    UnitCut,
}

/// Run the cut: start every item at the fullest form asked, then apply steps from the end of the order while
/// a pool the item is charged to is over.
pub(super) fn cut(items: &[Item], groups: &[usize], budget: &Budget) -> Vec<Fate> {
    let mut fate: Vec<Fate> = items.iter().map(|it| Fate::Shown(it.piece.form)).collect();
    let fixed_group: Vec<bool> = (0..items.len())
        .map(|i| (0..items.len()).any(|j| groups[j] == groups[i] && items[j].fixed_at().is_some()))
        .collect();
    let mut steps: Vec<Step> = items
        .iter()
        .enumerate()
        .flat_map(|(i, it)| steps_of(i, it, fixed_group[i]))
        .collect();
    steps.sort();
    let mut pools = Pools::of(items, &fate);
    for s in steps {
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
                    pools.charge(&items[s.item], form, Some(to));
                    fate[s.item] = Fate::Shown(to);
                }
            }
            To::Drop => {
                for j in (0..items.len()).filter(|&j| groups[j] == groups[s.item]) {
                    if let Fate::Shown(f) = fate[j] {
                        pools.charge(&items[j], f, None);
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
    fate
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
        if fixed_priority(priority) || it.fixed_at().is_some_and(|p| priority <= p) {
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

/// The three sums rule 5 holds: slot G against its allowance, everything else against the ceiling, and
/// the whole against the one budget.
struct Pools {
    here: u64,
    rest: u64,
}

impl Pools {
    fn of(items: &[Item], fate: &[Fate]) -> Self {
        let mut p = Self { here: 0, rest: 0 };
        for (it, f) in items.iter().zip(fate) {
            if let Fate::Shown(f) = f {
                *p.pool(it) += it.piece.sizes.at(*f);
            }
        }
        p
    }

    fn pool(&mut self, it: &Item) -> &mut u64 {
        if it.here() {
            &mut self.here
        } else {
            &mut self.rest
        }
    }

    fn total(&self) -> u64 {
        self.here + self.rest
    }

    /// Whether cutting this item would bring an over-full pool down.
    fn over(&self, it: &Item, b: &Budget) -> bool {
        let mine = if it.here() {
            self.here > b.here_allowance
        } else {
            self.rest > b.ceiling
        };
        mine || self.total() > b.total()
    }

    fn charge(&mut self, it: &Item, from: Form, to: Option<Form>) {
        let now = to.map_or(0, |t| it.piece.sizes.at(t));
        let pool = self.pool(it);
        *pool = pool.saturating_sub(it.piece.sizes.at(from)) + now;
    }

    pub(super) fn tokens(items: &[Item], fate: &[Fate]) -> u64 {
        Self::of(items, fate).total()
    }
}

/// Everything charged after the cut.
pub(super) fn charged(items: &[Item], fate: &[Fate]) -> u64 {
    Pools::tokens(items, fate)
}
