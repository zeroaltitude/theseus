//! Routing (M5 25e): which interaction mode the route pack judged a person's
//! message to need, and why the turn ran where it ran. A turn's result
//! carries it (`TurnSubmitResult.route`), so a client's status line can say
//! the mode beside the model. A refusal's fallback moves a turn too
//! (`TurnSubmitResult.fallback`, theseus-7gir.18), and says so in one line.

use serde::{Deserialize, Serialize};

/// How routing placed one turn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnRoute {
    /// The mode the route pack answered (`trivial`, `quick` since route.v2,
    /// `chat`, `sophisticated`, `deep_coding`, `routine_coding`, `other`),
    /// when a verdict was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub mode: Option<String>,
    /// `verdict`, `detour`, `capped`, `fallback`, `cache_hold`, `unsure`,
    /// `late`, `unreachable`, `no_verdict`, `pinned`, or `shadow`.
    pub reason: String,
    /// The profile the session ran on before routing.
    pub from: String,
    /// `correction` when the owner's correction (or the live layer's entry
    /// close to this message) placed the turn ahead of the verdict
    /// (theseus-q31l); absent when the verdict did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub source: Option<String>,
}

/// `route.correct` (theseus-q31l): the owner's correction of a turn's
/// routing, from a reaction, a footer's control, the cockpit, or the CLI.
/// Judged as `judge.label` is: the owner, from a private place.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteCorrectParams {
    pub session_id: String,
    /// The turn corrected; absent: the session's last turn routing decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    /// Where it should have run: a profile (`fable`), a profile's model, a
    /// mode of the acting route pack (`deep_coding`), `stronger` or `cheaper`.
    pub to: String,
    /// What the owner corrected with: `reaction`, `button`, `cockpit`, `cli`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub via: Option<String>,
    /// Its id there (a reaction's message and emoji, a press's interaction).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub provenance: Option<String>,
    /// The binding's reading of where it came from, as `judge.label`'s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What a correction did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteCorrectResult {
    /// The correction's id (`rcx_…`), its `route.corrected` row's key.
    pub correction: String,
    pub turn_id: String,
    /// The owner's label it wrote, when the turn's mode was wrong.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub judgment: Option<String>,
    /// Where the session runs from its next turn, when it moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
    /// One line that says it.
    pub line: String,
}

/// `route.corrections`: the live correction layer, a read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteCorrectionsResult {
    /// The route pack version acting; entries of any other have retired.
    pub pack: String,
    /// Newest first.
    pub entries: Vec<RouteCorrectionInfo>,
    pub max_entries: u32,
    pub similarity: f64,
    /// Entries retired since the daemon started.
    pub retired: u64,
    pub enabled: bool,
}

/// One correction in the layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteCorrectionInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub label: Option<String>,
    pub pack: String,
    pub session_id: String,
    pub turn_id: String,
    /// Where a close message runs: a profile, or `mode <id>`.
    pub to: String,
    /// The corrected message's content words, which a close message shares.
    pub words: Vec<String>,
    pub at_ms: u64,
}

/// A refusal's client-side fallback in one turn (theseus-7gir.18): the model
/// that declined, the one its request went to instead, for the rest of the
/// turn, and the refusal's category. `line` is the one wording every surface
/// shows (`cockpit/src/lib/fallback.ts` mirrors it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnFallback {
    pub from: String,
    pub to: String,
    /// The refusal's `stop_details.category` (`cyber`), when it named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub category: Option<String>,
    /// `to` answered: false when it declined too, or never ran (the spend
    /// limit, a stop, a failed call).
    #[serde(default)]
    pub answered: bool,
}

impl TurnFallback {
    /// `Sonnet 5.5 declined (cyber); Sonnet 5 answered.`, by the turn's end:
    /// `… declined (cyber), and so did Sonnet 5.` when its stop reason is a
    /// refusal too, and `…; the request went to Sonnet 5.` when `to` never
    /// answered.
    pub fn line(&self, stop_reason: &str) -> String {
        let (from, to) = (model_name(&self.from), model_name(&self.to));
        let declined = match &self.category {
            Some(c) => format!("{from} declined ({c})"),
            None => format!("{from} declined"),
        };
        if self.answered {
            format!("{declined}; {to} answered.")
        } else if stop_reason == "refusal" {
            format!("{declined}, and so did {to}.")
        } else {
            format!("{declined}; the request went to {to}.")
        }
    }
}

/// A model as a person names it: `claude-sonnet-5-5` is Sonnet 5.5, and a
/// date after the version (`claude-haiku-4-5-20251001`) is dropped. Any other
/// id is itself.
pub fn model_name(id: &str) -> String {
    let mut parts = id.strip_prefix("claude-").unwrap_or("").split('-');
    let family = parts.next().unwrap_or("");
    if family.is_empty() || !family.chars().all(|c| c.is_ascii_lowercase()) {
        return id.to_string();
    }
    let version: Vec<&str> = parts
        .take_while(|p| (1..=2).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let mut name = family[..1].to_ascii_uppercase() + &family[1..];
    if !version.is_empty() {
        name = format!("{name} {}", version.join("."));
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one line every surface shows, by how the turn ended (the cockpit's
    /// `test/fallback.test.ts` holds the same strings).
    #[test]
    fn a_fallbacks_line_says_who_declined_and_who_answered() {
        let mut f = TurnFallback {
            from: "claude-sonnet-5-5".into(),
            to: "claude-sonnet-5".into(),
            category: Some("cyber".into()),
            answered: true,
        };
        assert_eq!(
            f.line("end_turn"),
            "Sonnet 5.5 declined (cyber); Sonnet 5 answered."
        );
        f.answered = false;
        assert_eq!(
            f.line("refusal"),
            "Sonnet 5.5 declined (cyber), and so did Sonnet 5."
        );
        f.category = None;
        assert_eq!(
            f.line("budget"),
            "Sonnet 5.5 declined; the request went to Sonnet 5."
        );
        for (id, name) in [
            ("claude-opus-5-5", "Opus 5.5"),
            ("claude-haiku-4-5-20251001", "Haiku 4.5"),
            ("claude-haiku-5-5", "Haiku 5.5"),
            ("claude-fable-5", "Fable 5"),
            ("glm-5.3-flash", "glm-5.3-flash"),
            ("claude-", "claude-"),
        ] {
            assert_eq!(model_name(id), name, "{id}");
        }
    }
}
