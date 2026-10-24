//! The persona as data (theseus-slvl.2): which persona a session speaks as,
//! whether the header carries the built-in default or the owner's own files,
//! and what changed in a persona between compiles.
//!
//! - **Which** (`TurnRunner::persona_of`): the session's own choice
//!   (`session.open { persona }`, a META key `persona.session.<id>`, a task's
//!   its parent's), else its place's (`persona = "<name>"` in the bindings
//!   file), else `[context] default_persona`, else the built-in default
//!   (`Config::persona_for`). The choice is read from memory (`Board`).
//! - **What the header carries** (`owned`): the built-in default text
//!   (`persona/default.md`) unless the persona in play has a file of its own
//!   in this request, which replaces it whole. A persona whose files cannot
//!   be read, or are withheld from a shared place, compiles on the default,
//!   never thinner.
//! - **What changed** (`Board::observe`): each compile compares the persona's
//!   files' digests and its fields' with those it last saw (a META key
//!   `persona.seen.<name>`, which rides the turn's next frame), and a
//!   difference is a `persona.changed` row, once. No watcher.
//!
//! Tests: `tests_persona.rs`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, RwLock};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use theseus_store::{kinds, Store as _};

use crate::compiler::ContextFileRef;
use crate::config::PersonaView;
use crate::context_files::ContextFile;
use crate::store::Store;

/// The META key of a persona's last-seen state.
pub const SEEN_PREFIX: &str = "persona.seen.";
/// The META key of a session's own persona.
pub const SESSION_PREFIX: &str = "persona.session.";

/// Whether the persona in play has a readable file of its own in this
/// request: its text then replaces the built-in default whole.
pub fn owned(files: &[ContextFile]) -> bool {
    files
        .iter()
        .any(|f| f.file.persona.is_some() && f.file.digest.is_some() && f.file.withheld.is_none())
}

/// What a compile saw of a persona: each file's digest by path (empty when it
/// could not be read) and each field's by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub files: BTreeMap<String, String>,
    pub fields: BTreeMap<String, String>,
    pub at_ms: u64,
}

/// One difference between two sightings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// `file` or a field's name.
    pub field: String,
    /// The file's path, for a `file`.
    pub path: Option<String>,
    /// Digests; empty is absent.
    pub old: String,
    pub new: String,
}

/// The first 16 hex digits of a text's SHA-256, as a context file's digest is.
pub fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))[..16].to_string()
}

/// A sighting's digest: its files' and its fields', not its time.
pub fn digest_of(s: &Seen) -> String {
    digest(&format!("{:?}{:?}", s.files, s.fields))
}

