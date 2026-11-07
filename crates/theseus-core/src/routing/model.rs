//! route.v10's `model` path (theseus-h66v4, row 3; design §1.4): what the daemon
//! does with Jev's answers, and nothing beyond them.
//!
//! - **The pick** is the `model` answer at or above its `confirm` bar (`sure`).
//!   Under it nothing routes: the turn stays on its own profile, `unsure`.
//!
//!   **This diverges from the pack, knowingly** (theseus-ig13o). route.v10's
//!   `model` question declares `no_match = "sonnet"`, and the replay grades an
//!   under-bar answer as `sonnet` (`route_cases::check`), so the pack's
//!   measured accuracy is the accuracy of a daemon that routes to sonnet here.
//!   This one does not. Since theseus-ig13o the declared default is carried on
//!   the answer and [`super::Pick::ran`] would apply it in one line; the whole
//!   change was built and measured, and **it moves twelve tests**, among them
//!   `tests_task_brief::a_verdict_under_its_bar_leaves_the_parents_model`,
//!   which is a named rule and not an oversight, and
//!   `tests_data_open::a_judged_clean_session_runs_on_the_restricted_model`,
//!   where a session the data rule has *cleared* for a restricted model would
//!   be pulled off it by an answer Jev was not sure about. Both are the
//!   owner's to decide, so the pack and the live path are left apart and said
//!   to be apart, rather than reconciled by a builder in either direction.
//!   `tests_model::the_model_question_declares_a_default_the_live_path_does_not_run`
//!   holds the divergence so it cannot be closed by accident.
//! - **Which profile** runs a pick: one configured to run the option's catalog
//!   model ([`OPTIONS`]), the profile named for the option first. Jev reaches a
//!   model by its id, never by a profile's name.
//! - **The floor** (the data rule, never loosened): a pick whose profile the
//!   session's labels keep off, or that is of restricted origin for an ask that
//!   is not cleared, runs the turn's own profile, `data_gate`, whatever Jev said.
//!   A pick with no usable profile (no profile for the model, its provider or
//!   key not ready, a model that cannot read the turn's images) is `fallback`.
//! - **The cache hold** (design §1.4 step 7, the one hysteresis, money's and
//!   not a cap): a pick that differs from the session's model, at an
//!   estimate at or above `cold_switch_tokens`, waits one agreeing turn;
//!   the next agreeing pick switches. It defers a switch by one turn and never
//!   refuses a model.
//! - **Every turn compiles the whole working context**, on whatever model
//!   answers it. There is no `context` question and no short compile
//!   (theseus-h66v4.1: the owner's 22:50 answers); what a switch costs is the
//!   state's `switch_cost`, which Jev reads.
//!
//! No place cap, class list, dear-tier guard, step down or switch limit is read.

use super::{Ask, Decision, Hold, HoldNext, Profile, Profiles, Reason};
use crate::config::RoutingConfig;

/// The `model` question's options, and the catalog model each names. A model
/// nowhere best is not here: it is reachable only as the operator's pin.
pub const OPTIONS: [(&str, &str); 5] = [
    ("haiku", "claude-haiku-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("opus", "claude-opus-5-5"),
    ("fable", "claude-fable-5-1"),
    ("deepseek_flash", "deepseek-flash"),
];

/// The options of a pack's `model` question that [`OPTIONS`] does not name: a
/// pack that asks `model` (what makes route.v10 act, `JudgeService::route_pack`,
/// by whatever path the pack reached the daemon) must name only models the
/// daemon can run, or every pick of the unnamed one is a silent `fallback`.
/// Empty for a pack that asks no `model` question.
pub fn unknown_options(pack: &theseus_judge::Pack) -> Vec<String> {
    pack.questions
        .iter()
        .filter(|q| q.id == "model")
        .flat_map(|q| q.options.iter().map(|o| o.id.clone()))
        .filter(|id| model_of(id).is_none())
        .collect()
}

/// The catalog model an option names.
pub fn model_of(option: &str) -> Option<&'static str> {
    OPTIONS.iter().find(|(o, _)| *o == option).map(|(_, m)| *m)
}

/// The configured profile that runs an option's model: a usable one first,
/// then the one named for the option, then the first by name. A model that no
/// profile runs has none.
pub fn profile_for<'p>(option: &str, profiles: &'p Profiles) -> Option<&'p str> {
    let model = model_of(option)?;
    profiles
        .iter()
        .filter(|(_, p)| p.model == model)
        .min_by_key(|(name, p)| (p.unusable.is_some(), name.as_str() != option))
        .map(|(name, _)| name.as_str())
}

/// Why a pick's profile cannot run the turn, if it cannot: the data rule's
/// voice (`DataGate`) before any other.
fn refusal(p: &Profile, a: &Ask<'_>) -> Option<Reason> {
    if (p.restricted && !a.cleared) || p.unusable == Some("data_gate") {
        return Some(Reason::DataGate);
    }
    (p.unusable.is_some() || p.short_cost.is_none() || (a.images && !p.vision))
        .then_some(Reason::Fallback)
}

/// route.v10's switch rule.
pub fn decide(cfg: &RoutingConfig, profiles: &Profiles, a: &Ask<'_>) -> Decision {
    let v = a.verdict;
    let stay = |reason: Reason| Decision {
        profile: a.base.to_string(),
        reason,
        switch: false,
        hold: HoldNext::Clear,
    };
    let Some(m) = v.model.as_ref() else {
        // No `model` answer (a pin, or Jev left it): the turn's own profile.
        return stay(Reason::NoVerdict);
    };
    if !m.sure {
        return stay(Reason::Unsure);
    }
    let Some(name) = profile_for(&m.choice, profiles) else {
        return stay(Reason::Fallback);
    };
    if let Some(why) = refusal(&profiles[name], a) {
        return stay(why);
    }
    if name == a.base {
        return stay(Reason::Verdict);
    }
    let agreed = a.hold.is_some_and(|h| h.profile == name);
    if a.est_tokens < cfg.cold_switch_tokens || agreed {
        return Decision {
            profile: name.to_string(),
            reason: Reason::Verdict,
            switch: true,
            hold: HoldNext::Clear,
        };
    }
    Decision {
        profile: a.base.to_string(),
        reason: Reason::CacheHold,
        switch: false,
        hold: HoldNext::Set(Hold {
            mode: m.choice.clone(),
            profile: name.to_string(),
            turn: v.turn.clone(),
        }),
    }
}
