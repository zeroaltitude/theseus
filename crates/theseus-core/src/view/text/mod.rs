//! The text of a view (design §5.1 "Forms", "Whose words they are", the outside-text wrapper and the memory
//! page; §5.2's fold node; §5.3's closing counts), as pure functions: what a piece reads as, never which
//! pieces there are. The renderer ([`super::render`]) calls these; nothing here reads the store or the clock.
//!
//! - [`forms`]: the three forms of an exchange, from its stored nodes, and the marked cuts.
//! - [`labels`]: the five labels' headings, the label of one provenance, and the wrapper around outside text.
//! - [`parts`]: an exchange's parts, each labelled by its own provenance.
//! - [`literal`]: data in framing (a place, a source, a path, a name) as a visibly quoted literal.
//! - [`blocks`]: a fold node's block and a band's closing count.
//! - [`kept`]: the bytes of its text a one-line cut kept.
//! - [`page`]: the memory page, the fixed text of the system part that tells the model how its context is
//!   built.

pub mod blocks;
pub mod forms;
pub mod kept;
pub mod labels;
pub mod literal;
pub mod page;
pub mod parts;

#[cfg(test)]
mod tests;

use crate::catalog::TokenRates;

/// `text`'s tokens by today's estimate for prose (`provider::Census` at Claude's rate, the densest the
/// catalog knows): what [`super::Sizes`] are counted in.
pub fn tokens(text: &str) -> u64 {
    (text.len() as f64 / TokenRates::CLAUDE.text).ceil() as u64
}
