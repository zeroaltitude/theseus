//! The persona in play for a turn (theseus-slvl.2), and what a compile saw
//! of it. See `crate::persona`.

use super::{Target, Turn, TurnRunner};
use crate::compiler::RequestSpec;
use crate::fact::persona::PersonaChanged;
use crate::session::SessionRecord;

/// How far up a task's parents a persona is looked for.
const DEPTH: usize = 8;

impl Target {
    /// This target, speaking as `other`'s persona: a target resolved again
    /// for a moved profile keeps the session's persona.
    pub(super) fn speaking_as(self, other: &Target) -> Target {
        Target {
            persona: other.persona.clone(),
            ..self
        }
    }
}

impl TurnRunner {
    /// What a continuation runs on: the session's last turn's target, which
    /// every turn records from its start (theseus-kol), so a conversation
    /// does not change model under its own thinking blocks, whatever profile
    /// is live. A profile no longer configured gives its provider and model
    /// under the live profile's settings; only a session with neither falls
    /// back to the live profile.
    pub(super) fn target_as_ran(
        &self,
        s: &SessionRecord,
        live_profile: &str,
    ) -> anyhow::Result<Target> {
        if let Some(t) = &s.last_target {
            let as_it_ran = self.resolve_target(
                live_profile,
                Some(&t.profile),
                Some(&t.provider),
                Some(&t.model),
            );
            if let Ok(tg) = as_it_ran.or_else(|_| {
                self.resolve_target(live_profile, None, Some(&t.provider), Some(&t.model))
            }) {
                return Ok(tg);
            }
        }
        self.resolve_target(live_profile, None, None, None)
    }

    /// The `[personas]` key a session speaks as: its own choice (a task's is
    /// its parent's), else its place's, else `[context] default_persona`;
    /// none is the built-in default. Memory only: no read of the store but the
    /// first, and nothing at all while no persona is configured.
    pub fn persona_of(&self, session: &SessionRecord) -> Option<String> {
        if self.cfg.personas.is_empty() {
            return None;
        }
        let own = self.session_persona(session);
        let place = self.place_persona(&session.session_id);
        self.cfg
            .persona_key_for(own.as_deref(), place.as_deref())
            .map(str::to_string)
    }

    fn session_persona(&self, session: &SessionRecord) -> Option<String> {
        let mut cur = std::borrow::Cow::Borrowed(session);
        for _ in 0..DEPTH {
            if let Some(own) = self.persona.session(&self.store, &cur.session_id) {
                return Some(own);
            }
            let parent = cur.task.as_ref()?.parent_session.clone();
            cur = std::borrow::Cow::Owned(
                self.store
                    .get_session::<SessionRecord>(&parent)
                    .ok()
                    .flatten()?,
            );
        }
        None
    }

    /// The persona the bindings file gives the place where `session_id`
    /// speaks (a task's is its parent's, as its class is).
    pub(super) fn place_persona(&self, session_id: &str) -> Option<String> {
        let target = self
            .outbox
            .try_target(session_id)
            .and_then(|t| match t {
                Some(t) => Ok(Some(t)),
                None => self.outbox.try_wake_target(session_id),
            })
            .ok()
            .flatten()?;
        self.place_rule.persona_of(&target)
    }

    /// A persona's file that cannot be read, met for the first time this run:
    /// one notice to the owner, saying what the compile does about it. The
    /// row and the log line are the caller's.
    pub(super) fn persona_file_missing(&self, u: &crate::context_files::Unreadable) {
        let Some(persona) = &u.persona else { return };
        let text = format!(
            "The file {} of persona {persona} cannot be read ({}). A request whose persona has \
             no readable file compiles on the built-in default; its section says the file is \
             missing.",
            u.path, u.error
        );
        if let Err(e) = self
            .outbox
            .to_operator(None, serde_json::json!({"kind": "notice", "text": text}))
        {
            tracing::warn!(persona, error = %format!("{e:#}"), "the missing-persona notice was not posted");
        }
    }

    /// Compare what this compile reads of the persona in play with what the
    /// last one saw: a difference is a `persona.changed` row for each field,
    /// and the new state, both in the turn's next frame. A first sighting
    /// stores the state and says nothing.
    pub(super) fn observe_persona(&self, t: &mut Turn<'_>, target: &Target, spec: &RequestSpec) {
        let Some(key) = target.persona.as_deref() else {
            return;
        };
        let Some(p) = self.cfg.personas.get(key) else {
            return;
        };
        let view = crate::config::PersonaView::of(key, p);
        let Some(now) =
            crate::persona::sighting(&view, &spec.context_files, theseus_protocol::now_unix_ms())
        else {
            return;
        };
        let seen = self.persona.observe(&self.store, key, now);
        let Some(write) = seen.write else { return };
        for c in &seen.changes {
            t.record(&PersonaChanged {
                persona: key,
                field: &c.field,
                path: c.path.as_deref(),
                old: &c.old,
                new: &c.new,
            });
        }
        match theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&format!("{}{key}", crate::persona::SEEN_PREFIX)),
            &write,
        ) {
            Ok(r) => {
                if let Err(e) = t.tc.store.defer(r) {
                    tracing::warn!(persona = key, error = %e, "a persona's state was not stored");
                }
            }
            Err(e) => {
                tracing::warn!(persona = key, error = %e, "a persona's state cannot be encoded")
            }
        }
    }
}
