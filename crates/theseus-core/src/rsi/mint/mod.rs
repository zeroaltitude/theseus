//! From a mined motif to a checked tool (theseus-pw1q.21.6; RSI loop 1 §1.3,
//! §2.5, §3.1): the step between the pattern miner (`rsi::mine`) and the
//! deploy step. Everything here is built on R5's motif types and R6's flow
//! spec, never a copy of either.
//!
//! - [`spec`]: a motif's skeleton as a flow spec, with a placeholder name.
//! - [`kind`]: the rule for the tool's kind (flow, MCP, Rust), and the owner
//!   card for a skeleton that would need the network or a secret.
//! - [`brief`]: the keyed split of the occurrences into a build half and a
//!   check half, and the writer's brief, made from the build half alone.
//! - [`validate`]: the open replays (fixtures, a dry replay with no model, a
//!   model A/B replay) and [`validate::ValidateReport`].
//!
//! **The data rule** (contract 10): a motif drawn from private ledger
//! sessions is written and checked only on a model whose origin is not
//! restricted, failing closed: the mint session refuses a China-origin (or
//! unknown-origin) profile, decided by the profile's origin and never its
//! name ([`data_rule`]). A motif born in the bench alone may use any model.
//!
//! - [`deploy`] and [`retire`] (theseus-pw1q.21.8): a pass goes live (a flow
//!   loaded, an extension self-loaded, a Rust mint through self-pr) and the
//!   tender keeps, retires or graduates it by its use; the morning notice
//!   (`rsi/digest/notice.rs`) tells the owner what landed.
//!
//! **Off by default.** `[self.mint] mine = false` starts nothing; the start
//! asks `rsi::gate` first, and so does every deploy and retire.

use crate::catalog::Catalog;
use crate::config::Config;
use crate::rsi::mine::Motif;

pub mod brief;
pub mod deploy;
pub mod kind;
pub mod retire;
pub mod rows;
pub mod run;
pub mod source;
pub mod spec;
pub mod validate;

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_owned;
#[cfg(test)]
mod tests_retire;

pub use validate::ValidateReport;

/// A private motif is one drawn from the ledger's sessions.
pub fn private(m: &Motif) -> bool {
    !m.bench_born
}

/// The data rule: `Err(why)` when `profile`'s model may not write or check a
/// motif, by the profile's **origin** (`Config::profile_origin`). A bench-born
/// motif may use any model; a private one only a model of US origin
/// (`ModelOrigin::cleared_for_sensitive`, theseus-xs5ah), and a profile that
/// does not resolve is unknown, so refused.
pub fn data_rule(
    cfg: &Config,
    catalog: &Catalog,
    profile: &str,
    private: bool,
) -> Result<(), String> {
    if !private {
        return Ok(());
    }
    let origin = cfg.profile_origin(profile, catalog);
    if !cfg.profile_cleared_for_sensitive(profile, catalog) {
        return Err(format!(
            "the motif is drawn from private sessions and profile {profile:?} is of {origin:?} \
             origin: a private motif is written only on a model the catalog lists as of US origin"
        ));
    }
    Ok(())
}
