//! `precedence.v1`'s state (theseus-3nk): pairs of recorded testimony that
//! may disagree, each pair's two sources with who wrote them and when, and
//! the two facts the pack's rubric turns on.
//!
//! The pack asks which of two sources to trust now. Its rubric is the
//! owner's own: his own statement beats a document; a newer source beats an
//! older one. Neither of those can be asked of Jev — the loader forbids a
//! question that works out a date, and "who wrote this" is a field of the
//! record, not a reading of the text — so this builder states both as
//! words: [`Said`] per source, and `newer` and `rules_agree` per pair.
//!
//! The pair's order is the answer's carrier. `first_prevails` is a Noul, so
//! a yes means the pair's first source and a no its second; the caller
//! fixes that order when it builds the input and reads the answer back
//! against it. [`Pair::why`] words the reason from these same fields, so a
//! reader is given the reason the decision was actually made.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

/// The most pairs one request asks about (a per-item Noul asks at most ten).
pub const PRECEDENCE_PAIRS: usize = 10;
/// A source's text at most, in characters.
const SOURCE_CHARS: usize = 900;
/// A source's author and day at most, in characters.
const FIELD_CHARS: usize = 60;

/// Who wrote a source, as the pack's rubric reads it. Serialized as the
/// word the instructions name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Said {
    /// The owner, in his own words: the rubric's highest standing.
    Owner,
    /// Another person, in their own words.
    Person,
    /// The model: a reply, a summary, a synthesis, a report.
    Model,
    /// A document, a file, a note: what was true when it was written.
    Document,
    /// A tool's result, a page, a program's output: outside the harness.
    Outside,
    /// The record does not say, which the rubric reads as no standing of
    /// its own.
    Unknown,
}

impl Said {
    /// The word the state carries and the instructions name.
    pub fn word(self) -> &'static str {
        match self {
            Said::Owner => "the owner himself",
            Said::Person => "another person",
            Said::Model => "the model",
            Said::Document => "a document",
            Said::Outside => "text from outside",
            Said::Unknown => "not recorded",
        }
    }

    /// Whether the rubric's first rule gives this author the higher
    /// standing over `other`: the owner's own words over anything else, and
    /// a person's or a document's over the model's and outside text.
    fn rank(self) -> u8 {
        match self {
            Said::Owner => 4,
            Said::Person => 3,
            Said::Document => 2,
            Said::Model => 1,
            Said::Outside | Said::Unknown => 0,
        }
    }
}

/// One piece of recorded testimony, as the pack reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrecedenceSource {
    /// Who wrote it.
    pub said_by: Said,
    /// Its day, as its testimony header says it (`2026-09-30`). Stated, not
    /// asked: the pack never works out a date.
    pub day: String,
    /// Its own time, for `newer` alone; never shown to Jev, which reads
    /// `day` and the `newer` word.
    pub at_ms: u64,
    pub text: String,
}

/// Two sources that may disagree. The caller fixes the order; a yes to
/// `first_prevails` means [`Pair::first`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    /// What the answer comes back about: the caller's own key for the pair.
    pub key: String,
    /// What both are about, in the caller's words; empty when it has none.
    #[serde(default)]
    pub subject: String,
    pub first: PrecedenceSource,
    pub second: PrecedenceSource,
}

/// Which of a pair the later source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newer {
    First,
    Second,
    /// Both bear the same day: freshness decides nothing.
    Same,
}

impl Newer {
    /// The word the state carries.
    pub fn word(self) -> &'static str {
        match self {
            Newer::First => "the first",
            Newer::Second => "the second",
            Newer::Same => "neither: they bear the same day",
        }
    }
}

impl Pair {
    /// Which of the two was written later, by their days (their `at_ms`
    /// settles a tie within a day only when the days differ).
    pub fn newer(&self) -> Newer {
        if self.first.day != self.second.day {
            return match self.first.at_ms > self.second.at_ms {
                true => Newer::First,
                false => Newer::Second,
            };
        }
        Newer::Same
    }

