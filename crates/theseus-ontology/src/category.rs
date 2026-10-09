//! Categories, their guidance, and sessions' memberships (spec §4.1a; M4
//! design §2.8's data shapes): the records the core keeps as META keys.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::kind::Origin;
use crate::refusal::Refusal;
use crate::text;

/// The most characters in the part of a category's id after its kind.
pub const LOCAL_MAX: usize = 64;

/// A category's id, `<kind>:<local>`, spelled as §5.5's namespaces are
/// (`guild:<id>`, `channel:<id>`, `person:<discord_user>`, `topic:<id>`).
/// The kind is part of the id, so a category never changes kind. The local
/// part is up to 64 ASCII letters, digits, `.`, `_`, or `-`: a Discord id,
/// or a slug ([`crate::Ontology::mint_id`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CategoryId(String);

impl CategoryId {
    pub fn new(kind: &str, local: &str) -> Result<Self, Refusal> {
        Self::parse(&format!("{kind}:{local}"))
    }

    pub fn parse(s: &str) -> Result<Self, Refusal> {
        let what = || format!("the category id {s:?}");
        let Some((kind, local)) = s.split_once(':') else {
            return Err(Refusal::invalid(
                what(),
                "a category's id is `<kind>:<local>`, as in `topic:rust-harness`",
            ));
        };
        text::kind_name(kind)?;
        let ok = (1..=LOCAL_MAX).contains(&local.len())
            && local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !ok {
            return Err(Refusal::invalid(
                what(),
                format!(
                    "after the kind comes 1 to {LOCAL_MAX} ASCII letters, digits, `.`, `_`, or `-`"
                ),
            ));
        }
        Ok(CategoryId(s.to_string()))
    }

    /// The kind: the id up to its first `:`.
    pub fn kind(&self) -> &str {
        self.0.split_once(':').map(|(k, _)| k).unwrap_or(&self.0)
    }

    pub fn local(&self) -> &str {
        self.0.split_once(':').map(|(_, l)| l).unwrap_or("")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CategoryId {
    type Error = Refusal;
    fn try_from(s: String) -> Result<Self, Refusal> {
        CategoryId::parse(&s)
    }
}

impl From<CategoryId> for String {
    fn from(id: CategoryId) -> String {
        id.0
    }
}

impl fmt::Display for CategoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A category: the record at `onto:cat:<id>`. A given kind's are made from
/// the transport at a place's first bind; an interpreted kind's are
/// declared by the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: CategoryId,
    /// One line, as the guidance's headers show it.
    pub name: String,
    /// The parent, of the kind's parent kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<CategoryId>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub added_by: String,
    /// Taken away (ms since the epoch): a record that says so is no
    /// category, and the snapshot holds none for it. Only one nothing uses
    /// is taken away (no child, membership, or guidance): an import's erase
    /// takes the topics it made that way. Declaring it again supersedes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_ms: Option<u64>,
    /// A person's handles (theseus-wy7y; store format 26): `discord:<id>`,
    /// `slack:<id>`, `email:<addr>`, and display names as `name:<name>`
    /// ([`crate::person`]). Only a person carries them, and no two people
    /// hold one handle but a name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<String>,
    /// Merged into another person (theseus-wy7y; store format 26): a record
    /// that says so is no category, as a retired one is, and its memberships
    /// and guidance went to that person in the same frame. The ledger's
    /// `ontology.merged` row holds what the merge moved, so it can be undone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<CategoryId>,
}

impl Category {
    /// A category of `id` named `name`, with nothing else set: the fields a
    /// caller does not name stay empty (`Category { .., ..Category::new(..) }`).
    pub fn new(id: CategoryId, name: impl Into<String>, added_by: impl Into<String>) -> Self {
        Category {
            id,
            name: name.into(),
            parent: None,
            description: String::new(),
            added_by: added_by.into(),
            retired_ms: None,
            handles: Vec::new(),
            merged_into: None,
        }
    }

    /// Taken away: retired, or merged into another.
    pub fn is_gone(&self) -> bool {
        self.retired_ms.is_some() || self.merged_into.is_some()
    }

    pub fn kind(&self) -> &str {
        self.id.kind()
    }

    /// The record that takes `self` away at `at_ms`.
    pub fn retired(&self, at_ms: u64) -> Category {
        Category {
            retired_ms: Some(at_ms),
            ..self.clone()
        }
    }

