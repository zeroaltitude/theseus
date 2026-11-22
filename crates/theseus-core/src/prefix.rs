//! What the prefix library (theseus-rr15, retired 2026-11-22) left in stores: its records, still read, never
//! written. The library kept headers of a request's third layer and reused them across sessions; the view now
//! renders each request's system sections afresh, with the same content, and nothing is substituted. An old
//! store's META records (`prefix:<digest>`, `prefix.hits.<digest>`, `prefix.retired.<digest>`, store format 41)
//! still decode as these types, and an old compilation's `manifest.prefix` as [`PrefixUse`]. Nothing writes any of
//! them now.
//!
//! The types live in `stored`. (`rsi::sealed::hold`'s behaviour list, a keel file, still names `prefix.rs` and
//! `prefix/*`: those lines are the owner's to drop.)

mod stored;
pub use stored::*;
