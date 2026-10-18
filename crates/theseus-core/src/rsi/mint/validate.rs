//! Validating a minted tool before it may deploy (§2.5, §3.1): the open
//! replays, three tiers and the writer's isolation, and the one result a
//! deploy step reads, [`ValidateReport`].
//!
//! | Tier | Module | Pass |
//! |---|---|---|
//! | fixtures | [`fixtures`] | every arm's fixture ends in the arm's exit |
//! | dry replay | [`dry`] | 3 instances, 90% reproduce, 0 deviations |
//! | model A/B | [`ab`] | no loss of success, 15% saved, 2 decisions removed |
//! | isolation | [`isolated`] | the brief held no check-half id |
//!
//! The model tier runs only after the two tiers before it pass: a tool that
//! fails its fixtures is not worth a model's money. **Deploying is the next
//! step's** (R8): a pass here loads, joins and self-loads nothing.

use serde::{Deserialize, Serialize};

use super::brief::occurrence_id;
use super::kind::Kind;
use crate::rsi::mine::Occurrence;

pub mod ab;
pub mod dry;
pub mod fixtures;
pub mod walk;

#[cfg(test)]
mod tests_ab;
#[cfg(test)]
mod tests_dry;
#[cfg(test)]
mod tests_walk;

/// One tier of the checks, as a row shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tier {
    pub name: String,
    /// The tier ran (a model tier does not run after a failed tier).
    pub ran: bool,
    pub pass: bool,
    pub why: String,
}

/// The result of validating one minted tool: what a deploy step reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidateReport {
    /// The tool's name.
    pub name: String,
    pub kind: Kind,
    /// The frozen spec's digest (`sha256:…`).
    pub digest: String,
    pub fixtures: Option<fixtures::FixtureReport>,
    pub dry: Option<dry::DryReport>,
    pub ab: Option<ab::AbReport>,
    /// The brief held no check-half id.
    pub isolated: bool,
    /// Every tier ran and passed.
    pub pass: bool,
    /// Why the motif goes back to the backlog, and with what evidence: set
    /// when the model never called the tool.
    pub back_to_backlog: Option<String>,
}

impl ValidateReport {
    /// The tiers, in the order they run.
    pub fn tiers(&self) -> Vec<Tier> {
        let tier = |name: &str, ran: bool, pass: bool, why: String| Tier {
            name: name.into(),
            ran,
            pass,
            why,
        };
        let fx = self.fixtures.as_ref();
        let (dry, ab) = (self.dry.as_ref(), self.ab.as_ref());
        vec![
            tier(
                "fixtures",
                fx.is_some(),
                fx.is_some_and(|f| f.pass),
                fx.map_or("not run".into(), |f| {
                    if f.pass {
                        format!("{} of {} arms", f.passed, f.arms)
                    } else {
                        f.failed.join("; ")
                    }
                }),
            ),
            tier(
                "dry",
                dry.is_some(),
                dry.is_some_and(|d| d.pass),
                dry.map_or("not run".into(), |d| d.why.clone()),
            ),
            tier(
                "ab",
                ab.is_some(),
                ab.is_some_and(|a| a.pass),
                ab.map_or("not run".into(), |a| a.why.clone()),
            ),
            tier(
                "isolation",
                true,
                self.isolated,
                if self.isolated {
                    "the brief held no check-half id".into()
                } else {
                    "the brief held a check-half id".into()
                },
            ),
        ]
    }

    /// The report from the tiers' results.
    pub fn assemble(
        (name, kind, digest): (&str, Kind, &str),
        (fixtures, dry, ab): (
            Option<fixtures::FixtureReport>,
            Option<dry::DryReport>,
            Option<ab::AbReport>,
        ),
        isolated: bool,
    ) -> Self {
        let pass = fixtures.as_ref().is_some_and(|f| f.pass)
            && dry.as_ref().is_some_and(|d| d.pass)
            && ab.as_ref().is_some_and(|a| a.pass)
            && isolated;
        let back_to_backlog = ab.as_ref().and_then(|a| a.back_to_backlog.clone());
        Self {
            name: name.into(),
            kind,
            digest: digest.into(),
            fixtures,
            dry,
            ab,
            isolated,
            pass,
            back_to_backlog,
        }
    }
}

/// Whether `brief` holds no id of the check half: the writer's isolation,
/// asserted on the brief's bytes. An occurrence's turn is as much its id as
/// the whole string, so each is looked for.
pub fn isolated(brief: &str, check: &[&Occurrence]) -> bool {
    check.iter().all(|o| {
        !brief.contains(&occurrence_id(o)) && (o.turn.is_empty() || !brief.contains(&o.turn))
    })
}
