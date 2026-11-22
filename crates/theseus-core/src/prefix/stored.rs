//! The retired prefix library's records as old stores hold them, and an old compilation's `manifest.prefix`:
//! read, never written (`super`'s doc).

use serde::{Deserialize, Serialize};

/// The META key prefixes of a header, its hit log, and its retirement, as old stores hold them.
pub const HEADER: &str = "prefix:";
pub const HITS: &str = "prefix.hits.";
pub const RETIRED: &str = "prefix.retired.";

/// A section of a header's layer: its name, its text's digest and its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub digest: String,
    pub bytes: u64,
}

/// A header, as an old store holds it (META `prefix:<digest>`, store format 41). Nothing writes one now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub digest: String,
    /// `project` (the third layer).
    pub layer: String,
    /// `private` or `shared`.
    pub class: String,
    /// The digest of what the cache read before it: the tools, the header and the persona layer.
    pub lower: String,
    pub sections: Vec<Section>,
    pub text: String,
    pub sketch: Vec<i8>,
    pub created_at_ms: u64,
}

/// A header's hit log (META `prefix.hits.<digest>`). Nothing writes one now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Hits {
    pub reuses: u64,
    pub last_read_ms: u64,
}

/// Why a header retired (META `prefix.retired.<digest>`). Nothing writes one now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Retired {
    pub at_ms: u64,
    /// `evicted` or `source_changed`.
    pub why: String,
}

/// What an old compilation's manifest records of the library (store format 41): the header its third layer
/// was, and the digest of the fresh layer it stood for. A new compilation records none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixUse {
    pub header: String,
    pub fresh: String,
}
