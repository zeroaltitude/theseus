//! Which model reads which model's thinking (theseus-9bggi).
//!
//! A thinking block's signature is the writing model's, and a switch of model
//! used to strip every block of the prefix (theseus-kol). Anthropic documents
//! one direction as readable: Opus 5.5 reads Sonnet 5.5's blocks, `thinking`
//! and `redacted_thinking` alike, and not the reverse. Only a first-party
//! provider's pair is listed; every other pair, GLM's included, still strips.

/// (writer, reader): the provider documents the reader as accepting the
/// writer's blocks verbatim.
const READABLE: &[(&str, &str)] = &[("claude-sonnet-5-5", "claude-opus-5-5")];

/// The built-in provider key of Anthropic's own API (`catalog`).
const FIRST_PARTY: &str = "anthropic";

/// Whether `reader`, on `provider`, takes blocks `writer` wrote on that
/// provider. The caller has already required the two providers to be one.
pub(super) fn reads(provider: &str, writer: &str, reader: &str) -> bool {
    provider == FIRST_PARTY && READABLE.contains(&(writer, reader))
}
