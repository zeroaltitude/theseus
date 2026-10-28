//! Routing (M5 25e): which task class the route pack judged a person's
//! message to need, and why the turn ran where it ran. A turn's result
//! carries it (`TurnSubmitResult.route`), so a client's status line can say
//! the task class beside the model. A refusal's fallback moves a turn too
//! (`TurnSubmitResult.fallback`, theseus-7gir.18), and says so in one line.

use serde::{Deserialize, Serialize};

/// How routing placed one turn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnRoute {
    /// The task class the route pack answered (`trivial`, `quick` since route.v2,
    /// `chat`, `sophisticated`, `deep_coding`, `routine_coding`, `review`, `other`),
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
    /// (theseus-q31l), or `task brief` when the verdict on a task's brief picked
    /// its model (theseus-ypnxt.7), or `task checkpoint: failing` (`quiet`,
    /// `phase`, `report`) when a running task's checkpoint moved its model
    /// (theseus-ypnxt.9); absent when the verdict on a message did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub source: Option<String>,
    /// route.v3's answer about the reply's effort (`low`, `medium`, `high`,
    /// `xhigh`, `max`, or `unclear`), when a verdict was read (theseus-qe3v).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub effort: Option<String>,
    /// Why the turn ran at its effort, beside `effort`: `applied`, `clamped`
    /// (to `[routing] effort_bounds`), `held_high` (an `xhigh` or `max` at or
    /// under `high_effort_confidence`, run at `high`), `held_high_unsure` (the
    /// same, for an answer under its confirm bar), `unsure`, `applied_unsure`
    /// (under the confirm bar, at or above `effort_confidence`), `unclear`,
    /// `fixed` (the profile's `effort_fixed`), `no_effort` (its model takes
    /// none), `carried` (a late verdict's), `recorded` (shadow, or a pin),
    /// `sticky` (a one-step change held at the previous request's level to
    /// keep the cache: `effort_applied` is the level sent), `sticky_unsure`
    /// (that hold of an `applied_unsure` or `held_high_unsure` answer),
    /// `sticky_reverted` (a later answer sure enough ended the hold early),
    /// `floored` (raised to the routed profile's own effort), or `gated`
    /// (the answer's confidence was under `effort_floor_confidence`: it
    /// applied nothing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub effort_reason: Option<String>,
    /// The effort Jev's answer set on the turn's requests, when it applied:
    /// `effort` itself, or the bound it was clamped to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub effort_applied: Option<String>,
    /// Which persona answered the turn, when two or more could (route.v5's
    /// `persona`, theseus-c1uz).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<RoutePersona>,
    /// The data rule's part in the turn (theseus-a1wvq.11), when routing
    /// recorded it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub data: Option<TurnData>,
}

/// What the data rule did for a turn, for the route line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnData {
    /// Whether the session may use a model of restricted origin, as routing found it: `allowed`, the word of the flag that keeps it off one (`secret`, `confidential`, `personal`), `never` under `cn_models = "never"`, or `no_record`; `pending` and `unjudged` in a route from before theseus-gyjyo.6. Empty in a route from before content flags.
    #[serde(default)]
    pub cn: String,
    /// Why the data rule kept the turn off a restricted-origin model: an open word, absent when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub gate: Option<String>,
    /// The origin of the model the turn ran on (`us`, `eu`, `cn`, `unknown`).
    pub origin: String,
}

impl TurnData {
    /// The route line's data words, as every surface words them: `data gate:
    /// <why>` when the rule kept the turn off a restricted-origin model,
    /// nothing otherwise.
    pub fn footer(&self) -> Option<String> {
        self.gate.as_ref().map(|g| format!("data gate: {g}"))
    }
}

/// The persona that answered a turn and why, for the route line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RoutePersona {
    /// The `[personas]` key; absent for the built-in default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub key: Option<String>,
    /// What the person calls it.
    pub name: String,
    /// `explicit` (the message addressed it), `jev` (Jev's pick, sure),
    /// `jev_shown` (Jev's pick, less sure: show the why) or `prior` (it
    /// answered last).
    pub how: String,
    /// The why for the route line: the band and probability, the runner-up,
    /// the card terms the message matched. Empty when nothing is shown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
}

