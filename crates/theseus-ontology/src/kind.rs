//! The kinds table (spec §4.1a; M4 design §2.8): which kinds of context
//! exist is data, a row per kind, not code. The code is the closed sets a row
//! picks from: the composition rules the compiler reads, and the origins a
//! membership may come from. A rule or an origin that nothing builds yet is
//! refused by name until it is built (the reader rule, theseus-wjy).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::refusal::Refusal;
use crate::text;

/// The `added_by` of the rows the build carries.
pub const SEED: &str = "seed";

/// The kinds the transport gives. A given kind needs code that reads its
/// memberships from a session's place, and the transport has these three.
pub const GIVEN: [&str; 3] = ["guild", "channel", "person"];

/// Where a kind's memberships come from: the first guardrail (spec §4.1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Facts from the transport: read from the place at compile, never
    /// stored per session, never set through the API, never re-associated.
    Given,
    /// Interpretations: assigned, and re-associated by a newer record.
    Interpreted,
}

/// How a kind's guidance is admitted: the closed set the compiler knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Walk up the parents: the farthest ancestor's guidance first, so the
    /// nearest comes last and overrides (for rules).
    Chain,
    /// One line per category (for intent).
    IntentLine,
    /// Admitted within the budget by relevance (for lessons). Not built.
    Ranked,
    /// Never admitted automatically. Not built.
    RecallOnly,
}

impl Rule {
    pub const ALL: [Rule; 4] = [
        Rule::Chain,
        Rule::IntentLine,
        Rule::Ranked,
        Rule::RecallOnly,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Rule::Chain => "chain",
            Rule::IntentLine => "intent_line",
            Rule::Ranked => "ranked",
            Rule::RecallOnly => "recall_only",
        }
    }

    /// What builds the rule while nothing reads it; `None` once it is built.
    pub fn comes_with(self) -> Option<&'static str> {
        match self {
            Rule::Chain | Rule::IntentLine => None,
            Rule::Ranked => Some("M6, with lessons as guidance"),
            Rule::RecallOnly => Some("M6, with recall across sessions"),
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Who assigned a membership: the `member_of` edge's origin (spec §4.1a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// A session's place: given kinds only.
    Transport,
    /// The owner or an operator, through the CLI or the web UI.
    Operator,
    /// Jev's `categorize.v1`. Not built.
    Jev,
    /// A sweep. Not built.
    Sweep,
    /// A dream. Not built.
    Dream,
}

impl Origin {
    pub const ALL: [Origin; 5] = [
        Origin::Transport,
        Origin::Operator,
        Origin::Jev,
        Origin::Sweep,
        Origin::Dream,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Origin::Transport => "transport",
            Origin::Operator => "operator",
            Origin::Jev => "jev",
            Origin::Sweep => "sweep",
            Origin::Dream => "dream",
        }
    }

    /// What builds the origin while nothing writes from it; `None` once it
    /// is built.
    pub fn comes_with(self) -> Option<&'static str> {
        match self {
            Origin::Transport | Origin::Operator => None,
            Origin::Jev => Some("M5, with Jev's categorize.v1"),
            Origin::Sweep => Some("M6, with sweeps"),
            Origin::Dream => Some("M6, with dreams"),
        }
    }

    pub(crate) fn built(self) -> Result<(), Refusal> {
        match self.comes_with() {
            None => Ok(()),
            Some(comes_with) => Err(Refusal::UnbuiltOrigin {
                origin: self,
                comes_with,
            }),
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How many categories of a kind one session may hold. It is written as a
/// number, or as `"many"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerSession {
    /// At most this many, and at least 1.
    AtMost(u32),
    /// No limit: a channel's listed people.
    Many,
}

impl PerSession {
    pub fn allows(self, n: usize) -> bool {
        match self {
            PerSession::AtMost(max) => n <= max as usize,
            PerSession::Many => true,
        }
    }
}

impl fmt::Display for PerSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PerSession::AtMost(n) => write!(f, "{n}"),
            PerSession::Many => f.write_str("many"),
        }
    }
}

impl Serialize for PerSession {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            PerSession::AtMost(n) => s.serialize_u32(*n),
            PerSession::Many => s.serialize_str("many"),
        }
    }
}

impl<'de> Deserialize<'de> for PerSession {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Written {
            Count(u32),
            Word(String),
        }
        match Written::deserialize(d)? {
            Written::Count(n) => Ok(PerSession::AtMost(n)),
            Written::Word(w) if w == "many" => Ok(PerSession::Many),
            Written::Word(w) => Err(serde::de::Error::custom(format!(
                "per_session is a number or \"many\", not {w:?}"
            ))),
        }
    }
}

/// A row of the kinds table: the record at `onto:kind:<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kind {
    pub name: String,
    pub basis: Basis,
    /// The origins that may assign a membership of the kind: `transport`
    /// alone for a given kind, never `transport` for an interpreted one.
    pub assigned_by: Vec<Origin>,
    pub per_session: PerSession,
    /// The kind of a category's parent: the kind's own name when its
    /// categories nest (topics), absent when they have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The order of the kinds in a compile, lowest first: a higher kind's
    /// guidance comes later and governs. No two kinds share one.
    pub precedence: u32,
    pub rule: Rule,
    /// What the kind is for (embedded from M6).
    #[serde(default)]
    pub description: String,
    /// 1 when the row is first written, and one more at each change.
    pub version: u32,
    /// Who wrote this version: `seed` for the rows the build carries.
    pub added_by: String,
}

impl Kind {
    pub fn is_given(&self) -> bool {
        self.basis == Basis::Given
    }

