//! What is left of the route step's data floor (theseus-oejh3, theseus-9ymz0):
//! the slots a turn may move to, whether a profile resolves, and the words for
//! where a turn ran.
//!
//! **No profile is kept off a session for its model's origin.** The floor
//! that kept a turn off a model of restricted origin (`cn` or `unknown`) when
//! its session carried a `secret`, `confidential` or `personal` label, and
//! the wait of up to `[content_based_security] wait_ms` before such a turn,
//! were deleted on the owner's word of 2026-11-21 ("more and more skeptical
//! of any rules about different model providers"): such a model is sent
//! whatever its session holds.

use super::*;

/// The targets a turn may move to, each set once: routing's, a refusal's
/// fallback, and its checkpoints'.
pub(super) type Slots = (
    OnceLock<Target>,
    OnceLock<Target>,
    super::checkpoint_step::Slots,
);

pub(super) fn slots() -> Slots {
    (
        OnceLock::new(),
        OnceLock::new(),
        super::checkpoint_step::slots(),
    )
}

impl TurnRunner {
    /// Why a session cannot run on `profile`, if it cannot: the profile does
    /// not resolve. The check `session.profile` makes and a model menu greys
    /// out by.
    pub(crate) fn profile_refusal(&self, profile: &str) -> Option<String> {
        (!self.cfg.all_profiles().contains_key(profile))
            .then(|| format!("`{profile}` is not a configured profile"))
    }

    /// The provider, model and origin word of `profile`, for the rows that name
    /// where a turn ran: a profile that does not resolve has no provider or
    /// model and origin `unknown`.
    pub(super) fn ran_of(&self, profile: &str) -> fact::route::Ran<'_> {
        let p = self.cfg.all_profiles().get(profile);
        fact::route::Ran {
            provider: p.map(|p| p.provider.as_str()),
            model: p.map(|p| p.model.as_str()),
            origin: crate::catalog::origin_word(self.cfg.profile_origin(profile, &self.catalog)),
        }
    }
}