impl RoutePersona {
    /// The route line's who and why, as every surface words it:
    /// `Birch` when nothing more is shown, `Birch · "CVE" · 0.94 · next alder
    /// 0.04` with its why.
    pub fn words(&self) -> String {
        match self.why.is_empty() {
            true => self.name.clone(),
            false => format!("{} \u{b7} {}", self.name, self.why),
        }
    }
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
/// that declined, the one its request went to instead, and the refusal's
/// category. The fallback answers the refused request and the next goes back
/// to the routed model (theseus-wf482), so a turn can switch more than once;
/// it stays on the fallback only when `latched`. `line` is the one wording
/// every surface shows (`cockpit/src/lib/fallback.ts` mirrors it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnFallback {
    pub from: String,
    pub to: String,
    /// The latest refusal's `stop_details.category` (`cyber`), when it named
    /// one; `timeout` for a call cut at its whole-call bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub category: Option<String>,
    /// `to` answered at least once (`answers > 0`): false when it declined
    /// too, or never ran (the spend limit, a stop, a failed call).
    #[serde(default)]
    pub answered: bool,
    /// Requests `to` answered in this turn.
    #[serde(default)]
    pub answers: u32,
    /// Requests the routed model refused, or cut at the whole-call bound, and
    /// that went to `to` instead, in this turn.
    #[serde(default)]
    pub refusals: u32,
    /// The turn stayed on `to` from its last switch on: a hard-decline
    /// category, a timeout, or the third routed refusal in a row.
    #[serde(default)]
    pub latched: bool,
}

/// `twice`, `3 times`: how many times, in words, for a count above one.
fn times(n: u32) -> String {
    if n == 2 {
        "twice".into()
    } else {
        format!("{n} times")
    }
}

impl TurnFallback {
    /// `Sonnet 5.5 declined (cyber); Sonnet 5 answered.`, by the turn's end:
    /// `… declined (cyber), and so did Sonnet 5.` when its stop reason is a
    /// refusal too, and `…; the request went to Sonnet 5.` when `to` never
    /// answered. A turn that switched more than once says how many times
    /// (`declined (reasoning_extraction) twice; Sonnet 5 answered twice.`),
    /// and one that stayed on `to` **for more than one answer** says so
    /// (`…; Sonnet 5 answered twice, and the turn stayed there.`). A latched
    /// turn with a single answer ended on that answer, so there was nothing
    /// to stay for and the line is the ordinary one: every `cyber` refusal,
    /// which latches at once, reads exactly as it did before theseus-wf482.
    /// A record from before the counts reads as one refusal and, when
    /// `answered`, one answer.
    pub fn line(&self, stop_reason: &str) -> String {
        let (from, to) = (model_name(&self.from), model_name(&self.to));
        let refusals = self.refusals.max(1);
        let answers = if self.answered {
            self.answers.max(1)
        } else {
            0
        };
        let mut declined = match &self.category {
            Some(c) => format!("{from} declined ({c})"),
            None => format!("{from} declined"),
        };
        if refusals > 1 {
            declined = format!("{declined} {}", times(refusals));
        }
        if answers > 0 {
            let mut said = format!("{to} answered");
            if answers > 1 {
                said = format!("{said} {}", times(answers));
                if self.latched {
                    said.push_str(", and the turn stayed there");
                }
            }
            format!("{declined}; {said}.")
        } else if stop_reason == "refusal" {
            format!("{declined}, and so did {to}.")
        } else {
            format!("{declined}; the request went to {to}.")
        }
    }
}

/// The families admitted past a first request's base budget (theseus-xlri5): a
/// family the message names, or Jev judges relevant, loads past the place's
/// budget by exactly its size, under the ceiling. `TurnSubmitResult.raised`
/// and `tools.used`'s `raised` carry it; absent when nothing passed the budget.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolsRaised {
    /// The place kind's first-request budget, in tokens.
    #[cfg_attr(test, ts(type = "number"))]
    pub budget: u64,
    /// The hard ceiling (`budget × [tools.defer] admit_ceiling`).
    #[cfg_attr(test, ts(type = "number"))]
    pub ceiling: u64,
    /// The whole first request with the admitted families, as estimated.
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
    /// The families that took it past the budget, in the order admitted.
    pub families: Vec<RaisedFamily>,
}

/// One family that raised a request: its tokens, and who admitted it
/// (`named`: the message names it; `jev`: Jev judged it relevant).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RaisedFamily {
    pub family: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
    pub by: String,
}

