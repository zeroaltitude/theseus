//! route.v10's `model` path (theseus-h66v4, row 3; design §1.4): what the daemon
//! does with Jev's answers, and nothing beyond them.
//!
//! - **The pick** is the `model` answer at or above its `confirm` bar (`sure`).
//!   Under it **the question's own declared default runs** ([`super::Pick::ran`],
//!   carried off the pack that was asked, theseus-ig13o), and route.v10
//!   declares [`PARENT`]: the turn stays on the model it is already on,
//!   `unsure`. **So the pack's table and this path are one rule, read from the
//!   pack** — the owner, 2026-11-07 16:46, asked for one word, "Parent", and
//!   that word is the pack's.
//!
//!   Until then the pack declared `sonnet` while this path stood on the turn's
//!   own profile, and the replay graded an under-bar answer as `sonnet`
//!   (`route_cases::check`), so the pack's measured accuracy described a
//!   daemon that routed to Sonnet here and no such daemon existed. Honouring
//!   `sonnet` instead was built and measured and **moved twelve tests**, among
//!   them `tests_task_brief::a_verdict_under_its_bar_leaves_the_parents_model`
//!   and `tests_data_open::a_judged_clean_session_runs_on_the_restricted_model`
//!   (a session the data rule has *cleared* for a restricted model, pulled off
//!   it by an answer Jev was not sure about). Changing the declaration, not the
//!   act, is what the owner chose: all twelve stand.
//!   `tests_model::the_declared_default_is_what_the_live_path_runs` holds the
//!   two together — a planted default of a real model routes there, so the
//!   value is read and not merely agreed with.
//! - **Which profile** runs a pick: one configured to run the option's catalog
//!   model ([`OPTIONS`]), the profile named for the option first. Jev reaches a
//!   model by its id, never by a profile's name.
//! - **The floor** (the data rule, never loosened): a pick whose profile the
//!   session's labels keep off, or that is of restricted origin for an ask that
//!   is not cleared, runs the turn's own profile, `data_gate`, whatever Jev said.
//!   A pick with no usable profile (no profile for the model, its provider or
//!   key not ready, a model that cannot read the turn's images) is `fallback`.
//! - **The cache hold** (design §1.4 step 7, the one hysteresis, money's and
//!   not a cap): a pick that differs from the session's model, with the session
//!   carrying `cold_switch_tokens` or more of its own context, waits one
//!   agreeing turn; the next agreeing pick switches. It defers a switch by one
//!   turn and never refuses a model.
//!
//!   **What it weighs is the session's context, never the request's size**
//!   (theseus-ataas, 2026-11-07). [`Ask::context_tokens`] is the compile's
//!   estimate less the request's fixed prefix — the tools and the system
//!   blocks, which a session carries identically on its first message and its
//!   hundredth, and which whichever model answers writes cold either way. That
//!   prefix is most of an ordinary request — `tests_route_cliff` measures it
//!   and holds it so — so weighing the whole estimate left an ordinary first
//!   message four tokens under the line (29,996 of 30,000 on the rig the day
//!   this was written, which is in that test's doc): eighteen more
//!   characters in the model-facing tools note moved one turn from Opus to
//!   Sonnet, and the next release's prompt growth would have moved others. The
//!   hold protects what a conversation has accumulated; a prefix that is there
//!   before the conversation starts is no reason to defer a switch, so it is
//!   not counted.
//! - **Every turn compiles the whole working context**, on whatever model
//!   answers it. There is no `context` question and no short compile
//!   (theseus-h66v4.1: the owner's 22:50 answers); what a switch costs is the
//!   state's `switch_cost`, which Jev reads.
//!
//! No place cap, class list, dear-tier guard, step down or switch limit is read.

use super::{Ask, Decision, Hold, HoldNext, Profile, Profiles, Reason};
use crate::config::RoutingConfig;

/// The one `model` option that names no model: the turn keeps the model it is
/// already on (a session's own profile; for a task, its parent's). route.v10
/// declares it as the question's `no_match`, so an answer under the bar runs
/// it, and Jev may also name it when no listed model is the answer. It is not
/// in [`OPTIONS`] because there is no catalog model and no profile to resolve:
/// the decision is to stay, and [`decide`] takes it before any lookup.
pub const PARENT: &str = "parent";

/// The `model` question's options, and the catalog model each names. A model
/// nowhere best is not here: it is reachable only as the operator's pin.
/// [`PARENT`] is not here either, for the opposite reason: it names no model.
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
/// Empty for a pack that asks no `model` question. [`PARENT`] is not one of
/// them: the daemon runs it, by staying.
pub fn unknown_options(pack: &theseus_judge::Pack) -> Vec<String> {
    pack.questions
        .iter()
        .filter(|q| q.id == "model")
        .flat_map(|q| q.options.iter().map(|o| o.id.clone()))
        .filter(|id| id != PARENT && model_of(id).is_none())
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
    // What the answer runs: Jev's own option at or above the `confirm` bar,
    // else the question's declared default, carried off the pack that was
    // asked. A Choice always declares one (the loader refuses a pack whose
    // does not), so `None` here is only a judgment written before
    // theseus-ig13o carried the value: the turn's own profile, as then.
    let Some(choice) = m.ran() else {
        return stay(Reason::Unsure);
    };
    if choice == PARENT {
        // The answer is to stay: no model is named, so there is nothing to
        // look up, nothing to refuse and no hold to set. `unsure` where the
        // bar chose it, `verdict` where Jev did.
        return stay(match m.sure {
            true => Reason::Verdict,
            false => Reason::Unsure,
        });
    }
    let Some(name) = profile_for(choice, profiles) else {
        return stay(Reason::Fallback);
    };
    if let Some(why) = refusal(&profiles[name], a) {
        return stay(why);
    }
    if name == a.base {
        return stay(Reason::Verdict);
    }
    let agreed = a.hold.is_some_and(|h| h.profile == name);
    if a.context_tokens < cfg.cold_switch_tokens || agreed {
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
            mode: choice.to_string(),
            profile: name.to_string(),
            turn: v.turn.clone(),
        }),
    }
}
