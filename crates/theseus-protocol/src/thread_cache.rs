//! What threads cost the prompt cache and what keeping them warm saves
//! (theseus-ytt8, T9): per thread and in total, from the ledger's
//! `provider.call` and `keep_warm` rows of the local day. Health's `threads`
//! section, `thread show`, and `thread list` carry it.

use serde::{Deserialize, Serialize};

/// One thread's cache numbers for the day, or the sum of all the threads'.
///
/// A **cold resume** is a turn's first model call that found the session's
/// last call (a turn's, or a keep-warm read) older than the cache's life, so
/// the request wrote its prefix whole; its **rewrite** is what it wrote. A
/// **warm hit** is such a call that read the cache. Of the hits, the
/// **avoided** are those whose previous turn call alone was older than the
/// cache's life: only a keep-warm read kept them warm. `saved_usd` is what
/// each avoided one would have paid to write what it read, less the read's
/// price (the keep-warm reads' own dollars are `keep_warm_usd`, not netted).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ThreadCacheStats {
    pub cold_resumes: u64,
    /// Tokens the cold resumes wrote to the cache, and their price.
    pub rewrite_tokens: u64,
    pub rewrite_usd: f64,
    pub warm_hits: u64,
    pub avoided: u64,
    pub keep_warm_reads: u64,
    pub keep_warm_usd: f64,
    pub saved_usd: f64,
}

impl ThreadCacheStats {
    /// Add `o` to these numbers.
    pub fn add(&mut self, o: &ThreadCacheStats) {
        self.cold_resumes += o.cold_resumes;
        self.rewrite_tokens += o.rewrite_tokens;
        self.rewrite_usd += o.rewrite_usd;
        self.warm_hits += o.warm_hits;
        self.avoided += o.avoided;
        self.keep_warm_reads += o.keep_warm_reads;
        self.keep_warm_usd += o.keep_warm_usd;
        self.saved_usd += o.saved_usd;
    }

    /// Whether anything was counted.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// `2 cold resumes (180k tokens rewritten, $1.20) · 5 warm hits (3 kept by
    /// keep-warm, saving about $0.80) · 14 keep-warm reads, $0.09`: the one
    /// wording every surface shows.
    pub fn line(&self) -> String {
        let plural = |n: u64, one: &str, many: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {many}"),
        };
        let k = |t: u64| match t {
            t if t >= 1_000_000 => format!("{:.1}M", t as f64 / 1e6),
            t if t >= 1_000 => format!("{}k", t / 1_000),
            t => t.to_string(),
        };
        format!(
            "{} ({} tokens rewritten, ${:.2}) · {} ({} kept by keep-warm, saving about ${:.2}) · \
             {}, ${:.2}",
            plural(self.cold_resumes, "cold resume", "cold resumes"),
            k(self.rewrite_tokens),
            self.rewrite_usd,
            plural(self.warm_hits, "warm hit", "warm hits"),
            self.avoided,
            self.saved_usd,
            plural(self.keep_warm_reads, "keep-warm read", "keep-warm reads"),
            self.keep_warm_usd,
        )
    }
}

/// A thread's numbers, by its slug.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ThreadCacheEntry {
    pub slug: String,
    pub stats: ThreadCacheStats,
    /// Whether the keep-warm holds its session now.
    pub kept: bool,
}

/// Health's `threads.cache`: the local day's numbers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ThreadsCache {
    /// The local date the numbers are of.
    pub day: String,
    pub total: ThreadCacheStats,
    /// Threads with a number or a held session, newest activity first.
    pub threads: Vec<ThreadCacheEntry>,
    /// Thread sessions the keep-warm holds now.
    pub kept: u64,
    /// Reads the day's spend line kept the keep-warm from making today
    /// (it warms the most recently active first and skips the rest).
    pub skipped_today: u64,
}

/// Health's `threads` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ThreadsHealth {
    /// `[cache] threads_keep_warm_hours`: how long after its last owner message an
    /// active thread's session is kept warm (0: none).
    pub keep_warm_hours: f64,
    /// The day's numbers; absent until the first read of the ledger ends.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cache: Option<ThreadsCache>,
}
