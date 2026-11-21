//! A background job's model (route.v10 row 4, theseus-ej6bj; design §2 rows 6
//! to 9 and 13). One rule at every site that is not a turn:
//!
//! - **A configured profile key is a pin**: the job runs on it, and Jev is
//!   asked nothing about the model.
//! - **An unset key asks Jev** (`JudgeService::job_ask`), through the job
//!   builder, waiting at most `[routing] max_wait_ms`. A provider with no key,
//!   no answer in time, or an answer under the pack's confirm bar runs on
//!   `live`, the profile the caller names (a job has no conversation to keep:
//!   `[model] live`).
//!
//! No pick is refused for its model's origin: the rule that kept the owner's
//! material off a model not of US origin was deleted on 2026-11-21.

use std::fmt;
use std::time::Duration;

use theseus_judge::builders::JobKind;

use crate::judge::inbound::ROUTE_PACK;
use crate::judge::job::JobAnswer;
// The one option-to-model table (`routing::model::OPTIONS`), swept over every embedded pack that
// asks `model` by `model::unknown_options`: a second copy here could drift from it silently.
use crate::routing::model::model_of;
use crate::turn::{Target, TurnRunner};

/// One site's ask.
pub(crate) struct Site<'a> {
    /// The site's word in a log line (`card`, `consolidation`).
    pub name: &'static str,
    /// The configured key: a pin when set.
    pub pin: Option<&'a str>,
    /// The work, as the job builder says it to Jev.
    pub job: JobKind,
}

/// What a site's model is, before it is resolved to a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Choice {
    /// A key of `[profiles]`.
    Profile(String),
    /// A model Jev named, by provider and catalog id.
    Model { provider: String, model: String },
}

/// Why an unpinned job ran on `live`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// No answer in `max_wait_ms` (or the call failed).
    Late,
    /// The answer was under the pack's confirm bar.
    Unsure,
    /// Jev could not be asked: routing or the judge is off, the breaker is
    /// open, or the key is not there.
    Unasked,
    /// The pick's provider is not configured or has no key.
    Fallback,
}

impl Why {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Why::Late => "late",
            Why::Unsure => "unsure",
            Why::Unasked => "unreachable",
            Why::Fallback => "fallback",
        }
    }
}

/// How the model was settled, for the log and the tests.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum How {
    /// A configured key.
    Pinned,
    /// Jev's pick: the option, and how sure.
    Jev { option: String, confidence: f64 },
    /// Asked and not used: the profile the caller named as `live`.
    Live(Why),
}

/// A profile that does not resolve.
#[derive(Debug)]
pub(crate) struct Unresolved {
    pub profile: String,
    pub why: String,
}

impl fmt::Display for Unresolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} does not resolve: {}", self.profile, self.why)
    }
}

impl std::error::Error for Unresolved {}

impl TurnRunner {
    /// Settle the site's model: the pin, else Jev's pick, else `live`. Never
    /// waits on Jev past `[routing] max_wait_ms`.
    pub(crate) async fn job_choice(&self, live: &str, site: Site<'_>) -> (Choice, How) {
        if let Some(p) = site.pin {
            return (Choice::Profile(p.to_string()), How::Pinned);
        }
        let name = site.name;
        let on_live = |why: Why| {
            tracing::debug!(
                site = name,
                why = why.word(),
                "a background job runs on live"
            );
            (Choice::Profile(live.to_string()), How::Live(why))
        };
        let mode = self
            .cfg
            .routing
            .pack_mode(self.judge.mode_for(ROUTE_PACK, "").mode);
        let wait = Duration::from_millis(self.cfg.routing.max_wait_ms);
        let answer = self.judge.job_ask(site.job, mode, wait).await;
        let (option, confidence) = match answer {
            JobAnswer::Picked { option, confidence } => (option, confidence),
            JobAnswer::Unsure => return on_live(Why::Unsure),
            JobAnswer::Late => return on_live(Why::Late),
            JobAnswer::Unasked => return on_live(Why::Unasked),
        };
        let Some(model) = model_of(&option) else {
            return on_live(Why::Unsure);
        };
        let Some(provider) = self.catalog.get(model).map(|e| e.provider.clone()) else {
            return on_live(Why::Fallback);
        };
        if !self.providers.contains_key(&provider)
            || self.cfg.provider_without_key(&provider).is_some()
        {
            return on_live(Why::Fallback);
        }
        tracing::debug!(
            site = name,
            option,
            confidence,
            model,
            "a background job's model: Jev's pick"
        );
        (
            Choice::Model {
                provider,
                model: model.to_string(),
            },
            How::Jev { option, confidence },
        )
    }

    /// [`Self::job_choice`], resolved to a target on `live`'s settings.
    pub(crate) async fn job_target(
        &self,
        live: &str,
        site: Site<'_>,
    ) -> Result<(Target, How), Unresolved> {
        let (choice, how) = self.job_choice(live, site).await;
        self.choice_target(live, &choice).map(|t| (t, how))
    }

    /// A choice as a target.
    pub(crate) fn choice_target(&self, live: &str, choice: &Choice) -> Result<Target, Unresolved> {
        let (profile, resolved) = match choice {
            Choice::Profile(p) => (p.as_str(), self.resolve_target(live, Some(p), None, None)),
            Choice::Model { provider, model } => (
                model.as_str(),
                self.resolve_target(live, None, Some(provider), Some(model)),
            ),
        };
        resolved.map_err(|e| Unresolved {
            profile: profile.to_string(),
            why: format!("{e:#}"),
        })
    }
}
