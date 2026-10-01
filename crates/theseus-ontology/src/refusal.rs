//! Why the ontology refused a record. Each message says what is wrong in the
//! operator's terms and, where it can, what to do instead: the CLI and the
//! web UI show it as it is.

use crate::kind::{Origin, Rule};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// A field does not parse, or is out of bounds.
    #[error("{what}: {why}")]
    Invalid { what: String, why: String },
    /// A kind names a composition rule the compiler does not read yet.
    #[error(
        "kind `{kind}` names the rule `{rule}`, which is not built yet: it comes with {comes_with}. \
         Every kind needs a rule the compiler reads (the reader rule): use `chain` or `intent_line`"
    )]
    UnbuiltRule {
        kind: String,
        rule: Rule,
        comes_with: &'static str,
    },
    /// A kind or a membership names an origin nothing writes yet.
    #[error(
        "the origin `{origin}` is not built yet: it comes with {comes_with}, and nothing assigns \
         a membership from it before then (the reader rule)"
    )]
    UnbuiltOrigin {
        origin: Origin,
        comes_with: &'static str,
    },
    /// The first guardrail: given kinds are the transport's facts.
    #[error("{why}")]
    Given { kind: String, why: String },
    /// The writer may not write this record.
    #[error("{why}")]
    Writer { why: String },
    /// A record names something the ontology does not hold.
    #[error("no {what} `{id}`")]
    Missing { what: &'static str, id: String },
    /// A category's parents, or a kind's, lead back to it.
    #[error("the parents loop: {path}")]
    Cycle { path: String },
    /// Two kinds share a precedence, or a parent kind would come later.
    #[error("{why}")]
    Precedence { why: String },
    /// A change was made from a stale read.
    #[error(
        "{what} must be version {want}, not {got}: a row starts at 1, and each change is one more"
    )]
    Version { what: String, want: u32, got: u32 },
    #[error("category `{id}` would be nested {depth} deep; categories nest at most {max} deep")]
    TooDeep {
        id: String,
        depth: usize,
        max: usize,
    },
    /// More memberships than the kind allows one session.
    #[error("a session holds at most {max} `{kind}` categories, and this list has {got}")]
    TooMany { kind: String, max: u32, got: usize },
    /// Two of something that must be one.
    #[error("{why}")]
    Duplicate { why: String },
    /// A parent or a member of the wrong kind.
    #[error("{why}")]
    WrongKind { why: String },
}

impl Refusal {
    pub(crate) fn invalid(what: impl Into<String>, why: impl Into<String>) -> Self {
        Refusal::Invalid {
            what: what.into(),
            why: why.into(),
        }
    }
}
