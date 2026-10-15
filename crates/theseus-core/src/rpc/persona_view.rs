//! `persona.list` (theseus-slvl.2): the personas the config defines and the
//! built-in default, each with its fields, its files, the compiles that
//! carried it, and the places that speak as it. A read: no frame, no lock,
//! and nothing on a turn's path. The tally scans sessions' compilations, as
//! `standing.list`'s does.

use theseus_protocol::{
    PersonaFile, PersonaInfo, PersonaListParams, PersonaListResult, PersonaPlace, PersonaTally,
};

use super::server::RpcFailure;
use super::Core;
use crate::config::PersonaView;
use crate::context_files::{ContextFile, ContextPath, ContextReaders};
use crate::persona;
use crate::session::SessionRecord;

impl Core {
    pub(crate) fn persona_list(
        &self,
        p: PersonaListParams,
    ) -> Result<PersonaListResult, RpcFailure> {
        let in_play = match &p.session_id {
            Some(id) => {
                let rec = self
                    .store
                    .get_session::<SessionRecord>(id)?
                    .ok_or_else(|| RpcFailure::invalid(anyhow::anyhow!("no session {id}")))?;
                self.runner.persona_of(&rec)
            }
            None => self.cfg.persona_key_for(None, None).map(str::to_string),
        };
        let placed = self.runner.place_rule.personas();
        let places: Vec<PersonaPlace> = placed
            .iter()
            .map(|(place, target, persona)| PersonaPlace {
                place: place.clone(),
                target: target.clone(),
                persona: persona.clone(),
                unknown: !self.cfg.personas.contains_key(persona),
            })
            .collect();
        let mut personas: Vec<PersonaInfo> = self
            .cfg
            .personas
            .iter()
            .map(|(key, c)| {
                let view = PersonaView::of(key, c);
                let mut info = self.persona_info(&view, in_play.as_deref() == Some(key));
                info.places = places
                    .iter()
                    .filter(|x| x.persona == *key)
                    .cloned()
                    .collect();
                info
            })
            .collect();
        let mut built_in = self.persona_info(&PersonaView::built_in(), in_play.is_none());
        built_in.places = Vec::new();
        personas.push(built_in);
        Ok(PersonaListResult {
            personas,
            in_play,
            built_in_bytes: crate::config::persona::DEFAULT_TEXT.len() as u64,
            built_in_digest: persona::digest(crate::config::persona::DEFAULT_TEXT),
            places,
        })
    }

    fn persona_info(&self, view: &PersonaView, in_play: bool) -> PersonaInfo {
        let paths: Vec<ContextPath> = view
            .files
            .iter()
            .map(|f| ContextPath {
                path: f.path().into(),
                persona: view.key.clone(),
                public: f.readers() == ContextReaders::Public,
            })
            .collect();
        let read: Vec<ContextFile> = self.runner.context_files.peek(&paths);
        let files: Vec<PersonaFile> = read
            .iter()
            .map(|f| PersonaFile {
                path: f.file.path.clone(),
                bytes: f.file.bytes,
                digest: f.file.digest.clone(),
                missing: f.file.missing.clone(),
                edited_ms: std::fs::metadata(&f.file.path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64),
            })
            .collect();
        let refs: Vec<_> = read.iter().map(|f| f.file.clone()).collect();
        let sighting = persona::sighting(view, &refs, 0);
        let digest = match &sighting {
            Some(s) => persona::digest_of(s),
            None => persona::digest(&view.name),
        };
        let owned = persona::owned(&read);
        let mut warnings = Vec::new();
        for f in &files {
            if let Some(why) = &f.missing {
                warnings.push(format!(
                    "{} cannot be read ({why}){}",
                    f.path,
                    if owned {
                        ""
                    } else {
                        ": the built-in default stands in for this persona"
                    }
                ));
            }
        }
        let seen = view
            .key
            .as_deref()
            .and_then(|k| self.runner.persona.seen(&self.store, k));
        let tally = view
            .key
            .as_deref()
            .zip(seen.as_ref())
            .filter(|_| owned)
            .and_then(|(k, s)| self.persona_tally(k, s.at_ms, &files));
        PersonaInfo {
            key: view.key.clone(),
            name: view.name.clone(),
            emoji: view.emoji.clone(),
            card: view.card.clone(),
            tts_voice: view.tts_voice.clone(),
            avatar: view.avatar.clone(),
            initiative: view.initiative.as_str().into(),
            in_play,
            files,
            digest,
            changed_ms: seen.map(|s| s.at_ms),
            tally,
            on_default: !owned,
            places: Vec::new(),
            warnings,
        }
    }

    /// The compiles of the sessions that speak as `key`, since `since_ms`:
    /// how many there were in a private place, and how many lacked the
    /// current file. A read of the sessions' compilations, never a turn's.
    fn persona_tally(
        &self,
        key: &str,
        since_ms: u64,
        files: &[PersonaFile],
    ) -> Option<PersonaTally> {
        let current: Vec<&str> = files.iter().filter_map(|f| f.digest.as_deref()).collect();
        let mut t = PersonaTally {
            since_ms,
            ..Default::default()
        };
        let sessions = self.store.live_sessions::<SessionRecord>().ok()?;
        for s in sessions {
            if s.last_active_ms < since_ms || self.runner.persona_of(&s).as_deref() != Some(key) {
                continue;
            }
            for c in self.store.session_compilations(&s.session_id).ok()? {
                // A shared place's compile withheld the file: not in the count.
                let withheld = c
                    .manifest
                    .context_files
                    .iter()
                    .any(|f| f.persona.as_deref() == Some(key) && f.withheld.is_some());
                if c.created_at_ms < since_ms || withheld {
                    continue;
                }
                t.compiles += 1;
                let carried = c.manifest.context_files.iter().any(|f| {
                    f.persona.as_deref() == Some(key)
                        && f.withheld.is_none()
                        && f.digest.as_deref().is_some_and(|d| current.contains(&d))
                });
                if !carried {
                    t.missing += 1;
                }
            }
        }
        Some(t)
    }
}

impl Core {
    /// What is wrong with the personas in use, for health: the default and
    /// the places' own. A stat of each file (never a read), and a name the
    /// config lacks.
    pub(super) fn persona_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut used: Vec<String> = self.cfg.persona().map(str::to_string).into_iter().collect();
        for (place, _, name) in self.runner.place_rule.personas() {
            if !self.cfg.personas.contains_key(&name) {
                out.push(format!(
                    "{place} names persona {name:?}, which the config does not define: it speaks \
                     as the default"
                ));
            } else if !used.contains(&name) {
                used.push(name);
            }
        }
        for key in used {
            let Some(p) = self.cfg.personas.get(&key) else {
                continue;
            };
            let gone: Vec<String> = p
                .files
                .iter()
                .map(|f| crate::config::expand(f.path()))
                .filter(|f| !f.is_file())
                .map(|f| f.display().to_string())
                .collect();
            if !gone.is_empty() && gone.len() == p.files.len() {
                out.push(format!(
                    "persona {key}: {} cannot be read, so it compiles on the built-in default",
                    gone.join(", ")
                ));
            } else if !gone.is_empty() {
                out.push(format!(
                    "persona {key}: {} cannot be read; its section says it is missing",
                    gone.join(", ")
                ));
            }
        }
        out
    }
}
