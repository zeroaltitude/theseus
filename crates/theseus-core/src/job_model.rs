//! A background job's model (route.v10 row 4, theseus-ej6bj; design §2 rows 6
//! to 9 and 13). One rule at every site that is not a turn:
//!
//! - **A configured profile key is a pin**: the job runs on it, and Jev is
//!   asked nothing about the model.
//! - **An unset key asks Jev** (`JudgeService::job_ask`), through the job
//!   builder, waiting at most `[routing] max_wait_ms`. A pick the data floor
//!   refuses, a provider with no key, no answer in time, or an answer under
//!   the pack's confirm bar runs on `live`, the profile the caller names (a
//!   job has no conversation to keep: `[model] live`).
//! - **Legacy**: while the acting route pack asks no `model` question (every
//!   pack before route.v10), an unset key still runs on the profile it always
//!   did (`config::legacy`), so nothing changes a live decision until the row
//!   that flips the pack.
//!
//! The data rule is never loosened here. A job over the owner's material is
//! told `restricted_models = not_allowed` by the job builder whatever the
//! caller says, a pick of restricted origin is refused for it, and private
//! material goes only to a model the catalog lists as of US origin
//! (`cleared_for_sensitive`); the provider chokepoint stays under all of it.

use std::fmt;
use std::time::Duration;

use theseus_judge::builders::{JobKind, RestrictedModels};

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
    /// The profile an unset key ran on before Jev chose (`config::legacy`);
    /// none: the live profile.
    pub legacy: Option<&'static str>,
    /// The work, as the job builder says it to Jev.
    pub job: JobKind,
    /// What the session's labels allow (`allowed` only where a label read said
    /// so); the job's own owner-material floor is applied by the builder.
    pub restricted: RestrictedModels,
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
    /// The pick is of restricted origin and the job's material may not go there.
    DataGate,
    /// The pick's provider is not configured or has no key.
    Fallback,
}

impl Why {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Why::Late => "late",
            Why::Unsure => "unsure",
            Why::Unasked => "unreachable",
            Why::DataGate => "data_gate",
            Why::Fallback => "fallback",
        }
    }
}

/// How the model was settled, for the log and the tests.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum How {
    /// A configured key.
    Pinned,
    /// An unset key, and the acting pack asks no `model`: the profile it always ran on.
    Legacy,
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
    /// The origin of `model` served by `provider`, by the provider's endpoint,
    /// what it vouches for and the catalog's row (`catalog::origin_of`); a
    /// provider that is not configured is `unknown`.
    pub(crate) fn model_origin(&self, provider: &str, model: &str) -> crate::catalog::ModelOrigin {
        match self.cfg.all_providers().get(provider) {
            Some(p) => {
                crate::catalog::origin_of(&p.api_base, p.origin, model, self.catalog.get(model))
            }
            None => crate::catalog::ModelOrigin::Unknown,
        }
    }

    /// Whether private data may go to `model` on `provider`
    /// (`catalog::cleared_for_sensitive`); a provider that is not configured
    /// is not cleared.
    pub(crate) fn model_cleared(&self, provider: &str, model: &str) -> bool {
        self.cfg.all_providers().get(provider).is_some_and(|p| {
            crate::catalog::cleared_for_sensitive(
                &p.api_base,
                p.origin,
                model,
                self.catalog.get(model),
            )
        })
    }

    /// Whether `model` on `provider` is of restricted origin (`cn` or `unknown`).
    pub(crate) fn restricted_model(&self, provider: &str, model: &str) -> bool {
        self.model_origin(provider, model).restricted()
    }

    /// Settle the site's model: the pin, else Jev's pick, else `live`. Never
    /// waits on Jev past `[routing] max_wait_ms`.
    pub(crate) async fn job_choice(&self, live: &str, site: Site<'_>) -> (Choice, How) {
        if let Some(p) = site.pin {
            return (Choice::Profile(p.to_string()), How::Pinned);
        }
        if !self.judge.route_open() {
            let profile = site.legacy.unwrap_or(live);
            return (Choice::Profile(profile.to_string()), How::Legacy);
        }
        let (private, name) = (site.job.private(), site.name);
        let on_live = |why: Why| {
            tracing::debug!(
                site = name,
                why = why.word(),
                "a background job runs on live"
            );
            (Choice::Profile(live.to_string()), How::Live(why))
        };
        let restricted = if private {
            RestrictedModels::NotAllowed
        } else {
            site.restricted
        };
        let mode = self
            .cfg
            .routing
            .pack_mode(self.judge.mode_for(ROUTE_PACK, "").mode);
        let wait = Duration::from_millis(self.cfg.routing.max_wait_ms);
        let answer = self.judge.job_ask(site.job, restricted, mode, wait).await;
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
        // The data rule, for the pick as for any model: restricted origin only
        // where the labels allow it, and private material only to a model the
        // catalog lists as of US origin.
        let refused = (restricted == RestrictedModels::NotAllowed
            && self.restricted_model(&provider, model))
            || (private && !self.model_cleared(&provider, model));
        if refused {
            self.judge.record_data_gate(name, "restricted_origin");
            return on_live(Why::DataGate);
        }
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
