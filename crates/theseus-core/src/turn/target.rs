//! What a turn runs against: the `Target` a profile and any raw overrides resolve to, and the `TurnRunner`'s
//! methods that resolve it (`resolve_target`, `target_for_session`, `spec_of`) and read a session's place class.

use super::*;

/// What a turn runs against, resolved from a profile plus any raw overrides.
#[derive(Debug, Clone)]
pub struct Target {
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    pub system: Option<String>,
    /// The persona in play (theseus-c48): its context files follow the
    /// system level's. `[context].default_persona` until Jev chooses one.
    pub persona: Option<String>,
    /// A message in this session's place can engage another persona
    /// (`TurnRunner::engages`): the request carries every persona's files
    /// (theseus-nsp9l.1).
    pub engages: bool,
    pub effort: Option<Effort>,
    pub thinking_display: ThinkingDisplay,
    pub max_loops: u32,
    pub on_max_loops: crate::config::MaxLoopsMode,
    pub refusal_fallbacks: bool,
    /// The profile's prompt-cache TTL (theseus-ev1).
    pub cache_ttl: CacheTtl,
    /// The owner chose this turn's profile, provider, or model: routing
    /// never moves it (M5 25e). What was chosen, for the record.
    pub chosen: Option<String>,
}

impl From<&Target> for TargetRef {
    fn from(t: &Target) -> Self {
        TargetRef {
            profile: t.profile.clone(),
            provider: t.provider.clone(),
            model: t.model.clone(),
        }
    }
}

impl TurnRunner {
    /// Resolve what a turn runs against. Precedence: raw `provider`/`model`
    /// overrides > the named `profile` > the live profile.
    pub fn resolve_target(
        &self,
        live_profile: &str,
        profile: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<Target> {
        let name = profile.unwrap_or(live_profile);
        let prof = self.cfg.profile(name)?;
        let provider = provider.unwrap_or(&prof.provider).to_string();
        if !self.providers.contains_key(&provider) {
            anyhow::bail!(
                "unknown provider {provider:?}; configured: {}",
                self.providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let model = model.unwrap_or(&prof.model).to_string();
        let max_tokens = prof
            .max_output_tokens
            .or_else(|| self.catalog.get(&model).map(|e| e.max_output_tokens))
            .unwrap_or(16_384);
        Ok(Target {
            profile: name.to_string(),
            provider,
            model,
            max_tokens,
            system: prof.system.clone(),
            persona: self.personas().default_key().map(str::to_string),
            engages: false,
            effort: prof.effort,
            thinking_display: prof.thinking_display,
            max_loops: prof.max_loops,
            on_max_loops: prof.on_max_loops,
            refusal_fallbacks: prof.refusal_fallbacks,
            cache_ttl: prof.ttl(),
            chosen: None,
        })
    }

    /// What a continuation runs on (`target_as_ran`), speaking as the
    /// session's persona (theseus-slvl.2).
    pub fn target_for_session(&self, s: &SessionRecord, live_profile: &str) -> Result<Target> {
        let tg = self.target_as_ran(s, live_profile)?;
        let tg = self.helper_target(s, tg);
        Ok(Target {
            persona: self.persona_of(s),
            engages: self.engages(s),
            ..tg
        })
    }

    fn first_party(&self, provider: &str) -> bool {
        self.cfg
            .all_providers()
            .get(provider)
            .map(|p| crate::provider::is_first_party(&p.api_base))
            .unwrap_or(false)
    }

    /// `request_spec` over context files already read (`context.explain`
    /// reads them without warning of a missing one, theseus-7n3e).
    pub fn spec_of(
        &self,
        target: &Target,
        kind: SessionKind,
        place: crate::ceiling::PlaceView,
        mut files: Vec<ContextFile>,
    ) -> RequestSpec {
        // A shared place carries only the files marked public (the place
        // rule): the rest are their headers and why.
        if place.class == crate::places::PlaceClass::Shared {
            crate::context_files::withhold_shared(&mut files);
        }
        let (system_text, persona_text, context_text) = self.system_blocks(target, &files, place);
        RequestSpec {
            profile: target.profile.clone(),
            provider: target.provider.clone(),
            model: target.model.clone(),
            max_tokens: target.max_tokens,
            system_text,
            persona_text,
            context_text,
            // The session's block is its caller's (`session_block`).
            session_text: String::new(),
            card: None,
            context_files: files.iter().map(|f| f.file.clone()).collect(),
            persona: target.persona.clone(),
            defer: None,
            tools: self.tools.definitions_for(place),
            effort: target.effort,
            turn_effort: None,
            effort_refused: self.effort_refused(),
            thinking_display: target.thinking_display,
            refusal_fallbacks: target.refusal_fallbacks,
            fallback: None,
            first_party: self.first_party(&target.provider),
            cache_ttl: target.cache_ttl,
            conversation_ttl: match kind {
                SessionKind::Task => CacheTtl::FiveMinutes,
                SessionKind::Conversation => target.cache_ttl,
            },
            walk: None,
            memberships: vec![],
            guidance: vec![],
            prefix: None,
            prefix_used: None,
        }
    }

    /// The class of the place a session's turn speaks in (the place rule,
    /// theseus-nbsh): where its words go. That is the session's place (a
    /// task's is its parent's, `outbox.target`), or, for a session no place
    /// runs on any more (`/new`), the place its wakes and reports answer in
    /// (theseus-4lx), the latest its takes kept. With neither, the CLI's or
    /// the web UI's: private. Shared when where it goes cannot be read.
    pub fn class_of(&self, session_id: &str) -> crate::places::PlaceClass {
        self.view_of(session_id).class
    }
}
