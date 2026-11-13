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

/// Whether `reader` takes blocks `writer` wrote on the same provider, which
/// only a first-party one documents. `first_party` is the request's
/// (`RequestSpec::first_party`, from `provider.rs`'s `is_first_party` over
/// the provider's `api_base`): a provider's key is the operator's word and
/// says nothing about whose API answers it. The caller has already required
/// the two providers to be one.
pub(super) fn reads(first_party: bool, writer: &str, reader: &str) -> bool {
    first_party && READABLE.contains(&(writer, reader))
}
