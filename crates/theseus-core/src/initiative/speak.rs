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
//!
//! A cut-off answer is never posted (theseus-050g2): at a 200-token cap,
//! Sonnet 5.5's default thinking spent up to all of it and the morning note
//! went out as half a sentence. The call asks with thinking off where the
//! model lets it (`catalog::thinking_off`) at [`MAX_TOKENS`]; an answer that
//! stops on `max_tokens` is asked once more at [`RETRY_MAX_TOKENS`], within
//! the day's line, and past that the message is its template. The row
//! records the `stop_reason` and output tokens of the newest call that
//! answered (the cut first call, when the retry was refused or failed).

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
use crate::provider::{ModelResponse, ProviderRequest};
use crate::rpc::Core;
use crate::standing::{Block, Scope};

/// What the model is told to do, as one factual sentence.
pub const TASK: &str = "Write the one short message below to the owner, in your own voice, in \
two sentences at most. State only what the facts say, and promise nothing the facts do not.";

/// The answer's cap: a note of two sentences, with room for a model that
/// thinks anyway (theseus-050g2).
pub const MAX_TOKENS: u32 = 1024;

/// The one retry's cap, after an answer that stopped on `max_tokens`.
pub const RETRY_MAX_TOKENS: u32 = 2048;

/// One message to write: what kind it is, the facts it rests on, and the
/// template that stands in for it.
pub struct Facts<'a> {
    pub kind: &'static str,
    pub brief: &'a str,
    pub line: Line,
    pub n: u64,
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
    /// The newest answering call's stop and output tokens (the first, cut
    /// call when the retry was refused or failed); none when no call was made.
    pub stop_reason: Option<String>,
    pub output_tokens: Option<u64>,
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
    /// the system, the task's sentence after them, thinking off where the
    /// model lets it, at `max_tokens`.
    pub(crate) fn initiative_request(
        &self,
        persona: &PersonaView,
        facts: &Facts<'_>,
        model: &str,
        max_tokens: u32,
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
            max_tokens,
            system: vec![json!({"type": "text", "text": system})],
            messages: vec![json!({"role": "user", "content": [
                {"type": "text", "text": format!("Facts:\n{}", facts.brief)}]})],
            tools: Vec::new(),
            thinking: self.runner.catalog.thinking_off(model),
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

    /// The model a note of `kind` is written on: `[initiative] profile` when
    /// it is set (a pin), else Jev's choice for the note (route.v10,
    /// theseus-ej6bj), and the live profile when Jev cannot be used. A note is
    /// written from the owner's own calendar and mail: US options only.
    pub(crate) async fn initiative_target(
        &self,
        kind: &str,
    ) -> Result<crate::turn::Target, crate::job_model::Unresolved> {
        let (live, _) = self.live_profile();
        let site = crate::job_model::Site {
            name: "initiative",
            pin: self.cfg.initiative.profile.as_deref(),
            job: theseus_judge::builders::JobKind::Initiative { kind: kind.into() },
            restricted: theseus_judge::builders::RestrictedModels::NotAllowed,
        };
        self.runner.job_target(&live, site).await.map(|(t, _)| t)
    }

    /// Write `facts` as the persona in play: by a model within the day's
    /// line, else by the template. Never a turn's path, and never a cut-off
    /// answer: one stopped on `max_tokens` is asked once more with more room,
    /// then the template stands in.
    pub async fn initiative_speak(self: &Arc<Self>, facts: &Facts<'_>) -> Spoken {
        let persona = self.personas().persona_for(None, None);
        let id = crate::new_id("ini");
        let template = |cost: Micros, model: Option<String>, last: Option<&ModelResponse>| Spoken {
            id: id.clone(),
            text: say(facts.line, facts.n, &persona),
            voiced: false,
            cost_usd: micros_to_usd(cost),
            model,
            stop_reason: last.and_then(|r| r.stop_reason.clone()),
            output_tokens: last.map(|r| r.usage.output_tokens),
        };
        let line = usd_to_micros(self.cfg.initiative.day_usd);
        if line == 0 {
            return template(0, None, None);
        }
        let Ok(target) = self.initiative_target(facts.kind).await else {
            return template(0, None, None);
        };
        let Some(provider) = self.runner.providers.get(&target.provider).cloned() else {
            return template(0, None, None);
        };
        let mut spent: Micros = 0;
        let mut cut: Option<ModelResponse> = None;
        for cap in [MAX_TOKENS, RETRY_MAX_TOKENS] {
            let request = self.initiative_request(&persona, facts, &target.model, cap);
            let Some(need) = self.audit_reserve(&request) else {
                break;
            };
            let today = self.initiative_spent_today().saturating_add(spent);
            if today.saturating_add(need) > line {
                break;
            }
            let Ok(hold) = self.runner.day_hold(need, "initiative") else {
                break;
            };
            let started_ms = theseus_protocol::now_unix_ms();
            let mut quiet = |_: crate::provider::Delta<'_>| {};
            let answered = provider.stream_message(&request, &mut quiet).await;
            let cost = answered.as_ref().map_or(need, |r| {
                self.runner.priced(r, &target.model, need, started_ms)
            });
            hold.settle(cost);
            spent = spent.saturating_add(cost);
            // A failed call may have been billed: booked at its reservation.
            let Ok(r) = answered else {
                return template(spent, Some(target.model), cut.as_ref());
            };
            if crate::provider::hit_cap(r.stop_reason.as_deref()) {
                tracing::warn!(
                    kind = facts.kind,
                    cap,
                    output_tokens = r.usage.output_tokens,
                    "an initiative answer stopped on max_tokens; not posted"
                );
                cut = Some(r);
                continue;
            }
            let finished = matches!(
                r.stop_reason.as_deref(),
                None | Some("end_turn" | "stop_sequence")
            );
            if !finished || r.text.trim().is_empty() {
                return template(spent, Some(r.model.clone()), Some(&r));
            }
            return Spoken {
                id: id.clone(),
                text: r.text.trim().to_string(),
                voiced: true,
                cost_usd: micros_to_usd(spent),
                model: Some(r.model),
                stop_reason: r.stop_reason,
                output_tokens: Some(r.usage.output_tokens),
            };
        }
        match cut {
            Some(r) => template(spent, Some(r.model.clone()), Some(&r)),
            None => template(spent, None, None),
        }
    }

    /// Send a message to the owner's DM and write its `initiative.spoke` row
    /// in the same frame. The owner's alone: nothing reaches a shared place.
    pub fn initiative_post(&self, kind: &'static str, s: &Spoken) -> Result<()> {
        self.initiative_post_as(kind, s, false)
    }

    /// [`Core::initiative_post`], its row holding no text when `text_held`: a
    /// note drawn from a sensitive calendar reaches the owner's DM and no
    /// ledger row (theseus-jid7t.1).
    pub fn initiative_post_as(
        &self,
        kind: &'static str,
        s: &Spoken,
        text_held: bool,
    ) -> Result<()> {
        let persona = self.personas().persona_for(None, None);
        let body =
            json!({"kind": "initiative", "what": kind, "id": s.id, "text": headed(kind, &s.text)});
        let (a, mut frame) = self.outbox.stage_to_operator(None, body)?;
        frame.push(crate::fact::row(
            &InitiativeSpoke {
                id: &s.id,
                kind,
                persona: &persona.name,
                cost_usd: Some(s.cost_usd),
                model: s.model.as_deref(),
                text: (!text_held).then_some(s.text.as_str()),
                stop_reason: s.stop_reason.as_deref(),
                output_tokens: s.output_tokens,
            },
            None,
            None,
        )?);
        self.store.append(&frame)?;
        self.outbox.posted(&a);
        Ok(())
    }
}

/// A spoken message under its head (theseus-x8b9k): the words stay the
/// persona's; the head says what kind of note it is and what brought it.
pub fn headed(kind: &str, text: &str) -> String {
    let (title, trigger) = match kind {
        "morning" => (
            "Morning note",
            "what happened in the last day, told once each morning",
        ),
        "consolidation" => (
            "Something the night's consolidation noticed",
            "from the nightly pass over your memory",
        ),
        _ => ("A note from Theseus", "said on its own initiative"),
    };
    crate::owner_note::note(crate::owner_note::Kind::Fyi, title, trigger, text)
}
