//! The personas, as the ontology holds them (theseus-utfnl): a `persona:<key>`
//! category per persona, its card the short fields and the path of its soul
//! file. The [`Book`] is what every reader of the personas reads: built from
//! an ontology snapshot (`ontology::Board::personas`), so it follows every
//! write of the snapshot, and never from the config, which `migrate` reads
//! once.
//!
//! A persona with no soul yet is *awaiting* it: listed, never offered to
//! Jev, never addressable, no prompt tokens, and not counted among the
//! personas that can answer.

use std::collections::BTreeMap;

use theseus_ontology::persona::PersonaCard;
use theseus_ontology::{Initiative as CardInitiative, Ontology};

use crate::config::PersonaView;
use crate::context_files::{ContextEntry, ContextFileEntry, ContextReaders};

pub mod migrate;
pub mod note;
pub mod soul;
pub mod soul_read;
pub mod write;

/// The kind's name, and a category's id for a key.
pub use theseus_ontology::persona::KIND;

/// The persona categories in the ontology, by key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Book {
    live: BTreeMap<String, PersonaView>,
    awaiting: BTreeMap<String, PersonaView>,
    default: Option<String>,
}

/// A card's view of its persona: the display name is the category's.
pub fn view_of(key: &str, name: &str, p: &PersonaCard) -> PersonaView {
    let files = p
        .soul
        .iter()
        .map(|path| match p.soul_public {
            true => ContextEntry::Table(ContextFileEntry {
                path: path.clone(),
                readers: ContextReaders::Public,
            }),
            false => ContextEntry::Path(path.clone()),
        })
        .collect();
    PersonaView {
        key: Some(key.to_string()),
        name: name.to_string(),
        emoji: p.emoji.clone(),
        card: p.card.clone(),
        tts_voice: p.tts_voice.clone(),
        avatar: p.avatar.clone(),
        initiative: match p.initiative {
            CardInitiative::Quiet => crate::config::Initiative::Quiet,
            CardInitiative::Normal => crate::config::Initiative::Normal,
            CardInitiative::Chatty => crate::config::Initiative::Chatty,
        },
        files,
        shareable: p.shareable,
    }
}

impl Book {
    pub fn from_ontology(onto: &Ontology) -> Self {
        let mut book = Book::default();
        for c in onto.categories().filter(|c| c.kind() == KIND) {
            let Some(p) = &c.persona else { continue };
            let key = c.id.local().to_string();
            let view = view_of(&key, &c.name, p);
            if p.default {
                book.default = Some(key.clone());
            }
            match p.is_live() {
                true => book.live.insert(key, view),
                false => book.awaiting.insert(key, view),
            };
        }
        book
    }

    /// The personas that can answer, in key order.
    pub fn live(&self) -> impl Iterator<Item = (&String, &PersonaView)> {
        self.live.iter()
    }

    /// Every persona's key, awaiting a soul or not.
    pub fn keys(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .live
            .keys()
            .chain(self.awaiting.keys())
            .cloned()
            .collect();
        all.sort();
        all
    }

    /// The personas still awaiting a soul.
    pub fn awaiting(&self) -> impl Iterator<Item = (&String, &PersonaView)> {
        self.awaiting.iter()
    }

    /// A persona, awaiting its soul or not: one a session or a place names
    /// is in play either way (an awaiting one adds no text).
    pub fn get(&self, key: &str) -> Option<&PersonaView> {
        self.live.get(key).or_else(|| self.awaiting.get(key))
    }

    /// Whether `key` is a persona at all.
    pub fn exists(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Whether `key` has a soul, so Jev may pick it and a message may
    /// address it.
    pub fn is_live(&self, key: &str) -> bool {
        self.live.contains_key(key)
    }

    /// No persona at all, awaiting a soul or not.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty() && self.awaiting.is_empty()
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Two or more that can answer: where a message can engage another
    /// persona (`TurnRunner::engages`) the request carries all their souls,
    /// the header is the built-in default whoever answers, and each message
    /// is answered by the persona it calls for (`engage`, theseus-c1uz).
    pub fn engaging(&self) -> bool {
        self.live.len() >= 2
    }

    /// Every live key, comma-separated.
    pub fn names(&self) -> String {
        self.live.keys().cloned().collect::<Vec<_>>().join(", ")
    }

    /// The key of the default persona.
    pub fn default_key(&self) -> Option<&str> {
        self.default.as_deref()
    }

    /// The default persona when it has a soul: what Jev's `none` engages.
    /// Without one, `none` engages the built-in.
    pub fn default_live(&self) -> Option<&str> {
        self.default_key().filter(|k| self.is_live(k))
    }

    /// The key `persona_for` finds: the session's own choice, else its
    /// place's, else the default; a name that cannot answer is passed over
    /// to the next, never an error at a turn. None is the built-in.
    pub fn persona_key_for(&self, session: Option<&str>, place: Option<&str>) -> Option<&str> {
        [session, place, self.default_key()]
            .into_iter()
            .flatten()
            .find_map(|n| {
                let both = self.live.iter().chain(self.awaiting.iter());
                both.map(|(k, _)| k.as_str()).find(|k| *k == n)
            })
    }

    pub fn persona_for(&self, session: Option<&str>, place: Option<&str>) -> PersonaView {
        match self
            .persona_key_for(session, place)
            .and_then(|k| self.get(k))
        {
            Some(v) => v.clone(),
            None => PersonaView::built_in(),
        }
    }
}

impl crate::turn::TurnRunner {
    /// The personas now: the ontology's snapshot, read once if nothing has
    /// built it yet. A snapshot that cannot be read is no persona (warned),
    /// so a turn runs as the built-in default and never fails on it.
    pub fn personas(&self) -> std::sync::Arc<Book> {
        self.ontology.personas(&self.store).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"),
                "the ontology cannot be read: no persona is in play");
            std::sync::Arc::new(Book::default())
        })
    }
}

impl crate::rpc::Core {
    /// See [`crate::turn::TurnRunner::personas`].
    pub fn personas(&self) -> std::sync::Arc<Book> {
        self.runner.personas()
    }
}

#[cfg(test)]
impl Book {
    /// The book a config's `[personas]` would make once migrated: every
    /// persona live, the default as `[context] default_persona` names it.
    pub(crate) fn of_config(cfg: &crate::config::Config) -> Self {
        Book {
            live: cfg
                .personas
                .iter()
                .map(|(k, p)| (k.clone(), PersonaView::of(k, p)))
                .collect(),
            awaiting: BTreeMap::new(),
            default: cfg.context.default_persona.clone(),
        }
    }
}

/// A soul file for a test persona that names none: a small file in a
/// directory of the process's own, and its path as a config spells it.
#[cfg(test)]
pub(crate) fn test_soul(key: &str) -> crate::context_files::ContextEntry {
    let dir = std::env::temp_dir().join(format!("theseus-souls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{key}.md"));
    std::fs::write(&path, format!("You are {key}.\n")).unwrap();
    path.to_string_lossy().into_owned().into()
}

/// Who wrote a persona, as its category's `added_by`.
pub(crate) fn soul_added_by(who: &crate::approval::Answerer) -> String {
    crate::rpc::ontology::added_by(who)
}

/// The ledger fact of a soul written.
pub(crate) fn soul_fact<'a>(
    key: &'a str,
    w: &'a soul::Written,
    who: &'a crate::approval::Answerer,
) -> crate::fact::persona::PersonaSoul<'a> {
    crate::fact::persona::PersonaSoul {
        persona: key,
        path: &w.path,
        old: w.old.as_deref(),
        new: &w.digest,
        who: who.who(),
        via: who.via(),
    }
}
