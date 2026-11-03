//! What a switch costs this message (theseus-h66v4.1, the owner 2026-10-29
//! 22:50): the resident prefix at the cheapest other model's input price,
//! against the same prefix at the warm model's cache-read price. route.v10's
//! `model` instructions read the answer (`switch_cost`) as a fact the daemon
//! computed, so Jev never does the arithmetic and no `context` question is
//! asked.
//!
//! The candidates are route.v10's five options, DeepSeek Flash among them, each
//! priced at the hour's rate (theseus-h66v4.8, the owner 2026-11-03: "Can we fix
//! flash in 1.0.4 instead of 1.0.5? I'm not sure I get this one, maybe another
//! try by itself"). Two earlier readings of Flash both failed, and the record of
//! why is the reason this module says the price twice:
//!
//! - **Leaving Flash out** (h66v4.1's rule, to 2026-11-03) made its block
//!   unreachable through the price: a 180K-token mechanical rename read
//!   `cheaper_to_stay` though Flash is a fraction of the cost of staying, and
//!   W11, a `must` case, went 3/3 to 0/3.
//! - **Counting it with the one word only** (`p4/h66v48-flash`, measured and
//!   not merged) put `cheaper_to_switch` on every long warm session at off-peak,
//!   which is arithmetically true and useless as advice: told only that
//!   something was cheaper, Jev sent a thank-you on a 200K session to Haiku,
//!   which at that length costs more than staying. W1 and W41 went 3/3 to 0/3.
//!
//! So the word alone is not the fact. [`switch_cost`] returns the word **and
//! the cheapest candidate's name** ([`Switch::cheaper_model`]), and the pack's
//! `model` instructions say that a cheaper model is cheaper only for work it
//! should do. A name Jev can weigh against that option's own block is what makes
//! "cheaper" actionable: `deepseek_flash (off-peak)` beside a crate-wide rename
//! is a reason to switch and beside a thank-you is not, and either way a model
//! the state does NOT name is no cheaper for being cheap elsewhere.
//!
//! A prompt past a model's long tier pays the tier's prices, on both sides, and
//! a provider's off-peak hours are the state's `price_period`, not the clock
//! read again here.
//!
//! **The data rule is read, never loosened.** Where the session's labels keep
//! restricted-origin models off (`restricted_models = not_allowed`), such a
//! model is no candidate here either: the state would otherwise name a cheaper
//! model in one breath and forbid it in the next. The floor that actually
//! refuses the pick is `routing::model`'s and is untouched; this only keeps the
//! advice from contradicting it.

use theseus_judge::builders::{PricePeriod, RestrictedModels, SwitchCost};

use super::Catalog;

/// The models a switch is weighed against: route.v10's `model` options and the
/// catalog model each names. `routing::model::OPTIONS` is the same list, and a
/// test here holds the two together — an option this rule cannot see is one Jev
/// is never told is cheaper.
pub const CANDIDATES: [(&str, &str); 5] = [
    ("haiku", "claude-haiku-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("opus", "claude-opus-5-5"),
    ("fable", "claude-fable-5-1"),
    ("deepseek_flash", "deepseek-flash"),
];

/// `about_the_same` is within this share of the stay cost, either way.
const BAND: f64 = 0.20;

/// What a switch costs this message: the word, and the cheapest candidate's
/// name where one is genuinely cheaper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub cost: SwitchCost,
    /// The cheapest other option at this message's cost, as the state names it
    /// (`deepseek_flash (off-peak)`, `haiku`), and only where the word is
    /// `cheaper_to_switch`: a model that saves nothing is not named, so a name
    /// in the state always means money. Boxed because it rides in
    /// `Input::Inbound`, which is already near clippy's enum-size limit.
    pub cheaper_model: Option<Box<str>>,
}

/// The cost of a switch against the cost of staying, as a word.
pub(super) fn classify(stay: f64, switch: f64) -> SwitchCost {
    if stay <= 0.0 {
        // Nothing to compare with: no claim either way.
        SwitchCost::AboutTheSame
    } else if switch > stay * (1.0 + BAND) {
        SwitchCost::CheaperToStay
    } else if switch < stay * (1.0 - BAND) {
        SwitchCost::CheaperToSwitch
    } else {
        SwitchCost::AboutTheSame
    }
}

/// How the state names a candidate: its option id, and, where the price used is
/// the provider's off-peak one, that it is off-peak. Jev reads the hour from
/// `price_period`; this says which price the comparison was made at.
fn named(option: &str, off_peak: bool) -> Box<str> {
    if off_peak {
        format!("{option} (off-peak)").into()
    } else {
        option.into()
    }
}

/// What switching costs `prefix_tokens` of resident prefix, from the `current`
/// warm model, at `period`'s prices and under `restricted`: `about_the_same`
/// and no name where none is warm, or the catalog does not price it, or no
/// other candidate is left to price.
pub fn switch_cost(
    catalog: &Catalog,
    current: Option<&str>,
    prefix_tokens: u64,
    period: PricePeriod,
    restricted: RestrictedModels,
) -> Switch {
    let same = Switch {
        cost: SwitchCost::AboutTheSame,
        cheaper_model: None,
    };
    let Some((current, warm)) = current.and_then(|m| Some((m, catalog.get(m)?))) else {
        return same;
    };
    // Per million tokens: the same prefix, so the million cancels out of the ratio.
    let stay = warm.cache_read_price_in(prefix_tokens, period);
    // The cheapest, and the first of the list's order at a tie: the order is
    // route.v10's, so a tie names the option listed first and never whichever
    // the iteration happened to reach last.
    let cheapest = CANDIDATES
        .iter()
        .filter(|(_, m)| *m != current)
        .filter_map(|(option, m)| {
            let e = catalog.get(m)?;
            if e.origin.restricted() && restricted == RestrictedModels::NotAllowed {
                return None;
            }
            Some((
                *option,
                e.input_price_in(prefix_tokens, period),
                e.off_peak.is_some() && period == PricePeriod::OffPeak,
            ))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((option, switch, off_peak)) = cheapest else {
        return same;
    };
    let cost = classify(stay, switch);
    Switch {
        cheaper_model: match cost {
            SwitchCost::CheaperToSwitch => Some(named(option, off_peak)),
            _ => None,
        },
        cost,
    }
}