    /// Which of the two the rubric's author rule favours, when it favours
    /// one: `None` when the two authors have the same standing.
    pub fn by_author(&self) -> Option<Newer> {
        let (a, b) = (self.first.said_by.rank(), self.second.said_by.rank());
        match a.cmp(&b) {
            std::cmp::Ordering::Greater => Some(Newer::First),
            std::cmp::Ordering::Less => Some(Newer::Second),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// Whether the rubric's two starting rules point the same way. `true`
    /// when the author rule decides nothing (freshness then stands alone,
    /// unopposed) or when both favour the same source.
    pub fn rules_agree(&self) -> bool {
        match (self.by_author(), self.newer()) {
            (None, _) | (_, Newer::Same) => true,
            (Some(a), n) => a == n,
        }
    }

    /// The word the state carries for `rules_agree`.
    fn agree_word(&self) -> &'static str {
        match (self.rules_agree(), self.by_author()) {
            (true, None) => "the two rules do not disagree: only the later one is in play",
            (true, _) => "the two rules point the same way",
            (false, _) => {
                "the two rules pull against each other: who said it, against which is newer"
            }
        }
    }

    /// Why `which` prevails, in one clause, from the same fields the state
    /// stated: the reason the decision was made, never a sentence a model
    /// composed. [`Newer::Same`] is not an answer and reads as the first.
    ///
    /// The clause says which of the rubric's two rules carried it, and says
    /// so plainly when neither did: a caller passes Jev's answer here, and
    /// an answer that departed from the rubric must read as a departure and
    /// not as a reason it did not have.
    pub fn why(&self, which: Newer) -> String {
        let (won, lost) = match which {
            Newer::Second => (&self.second, &self.first),
            _ => (&self.first, &self.second),
        };
        let (w, l) = (won.said_by.word(), lost.said_by.word());
        let stands = self.by_author();
        let how = match (stands == Some(which), stands.is_none(), self.newer()) {
            (true, _, n) if n == which => {
                format!("{w} said it, over {l}, and it is the later of the two")
            }
            (true, _, Newer::Same) => {
                format!("{w} said it, over {l}, and the two bear the same day")
            }
            (true, ..) => format!("{w} said it, over {l}, though it is the earlier of the two"),
            (false, true, n) if n == which => {
                format!("it is the later of the two, both being {w}")
            }
            (false, true, Newer::Same) => {
                format!("neither rule favours it: both are {w} and both bear the same day")
            }
            (false, true, ..) => {
                format!("neither rule favours it: both are {w} and it is the earlier of the two")
            }
            (false, false, n) if n == which => {
                format!("it is the later of the two, though {l} said the other")
            }
            (false, false, ..) => {
                format!(
                    "neither who said it nor its day favours it: {w} said it, {l} said the other"
                )
            }
        };
        format!("{} ({w}) took precedence: {how}", won.day)
    }
}

/// `precedence.v1`'s input: the pairs, most worth asking about first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrecedenceInput {
    #[serde(default)]
    pub pairs: Vec<Pair>,
}

pub fn precedence(i: &PrecedenceInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let pairs = &i.pairs[..i.pairs.len().min(PRECEDENCE_PAIRS)];
    let mut b = StateBuilder::new("precedence", PRECEDENCE_VERSION, cap, scrub);
    let one = |s: &PrecedenceSource| -> Value {
        json!({
            "said_by": s.said_by.word(),
            "when": c.clip(s.day.trim(), FIELD_CHARS),
            "says": c.clip(s.text.trim(), SOURCE_CHARS),
        })
    };
    let items: Vec<Value> = pairs
        .iter()
        .enumerate()
        .map(|(n, p)| {
            json!({
                "pair": n + 1,
                "about": c.clip(p.subject.trim(), FIELD_CHARS),
                "first": one(&p.first),
                "second": one(&p.second),
                "newer": p.newer().word(),
                "rules_agree": p.agree_word(),
            })
        })
        .collect();
    b.list_head(
        "pairs",
        8,
        share(cap, 95),
        items,
        left_out(&i.pairs, pairs.len()),
    )
    .cut_if("pairs", c.cut());
    let state = b.build();
    let kept: Vec<usize> = state.value()["pairs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["pair"].as_u64())
                .map(|n| n as usize)
                .collect()
        })
        .unwrap_or_default();
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::SourcePairs,
        kept.iter()
            .filter(|&&n| (1..=PRECEDENCE_PAIRS).contains(&n))
            .map(|&n| Item {
                key: pairs[n - 1].key.clone(),
                text: n.to_string(),
            })
            .collect(),
    );
    Prepared {
        state: Arc::new(state),
        dynamic,
    }
}

#[cfg(test)]
mod tests;
