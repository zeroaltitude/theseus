//! What a call a stop cut is booked at (theseus-3p36). A dropped stream loses
//! the provider's own usage, so the settle estimates it: the input the call's
//! reservation assumed, and the output from the characters streamed. Most of
//! a long turn's input is a cache read, at about a tenth of the input price,
//! so the input is split as the request was sent, never all of it booked as
//! plain input:
//!
//! - **The cached part** is what the turn's previous call read from the cache
//!   or wrote to it: the request repeats that call's prompt, through its
//!   conversation breakpoint, and adds the answer and the results after it.
//!   It is priced at the cache-read price. A request on a new compilation (a
//!   recompile) repeats nothing, and the first call of a turn knows no cache:
//!   both price as before, all of it plain input.
//! - **The rest**, past the cached part, is a cache write at the
//!   conversation breakpoint's TTL when the request carried one (the
//!   provider writes the whole prompt through it), and plain input when not.
//!
//! The settle's cap stays the call's reservation. The `provider.cut` row and
//! the completion's detail say it is an estimate, and `split` says how.

use super::*;

/// How a cut call's input was split: none was cached.
pub const SPLIT_UNCACHED: &str = "uncached";
/// The previous call's prompt read from the cache, the rest plain input.
pub const SPLIT_READ: &str = "cache_read";
/// The previous call's prompt read from the cache, the rest written to it.
pub const SPLIT_READ_WRITE: &str = "cache_read_write";

/// What a cut call's request held of the provider's cache, as it was sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Prefix {
    /// The tokens the turn's previous call read from the cache or wrote to
    /// it: the prefix this request repeats.
    cached: u64,
    /// The request carried the conversation's breakpoint, so its prompt past
    /// the cached part is written; `Some(true)` at the 1-hour TTL.
    writes: Option<bool>,
}

impl Prefix {
    /// The prefix of `compiled`'s request, after the turn's last answered
    /// call `last` (none in a turn's first loop).
    pub(super) fn of(last: Option<&ModelResponse>, compiled: &Compiled) -> Self {
        let cached = match last {
            Some(r) if !compiled.new_compilation => {
                r.usage.cache_read_input_tokens + r.usage.cache_creation_input_tokens
            }
            _ => 0,
        };
        let writes = compiled
            .request
            .cache_control
            .as_ref()
            .map(|m| m["ttl"] == "1h");
        Self { cached, writes }
    }

    /// The input of a call whose reservation assumed `est_input` tokens,
    /// split by this prefix, and the split's name.
    pub(super) fn split(self, est_input: u64) -> (Usage, &'static str) {
        if self.cached == 0 {
            let usage = Usage {
                input_tokens: est_input,
                ..Usage::default()
            };
            return (usage, SPLIT_UNCACHED);
        }
        let read = self.cached.min(est_input);
        let rest = est_input - read;
        let mut usage = Usage {
            cache_read_input_tokens: read,
            ..Usage::default()
        };
        match self.writes {
            Some(hour) => {
                usage.cache_creation_input_tokens = rest;
                if hour {
                    usage.cache_creation_1h_input_tokens = rest;
                }
                (usage, SPLIT_READ_WRITE)
            }
            None => {
                usage.input_tokens = rest;
                (usage, SPLIT_READ)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(cached: u64, writes: Option<bool>) -> Prefix {
        Prefix { cached, writes }
    }

    #[test]
    fn a_cut_call_reads_what_the_previous_call_cached_and_writes_the_rest() {
        let (u, s) = prefix(7_000, Some(false)).split(8_066);
        assert_eq!(s, SPLIT_READ_WRITE);
        assert_eq!(
            (
                u.input_tokens,
                u.cache_read_input_tokens,
                u.cache_creation_input_tokens,
                u.cache_creation_1h_input_tokens
            ),
            (0, 7_000, 1_066, 0)
        );
        let (u, _) = prefix(7_000, Some(true)).split(8_066);
        assert_eq!(u.cache_creation_1h_input_tokens, 1_066);
        // No breakpoint: the rest is plain input.
        let (u, s) = prefix(7_000, None).split(8_066);
        assert_eq!(s, SPLIT_READ);
        assert_eq!((u.input_tokens, u.cache_read_input_tokens), (1_066, 7_000));
        // A cache larger than the estimate reads the estimate, never more.
        let (u, _) = prefix(30_000, Some(false)).split(8_066);
        assert_eq!(
            (u.cache_read_input_tokens, u.cache_creation_input_tokens),
            (8_066, 0)
        );
    }

    #[test]
    fn a_cut_with_nothing_cached_is_all_plain_input() {
        for writes in [None, Some(false), Some(true)] {
            let (u, s) = prefix(0, writes).split(8_066);
            assert_eq!(s, SPLIT_UNCACHED);
            assert_eq!(
                u,
                Usage {
                    input_tokens: 8_066,
                    ..Usage::default()
                }
            );
        }
    }
}
