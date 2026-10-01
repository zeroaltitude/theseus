//! The content graph's vocabulary: the kinds of edge between nodes (§4.1,
//! §6.1), and the labels a node carries (§3.9: integrity by transmission, and
//! the audience's confidentiality labels).
//!
//! Both are empty. Row 12 (12a) of the roadmap re-cut adds the first edge,
//! `derived_from` on the report route, and row 21 (19a) the first labels. A
//! variant lands with its reader, on the same commit, or with a reserved
//! marker (the reader rule, P0's rule 3, theseus-wjy): the registry test,
//! `tests_registry`, enumerates `VARIANTS` and fails a variant that nothing
//! reads. A reader names the variant by its type (`EdgeKind::DerivedFrom`) in
//! a `match` arm, a pattern, or an `==`.

/// An enum from one table: each variant with its docs and its name in the
/// store and on the wire, and `VARIANTS`, built from the same table, so none
/// is left out of the registry test.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $($(#[$doc:meta])* $variant:ident = $wire:literal,)* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$doc])* $variant,)*
        }

        impl $name {
            /// Every variant's name, with its name in the store and on the wire.
            pub const VARIANTS: &[(&str, &str)] = &[$((stringify!($variant), $wire),)*];
        }
    };
}

vocabulary! {
    /// What an EDGE record says of its two nodes (`kinds::EDGE`, keyed
    /// `<kind>|<from>|<to>`). Row 12 (12a) adds `derived_from`: a node relayed
    /// into another session, from the node it copies.
    pub enum EdgeKind {}
}

vocabulary! {
    /// A label on a node. Row 21 (19a) adds the first: the audience's
    /// confidentiality labels, then integrity by transmission (`untrusted`,
    /// `quarantined`, §3.9's Exposure, row 24).
    pub enum Label {}
}
