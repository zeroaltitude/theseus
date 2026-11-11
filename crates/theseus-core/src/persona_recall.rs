//! The persona's part of a recall (theseus-a8n7): while a persona is engaged,
//! its own agent's imported episodes rank above others at equal relevance
//! (`[persona_import] own_boost`, a bounded multiplier), and a recalled item of
//! theirs says whose it is in its header (`[from Wren, 2026-09-14, @4210]`).
//! Off when no persona is engaged, and when the boost is 1.0.
//!
//! Engaged here means the persona the turn speaks as (`Target.persona`: the
//! session's own choice, its place's, or the default). A per-message verdict
//! of route.v5 will engage a persona the same way, through the same scene.
//! The agent a persona's episodes carry is its import's record
//! (`persona_import::Manifest.agent`), else the persona's key; an episode's
//! agent is read once per session and kept in memory.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use theseus_memory::{Candidate, PersonaBoost};

use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRunner;

/// What the persona does to one recall.
#[derive(Clone)]
pub struct PersonaScene {
    /// The persona's display name: the header's source.
    pub name: String,
    /// Its agent, lower case.
    agent: String,
    boost: f64,
    store: Store,
    memo: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl PersonaScene {
    /// The agent an imported session came from, lower case; none for any
    /// other session. Read once.
    pub fn agent_of(&self, session: &str) -> Option<String> {
        if !crate::import::is_imported(session) {
            return None;
        }
        let mut memo = self.memo.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(a) = memo.get(session) {
            return a.clone();
        }
        let agent = match self.store.get_session::<SessionRecord>(session) {
            Ok(rec) => Some(
                rec.and_then(|r| r.imported.and_then(|i| i.agent))
                    .map(|a| a.to_lowercase()),
            )
            .flatten(),
            // Not kept: a read that failed is asked again.
            Err(_) => return None,
        };
        memo.insert(session.to_string(), agent.clone());
        agent
    }

    /// Whether `session` is an imported one of this persona's own agent.
    pub fn is_own(&self, session: &str) -> bool {
        self.agent_of(session).as_deref() == Some(self.agent.as_str())
    }

    /// The pure pipeline's lens over `candidates`; none when the boost is
    /// off.
    pub fn boost(&self, candidates: &[Candidate]) -> Option<PersonaBoost> {
        if self.boost <= 1.0 {
            return None;
        }
        let mut of_session = std::collections::BTreeMap::new();
        for c in candidates {
            if of_session.contains_key(&c.session_id) {
                continue;
            }
            if let Some(a) = self.agent_of(&c.session_id) {
                of_session.insert(c.session_id.clone(), a);
            }
        }
        Some(PersonaBoost {
            agent: self.agent.clone(),
            boost: self.boost,
            of_session,
        })
    }
}

impl TurnRunner {
    /// The state dir this daemon runs on, as `Config::state_dir()` reads it:
    /// a persona's folder is under it (`personas/<name>`), beside the store,
    /// the spool, the bindings file and `AGENTS.md`.
    ///
    /// `theseusd` moves `--state-dir` (or `THESEUS_STATE_DIR`) into
    /// `[server].state_dir` before it builds a `Core` (`state_dir_in_use`, in
    /// `theseusd/src/main.rs`), with `~` and `$NAME` expanded, so the config
    /// *does* know the flag and this is the directory the store was opened in.
    /// It was read back from `store.dir().parent()` on the belief that the
    /// config did not, which is the shape the tool floor's `build_runtime`
    /// had before it moved to `state_dir()` (theseus-3i6d1). That held only
    /// while the store sat directly under the state dir, which is how the
    /// start path lays things out and not a promise: a store anywhere else
    /// put `personas/` somewhere the floor and `project_context` were not
    /// looking.
    ///
    /// A `Core` built by hand (a test, the simulator) must set
    /// `server.state_dir` to where its store lives: a default config points
    /// at `~/.theseus`. `rpc::tests_persona_import` goes red if the store is
    /// made the source again.
    pub fn state_root(&self) -> std::path::PathBuf {
        self.cfg.state_dir()
    }

    /// The persona's part of a recall for a turn that speaks as `persona`
    /// (a `[personas]` key); none while no persona is engaged or the boost is
    /// off.
    pub fn persona_scene(&self, persona: Option<&str>) -> Option<PersonaScene> {
        let key = persona?;
        if self.cfg.persona_import.own_boost <= 1.0 {
            return None;
        }
        let view = self.personas().get(key)?.clone();
        Some(PersonaScene {
            name: view.name,
            agent: self.persona.agent_of(&self.state_root(), key),
            boost: self.cfg.persona_import.own_boost,
            store: self.store.clone(),
            memo: self.persona.agents.clone(),
        })
    }
}
