//! Claude Mythos 5.2's row, every figure of it from what its maker publishes.
//!
//! Its own file, beside `fable_rows` and not in it: Mythos 5.2 is a model of its
//! own that the maker offers only to organizations it has verified, and the
//! model list says where each figure came from. The maker's pages, read on
//! 2026-11-18 (platform.claude.com/docs): the Claude Mythos 5.2 model page, the
//! models overview, the pricing page, the prompt-caching page, the effort
//! page, the refusals page, the tool-search page, and the PDF page.
//!
//! What is published, and where:
//! - the name the provider takes, `claude-mythos-5-2` (the model page's "Model
//!   IDs"); status "Active (verification required)", released 2026-11-09;
//! - a 1M window and a 128K output (model page); $8 in, $40 out, a 5-minute
//!   write at $10, an hour's at $16, a cache read at $0.20 a million (model
//!   page and pricing page);
//! - adaptive thinking that is always on, default effort high (model page), and
//!   a per-message effort (effort page names Mythos 5.2);
//! - text and images in, text out (model page); the 512-token caching minimum
//!   (caching page names Mythos 5.2); tool search (its page names the id); PDFs
//!   ("All models", PDF page);
//! - the maker is Anthropic, so the row's origin is `us`.
//!
//! What the maker does not publish for it, and the row leaves out:
//! - the provider's own refusal fallback: the refusals page's availability line
//!   for it names the Fable, Opus, Sonnet and Haiku models and not Mythos, so
//!   `refusal_fallbacks` is false and Theseus asks for none;
//! - its tokenizer's density: the pricing page says only that Claude 4.7 and
//!   later models and Mythos Preview read about 30% more tokens, and gives no
//!   figure for 5.2. The row takes Theseus's estimate for any Claude id that
//!   has none of its own ([`TokenRates::of`]), an over-estimate that prices
//!   nothing: a call is billed on the provider's own count.
//!
//! The maker says Mythos 5.2 has the same specifications and pricing as Fable
//! 5.2, but no figure here is copied from that row: each is the Mythos page's.
//! The router does not choose this model (no `routing::model` option names it);
//! a profile pins it (`[profiles.mythos]` in the example configuration).

use std::collections::BTreeMap;

use super::{claude, CatalogEntry, ThinkingMode};
use crate::config::Effort;

/// The name the provider takes for Claude Mythos 5.2.
pub const ID: &str = "claude-mythos-5-2";

/// Mythos 5.2's row (see the module's comment for what each figure rests on).
pub(super) fn rows(e: &mut BTreeMap<String, CatalogEntry>) {
    e.insert(
        ID.into(),
        CatalogEntry {
            source: "Anthropic Claude Mythos 5.2 model page, models overview, pricing page, \
                     prompt-caching page, effort page, refusals page, tool-search page and \
                     PDF page, read 2026-11-18 (released 2026-11-09; offered only to \
                     organizations Anthropic has verified). Not published for it, so left \
                     out: the provider's own refusal fallback, and its tokenizer's density \
                     (the row carries Theseus's estimate for a Claude id with none)"
                .into(),
            per_message_effort: true,
            default_effort: Some(Effort::High),
            ..claude(
                1_000_000,
                128_000,
                8.0,
                40.0,
                0.20,
                10.00,
                ThinkingMode::Always,
                false,
                512,
            )
        },
    );
}

#[cfg(test)]
mod tests;