impl ToolsRaised {
    /// `Tools raised the request to 9,612 tokens (budget 8,000, ceiling
    /// 16,000): aws +1,012 (named), terminals +1,264 (Jev).`
    pub fn line(&self) -> String {
        let families: Vec<String> = self
            .families
            .iter()
            .map(|f| {
                let by = if f.by == "jev" { "Jev" } else { f.by.as_str() };
                format!("{} +{} ({by})", f.family, grouped(f.tokens))
            })
            .collect();
        format!(
            "Tools raised the request to {} tokens (budget {}, ceiling {}): {}.",
            grouped(self.tokens),
            grouped(self.budget),
            grouped(self.ceiling),
            families.join(", ")
        )
    }
}

/// `9612` as `9,612`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
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

    /// A turn's route with and without its data part round-trips, and a result
    /// from before it reads as none; the footer words it once for every surface.
    #[test]
    fn turn_route_data_round_trips() {
        let data = TurnData {
            cn: "allowed".into(),
            gate: None,
            origin: "cn".into(),
        };
        let with = TurnRoute {
            reason: "verdict".into(),
            from: "sonnet".into(),
            data: Some(data.clone()),
            ..Default::default()
        };
        let json = serde_json::to_string(&with).unwrap();
        assert_eq!(serde_json::from_str::<TurnRoute>(&json).unwrap(), with);
        let without = TurnRoute { data: None, ..with };
        let json = serde_json::to_string(&without).unwrap();
        assert!(!json.contains("data"), "{json}");
        assert_eq!(serde_json::from_str::<TurnRoute>(&json).unwrap(), without);
        let old = r#"{"reason":"verdict","from":"sonnet"}"#;
        assert_eq!(serde_json::from_str::<TurnRoute>(old).unwrap().data, None);
        // A route from before content flags carried a `clearance` and a
        // `verdict`: it still reads, its `cn` empty.
        let before = r#"{"reason":"verdict","from":"sonnet","data":{"verdict":"open","clearance":"open","origin":"cn"}}"#;
        let read = serde_json::from_str::<TurnRoute>(before)
            .unwrap()
            .data
            .unwrap();
        assert_eq!((read.cn.as_str(), read.origin.as_str()), ("", "cn"));
        let gated = |cn: &str, gate: Option<&str>| TurnData {
            cn: cn.into(),
            gate: gate.map(Into::into),
            ..data.clone()
        };
        assert_eq!(
            data.footer(),
            None,
            "a turn that ran on a cn model words nothing"
        );
        assert_eq!(
            gated("confidential", Some("confidential"))
                .footer()
                .as_deref(),
            Some("data gate: confidential")
        );
        assert_eq!(gated("allowed", None).footer(), None);
    }

    /// The one line every surface shows, by how the turn ended (the cockpit's
    /// `test/fallback.test.ts` holds the same strings).
    #[test]
    fn a_fallbacks_line_says_who_declined_and_who_answered() {
        let mut f = TurnFallback {
            from: "claude-sonnet-5-5".into(),
            to: "claude-sonnet-5".into(),
            category: Some("cyber".into()),
            answered: true,
            ..Default::default()
        };
        assert_eq!(
            f.line("end_turn"),
            "Sonnet 5.5 declined (cyber); Sonnet 5 answered."
        );
        // A turn that went back and forth, and one that stayed (theseus-wf482).
        let mut many = TurnFallback {
            category: Some("reasoning_extraction".into()),
            answers: 2,
            refusals: 2,
            ..f.clone()
        };
        assert_eq!(
            many.line("end_turn"),
            "Sonnet 5.5 declined (reasoning_extraction) twice; Sonnet 5 answered twice."
        );
        many.latched = true;
        many.answers = 1;
        many.refusals = 3;
        // Latched on its last switch, but it ended on that one answer: there
        // was nothing to stay for, so the line does not say it did.
        assert_eq!(
            many.line("end_turn"),
            "Sonnet 5.5 declined (reasoning_extraction) 3 times; Sonnet 5 answered."
        );
        many.refusals = 1;
        many.answers = 4;
        assert_eq!(
            many.line("end_turn"),
            "Sonnet 5.5 declined (reasoning_extraction); Sonnet 5 answered 4 times, and the turn stayed there."
        );
        // A record from before the counts still reads.
        let old: TurnFallback = serde_json::from_value(serde_json::json!(
            {"from": "claude-sonnet-5-5", "to": "claude-sonnet-5", "answered": true}
        ))
        .unwrap();
        assert_eq!(
            (old.answers, old.refusals, old.latched),
            (0, 0, false),
            "{old:?}"
        );
        assert_eq!(
            old.line("end_turn"),
            "Sonnet 5.5 declined; Sonnet 5 answered."
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
