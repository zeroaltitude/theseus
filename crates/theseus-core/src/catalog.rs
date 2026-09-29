//! The model catalog (spec Part II P5, decided with Eddie 2026-09-26): per model,
//! the serving provider, context window, output ceiling, prices, and the
//! capabilities that change how a request is built. A versioned table: prices
//! and limits cannot self-update (the provider's models endpoint lists ids, not
//! prices), so every priced ledger row names the catalog version that priced it.
//!
//! Sources for the built-in rows: the Claude API reference (Anthropic models;
//! cache writes are the 5-minute TTL rate) and OpenClaw's model catalog (GLM).
//! OpenClaw lists Sonnet 5 at 3/15 and Haiku 4.5 at 0.8/4 with an 8,192-token
//! output cap; the reference says 2/10 and 1/5 with 64K, and the reference wins.
//! Sonnet 5.5 (released 2026-09-28) comes from the Anthropic Models API (window
//! and output cap) and the live pricing page, read on release day.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use theseus_kernel::{usd_to_micros, Micros, MICROS_PER_USD};
use theseus_protocol::Usage;

pub const BUILTIN_VERSION: &str = "2026-09-28.1";

/// How a model takes (or refuses) the `thinking` request parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    /// Do not send `thinking` (GLM, unknown models).
    #[default]
    None,
    /// `{type: "adaptive"}` is the on-mode; sent explicitly (Opus 5, Sonnet 5, Opus 4.8).
    Adaptive,
    /// Thinking cannot be turned off; `{type: "adaptive"}` is accepted (Fable 5.x, Opus 5.5).
    Always,
    /// Only `{type: "enabled", budget_tokens}` (Haiku 4.5); Theseus leaves it off.
    Budget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    /// The provider that serves this model (a key of `[providers]`, or "anthropic").
    pub provider: String,
    /// Maximum input tokens (the context window).
    pub context_window: u64,
    /// Maximum tokens per response; the default `max_output_tokens` for a profile that omits it.
    pub max_output_tokens: u32,
    /// US dollars per million tokens.
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    /// 5-minute cache writes.
    pub cache_write_per_mtok: f64,
    #[serde(default)]
    pub thinking: ThinkingMode,
    /// `output_config.effort` is accepted.
    #[serde(default)]
    pub effort: bool,
    /// Server-side refusal fallbacks (`fallbacks: "default"`) are supported.
    #[serde(default)]
    pub refusal_fallbacks: bool,
    /// Shortest prefix the provider will cache; shorter prefixes silently do not.
    #[serde(default)]
    pub cache_min_tokens: u32,
    #[serde(default)]
    pub vision: bool,
    /// Where the figures came from.
    #[serde(default)]
    pub source: String,
}

impl CatalogEntry {
    /// Dollars for one call's usage. Input fields are disjoint in the API's
    /// accounting: `input_tokens` is the uncached remainder.
    pub fn cost_usd(&self, u: &Usage) -> f64 {
        (u.input_tokens as f64 * self.input_per_mtok
            + u.output_tokens as f64 * self.output_per_mtok
            + u.cache_read_input_tokens as f64 * self.cache_read_per_mtok
            + u.cache_creation_input_tokens as f64 * self.cache_write_per_mtok)
            / 1_000_000.0
    }

    /// What a call's usage costs in micro-dollars, the budget's unit: input,
    /// output, cache reads, and cache writes each at its own price, rounded
    /// up to the next micro-dollar (theseus-0sg).
    pub fn cost_micros(&self, u: &Usage) -> Micros {
        micros_of(&[
            (u.input_tokens, self.input_per_mtok),
            (u.output_tokens, self.output_per_mtok),
            (u.cache_read_input_tokens, self.cache_read_per_mtok),
            (u.cache_creation_input_tokens, self.cache_write_per_mtok),
        ])
    }

