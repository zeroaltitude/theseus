//! Labels (M4 19a; design m4-boundaries §2.5): each node written from 19a on
//! carries one, set in the frame that writes it and never rewritten. Its
//! integrity says whether the text came from outside, and its readers who may
//! see it; the owner always may. A session's audience is who sees what its
//! model says, and a compile admits a node only when the node's readers cover
//! that audience (§2.7). The rules that combine them (`covers`, the meet) are
//! the core's (`theseus_core::labels`); these are the shapes every surface
//! reads.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::ExternalText;

/// Whether a node's text came from outside Theseus. `untrusted` wins when
/// two meet. `Quarantined` comes with the Advisory (the reader rule).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Integrity {
    #[default]
    Trusted,
    Untrusted,
}

/// Who may see a node; the owner always may. A place is a guild channel,
/// `discord:<channel id>`, and a person `discord:<user id>`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readers {
    /// Anyone: a fetched page, a tree `[labels] public_paths` names.
    Public,
    /// Whoever can view the guild channel: what was said there.
    Place(String),
    /// Named people: a DM with `u` is `People{u}`.
    People(BTreeSet<String>),
    /// The owner alone: the CLI's and the web UI's words, and what the
    /// operator's own files and programs return.
    Owner,
}

impl Readers {
    /// The readers in a few words, as a placeholder and `theseus labels`
    /// say them: `owner-only`, `public`, `for discord:42`, `for 3 people`,
    /// `for channel discord:7`.
    pub fn describe(&self) -> String {
        match self {
            Readers::Public => "public".into(),
            Readers::Owner => "owner-only".into(),
            Readers::Place(p) => format!("for channel {p}"),
            Readers::People(s) if s.len() == 1 => {
                format!("for {}", s.iter().next().map_or("", String::as_str))
            }
            Readers::People(s) => format!("for {} people", s.len()),
        }
    }
}

/// A node's label (§2.5).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub integrity: Integrity,
    /// Why it is untrusted: where its text came from, in T1's shape (the
    /// tool, the URL, the node). Absent on a trusted node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub source: Option<ExternalText>,
    pub readers: Readers,
}

impl Label {
    pub fn trusted(readers: Readers) -> Self {
        Self {
            integrity: Integrity::Trusted,
            source: None,
            readers,
        }
    }

    pub fn untrusted(source: ExternalText, readers: Readers) -> Self {
        Self {
            integrity: Integrity::Untrusted,
            source: Some(source),
            readers,
        }
    }
}

/// A session's audience, as a compile evaluated it: who sees what its model
/// says. It comes from the session's place (`outbox.target`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Audience {
    /// No place (the CLI, the web UI, a task that reports nowhere): the
    /// owner alone.
    Owner,
    /// A DM: the person in it.
    People { people: BTreeSet<String> },
    /// A guild channel, `discord:<channel id>`: whoever can view it.
    Place {
        place: String,
        /// The channel's name, when the binding gave it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        name: Option<String>,
        /// How many can view it, the bot aside. Absent when they cannot be
        /// read (the Server Members intent is off, or the binding has not
        /// said yet): then the channel counts as public.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        viewers: Option<u32>,
        /// The first 16 hex digits of the SHA-256 of the viewers' ids,
        /// sorted, so a change of who views it with the same count is a
        /// change of audience.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        digest: Option<String>,
    },
}

impl Audience {
    /// The audience in words: `the owner`, `discord:42`, `#lab (2 people)`,
    /// or `#lab (public: its viewers cannot be read)`.
    pub fn describe(&self) -> String {
        match self {
            Audience::Owner => "the owner".into(),
            Audience::People { people } if people.len() == 1 => {
                people.iter().next().cloned().unwrap_or_default()
            }
            Audience::People { people } => format!("{} people", people.len()),
            Audience::Place {
                place,
                name,
                viewers,
                ..
            } => {
                let at = name
                    .as_deref()
                    .map_or_else(|| format!("channel {place}"), |n| format!("#{n}"));
                match viewers {
                    Some(1) => format!("{at} (1 person)"),
                    Some(n) => format!("{at} ({n} people)"),
                    None => format!("{at} (public: its viewers cannot be read)"),
                }
            }
        }
    }
}

/// A node a compile left out, and why (the manifest's `withheld`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withheld {
    pub node_id: String,
    /// The node's readers, in words (`owner-only`).
    pub reason: String,
}

/// Integrity in play (the manifest's `integrity`): the session holds
/// external text (T1's latch), and how many admitted nodes are untrusted.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InPlay {
    pub latched: bool,
    pub untrusted: u32,
}
