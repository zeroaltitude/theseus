//! `[index]`: the index tender (M6 §2.2, roadmap row 51), `theseus-index`
//! installed beside `theseusd`, run after serving and restarted when it exits.

use serde::{Deserialize, Serialize};

use super::default_true;

/// `[index]`. The defaults need no paste: it runs, BM25 and entities always,
/// and vectors once the weights are in `weights_dir`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Where the embedding model's files are (`nomic-embed-text-v1.5/`).
    /// Nothing is fetched: without them, BM25 and entities answer alone.
    #[serde(default = "default_weights_dir")]
    pub weights_dir: String,
    /// The embedding model's threads.
    #[serde(default = "default_index_threads")]
    pub threads: u32,
    /// A recall query's embedding's threads, a pool of their own ahead of the
    /// backfill (theseus-zo1y): `serve --query-threads`. 0: the tender's
    /// default, four (never more than the cores). Beside a busy machine the
    /// tender's load gate runs a query on one thread, one at a time
    /// (theseus-d7fy).
    #[serde(default)]
    pub query_threads: u32,
    /// The model unloads after this many minutes unused, and loads again on
    /// the next use.
    #[serde(default = "default_idle_unload_mins")]
    pub idle_unload_mins: f64,
    /// The index's resident memory the hot set is sized to, in MiB (issue 21 of 1.0.7): the
    /// records a search reads first are the recent, used and important ones that fit three
    /// quarters of it, and the rest are read in groups; the model unloaded, the index's memory at
    /// rest and after a run of recalls stays under it however large the store grows.
    #[serde(default = "default_resident_mb")]
    pub resident_mb: u64,
}

/// The least `resident_mb` the index can keep: its own parts (the word index, the groups' centres,
/// the table's bookkeeping) need about 48 MiB, and the hot set 16 MiB more. The tender's
/// `theseus_index::vectors::MIN_RESIDENT_MB` is the same number (the core does not link the index).
pub const MIN_RESIDENT_MB: u64 = 64;

impl IndexConfig {
    /// `resident_mb` is a value the index can keep, or why not.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.resident_mb < MIN_RESIDENT_MB {
            anyhow::bail!(
                "[index] resident_mb = {} is too small: the word index, the groups' centres and \
                 the table's own bookkeeping need about 48 MiB, and the hot set at least 16 MiB \
                 more; the least that holds them is {MIN_RESIDENT_MB}",
                self.resident_mb
            );
        }
        Ok(())
    }
}

fn default_weights_dir() -> String {
    "~/.cache/theseus/models".into()
}
fn default_index_threads() -> u32 {
    1
}
fn default_idle_unload_mins() -> f64 {
    10.0
}
fn default_resident_mb() -> u64 {
    128
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            weights_dir: default_weights_dir(),
            threads: default_index_threads(),
            query_threads: 0,
            idle_unload_mins: default_idle_unload_mins(),
            resident_mb: default_resident_mb(),
        }
    }
}