    /// What a call reserves before it runs: its output cap at the output
    /// price plus its input estimate at the input price.
    pub fn reserve_micros(&self, max_output_tokens: u32, input_estimate: u64) -> Micros {
        micros_of(&[
            (max_output_tokens as u64, self.output_per_mtok),
            (input_estimate, self.input_per_mtok),
        ])
    }
}

/// Σ tokens × price, rounded up to the next micro-dollar. A price in dollars
/// per million tokens is micro-dollars per token, so a price with up to six
/// decimals is an exact integer once scaled by a million, and the sum is
/// exact before the one rounding.
fn micros_of(terms: &[(u64, f64)]) -> Micros {
    let scaled: u128 = terms
        .iter()
        .map(|&(tokens, per_mtok)| tokens as u128 * usd_to_micros(per_mtok) as u128)
        .sum();
    u64::try_from(scaled.div_ceil(MICROS_PER_USD as u128)).unwrap_or(u64::MAX)
}

/// A `[catalog."<id>"]` table in the config. Over a built-in model every
/// field is optional and replaces only what it names; the template names the
/// four prices. A model the built-in table lacks needs `provider`,
/// `context_window`, `max_output_tokens`, and the four prices.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_fallbacks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_min_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl CatalogRow {
    /// The prices this row sets, by key, for checking them.
    pub fn prices(&self) -> [(&'static str, Option<f64>); 4] {
        [
            ("input_per_mtok", self.input_per_mtok),
            ("output_per_mtok", self.output_per_mtok),
            ("cache_read_per_mtok", self.cache_read_per_mtok),
            ("cache_write_per_mtok", self.cache_write_per_mtok),
        ]
    }

    /// This row over the built-in entry, if the model has one. Without one,
    /// `Err` names the fields a new model still needs.
    pub fn over(&self, base: Option<&CatalogEntry>) -> Result<CatalogEntry, Vec<&'static str>> {
        let entry = match base {
            Some(b) => b.clone(),
            None => {
                let missing: Vec<&'static str> = [
                    ("provider", self.provider.is_none()),
                    ("context_window", self.context_window.is_none()),
                    ("max_output_tokens", self.max_output_tokens.is_none()),
                ]
                .into_iter()
                .chain(self.prices().map(|(k, v)| (k, v.is_none())))
                .filter_map(|(k, gone)| gone.then_some(k))
                .collect();
                if !missing.is_empty() {
                    return Err(missing);
                }
                CatalogEntry {
                    provider: String::new(),
                    context_window: 0,
                    max_output_tokens: 0,
                    input_per_mtok: 0.0,
                    output_per_mtok: 0.0,
                    cache_read_per_mtok: 0.0,
                    cache_write_per_mtok: 0.0,
                    thinking: ThinkingMode::None,
                    effort: false,
                    refusal_fallbacks: false,
                    cache_min_tokens: 0,
                    vision: false,
                    source: String::new(),
                }
            }
        };
        let r = self.clone();
        Ok(CatalogEntry {
            provider: r.provider.unwrap_or(entry.provider),
            context_window: r.context_window.unwrap_or(entry.context_window),
            max_output_tokens: r.max_output_tokens.unwrap_or(entry.max_output_tokens),
            input_per_mtok: r.input_per_mtok.unwrap_or(entry.input_per_mtok),
            output_per_mtok: r.output_per_mtok.unwrap_or(entry.output_per_mtok),
            cache_read_per_mtok: r.cache_read_per_mtok.unwrap_or(entry.cache_read_per_mtok),
            cache_write_per_mtok: r.cache_write_per_mtok.unwrap_or(entry.cache_write_per_mtok),
            thinking: r.thinking.unwrap_or(entry.thinking),
            effort: r.effort.unwrap_or(entry.effort),
            refusal_fallbacks: r.refusal_fallbacks.unwrap_or(entry.refusal_fallbacks),
            cache_min_tokens: r.cache_min_tokens.unwrap_or(entry.cache_min_tokens),
            vision: r.vision.unwrap_or(entry.vision),
            source: r.source.unwrap_or_else(|| "config".into()),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    pub version: String,
    pub entries: BTreeMap<String, CatalogEntry>,
}

#[allow(clippy::too_many_arguments)]
fn claude(
    window: u64,
    max_out: u32,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    thinking: ThinkingMode,
    fallbacks: bool,
    cache_min: u32,
) -> CatalogEntry {
    CatalogEntry {
        provider: "anthropic".into(),
        context_window: window,
        max_output_tokens: max_out,
        input_per_mtok: input,
        output_per_mtok: output,
        cache_read_per_mtok: cache_read,
        cache_write_per_mtok: cache_write,
        thinking,
        effort: thinking != ThinkingMode::Budget,
        refusal_fallbacks: fallbacks,
        cache_min_tokens: cache_min,
        vision: true,
        source: "Claude API reference (2026-06-24 model table; model-migration pricing)".into(),
    }
}

fn glm(
    window: u64,
    max_out: u32,
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
) -> CatalogEntry {
    CatalogEntry {
        provider: "zai".into(),
        context_window: window,
        max_output_tokens: max_out,
        input_per_mtok: input,
        output_per_mtok: output,
        cache_read_per_mtok: cache_read,
        cache_write_per_mtok: cache_write,
        thinking: ThinkingMode::None,
        effort: false,
        refusal_fallbacks: false,
        cache_min_tokens: 0,
        vision: true,
        source: "OpenClaw model catalog (models.providers.zai), 2026-09".into(),
    }
}

impl Catalog {
    pub fn builtin() -> Self {
        use ThinkingMode::*;
        let mut e = BTreeMap::new();
        let m = 1_000_000;
        e.insert(
            "claude-fable-5-1".into(),
            claude(m, 128_000, 10.0, 50.0, 0.25, 12.50, Always, true, 512),
        );
        e.insert(
            "claude-fable-5".into(),
            claude(m, 128_000, 10.0, 50.0, 1.00, 12.50, Always, false, 512),
        );
        e.insert(
            "claude-opus-5-5".into(),
            claude(m, 128_000, 4.0, 20.0, 0.20, 5.00, Always, false, 512),
        );
        e.insert(
            "claude-opus-5".into(),
            claude(m, 128_000, 5.0, 25.0, 0.50, 6.25, Adaptive, true, 512),
        );
        e.insert(
            "claude-opus-4-8".into(),
            claude(m, 128_000, 5.0, 25.0, 0.50, 6.25, Adaptive, false, 1024),
        );
        e.insert(
            "claude-sonnet-5-5".into(),
            CatalogEntry {
                source: "Anthropic Models API and pricing page, 2026-09-28".into(),
                ..claude(m, 128_000, 2.0, 10.0, 0.20, 2.50, Adaptive, false, 1024)
            },
        );
        e.insert(
            "claude-sonnet-5".into(),
            claude(m, 128_000, 2.0, 10.0, 0.20, 2.50, Adaptive, false, 1024),
        );
        e.insert(
            "claude-haiku-4-5".into(),
            claude(200_000, 64_000, 1.0, 5.0, 0.10, 1.25, Budget, false, 4096),
        );
        e.insert(
            "glm-5.3".into(),
            glm(1_000_000, 128_000, 1.40, 4.40, 0.26, 1.40),
        );
        e.insert(
            "glm-5.2".into(),
            glm(1_000_000, 128_000, 1.40, 4.40, 0.26, 1.40),
        );
        e.insert(
            "glm-5.3-flash".into(),
            glm(1_048_576, 131_072, 0.15, 0.50, 0.03, 0.0),
        );
        e.insert(
            "glm-5.3-flashx".into(),
            glm(1_048_576, 131_072, 0.37, 1.25, 0.075, 0.0),
        );
        Self {
            version: BUILTIN_VERSION.into(),
            entries: e,
        }
    }

    /// The built-in table with the config's `[catalog."<id>"]` tables over
    /// it: a table over a built-in row replaces the fields it names, and a
    /// complete table adds a model. Any table changes the version string, so
    /// a priced row says so. `Config::validate` refuses an incomplete new
    /// model, so none is skipped here.
    pub fn with_overrides(overrides: &BTreeMap<String, CatalogRow>) -> Self {
        let mut c = Self::builtin();
        if overrides.is_empty() {
            return c;
        }
        for (k, row) in overrides {
            if let Ok(e) = row.over(c.entries.get(k)) {
                c.entries.insert(k.clone(), e);
            }
        }
        c.version = format!("{BUILTIN_VERSION}+config:{}", overrides.len());
        c
    }

    /// Built-in models the config has no `[catalog]` table for: they run at
    /// the built-in prices, and startup names them.
    pub fn missing_from(overrides: &BTreeMap<String, CatalogRow>) -> Vec<String> {
        Self::builtin()
            .entries
            .into_keys()
            .filter(|id| !overrides.contains_key(id))
            .collect()
    }

    pub fn get(&self, model: &str) -> Option<&CatalogEntry> {
        self.entries.get(model)
    }

    /// Dollars for a call, or `None` when the model is not in the catalog.
    pub fn cost_usd(&self, model: &str, usage: &Usage) -> Option<f64> {
        self.get(model).map(|e| e.cost_usd(usage))
    }

    /// The template's `[catalog]` tables: every built-in model with its four
    /// prices. The template holds this text verbatim, and a test compares
    /// the two, so the prices Eddie reads in his config are the built-in ones.
    pub fn template_tables() -> String {
        let mut out = String::new();
        for (id, e) in &Self::builtin().entries {
            let head = format!("[catalog.\"{id}\"]");
            out.push_str(&format!(
                "{head:<40}# {} · {}-token window · {} out\n",
                e.provider,
                crate::narrative::thousands(e.context_window),
                crate::narrative::thousands(e.max_output_tokens as u64)
            ));
            for (key, price) in [
                ("input_per_mtok", e.input_per_mtok),
                ("output_per_mtok", e.output_per_mtok),
                ("cache_read_per_mtok", e.cache_read_per_mtok),
                ("cache_write_per_mtok", e.cache_write_per_mtok),
            ] {
                out.push_str(&format!("{key} = {price:?}\n"));
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rows_are_sane_and_priced() {
        let c = Catalog::builtin();
        for (id, e) in &c.entries {
            assert!(e.context_window >= 200_000, "{id}");
            assert!(e.max_output_tokens >= 64_000, "{id}");
            assert!(e.output_per_mtok >= e.input_per_mtok, "{id}");
            assert!(e.cache_read_per_mtok < e.input_per_mtok, "{id}");
        }
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_input_tokens: 1_000_000,
            cache_creation_input_tokens: 1_000_000,
        };
        // Sonnet 5 and Sonnet 5.5: 2 + 10 + 0.20 + 2.50
        for id in ["claude-sonnet-5", "claude-sonnet-5-5"] {
            let s = c.cost_usd(id, &u).unwrap();
            assert!((s - 14.70).abs() < 1e-9, "{id}: {s}");
        }
        let s55 = c.get("claude-sonnet-5-5").unwrap();
        assert_eq!(
            (s55.context_window, s55.max_output_tokens),
            (1_000_000, 128_000)
        );
        assert!(c.cost_usd("no-such-model", &u).is_none());
        assert_eq!(
            c.get("claude-opus-5-5").unwrap().thinking,
            ThinkingMode::Always
        );
        assert_eq!(c.get("glm-5.3-flash").unwrap().provider, "zai");
    }

    #[test]
    fn overrides_replace_add_and_change_the_version() {
        let mut o = BTreeMap::new();
        o.insert(
            "claude-sonnet-5".to_string(),
            CatalogRow {
                input_per_mtok: Some(3.0),
                ..Default::default()
            },
        );
        let c = Catalog::with_overrides(&o);
        let s5 = c.get("claude-sonnet-5").unwrap();
        assert_eq!(s5.input_per_mtok, 3.0);
        assert_eq!(s5.output_per_mtok, 10.0, "an unnamed field stays built in");
        assert_eq!(s5.context_window, 1_000_000);
        assert_eq!(s5.source, "config");
        assert!(c.version.starts_with(BUILTIN_VERSION) && c.version.contains("+config:1"));
        assert_eq!(
            Catalog::with_overrides(&BTreeMap::new()).version,
            BUILTIN_VERSION
        );
        // A model the built-in table lacks needs every figure a call uses.
        let partial = CatalogRow {
            input_per_mtok: Some(1.0),
            ..Default::default()
        };
        assert_eq!(
            partial.over(None).unwrap_err(),
            [
                "provider",
                "context_window",
                "max_output_tokens",
                "output_per_mtok",
                "cache_read_per_mtok",
                "cache_write_per_mtok"
            ]
        );
        let full = CatalogRow {
            provider: Some("zai".into()),
            context_window: Some(200_000),
            max_output_tokens: Some(8_192),
            input_per_mtok: Some(1.0),
            output_per_mtok: Some(2.0),
            cache_read_per_mtok: Some(0.1),
            cache_write_per_mtok: Some(0.0),
            ..Default::default()
        };
        o.insert("new-model".into(), full);
        let c = Catalog::with_overrides(&o);
        assert_eq!(c.get("new-model").unwrap().max_output_tokens, 8_192);
        assert!(c.version.ends_with("+config:2"), "{}", c.version);
        assert_eq!(
            Catalog::missing_from(&o).len(),
            Catalog::builtin().entries.len() - 1
        );
    }

    /// One call's reservation and settlement in micro-dollars, by hand from
    /// the catalog (theseus-0sg). Sonnet 5.5: $2 in, $10 out, $0.20 cache
    /// read, $2.50 cache write per million tokens, so a price per million
    /// tokens is micro-dollars per token.
    #[test]
    fn a_calls_reservation_and_cost_in_micro_dollars_match_the_prices() {
        let s55 = Catalog::builtin().get("claude-sonnet-5-5").unwrap().clone();
        // Reserve: the 128,000-token output cap at $10, a 44,000-token input
        // estimate at $2: 1,280,000 + 88,000 µ$ = $1.368.
        assert_eq!(s55.reserve_micros(128_000, 44_000), 1_368_000);
        // Settle: 1,200 in at $2, 900 out at $10, 40,000 cache reads at
        // $0.20, 3,000 cache writes at $2.50: 2,400 + 9,000 + 8,000 + 7,500.
        let u = Usage {
            input_tokens: 1_200,
            output_tokens: 900,
            cache_read_input_tokens: 40_000,
            cache_creation_input_tokens: 3_000,
        };
        assert_eq!(s55.cost_micros(&u), 26_900);
        assert!((s55.cost_usd(&u) - 0.0269).abs() < 1e-12);
        // A cache read weighs a fiftieth of an output token, not the same.
        let reads = Usage {
            cache_read_input_tokens: 1_000_000,
            ..Default::default()
        };
        let outs = Usage {
            output_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(
            (s55.cost_micros(&reads), s55.cost_micros(&outs)),
            (200_000, 10_000_000)
        );
        // Fractions of a micro-dollar round up, once per call: 7 GLM-5.3
        // Flash cache reads at $0.03 are 0.21 µ$, so 1.
        let flash = Catalog::builtin().get("glm-5.3-flash").unwrap().clone();
        let tiny = Usage {
            cache_read_input_tokens: 7,
            ..Default::default()
        };
        assert_eq!(flash.cost_micros(&tiny), 1);
        assert_eq!(flash.cost_micros(&Usage::default()), 0);
    }
}
