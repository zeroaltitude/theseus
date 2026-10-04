//! CONTINUE's candidate signals (M5 25b; design `m5-judgment.md` §2.4; spec
//! §4.4a, step 2): what a compile saw that might make the current
//! compilation stale, each deterministic and cheap. `context.compiled`
//! carries every one that fired, and a compile that fired one and no
//! deterministic trigger asks `continue.v1` in shadow.

use serde::{Deserialize, Serialize};

/// One candidate signal that fired at a compile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct CompileSignal {
    /// `dormancy`, `tail_band`, `report`, `wake`, or `cache_miss`.
    pub name: String,
    /// Its measure, in the signal's unit: the gap's minutes, the share of
    /// the window the tail reached (in percent), the reports or wakes that
    /// arrived, or the cache read before the miss (in tokens).
    pub value: u64,
    /// The same in words, as `continue.v1` reads it.
    pub detail: String,
}
