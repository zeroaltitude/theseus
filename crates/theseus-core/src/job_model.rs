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
//! STUB (red commit): pins and the legacy profile only; Jev is never asked.

use std::fmt;

use theseus_judge::builders::{JobKind, RestrictedModels};

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
    /// Jev could not be asked: routing or the judge is off, the breaker is open, or the key is not there.
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

    /// Whether `model` on `provider` is of restricted origin (`cn` or `unknown`).
    pub(crate) fn restricted_model(&self, provider: &str, model: &str) -> bool {
        self.model_origin(provider, model).restricted()
    }

    /// Settle the site's model: the pin, else Jev's pick, else `live`.
    pub(crate) async fn job_choice(&self, live: &str, site: Site<'_>) -> (Choice, How) {
        let _ = (site.name, &site.job, site.restricted);
        match site.pin {
            Some(p) => (Choice::Profile(p.to_string()), How::Pinned),
            None => (
                Choice::Profile(site.legacy.unwrap_or(live).to_string()),
                How::Legacy,
            ),
        }
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
