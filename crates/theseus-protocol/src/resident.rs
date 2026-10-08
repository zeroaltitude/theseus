//! Health's `resident` block (theseus-9lxe): the daemon's own memory, what
//! its allocator holds beyond what is in use, the trims that gave it back,
//! and the largest caches by size.

use serde::{Deserialize, Serialize};

/// The daemon's resident memory and its largest caches.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ResidentHealth {
    /// The process's resident set (`/proc/self/statm`), in bytes.
    #[cfg_attr(test, ts(type = "number"))]
    pub rss_bytes: u64,
    /// The allocator's heap: absent where it cannot be read (a musl build).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub heap: Option<HeapHealth>,
    /// Trims since the start: each gives the allocator's free pages back
    /// after a quiet stretch that followed work.
    #[cfg_attr(test, ts(type = "number"))]
    pub trims: u64,
    /// The newest trim.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_trim: Option<TrimHealth>,
    /// The largest caches, each with its size and its bound.
    pub caches: Vec<CacheHealth>,
}

/// The allocator's heap (glibc's `mallinfo2`, every arena).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct HeapHealth {
    /// Bytes the daemon's allocations hold now.
    #[cfg_attr(test, ts(type = "number"))]
    pub in_use_bytes: u64,
    /// Bytes the allocator holds from the system: in use and free.
    #[cfg_attr(test, ts(type = "number"))]
    pub held_bytes: u64,
}

/// One trim: when, how long it took, and the resident set it gave back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TrimHealth {
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    pub ms: f64,
    #[cfg_attr(test, ts(type = "number"))]
    pub rss_before_bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub rss_after_bytes: u64,
}

/// A cache by size: `bytes` is an estimate where `estimated`, and
/// `cap_bytes` its bound (0: none of its own, as the import's catalog,
/// which is dropped after an idle stretch instead).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct CacheHealth {
    pub name: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub cap_bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub entries: u64,
    pub estimated: bool,
    /// What else to know: its idle bound, its drops, or why it is empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}