    /// The record's own rules, apart from the rest of the ontology.
    pub(crate) fn check_fields(&self) -> Result<(), Refusal> {
        text::line(
            &format!("category `{}`'s name", self.id),
            &self.name,
            text::NAME_MAX,
        )?;
        text::prose(
            &format!("category `{}`'s description", self.id),
            &self.description,
            text::DESCRIPTION_MAX,
        )?;
        text::line(
            &format!("category `{}`'s added_by", self.id),
            &self.added_by,
            text::ADDED_BY_MAX,
        )?;
        if self.parent.as_ref() == Some(&self.id) {
            return Err(Refusal::Cycle {
                path: format!("{} › {}", self.id, self.id),
            });
        }
        crate::person::check_handles(self)
    }
}

/// A category's guidance: the record at `onto:guide:<category>`. Empty text
/// is no guidance, which is how guidance is taken away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guidance {
    pub category: CategoryId,
    pub text: String,
    /// 1 when the category's guidance is first written, and one more at each
    /// change.
    pub version: u32,
    /// The first 16 hex digits of the SHA-256 of `text`.
    pub digest: String,
    pub added_by: String,
}

impl Guidance {
    /// Guidance as it is stored: `\r\n` made `\n`, the ends trimmed, and the
    /// digest of that.
    pub fn new(category: CategoryId, text: &str, version: u32, added_by: &str) -> Self {
        let text = text::normalized(text);
        Guidance {
            digest: text::sha16(&text),
            category,
            text,
            version,
            added_by: added_by.to_string(),
        }
    }

    /// No guidance: empty text.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The record's own rules; the text's shape under its kind's rule is
    /// checked against the table.
    pub(crate) fn check_fields(&self) -> Result<(), Refusal> {
        let what = || format!("the guidance of `{}`", self.category);
        if self.text != text::normalized(&self.text) {
            return Err(Refusal::invalid(
                what(),
                "its text is not stored as Guidance::new stores it (trimmed, with `\\n` line ends)",
            ));
        }
        if self.digest != text::sha16(&self.text) {
            return Err(Refusal::invalid(what(), "its digest is not its text's"));
        }
        if self.version == 0 {
            return Err(Refusal::Version {
                what: what(),
                want: 1,
                got: 0,
            });
        }
        text::line(
            &format!("{}'s added_by", what()),
            &self.added_by,
            text::ADDED_BY_MAX,
        )
    }
}

/// One membership: a session's place in a category (the `member_of` edge).
/// A given one is read from the session's place at compile and never
/// stored; an interpreted one is an entry of a [`MemberList`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Membership {
    pub category: CategoryId,
    pub origin: Origin,
    /// How sure the origin is, from 0 to 1. Absent for the transport and the
    /// operator, which are sure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// When the membership was assigned (ms since the epoch); for a given
    /// one, when its place was read.
    pub as_of_ms: u64,
}

impl Membership {
    /// A membership from the session's place.
    pub fn given(category: CategoryId, as_of_ms: u64) -> Self {
        Membership {
            category,
            origin: Origin::Transport,
            confidence: None,
            as_of_ms,
        }
    }

    /// A membership the operator assigned.
    pub fn operator(category: CategoryId, as_of_ms: u64) -> Self {
        Membership {
            category,
            origin: Origin::Operator,
            confidence: None,
            as_of_ms,
        }
    }

    /// A membership the import assigned, from an imported session's labels.
    pub fn import(category: CategoryId, as_of_ms: u64) -> Self {
        Membership {
            category,
            origin: Origin::Import,
            confidence: None,
            as_of_ms,
        }
    }

    pub fn kind(&self) -> &str {
        self.category.kind()
    }

    pub(crate) fn check_confidence(&self) -> Result<(), Refusal> {
        match self.confidence {
            Some(c) if !(0.0..=1.0).contains(&c) => Err(Refusal::invalid(
                format!("the membership in `{}`", self.category),
                format!("its confidence is {c}, and a confidence is from 0 to 1"),
            )),
            _ => Ok(()),
        }
    }
}

/// One session's memberships of one interpreted kind: the record at
/// `onto:member:<session>:<kind>`. A change is a new list that supersedes
/// the old (the WAL keeps the history), and an empty list takes them all
/// away.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemberList {
    pub session: String,
    pub kind: String,
    pub members: Vec<Membership>,
}

impl MemberList {
    /// The list's own fields; its entries are checked against the ontology.
    pub(crate) fn check_fields(&self) -> Result<(), Refusal> {
        text::line("a session's id", &self.session, text::ADDED_BY_MAX)?;
        if self.session.contains(':') {
            return Err(Refusal::invalid(
                format!("the session id {:?}", self.session),
                "it holds a `:`, which would make its record's key ambiguous",
            ));
        }
        text::kind_name(&self.kind)
    }
}
