//! A message Theseus starts itself, in its persona's voice (theseus-slvl.9).
//!
//! The request is the persona's file (the built-in default when it has none
//! of its own) and the standing block as its system, one factual sentence
//! about the task, and the facts as the user's message: the same text a turn
//! of that persona would be written under, with no tools and no history. It
//! runs on `[initiative] profile` (the live profile unset), as a task of the
//! runtime and never inside an owner's turn, reserved on the daemon's day
//! ceiling like any background call and held to `[initiative] day_usd`: past
//! it, or with the model off or unreachable, the message goes out as its
//! template (`lines`), which reads the persona's name and emoji.
//!
//! What it costs, in the header of nothing: the call is made only for a
//! message the persona's level lets through (`Initiative::lets`), a few a day.

use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use theseus_judge::price::{micros_to_usd, usd_to_micros, Micros};
use theseus_protocol::LedgerKind;

use super::lines::{say, Line};
use crate::config::PersonaView;
use crate::context_files::{ContextPath, ContextReaders};
use crate::fact::initiative::InitiativeSpoke;
use crate::places::PlaceClass;
use crate::provider::ProviderRequest;
use crate::rpc::Core;
use crate::standing::{Block, Scope};

/// What the model is told to do, as one factual sentence.
pub const TASK: &str = "Write the one short message below to the owner, in your own voice, in \
two sentences at most. State only what the facts say, and promise nothing the facts do not.";

/// The answer's cap: a note of two sentences.
pub const MAX_TOKENS: u32 = 200;

/// One message to write: what kind it is, the facts it rests on, and the
/// template that stands in for it.
pub struct Facts<'a> {
    pub kind: &'static str,
    pub brief: &'a str,
    pub line: Line,
    pub n: u64,
    /// The brief holds the owner's Google content: the call is made as a
    /// private session's, so a model of restricted origin is never asked.
    pub private: bool,
}

/// What was said, and what saying it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    pub id: String,
    pub text: String,
    /// A model wrote it (false: its template did).
    pub voiced: bool,
    pub cost_usd: f64,
    pub model: Option<String>,
}

impl Core {
    /// The persona's words for a daemon-written message: its own files'
    /// sections when it has a readable one, else the built-in default.
    pub(crate) fn persona_system(&self, persona: &PersonaView) -> String {
        let Some(key) = persona.key.as_deref() else {
            return crate::turn::PERSONA.to_string();
        };
        let paths: Vec<ContextPath> = persona
            .files
            .iter()
            .map(|f| ContextPath {
                path: f.path().into(),
                persona: Some(key.to_string()),
                public: f.readers() == ContextReaders::Public,
            })
            .collect();
        let files = self.runner.context_files.peek(&paths);
        if !crate::persona::owned(&files) {
            return crate::turn::PERSONA.to_string();
        }
        files
            .iter()
            .filter(|f| f.file.persona.is_some() && f.file.withheld.is_none())
            .map(|f| f.section.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The request that writes `facts` as `persona`: persona and standing in
    /// the system, the task's sentence after them.
    pub(crate) fn initiative_request(
        &self,
        persona: &PersonaView,
        facts: &Facts<'_>,
        model: &str,
    ) -> ProviderRequest {
        let mut system = self.persona_system(persona);
        if let Ok(all) = self.runner.standing.all(&self.store) {
            let scope = Scope {
                session: "",
                place: None,
                surface: None,
                persona: persona.key.as_deref(),
            };
            let now = theseus_protocol::now_unix_ms();
            if let Some(b) = Block::of(&all, scope, PlaceClass::Private, now) {
                system = format!("{system}\n\n{}", b.text);
            }
        }
        system = format!("{system}\n\n{TASK}");
        ProviderRequest {
            model: model.to_string(),
            max_tokens: MAX_TOKENS,
            system: vec![json!({"type": "text", "text": system})],
            messages: vec![json!({"role": "user", "content": [
                {"type": "text", "text": format!("Facts:\n{}", facts.brief)}]})],
            tools: Vec::new(),
            thinking: None,
            output_config: None,
            cache_control: None,
            betas: Vec::new(),
            extra: Default::default(),
            image_tokens: 0,
        }
    }

    /// What the voiced messages reserved and spent today, from their rows.
    pub(crate) fn initiative_spent_today(&self) -> Micros {
        let (_, since, _) = {
            let c = self.kernel.day_ceiling();
            c.day_of(c.now())
        };
        self.synthesis_rows(LedgerKind::InitiativeSpoke, Some(since))
            .unwrap_or_default()
            .iter()
            .filter_map(|r| r.data["cost_usd"].as_f64())
            .map(usd_to_micros)
            .fold(0, Micros::saturating_add)
    }

    /// Write `facts` as the persona in play: by a model within the day's
    /// line, else by the template. Never a turn's path.
    pub async fn initiative_speak(self: &Arc<Self>, facts: &Facts<'_>) -> Spoken {
        let persona = self.cfg.persona_for(None, None);
        let id = crate::new_id("ini");
        let template = |cost: f64, model: Option<String>| Spoken {
            id: id.clone(),
            text: say(facts.line, facts.n, &persona),
            voiced: false,
            cost_usd: cost,
            model,
        };
        let line = usd_to_micros(self.cfg.initiative.day_usd);
        if line == 0 {
            return template(0.0, None);
        }
        let (live, _) = self.live_profile();
        let profile = self.cfg.initiative.profile.clone();
        let Ok(target) = self
            .runner
            .resolve_target(&live, profile.as_deref(), None, None)
        else {
            return template(0.0, None);
        };
        let Some(provider) = self.runner.providers.get(&target.provider).cloned() else {
            return template(0.0, None);
        };
        let request = self.initiative_request(&persona, facts, &target.model);
        let Some(need) = self.audit_reserve(&request) else {
            return template(0.0, None);
        };
        if self.initiative_spent_today().saturating_add(need) > line {
            return template(0.0, None);
        }
        let hold = match self.runner.day_hold(need, "initiative") {
            Ok(h) => h,
            Err(_) => return template(0.0, None),
        };
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = crate::google::private::scoped(facts.private, || {
            provider.stream_message(&request, &mut quiet)
        })
        .await;
        let cost = answered
            .as_ref()
            .map_or(need, |r| self.runner.priced(r, &target.model, need));
        hold.settle(cost);
        match answered {
            Ok(r) if !r.text.trim().is_empty() => Spoken {
                id,
                text: r.text.trim().to_string(),
                voiced: true,
                cost_usd: micros_to_usd(cost),
                model: Some(r.model),
            },
            // A failed call may have been billed: booked at its reservation.
            _ => template(micros_to_usd(cost), Some(target.model)),
        }
    }

    /// Send a message to the owner's DM and write its `initiative.spoke` row
    /// in the same frame. The owner's alone: nothing reaches a shared place.
    pub fn initiative_post(&self, kind: &'static str, s: &Spoken) -> Result<()> {
        let persona = self.cfg.persona_for(None, None);
        let body = json!({"kind": "initiative", "what": kind, "id": s.id, "text": s.text});
        let (a, mut frame) = self.outbox.stage_to_operator(None, body)?;
        frame.push(crate::fact::row(
            &InitiativeSpoke {
                id: &s.id,
                kind,
                persona: &persona.name,
                cost_usd: Some(s.cost_usd),
                model: s.model.as_deref(),
                text: Some(&s.text),
            },
            None,
            None,
        )?);
        self.store.append(&frame)?;
        self.outbox.posted(&a);
        Ok(())
    }
}
