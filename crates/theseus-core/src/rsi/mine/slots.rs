//! Slot classification across occurrences (§2.2). A step's slots are
//! classified from what they held in every occurrence of the motif:
//!
//! | Kind | What it is |
//! |---|---|
//! | `Constant` | identical in at least 80% of the occurrences, and safe to keep |
//! | `Bound` | equals an earlier slot of the occurrence, or a value an earlier result named |
//! | `Input` | varies, is bound to nothing, and is short and typed: a tool input |
//! | `Authored` | long text the model wrote; its calls cut the stretches |
//!
//! The classifier sees digests, never values. A digest of 0 is a value the
//! miner would not keep (the scrubber or the shape check refused it); such a
//! slot can be neither a constant nor bound, so it is an input.

use serde::{Deserialize, Serialize};

use super::shape::SlotType;

/// A constant is the same in this share of the occurrences that have the slot.
pub const CONSTANT_SHARE: f64 = 0.8;

/// What a slot is across a motif's occurrences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Constant,
    Bound,
    Input,
    Authored,
}

/// Where a bound slot's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum Bound {
    /// The same value as an earlier step's slot.
    Slot { step: usize, slot: usize },
    /// A value the result of an earlier step named.
    Result { step: usize },
}

/// One slot of one step, classified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotBinding {
    #[serde(rename = "type")]
    pub ty: SlotType,
    pub kind: SlotKind,
    /// The constant's text: only for a `Constant`, and only text the miner
    /// kept after the scrubber and the shape check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound: Option<Bound>,
}

/// One call as the classifier sees it: its slots' digests and the digests of
/// the values its result named.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotCall<'a> {
    pub digests: &'a [u64],
    pub outs: &'a [u64],
}

/// Classify the slots of `types.len()` aligned steps. `columns[step][occ]` is
/// the occurrence's call at the step, or none when it skipped it; `text`
/// gives a kept constant's text from its digest.
pub fn classify(
    types: &[Vec<SlotType>],
    columns: &[Vec<Option<SlotCall<'_>>>],
    text: &dyn Fn(u64) -> Option<String>,
) -> Vec<Vec<SlotBinding>> {
    let occs = columns.first().map_or(0, Vec::len);
    types
        .iter()
        .enumerate()
        .map(|(s, tys)| {
            tys.iter()
                .enumerate()
                .map(|(j, ty)| one(s, j, *ty, columns, occs, text))
                .collect()
        })
        .collect()
}

fn one(
    step: usize,
    slot: usize,
    ty: SlotType,
    columns: &[Vec<Option<SlotCall<'_>>>],
    occs: usize,
    text: &dyn Fn(u64) -> Option<String>,
) -> SlotBinding {
    let input = |ty| SlotBinding {
        ty,
        kind: SlotKind::Input,
        constant: None,
        bound: None,
    };
    // Each occurrence that has the slot, with its digest.
    let present: Vec<(usize, u64)> = (0..occs)
        .filter_map(|o| {
            let c = columns[step][o].as_ref()?;
            c.digests.get(slot).map(|d| (o, *d))
        })
        .collect();
    if present.is_empty() {
        return input(ty);
    }
    let need = (present.len() as f64 * CONSTANT_SHARE).ceil() as usize;
    // A constant first: two steps that name `origin` are not bound.
    let mut counts: std::collections::BTreeMap<u64, usize> = Default::default();
    for (_, d) in &present {
        if *d != 0 {
            *counts.entry(*d).or_default() += 1;
        }
    }
    if let Some((d, n)) = counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
    {
        if *n >= need {
            if let Some(t) = text(*d) {
                return SlotBinding {
                    ty,
                    kind: SlotKind::Constant,
                    constant: Some(t),
                    bound: None,
                };
            }
        }
    }
    // Bound: the same value as an earlier slot, or one an earlier result named.
    let mut tally: std::collections::BTreeMap<(bool, usize, usize), usize> = Default::default();
    for (o, d) in &present {
        if *d == 0 {
            continue;
        }
        let mut seen = std::collections::BTreeSet::new();
        for (e, col) in columns.iter().enumerate().take(step) {
            let Some(c) = col[*o].as_ref() else {
                continue;
            };
            if let Some(k) = c.digests.iter().position(|x| x == d) {
                if seen.insert((false, e, k)) {
                    *tally.entry((false, e, k)).or_default() += 1;
                }
            }
            if c.outs.contains(d) && seen.insert((true, e, 0)) {
                *tally.entry((true, e, 0)).or_default() += 1;
            }
        }
    }
    // The nearest source wins a tie, and a slot beats a result.
    let best = tally.iter().filter(|(_, n)| **n >= need).max_by(|a, b| {
        a.1.cmp(b.1)
            .then_with(|| (a.0 .1, !a.0 .0).cmp(&(b.0 .1, !b.0 .0)))
    });
    if let Some(((from_result, e, k), _)) = best {
        return SlotBinding {
            ty,
            kind: SlotKind::Bound,
            constant: None,
            bound: Some(if *from_result {
                Bound::Result { step: *e }
            } else {
                Bound::Slot { step: *e, slot: *k }
            }),
        };
    }
    input(ty)
}

#[cfg(test)]
mod tests;
