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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use theseus_protocol::Usage;

pub const BUILTIN_VERSION: &str = "2026-09-26.1";

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

    /// The built-in table with `[catalog."<id>"]` entries replacing or adding
    /// rows. Any override changes the version string, so a priced row says so.
    pub fn with_overrides(overrides: &BTreeMap<String, CatalogEntry>) -> Self {
        let mut c = Self::builtin();
        if overrides.is_empty() {
            return c;
        }
        for (k, v) in overrides {
            let mut v = v.clone();
            if v.source.is_empty() {
                v.source = "config".into();
            }
            c.entries.insert(k.clone(), v);
        }
        c.version = format!("{BUILTIN_VERSION}+config:{}", overrides.len());
        c
    }

    pub fn get(&self, model: &str) -> Option<&CatalogEntry> {
        self.entries.get(model)
    }

    /// Dollars for a call, or `None` when the model is not in the catalog.
    pub fn cost_usd(&self, model: &str, usage: &Usage) -> Option<f64> {
        self.get(model).map(|e| e.cost_usd(usage))
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
        // Sonnet 5: 2 + 10 + 0.20 + 2.50
        let s = c.cost_usd("claude-sonnet-5", &u).unwrap();
        assert!((s - 14.70).abs() < 1e-9, "{s}");
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
        let mut e = Catalog::builtin().get("claude-sonnet-5").unwrap().clone();
        e.input_per_mtok = 3.0;
        e.source = String::new();
        o.insert("claude-sonnet-5".to_string(), e);
        let c = Catalog::with_overrides(&o);
        assert_eq!(c.get("claude-sonnet-5").unwrap().input_per_mtok, 3.0);
        assert_eq!(c.get("claude-sonnet-5").unwrap().source, "config");
        assert!(c.version.starts_with(BUILTIN_VERSION) && c.version.contains("+config:1"));
        assert_eq!(
            Catalog::with_overrides(&BTreeMap::new()).version,
            BUILTIN_VERSION
        );
    }
}
