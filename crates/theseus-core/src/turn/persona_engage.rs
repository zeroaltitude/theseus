//! Which persona answers a turn (theseus-c1uz; the rules are `crate::engage`).
//!
//! With two or more `[personas]` every persona's files are in the cached
//! persona layer and the header is the built-in default, so a switch changes
//! no byte of the prefix: the engaged persona is one line in the request's
//! tail, after the cached blocks (`persona_line`), like the owner's standing
//! records for the surface. Outside that, nothing here reads or writes.
//!
//! - **Start** (`engage_start`, at the inbound point): the choices the place
//!   allows, the prior (who answered the session's last turn, else its own
//!   persona), and an explicit address in the message. An address, or a turn
//!   routing does not wait on, is decided there; otherwise the verdict decides
//!   (`engage_verdict`, beside the route decision).
//! - **Late**: a verdict the wait missed, or carried from the last message,
//!   changes no persona; the prior stands.

use super::{Turn, TurnRunner};
use crate::compiler::Compiled;
use crate::engage::{self, Choice, How};
use crate::fact::persona::PersonaEngaged;
use crate::judge::inbound::{PersonaCtx, ThreadHead};
use crate::places::PlaceClass;
use crate::routing::Verdict;
use crate::session::SessionRecord;
use theseus_judge::builders::PersonaInput;
use theseus_protocol::route::RoutePersona;

/// What the inbound point worked out for the turn's persona.
pub(super) struct Engage {
    pub(super) options: Vec<Choice>,
    pub(super) prior: Choice,
    pub(super) explicit: Option<Choice>,
    pub(super) message: String,
}

impl TurnRunner {
    /// The personas a place of `class` may be answered as: two or more, or
    /// none (a single persona is the session's, and nothing is engaged).
    pub(crate) fn persona_choices(&self, class: PlaceClass, session_id: &str) -> Vec<Choice> {
        if self.cfg.personas.len() < 2 {
            return Vec::new();
        }
        let own = self.place_persona(session_id);
        let shared = class == PlaceClass::Shared;
        let all = engage::choices(&self.cfg.personas, shared, own.as_deref());
        match all.len() >= 2 {
            true => all,
            false => Vec::new(),
        }
    }

    /// route.v5's inputs: the personas that could answer (with a card), the
    /// one that answered last, and the thread the session serves, if any. Memory
    /// only; empty with fewer than two personas, so a plain turn reads nothing.
    pub(crate) fn persona_ctx(&self, session_id: &str, class: PlaceClass) -> PersonaCtx {
        let options = self.persona_choices(class, session_id);
        if options.is_empty() {
            return PersonaCtx::default();
        }
        let prior = self.persona_prior(session_id, &options);
        let thread = self
            .threads
            .held()
            .and_then(|b| b.of_session(session_id).cloned())
            .map(|t| ThreadHead {
                title: t.title,
                goal: t.goal,
                answered: self.persona.engaged.recent(session_id),
            });
        PersonaCtx {
            personas: options
                .iter()
                .filter_map(|o| {
                    Some(PersonaInput {
                        id: o.key.clone()?,
                        name: o.name.clone(),
                        card: o.card.clone()?,
                    })
                })
                .collect(),
            speaker: prior.key,
            thread,
        }
    }

    /// The persona that answered the session's last turn, else its own.
    fn persona_prior(&self, session_id: &str, options: &[Choice]) -> Choice {
        let key = match self.persona.engaged.last(session_id) {
            Some(k) => k,
            None => self
                .store
                .get_session::<SessionRecord>(session_id)
                .ok()
                .flatten()
                .and_then(|s| self.persona_of(&s)),
        };
        let built_in = || Choice {
            key: None,
            name: crate::config::DEFAULT_NAME.into(),
            card: None,
        };
        match key {
            Some(k) => options
                .iter()
                .find(|o| o.key.as_deref() == Some(k.as_str()))
                .cloned()
                .unwrap_or_else(built_in),
            None => built_in(),
        }
    }

