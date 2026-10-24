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
            engages: other.engages,
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
        let book = self.personas();
        if book.is_empty() {
            return None;
        }
        let own = self.session_persona(session);
        let place = self.place_persona(&session.session_id);
        book.persona_key_for(own.as_deref(), place.as_deref())
            .map(str::to_string)
    }

    /// The persona in play and whether a message can engage another, for a turn of `session`.
    pub(super) fn speak_as_session(&self, target: &mut Target, session: &SessionRecord) {
        target.persona = self.persona_of(session);
        target.engages = self.engages(session);
    }

    /// Whether a message in this session's place can engage another persona
    /// (route.v5, theseus-c1uz): two or more `[personas]`, and a DM, a
    /// channel or a thread, where Jev chooses the persona per message. A cli,
    /// task or headless session's persona is fixed for the session, so its
    /// request carries only that persona's files (theseus-nsp9l.1). One
    /// answer for every reader of the context files: the turn, the keep-warm
    /// replay, the census.
    pub fn engages(&self, session: &SessionRecord) -> bool {
        use crate::tools_defer::PlaceKind;
        if !self.personas().engaging() {
            return false;
        }
        match self.place_kind(&session.session_id, session.task.is_some()) {
            PlaceKind::Dm | PlaceKind::Channel => true,
            PlaceKind::Task | PlaceKind::Headless => false,
            PlaceKind::Cli => self
                .threads
                .held()
                .is_some_and(|b| b.of_session(&session.session_id).is_some()),
        }
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
        let text = missing_text(persona, &u.path, &u.error);
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
        self.alert_over_cap(spec);
        let Some(key) = target.persona.as_deref() else {
            return;
        };
        let Some(view) = self.personas().get(key).cloned() else {
            return;
        };
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

impl TurnRunner {
    /// A soul the compile carried over `[context] persona_max_tokens` (a hand
    /// edit, or a migrated file): carried whole, never cut, and said to the
    /// owner once for each digest. A shared place's withheld file says
    /// nothing of its size.
    fn alert_over_cap(&self, spec: &RequestSpec) {
        let cap = self.cfg.context.persona_cap();
        let book = self.personas();
        for f in &spec.context_files {
            let (Some(key), Some(digest)) = (f.persona.as_deref(), f.digest.as_deref()) else {
                continue;
            };
            let tokens = f.bytes.div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
            if f.withheld.is_some() || tokens <= cap || !self.persona.first_over_cap(key, digest) {
                continue;
            }
            let name = book.get(key).map_or(key, |v| v.name.as_str());
            let text = over_cap_text(name, &f.path, tokens, cap);
            if let Err(e) = self
                .outbox
                .to_operator(None, serde_json::json!({"kind": "notice", "text": text}))
            {
                tracing::warn!(persona = key, error = %format!("{e:#}"), "the over-cap note was not posted");
            }
        }
    }
}

/// The note: a soul over its cap is still carried whole.
pub(crate) fn over_cap_text(name: &str, path: &str, tokens: u64, cap: u64) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        &format!("Persona {name}'s soul is over its cap"),
        &format!("the compile carried {path}: {tokens} tokens, the cap is {cap}"),
        "It is carried whole and sits in every DM's cached persona layer. Trim it to the cap: \
         a persona is one small file.",
    )
}

/// A persona's unreadable file, said to the owner (theseus-x8b9k).
pub(crate) fn missing_text(persona: &str, path: &str, error: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        &format!("Persona {persona}'s file cannot be read"),
        &format!("the compile met {path} unreadable: {error}"),
        "A request whose persona has no readable file compiles on the built-in default; its \
         section says the file is missing.",
    )
}
