//! The text of a view (design §5.1 "Forms", "Whose words they are", the outside-text wrapper and the memory
//! page; §5.2's fold node; §5.3's closing counts), as pure functions: what a piece reads as, never which
//! pieces there are. The renderer ([`super::render`]) calls these; nothing here reads the store or the clock.
//!
//! - [`forms`]: the three forms of an exchange, from its stored nodes, and the marked cuts.
//! - [`labels`]: the five labels' headings, and the wrapper around outside text.
//! - [`blocks`]: a fold node's block and a band's closing count.
//! - [`page`]: the memory page, the fixed text of the system part that tells the model how its context is
//!   built.

pub mod blocks;
pub mod forms;
pub mod labels;
pub mod page;

#[cfg(test)]
mod tests;

use crate::catalog::TokenRates;

/// `text`'s tokens by today's estimate for prose (`provider::Census` at Claude's rate, the densest the
/// catalog knows): what [`super::Sizes`] are counted in.
pub fn tokens(text: &str) -> u64 {
    (text.len() as f64 / TokenRates::CLAUDE.text).ceil() as u64
}