    /// The inbound point's part: see the module's header. `asked`: routing
    /// waits for a verdict, which may decide.
    pub(super) fn engage_start(&self, t: &mut Turn<'_>, text: &str, asked: bool) {
        let options = self.persona_choices(t.tc.class, t.tc.session_id);
        if options.is_empty() {
            return;
        }
        let prior = self.persona_prior(t.tc.session_id, &options);
        // The built-in default answers to its name too.
        let mut addressable = options.clone();
        addressable.push(Choice {
            key: None,
            name: crate::config::DEFAULT_NAME.into(),
            card: None,
        });
        let explicit = engage::address(text, &addressable).map(|(c, _)| c);
        let decided = explicit.is_some() || t.route.persona_steer.is_some() || !asked;
        t.route.engage = Some(Engage {
            options,
            prior,
            explicit,
            message: text.to_string(),
        });
        if decided {
            self.engage_decide(t, None, None);
        }
    }

    /// The route decision's part: Jev's answer, from this turn's own verdict
    /// only (a carried one is the last message's).
    pub(super) fn engage_verdict(&self, t: &mut Turn<'_>, v: Option<&Verdict>) {
        if t.route.engage.is_none() || t.route.persona.is_some() {
            return;
        }
        let own = v.filter(|v| v.turn == t.tc.turn_id);
        self.engage_decide(t, own.and_then(|v| v.persona.as_deref()), own);
    }

    fn engage_decide(
        &self,
        t: &mut Turn<'_>,
        answer: Option<&engage::PersonaAnswer>,
        v: Option<&Verdict>,
    ) {
        let Some(e) = t.route.engage.as_ref() else {
            return;
        };
        let acts = matches!(
            self.cfg.threads.persona_engage,
            crate::config::PersonaEngage::Act
        );
        let steered = t.route.persona_steer.as_ref().and_then(|s| {
            let crate::correction::Steer::Persona(k) = &s.steer else {
                return None;
            };
            let c = e
                .options
                .iter()
                .find(|o| o.key.as_deref() == Some(k.as_str()))?;
            let how = match s.by {
                crate::correction::By::Owner => How::Corrected,
                crate::correction::By::Layer => How::Learned,
            };
            Some((c, how))
        });
        let engaged = engage::decide(
            &e.prior,
            &e.options,
            e.explicit.as_ref(),
            steered,
            answer,
            acts,
            &e.message,
        );
        let runner = engaged.runner_up.as_ref().map(|(o, p)| (o.as_str(), *p));
        t.record(&PersonaEngaged {
            turn: t.tc.turn_id,
            persona: engaged.key.as_deref(),
            name: &engaged.name,
            how: engaged.how.as_str(),
            p: engaged.p,
            runner_up: runner,
            terms: &engaged.terms,
            judgment: v.map(|v| v.judgment.as_str()),
            jev: engaged.jev.as_deref(),
        });
        self.persona
            .engaged
            .note(t.tc.session_id, t.tc.turn_id, engaged.key.clone());
        let shown = RoutePersona {
            key: engaged.key.clone(),
            name: engaged.name.clone(),
            how: engaged.how.as_str().into(),
            why: match engaged.shown() {
                true => engaged.why(),
                false => String::new(),
            },
        };
        t.route.result.get_or_insert_with(Default::default).persona = Some(shown);
        t.route.persona = Some(engaged);
    }

    /// The engaged persona's line, in the request's tail after the cached
    /// prefix, counted in the estimate. Once per request: a request that
    /// carries it already is left alone. Like the tail's other blocks it is
    /// outside the request digest the compile took.
    pub(super) fn persona_line(t: &Turn<'_>, compiled: &mut Compiled) {
        let Some(p) = &t.route.persona else { return };
        let line = p.line();
        let has = compiled
            .request
            .messages
            .last()
            .and_then(|m| m["content"].as_array())
            .is_some_and(|b| b.iter().any(|b| b["text"] == line.as_str()));
        if has {
            return;
        }
        crate::task_graph::view::attach_text(&mut compiled.request, &line);
        let n = (line.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
        compiled.est_tokens += n;
        compiled.estimate.tokens += n;
        compiled.estimate.estimated += n;
        compiled.estimate.upper += n + n * crate::compiler::MARGIN_PERCENT / 100;
    }
}