    /// Whether a category of this kind may have a parent of `kind`.
    pub fn nests_under(&self, kind: &str) -> bool {
        self.parent.as_deref() == Some(kind)
    }

    /// The row's own rules, apart from the rest of the table.
    pub fn check_row(&self) -> Result<(), Refusal> {
        text::kind_name(&self.name)?;
        if let Some(comes_with) = self.rule.comes_with() {
            return Err(Refusal::UnbuiltRule {
                kind: self.name.clone(),
                rule: self.rule,
                comes_with,
            });
        }
        if self.assigned_by.is_empty() {
            return Err(Refusal::invalid(
                format!("kind `{}`'s assigned_by", self.name),
                "it is empty: name the origins that may assign a membership",
            ));
        }
        for (i, o) in self.assigned_by.iter().enumerate() {
            if self.assigned_by[..i].contains(o) {
                return Err(Refusal::Duplicate {
                    why: format!("kind `{}` lists the origin `{o}` twice", self.name),
                });
            }
            o.built()?;
        }
        if let Some(p) = &self.parent {
            text::kind_name(p)?;
        }
        if self.per_session == PerSession::AtMost(0) {
            return Err(Refusal::invalid(
                format!("kind `{}`'s per_session", self.name),
                "it is 0, so no session could hold one: give 1 or more, or \"many\"",
            ));
        }
        text::prose(
            &format!("kind `{}`'s description", self.name),
            &self.description,
            text::DESCRIPTION_MAX,
        )?;
        if self.version == 0 {
            return Err(Refusal::Version {
                what: format!("kind `{}`", self.name),
                want: 1,
                got: 0,
            });
        }
        text::line(
            &format!("kind `{}`'s added_by", self.name),
            &self.added_by,
            text::ADDED_BY_MAX,
        )?;
        self.check_basis()
    }

    /// The first guardrail, for rows: only the transport's three kinds are
    /// given, their facts (basis, origins, count, and parent) are the
    /// transport's and stay as the seed has them, and an interpreted kind
    /// never takes a membership from the transport. Precedence, rule, and
    /// description are the operator's to change.
    fn check_basis(&self) -> Result<(), Refusal> {
        match given_seed(&self.name) {
            Some(seed) => {
                let facts = [
                    ("basis", self.basis == seed.basis),
                    ("assigned_by", self.assigned_by == seed.assigned_by),
                    ("per_session", self.per_session == seed.per_session),
                    ("parent", self.parent == seed.parent),
                ];
                if let Some((column, _)) = facts.iter().find(|(_, same)| !same) {
                    return Err(Refusal::Given {
                        kind: self.name.clone(),
                        why: format!(
                            "`{}` is given by the transport, and its {column} is the transport's \
                             fact: it stays as it is ({}). Its precedence, rule, and description \
                             may change",
                            self.name,
                            seed.fact(column)
                        ),
                    });
                }
                Ok(())
            }
            None if self.is_given() => Err(Refusal::Given {
                kind: self.name.clone(),
                why: format!(
                    "`{}` cannot be given: the transport gives only guild, channel, and person. \
                     Make it interpreted, assigned by the operator",
                    self.name
                ),
            }),
            None if self.assigned_by.contains(&Origin::Transport) => Err(Refusal::Given {
                kind: self.name.clone(),
                why: format!(
                    "`{}` is interpreted, and the transport assigns only given kinds: \
                     remove `transport` from its assigned_by",
                    self.name
                ),
            }),
            None => Ok(()),
        }
    }

    fn fact(&self, column: &str) -> String {
        match column {
            "basis" => match self.basis {
                Basis::Given => "given".into(),
                Basis::Interpreted => "interpreted".into(),
            },
            "assigned_by" => {
                let names: Vec<&str> = self.assigned_by.iter().map(|o| o.name()).collect();
                names.join(", ")
            }
            "per_session" => self.per_session.to_string(),
            _ => self.parent.clone().unwrap_or_else(|| "none".into()),
        }
    }
}

/// The rows M4 seeds: the three given kinds, and topic, the first
/// interpreted one. Culture and expertise come with their reader, Jev (M5).
pub fn seeds() -> Vec<Kind> {
    let row = |name: &str,
               basis,
               assigned_by,
               per_session,
               parent: Option<&str>,
               precedence,
               rule,
               description: &str| Kind {
        name: name.into(),
        basis,
        assigned_by,
        per_session,
        parent: parent.map(str::to_string),
        precedence,
        rule,
        description: description.into(),
        version: 1,
        added_by: SEED.into(),
    };
    vec![
        row(
            "guild",
            Basis::Given,
            vec![Origin::Transport],
            PerSession::AtMost(1),
            None,
            10,
            Rule::Chain,
            "The server a session's place is in, from the transport.",
        ),
        row(
            "channel",
            Basis::Given,
            vec![Origin::Transport],
            PerSession::AtMost(1),
            Some("guild"),
            20,
            Rule::Chain,
            "The place a session lives in, from the transport. A channel's parent is its guild.",
        ),
        row(
            "person",
            Basis::Given,
            vec![Origin::Transport],
            PerSession::Many,
            None,
            30,
            Rule::IntentLine,
            "The people a session talks with, from the transport: a DM's user, or a channel's \
             listed users.",
        ),
        row(
            "topic",
            Basis::Interpreted,
            vec![Origin::Operator],
            PerSession::AtMost(3),
            Some("topic"),
            40,
            Rule::Chain,
            "What a session is about, declared by the operator. Topics nest.",
        ),
    ]
}

/// The seed row of a given kind, whose facts no row may change.
fn given_seed(name: &str) -> Option<Kind> {
    if !GIVEN.contains(&name) {
        return None;
    }
    seeds().into_iter().find(|k| k.name == name)
}
