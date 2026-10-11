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
    /// backfill (theseus-zo1y): `serve --query-threads`. 0: half the
    /// machine's cores, at most 4.
    #[serde(default)]
    pub query_threads: u32,
    /// The model unloads after this many minutes unused, and loads again on
    /// the next use.
    #[serde(default = "default_idle_unload_mins")]
    pub idle_unload_mins: f64,
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

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            weights_dir: default_weights_dir(),
            threads: default_index_threads(),
            query_threads: 0,
            idle_unload_mins: default_idle_unload_mins(),
        }
    }
}