/// A persona's sighting from its view and the files this request carries.
/// None when a file is withheld (a shared place's request sees part of the
/// persona, which says nothing of what changed).
pub fn sighting(view: &PersonaView, files: &[ContextFileRef], at_ms: u64) -> Option<Seen> {
    let key = view.key.as_deref()?;
    let mine = || files.iter().filter(|f| f.persona.as_deref() == Some(key));
    if mine().any(|f| f.withheld.is_some()) {
        return None;
    }
    let files = mine()
        .map(|f| (f.path.clone(), f.digest.clone().unwrap_or_default()))
        .collect();
    let field = |v: &Option<String>| v.as_deref().map(digest).unwrap_or_default();
    let fields = [
        ("name", digest(&view.name)),
        ("emoji", field(&view.emoji)),
        ("card", field(&view.card)),
        ("tts_voice", field(&view.tts_voice)),
        ("avatar", field(&view.avatar)),
        ("initiative", digest(view.initiative.as_str())),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    Some(Seen {
        files,
        fields,
        at_ms,
    })
}

/// What differs between two sightings, files first.
pub fn diff(old: &Seen, new: &Seen) -> Vec<Change> {
    let mut out = Vec::new();
    let paths: std::collections::BTreeSet<_> = old.files.keys().chain(new.files.keys()).collect();
    for p in paths {
        let (o, n) = (old.files.get(p), new.files.get(p));
        if o != n {
            out.push(Change {
                field: "file".into(),
                path: Some(p.clone()),
                old: o.cloned().unwrap_or_default(),
                new: n.cloned().unwrap_or_default(),
            });
        }
    }
    let names: std::collections::BTreeSet<_> = old.fields.keys().chain(new.fields.keys()).collect();
    for k in names {
        let (o, n) = (old.fields.get(k), new.fields.get(k));
        if o != n {
            out.push(Change {
                field: k.clone(),
                path: None,
                old: o.cloned().unwrap_or_default(),
                new: n.cloned().unwrap_or_default(),
            });
        }
    }
    out
}

/// What the board makes of a sighting.
#[derive(Debug, Default)]
pub struct Observed {
    /// The differences from the last sighting; none for the first.
    pub changes: Vec<Change>,
    /// The state to store: set when this sighting is the first or differs.
    pub write: Option<Seen>,
}

/// The daemon's memory of personas: what each last looked like, and the
/// sessions' own choices. Both are read from the store once, on the first
/// turn that needs them or after serving (`Core::warm_persona`).
#[derive(Default)]
pub struct Board {
    seen: Mutex<Option<BTreeMap<String, Seen>>>,
    sessions: RwLock<Option<HashMap<String, String>>>,
    /// Which persona answered each session's recent turns, in memory
    /// (theseus-c1uz).
    pub engaged: crate::engage::Engagements,
    /// Each persona's agent, as its import recorded it (theseus-a8n7), read
    /// once.
    persona_agents: Mutex<HashMap<String, String>>,
    /// The (persona, digest) of each soul over its cap the owner was told of
    /// this run: one note a digest (theseus-utfnl).
    over_cap: Mutex<std::collections::HashSet<(String, String)>>,
    /// The agent of each imported session recall met, read once
    /// (`persona_recall::PersonaScene`).
    pub(crate) agents: std::sync::Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl Board {
    /// Compare a sighting with the last one and keep it.
    pub fn observe(&self, store: &Store, persona: &str, now: Seen) -> Observed {
        let mut g = self.seen.lock().unwrap();
        if g.is_none() {
            *g = Some(read_seen(store));
        }
        let map = g.as_mut().expect("loaded");
        let changes = match map.get(persona) {
            Some(old) => diff(old, &now),
            None => Vec::new(),
        };
        if map.contains_key(persona) && changes.is_empty() {
            return Observed::default();
        }
        map.insert(persona.to_string(), now.clone());
        Observed {
            changes,
            write: Some(now),
        }
    }

    /// Whether this is the first time this run that `persona`'s soul of this
    /// digest is met over its cap.
    pub fn first_over_cap(&self, persona: &str, digest: &str) -> bool {
        self.over_cap
            .lock()
            .unwrap()
            .insert((persona.to_string(), digest.to_string()))
    }

    /// The name a persona's imported episodes carry, lower case: its import's
    /// record, else its key.
    pub fn agent_of(&self, state_dir: &std::path::Path, persona: &str) -> String {
        let mut g = self.persona_agents.lock().unwrap();
        g.entry(persona.to_string())
            .or_insert_with(|| {
                crate::persona_import::Manifest::read(&crate::persona_import::run::dir_of(
                    state_dir, persona,
                ))
                .map(|m| m.agent)
                .unwrap_or_else(|| persona.to_string())
                .to_lowercase()
            })
            .clone()
    }

    /// The last sighting of a persona.
    pub fn seen(&self, store: &Store, persona: &str) -> Option<Seen> {
        let mut g = self.seen.lock().unwrap();
        if g.is_none() {
            *g = Some(read_seen(store));
        }
        g.as_ref()?.get(persona).cloned()
    }

    /// A session's own persona, if it chose one.
    pub fn session(&self, store: &Store, session_id: &str) -> Option<String> {
        if let Some(m) = self.sessions.read().unwrap().as_ref() {
            return m.get(session_id).cloned();
        }
        let mut w = self.sessions.write().unwrap();
        let m = w.get_or_insert_with(|| read_sessions(store));
        m.get(session_id).cloned()
    }

    /// Record a session's choice in memory (the caller stores it).
    pub fn choose(&self, store: &Store, session_id: &str, persona: &str) {
        let mut w = self.sessions.write().unwrap();
        w.get_or_insert_with(|| read_sessions(store))
            .insert(session_id.to_string(), persona.to_string());
    }

    /// Read both from the store now.
    pub fn warm(&self, store: &Store) {
        let mut g = self.seen.lock().unwrap();
        if g.is_none() {
            *g = Some(read_seen(store));
        }
        let mut w = self.sessions.write().unwrap();
        w.get_or_insert_with(|| read_sessions(store));
    }

    /// How many sessions chose a persona (for tests and health).
    pub fn chosen(&self) -> usize {
        self.sessions
            .read()
            .unwrap()
            .as_ref()
            .map_or(0, HashMap::len)
    }
}

fn read_seen(store: &Store) -> BTreeMap<String, Seen> {
    let mut out = BTreeMap::new();
    let rows = theseus_store::blocking(|| -> Result<_> {
        store.inner().latest_with_prefix(kinds::META, SEEN_PREFIX)
    });
    match rows {
        Ok(rows) => {
            for r in rows {
                let Some(name) = r.key.as_deref().and_then(|k| k.strip_prefix(SEEN_PREFIX)) else {
                    continue;
                };
                match r.decode::<Seen>() {
                    Ok(s) => {
                        out.insert(name.to_string(), s);
                    }
                    Err(e) => tracing::warn!(persona = name, error = %e,
                        "a persona's last-seen state does not read: it counts as new"),
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "personas' last-seen state cannot be read")
        }
    }
    out
}

fn read_sessions(store: &Store) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let rows = theseus_store::blocking(|| -> Result<_> {
        store
            .inner()
            .latest_with_prefix(kinds::META, SESSION_PREFIX)
    });
    match rows {
        Ok(rows) => {
            for r in rows {
                let Some(id) = r
                    .key
                    .as_deref()
                    .and_then(|k| k.strip_prefix(SESSION_PREFIX))
                else {
                    continue;
                };
                if let Ok(name) = r.decode::<String>() {
                    out.insert(id.to_string(), name);
                }
            }
        }
        Err(e) => tracing::warn!(error = %format!("{e:#}"), "sessions' personas cannot be read"),
    }
    out
}
