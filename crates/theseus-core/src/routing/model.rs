//! route.v10's `model` path (theseus-h66v4, row 3; design §1.4): what the daemon
//! does with Jev's answers, and nothing beyond them.
//!
//! - **The pick** is the `model` answer at or above its `confirm` bar (`sure`).
//!   Under it nothing routes: the turn stays on its own profile, `unsure`.
//! - **Which profile** runs a pick: one configured to run the option's catalog
//!   model ([`OPTIONS`]), the profile named for the option first. Jev reaches a
//!   model by its id, never by a profile's name.
//! - **The floor** (the data rule, never loosened): a pick whose profile the
//!   session's labels keep off, or that is of restricted origin for an ask that
//!   is not cleared, runs the turn's own profile, `data_gate`, whatever Jev said.
//!   A pick with no usable profile (no profile for the model, its provider or
//!   key not ready, a model that cannot read the turn's images) is `fallback`.
//! - **The cache hold** (design §1.4 step 7, the one hysteresis, money's and
//!   not a cap): a `full` pick that differs from the session's model, at an
//!   estimate at or above `cold_switch_tokens`, waits one agreeing turn;
//!   the next agreeing pick switches. It defers a switch by one turn and never
//!   refuses a model.
//! - **`short`** is the `context` answer: the message alone is answered from the
//!   persona, the last exchanges and the recall, on the profile the pick (or the
//!   turn's own) names. Never a switch of the session, never held.
//!
//! No place cap, class list, dear-tier guard, step down or switch limit is read.

use super::{Ask, Decision, Hold, HoldNext, Profile, Profiles, Reason};
use crate::config::RoutingConfig;

/// route.v10's `context` option for a message answered from the last exchanges.
pub const SHORT: &str = "short";
/// ... and for one answered from the session's compilation.
pub const FULL: &str = "full";

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
/// pack that asks `model` (what makes route.v10 act, `JudgeService::route_open`,
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

/// A block's explanation for `theseus why`: its first sentence, then the
/// sentence that says what it is for (`For ...` or `Only for ...`), joined by an
/// ellipsis, so a reader sees the condition that fired. A block with no such
/// sentence is its first alone.
pub fn block_why(means: &str) -> String {
    let sentences = sentences(means);
    let Some(first) = sentences.first() else {
        return String::new();
    };
    match sentences
        .iter()
        .skip(1)
        .find(|s| s.starts_with("For ") || s.starts_with("Only for "))
    {
        Some(used) => format!("{first} … {used}"),
        None => first.clone(),
    }
}

/// A text's sentences: split after `. ` before a capital, so a price's decimal
/// point and a version's end none.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut start = 0;
    for (k, (at, c)) in chars.iter().enumerate() {
        let next = chars.get(k + 1).map(|(_, c)| *c);
        let after = chars.get(k + 2).map(|(_, c)| *c);
        if *c == '.' && next == Some(' ') && after.is_some_and(char::is_uppercase) {
            out.push(text[start..=*at].trim().to_string());
            start = chars[k + 1].0 + 1;
        }
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
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
    let short = v.short();
    let stay = |reason: Reason| Decision {
        profile: a.base.to_string(),
        reason,
        // A short context is the base's own detour; a full one keeps nothing.
        detour: short,
        switch: false,
        hold: match short {
            true => HoldNext::Keep,
            false => HoldNext::Clear,
        },
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
    if short {
        // This message alone, on the pick: the session's profile, its last
        // target and its compilation stay, and nothing is held.
        return Decision {
            profile: name.to_string(),
            reason: Reason::Verdict,
            detour: true,
            switch: false,
            hold: HoldNext::Keep,
        };
    }
    if name == a.base {
        return stay(Reason::Verdict);
    }
    let agreed = a.hold.is_some_and(|h| h.profile == name);
    if a.est_tokens < cfg.cold_switch_tokens || agreed {
        return Decision {
            profile: name.to_string(),
            reason: Reason::Verdict,
            detour: false,
            switch: true,
            hold: HoldNext::Clear,
        };
    }
    Decision {
        profile: a.base.to_string(),
        reason: Reason::CacheHold,
        detour: false,
        switch: false,
        hold: HoldNext::Set(Hold {
            mode: m.choice.clone(),
            profile: name.to_string(),
            turn: v.turn.clone(),
        }),
    }
}
