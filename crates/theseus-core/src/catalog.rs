//! The model catalog (spec Part II P5, decided with Eddie 2026-09-26): per model,
//! the serving provider, context window, output ceiling, prices, and the
//! capabilities that change how a request is built. A versioned table: prices
//! and limits cannot self-update (the provider's models endpoint lists ids, not
//! prices), so every priced ledger row names the catalog version that priced it.
//!
//! Sources for the built-in rows: the Claude API reference (Anthropic models;
//! cache writes at 1.25 × input for the 5-minute TTL and 2 × for the 1-hour
//! one) and OpenClaw's model catalog (GLM).
//! OpenClaw lists Sonnet 5 at 3/15 and Haiku 4.5 at 0.8/4 with an 8,192-token
//! output cap; the reference says 2/10 and 1/5 with 64K, and the reference wins.
//! Sonnet 5.5 (released 2026-09-28) comes from the Anthropic Models API (window
//! and output cap) and the live pricing page, read on release day.
//!
//! `caches` says the provider reads a prompt's repeated prefix from its cache
//! and reports the tokens it read, which the ledger prices. It does not say the
//! provider honors cache breakpoints: Z.ai caches GLM prompts with or without
//! markers, and read nothing of a prefix that matched only up to a breakpoint
//! (the cache lane's probe, 2026-09-30). Every built-in model caches. A model
//! that does not gets no breakpoints, and its manifest says so (theseus-ev1).
//!
//! `cache_min_tokens` is the shortest prefix the provider caches: a breakpoint
//! on a shorter one is silently not cached. From the claude-api skill's
//! caching reference (2026-10-01): 512 on Fable 5 and 5.1, Opus 5 and 5.5, and
//! Sonnet 5.5; 1,024 on Opus 4.8 and Sonnet 5; 4,096 on Haiku 4.5. The
//! reference marks Sonnet 5.5's 512 as one to confirm in the docs.
//!
//! `bytes_per_token` is how densely the model's tokenizer reads a request's
//! JSON and its prose, for the compiler's estimate (theseus-f5hf): see
//! [`TokenRates`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use theseus_kernel::{usd_to_micros, Micros, MICROS_PER_USD};
use theseus_protocol::Usage;

pub const BUILTIN_VERSION: &str = "2026-10-01.1";

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
    /// 1-hour cache writes (theseus-ev1): 2 × input on Anthropic's models.
    pub cache_write_1h_per_mtok: f64,
    /// The provider reads repeated prefixes from its cache and reports it
    /// (the module's notes); without it a request carries no breakpoints.
    #[serde(default = "yes")]
    pub caches: bool,
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
    /// The model reads a PDF as a `document` block, its pages' text and
    /// pictures both (theseus-c9l6); any other reads the PDF's text.
    #[serde(default)]
    pub pdf: bool,
    /// Where the figures came from.
    #[serde(default)]
    pub source: String,
    /// How densely the model's tokenizer reads a request (theseus-f5hf).
    #[serde(default)]
    pub bytes_per_token: TokenRates,
}

fn yes() -> bool {
    true
}

/// Bytes a model's tokenizer reads as one token, by what the bytes are
/// (theseus-f5hf), for the compiler's estimate of a request
/// (`provider::Census`). Measured on Theseus's own requests against the
/// provider's count (the tokens lane, 2026-10-01). A tokenizer reads JSON,
/// code, and command output far more densely than prose. On Sonnet 5.5 the
/// 15 tool schemas, with the provider's tool prompt, ran 2.5 bytes a token,
/// a Rust file read by a tool 2.32, the tool results in Eddie's DM 1.76 to
/// 2.84 (2.4 typically), and prose 3.35; chars/4 read them all at 4. A
/// figure is the typical one: the compiler's estimate allows for denser
/// content by its margin (`compiler::MARGIN_PERCENT`), and counts on the
/// provider's own count once a compilation has made a call.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRates {
    /// Tool schemas, tool inputs, and tool results (code, command output,
    /// file contents).
    pub json: f64,
    /// The system's text, and the messages' text and thinking.
    pub text: f64,
}

impl TokenRates {
    /// Claude's tokenizer since Opus 4.7: Sonnet 5 and 5.5, Opus 4.7, 4.8,
    /// 5, and 5.5, and Fable 5 and 5.1 (the claude-api reference: "roughly
    /// 1× to 1.35×" the older one's tokens). Measured on Sonnet 5.5.
    pub const CLAUDE: TokenRates = TokenRates {
        json: 2.4,
        text: 3.3,
    };
    /// Claude's older tokenizer (Haiku 4.5): it counted Theseus's first
    /// request at 4,274 where Sonnet 5.5 counted 5,204 (the cache2 lane),
    /// so Claude's figures × 1.22.
    pub const CLAUDE_OLD: TokenRates = TokenRates {
        json: 2.9,
        text: 4.0,
    };
    /// GLM 5.x, measured on GLM-5.3 Flash: the tool schemas 3.77 bytes a
    /// token, a Rust file about 3.3, prose 4.54.
    pub const GLM: TokenRates = TokenRates {
        json: 3.7,
        text: 4.4,
    };

    /// The built-in figures for a model, by its family. A model of no known
    /// family gets Claude's, the densest measured, so it is over-estimated
    /// rather than under.
    pub fn of(model: &str) -> TokenRates {
        if model.starts_with("claude-haiku-4") {
            Self::CLAUDE_OLD
        } else if model.starts_with("glm-") {
            Self::GLM
        } else {
            Self::CLAUDE
        }
    }
}

impl Default for TokenRates {
    fn default() -> Self {
        Self::CLAUDE
    }
}

impl CatalogEntry {
    /// Dollars for one call's usage. Input fields are disjoint in the API's
    /// accounting: `input_tokens` is the uncached remainder.
    pub fn cost_usd(&self, u: &Usage) -> f64 {
        self.terms(u)
            .iter()
            .map(|&(tokens, per_mtok)| tokens as f64 * per_mtok)
            .sum::<f64>()
            / 1_000_000.0
    }

    /// What a call's usage costs in micro-dollars, the budget's unit: input,
    /// output, cache reads, and cache writes each at its own price, rounded
    /// up to the next micro-dollar (theseus-0sg).
    pub fn cost_micros(&self, u: &Usage) -> Micros {
        micros_of(&self.terms(u))
    }

    /// Each token class of a usage with its price. The writes are split by
    /// TTL: those of `cache_creation_1h_input_tokens` at the 1-hour price, the
    /// rest at the 5-minute one (theseus-ev1).
    fn terms(&self, u: &Usage) -> [(u64, f64); 5] {
        let w1h = u
            .cache_creation_1h_input_tokens
            .min(u.cache_creation_input_tokens);
        [
            (u.input_tokens, self.input_per_mtok),
            (u.output_tokens, self.output_per_mtok),
            (u.cache_read_input_tokens, self.cache_read_per_mtok),
            (
                u.cache_creation_input_tokens - w1h,
                self.cache_write_per_mtok,
            ),
            (w1h, self.cache_write_1h_per_mtok),
        ]
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

/// About how many input tokens an image costs a model (theseus-9g2), for
/// the budget reservation; the provider's usage is what is charged. Claude
/// scales an image down to fit its long-edge limit, then counts one token
/// per 28×28 tile, up to a cap: 1,568 px and 1,568 tokens on Haiku 4.5,
/// 2,576 px and 4,784 tokens on the models from Claude 4.7 on. GLM models are
/// estimated the same way.
pub fn image_tokens(model: &str, width: u32, height: u32) -> u64 {
    let (long_edge, cap) = if model.starts_with("claude-haiku-4") {
        (1_568.0, 1_568)
    } else {
        (2_576.0, 4_784)
    };
    let (w, h) = (width.max(1) as f64, height.max(1) as f64);
    let scale = (long_edge / w.max(h)).min(1.0);
    let tiles = |px: f64| ((px * scale).round().max(1.0) / 28.0).ceil() as u64;
    (tiles(w) * tiles(h)).min(cap)
}

/// About how many input tokens `pages` pages of a PDF cost a model besides
/// their text (theseus-c9l6), for the budget reservation; the provider's
/// usage is what is charged. The provider reads each page as its text and
/// as a picture of it; `PDF_PAGE_TOKENS` is the picture's share, and the
/// text counts at the model's rate for prose.
pub fn pdf_tokens(model: &str, pages: u32, text_bytes: u64) -> u64 {
    let rates = TokenRates::of(model);
    u64::from(pages) * PDF_PAGE_TOKENS + (text_bytes as f64 / rates.text).ceil() as u64
}

/// A PDF page's picture, in tokens, on Claude's models: measured on Sonnet
/// 5.5 by the files lane's live check (theseus-c9l6).
pub const PDF_PAGE_TOKENS: u64 = 1_600;

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
    /// Omitted on a new model: 2 × its input price, Anthropic's rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_1h_per_mtok: Option<f64>,
    /// Omitted on a new model: true, so its requests carry breakpoints as
    /// every request did before the catalog said (theseus-ev1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caches: Option<bool>,
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
    pub pdf: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Omitted on a new model: its family's figures ([`TokenRates::of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_per_token: Option<TokenRates>,
}

impl CatalogRow {
    /// The prices this row sets, by key, for checking them. A new model
    /// must name the first four; the 1-hour write price has a default.
    pub fn prices(&self) -> [(&'static str, Option<f64>); 5] {
        [
            ("input_per_mtok", self.input_per_mtok),
            ("output_per_mtok", self.output_per_mtok),
            ("cache_read_per_mtok", self.cache_read_per_mtok),
            ("cache_write_per_mtok", self.cache_write_per_mtok),
            ("cache_write_1h_per_mtok", self.cache_write_1h_per_mtok),
        ]
    }

    /// Whether this row, over the built-in `base`, changes none of its
    /// figures: a copy of the code's row, as the template's tables were
    /// until theseus-vwar. Its `source` alone changes nothing: it says only
    /// where the figures came from.
    pub fn copies(&self, base: &CatalogEntry) -> bool {
        self.over(Some(base)).is_ok_and(|e| {
            CatalogEntry {
                source: base.source.clone(),
                ..e
            } == *base
        })
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
                .chain(self.prices()[..4].iter().map(|&(k, v)| (k, v.is_none())))
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
                    cache_write_1h_per_mtok: 2.0 * self.input_per_mtok.unwrap_or(0.0),
                    caches: true,
                    thinking: ThinkingMode::None,
                    effort: false,
                    refusal_fallbacks: false,
                    cache_min_tokens: 0,
                    vision: false,
                    pdf: false,
                    source: String::new(),
                    bytes_per_token: TokenRates::default(),
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
            cache_write_1h_per_mtok: r
                .cache_write_1h_per_mtok
                .unwrap_or(entry.cache_write_1h_per_mtok),
            caches: r.caches.unwrap_or(entry.caches),
            thinking: r.thinking.unwrap_or(entry.thinking),
            effort: r.effort.unwrap_or(entry.effort),
            refusal_fallbacks: r.refusal_fallbacks.unwrap_or(entry.refusal_fallbacks),
            cache_min_tokens: r.cache_min_tokens.unwrap_or(entry.cache_min_tokens),
            vision: r.vision.unwrap_or(entry.vision),
            pdf: r.pdf.unwrap_or(entry.pdf),
            source: r.source.unwrap_or_else(|| "config".into()),
            bytes_per_token: r.bytes_per_token.unwrap_or(entry.bytes_per_token),
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
        cache_write_1h_per_mtok: 2.0 * input,
        caches: true,
        thinking,
        effort: thinking != ThinkingMode::Budget,
        refusal_fallbacks: fallbacks,
        cache_min_tokens: cache_min,
        vision: true,
        pdf: true,
        source: "Claude API reference (2026-06-24 model table; model-migration pricing)".into(),
        bytes_per_token: TokenRates::CLAUDE,
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
        // Z.ai reports no writes split by TTL, so any write is priced as one.
        cache_write_1h_per_mtok: cache_write,
        caches: true,
        thinking: ThinkingMode::None,
        effort: false,
        refusal_fallbacks: false,
        cache_min_tokens: 0,
        vision: true,
        // Z.ai's Anthropic-compatible endpoint takes no document blocks:
        // GLM reads a PDF's text (theseus-c9l6).
        pdf: false,
        source: "OpenClaw model catalog (models.providers.zai), 2026-09".into(),
        bytes_per_token: TokenRates::GLM,
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
        // The caching minimum is the reference's 512 (theseus-ev1); the
        // table said 1,024, Sonnet 5's, until 2026-10-01.
        e.insert(
            "claude-sonnet-5-5".into(),
            CatalogEntry {
                source: "Anthropic Models API and pricing page, 2026-09-28".into(),
                ..claude(m, 128_000, 2.0, 10.0, 0.20, 2.50, Adaptive, false, 512)
            },
        );
        e.insert(
            "claude-sonnet-5".into(),
            claude(m, 128_000, 2.0, 10.0, 0.20, 2.50, Adaptive, false, 1024),
        );
        e.insert(
            "claude-haiku-4-5".into(),
            CatalogEntry {
                bytes_per_token: TokenRates::CLAUDE_OLD,
                ..claude(200_000, 64_000, 1.0, 5.0, 0.10, 1.25, Budget, false, 4096)
            },
        );
        // Text only (theseus-9g2): OpenClaw's model table marks glm-5.3 and
        // glm-5.2 text-only, and glm-5.3 answered a test PNG with empty text
        // (2026-09-29). The "no vision" line beats a silent empty answer.
        e.insert(
            "glm-5.3".into(),
            CatalogEntry {
                vision: false,
                ..glm(1_000_000, 128_000, 1.40, 4.40, 0.26, 1.40)
            },
        );
        e.insert(
            "glm-5.2".into(),
            CatalogEntry {
                vision: false,
                ..glm(1_000_000, 128_000, 1.40, 4.40, 0.26, 1.40)
            },
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
    /// complete table adds a model. A table that copies the code's row
    /// changes nothing and counts for nothing (theseus-vwar): the row stays
    /// the code's, source and all. Every other table changes the version
    /// string, so a priced row says so. `Config::validate` refuses an
    /// incomplete new model, so none is skipped here.
    pub fn with_overrides(overrides: &BTreeMap<String, CatalogRow>) -> Self {
        let mut c = Self::builtin();
        let mut changed = 0;
        for (k, row) in overrides {
            let base = c.entries.get(k);
            let new_model = base.is_none();
            if base.is_some_and(|b| row.copies(b)) {
                continue;
            }
            if let Ok(mut e) = row.over(base) {
                if new_model && row.bytes_per_token.is_none() {
                    e.bytes_per_token = TokenRates::of(k);
                }
                c.entries.insert(k.clone(), e);
                changed += 1;
            }
        }
        if changed > 0 {
            c.version = format!("{BUILTIN_VERSION}+config:{changed}");
        }
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

/// What speech costs (45b, rows 77 and 78), beside the models' token prices:
/// speech to text by the minute of audio, synthesis by the thousand
/// characters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeechPrice {
    pub provider: &'static str,
    /// A model's id, or the family its ids start with: `aura-2` prices
    /// `aura-2-andromeda-en`.
    pub model: &'static str,
    pub unit: SpeechUnit,
    /// US dollars per unit.
    pub usd: f64,
    /// Where the figure came from.
    pub source: &'static str,
}

/// What a speech price is per.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechUnit {
    /// A minute of audio, priced by the millisecond.
    Minute,
    /// A thousand characters, priced by the character.
    ThousandChars,
}

/// Deepgram's pay-as-you-go list prices, as best known (2025's), until Eddie
/// confirms them against his account: nova-3 and nova-2 pre-recorded speech
/// to text about $0.0043 a minute, Aura-2 about $0.030 per 1,000 characters,
/// and Aura (the first) $0.015.
pub const SPEECH_PRICES: &[SpeechPrice] = &[
    SpeechPrice {
        provider: "deepgram",
        model: "nova-3",
        unit: SpeechUnit::Minute,
        usd: 0.0043,
        source: "Deepgram pay-as-you-go list price, pre-recorded (2025), to confirm",
    },
    SpeechPrice {
        provider: "deepgram",
        model: "nova-2",
        unit: SpeechUnit::Minute,
        usd: 0.0043,
        source: "Deepgram pay-as-you-go list price, pre-recorded (2025), to confirm",
    },
    SpeechPrice {
        provider: "deepgram",
        model: "aura-2",
        unit: SpeechUnit::ThousandChars,
        usd: 0.030,
        source: "Deepgram pay-as-you-go list price (2025), to confirm",
    },
    SpeechPrice {
        provider: "deepgram",
        model: "aura",
        unit: SpeechUnit::ThousandChars,
        usd: 0.015,
        source: "Deepgram pay-as-you-go list price (2025), to confirm",
    },
];

/// The price of `provider`'s `model`: its own row, or its family's, the
/// longest that names it (`aura-2-…` is Aura-2's, not Aura's).
pub fn speech_price(provider: &str, model: &str) -> Option<&'static SpeechPrice> {
    SPEECH_PRICES
        .iter()
        .filter(|p| p.provider == provider)
        .filter(|p| {
            model == p.model
                || model
                    .strip_prefix(p.model)
                    .is_some_and(|rest| rest.starts_with('-'))
        })
        .max_by_key(|p| p.model.len())
}

impl SpeechPrice {
    /// What a call costs in micro-dollars, rounded up once: its audio by the
    /// millisecond at the minute's price, or its characters at the
    /// thousand's.
    pub fn cost_micros(&self, audio: std::time::Duration, chars: usize) -> Micros {
        let per_unit = u128::from(usd_to_micros(self.usd));
        let scaled = match self.unit {
            SpeechUnit::Minute => (audio.as_millis() * per_unit).div_ceil(60_000),
            SpeechUnit::ThousandChars => (chars as u128 * per_unit).div_ceil(1_000),
        };
        u64::try_from(scaled).unwrap_or(u64::MAX)
    }
}

/// What a Jev judgment costs (M5 23a; design §2.6), beside the models' and
/// speech's prices: each pinned Jev model's row, built in only. A pack whose
/// model has no row here is never called (`unpriced`). Jev is billed by the
/// token, input and output, with no cache prices.
pub fn judge_prices() -> BTreeMap<String, theseus_judge::JevPrice> {
    let row = theseus_judge::JevPrice::jev_1_13_0();
    [(theseus_judge::price::JEV_MODEL.to_string(), row)].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jev's pinned model is priced (M5 23a): every pack's model has a row,
    /// a call's cost is its usage at that row's prices, rounded up, and the
    /// models' table is unchanged by it.
    #[test]
    fn every_packs_jev_model_is_priced() {
        let prices = judge_prices();
        let packs = theseus_judge::pack::embedded().as_ref().unwrap();
        for p in packs
            .iter()
            .filter(|p| p.jev_model != theseus_judge::judge::UNPINNED)
        {
            assert!(
                prices.contains_key(&p.jev_model),
                "{} is unpriced",
                p.name()
            );
        }
        let row = &prices["jev-1.13.0"];
        assert_eq!(
            (row.kind.as_str(), row.provider.as_str()),
            ("judge", "typesafe")
        );
        let u = theseus_judge::Usage {
            input_tokens: 2_000,
            output_tokens: 100,
        };
        // 2,100 tokens at $0.042 a million: 88.2 micro-dollars, rounded up.
        assert_eq!(row.cost_micros(&u), 89);
        assert!(Catalog::builtin().get("jev-1.13.0").is_none());
    }

    #[test]
    fn builtin_rows_are_sane_and_priced() {
        let c = Catalog::builtin();
        for (id, e) in &c.entries {
            assert!(e.context_window >= 200_000, "{id}");
            assert!(e.max_output_tokens >= 64_000, "{id}");
            assert!(e.output_per_mtok >= e.input_per_mtok, "{id}");
            assert!(e.cache_read_per_mtok < e.input_per_mtok, "{id}");
            assert!(e.caches, "{id}: every built-in model caches");
            if e.provider == "anthropic" {
                // The reference: 1.25 × input for 5 minutes, 2 × for an hour.
                assert_eq!(e.cache_write_per_mtok, 1.25 * e.input_per_mtok, "{id}");
                assert_eq!(e.cache_write_1h_per_mtok, 2.0 * e.input_per_mtok, "{id}");
            } else {
                assert_eq!(e.cache_write_1h_per_mtok, e.cache_write_per_mtok, "{id}");
            }
        }
        // The caching minimums, from the claude-api skill's reference.
        let min = |id: &str| c.get(id).unwrap().cache_min_tokens;
        for id in [
            "claude-fable-5-1",
            "claude-fable-5",
            "claude-opus-5-5",
            "claude-opus-5",
            "claude-sonnet-5-5",
        ] {
            assert_eq!(min(id), 512, "{id}");
        }
        assert_eq!(
            (min("claude-opus-4-8"), min("claude-sonnet-5")),
            (1024, 1024)
        );
        assert_eq!(min("claude-haiku-4-5"), 4096);
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_input_tokens: 1_000_000,
            cache_creation_input_tokens: 1_000_000,
            ..Default::default()
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

    /// Which models see images, and what an image is estimated to cost
    /// (theseus-9g2).
    #[test]
    fn glm_5_3_and_5_2_are_text_only_and_images_cost_their_tiles() {
        let c = Catalog::builtin();
        let blind: Vec<&str> = c
            .entries
            .iter()
            .filter(|(_, e)| !e.vision)
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(blind, vec!["glm-5.2", "glm-5.3"]);
        assert!(c.get("glm-5.3-flash").unwrap().vision);
        // 1920×1080 fits Sonnet 5.5's 2,576 px: 69 × 39 tiles of 28 px.
        assert_eq!(image_tokens("claude-sonnet-5-5", 1920, 1080), 2_691);
        // Haiku 4.5 scales it to 1568×882 first: 56 × 32, capped at 1,568.
        assert_eq!(image_tokens("claude-haiku-4-5", 1920, 1080), 1_568);
        assert_eq!(image_tokens("claude-haiku-4-5", 200, 100), 8 * 4);
        // A huge square is capped, never absurd.
        assert_eq!(image_tokens("claude-opus-5-5", 8_000, 8_000), 4_784);
        assert_eq!(image_tokens("glm-5.3-flash", 1, 1), 1);
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
        o.insert("new-model".into(), full.clone());
        let c = Catalog::with_overrides(&o);
        let new = c.get("new-model").unwrap();
        assert_eq!(new.max_output_tokens, 8_192);
        // Unnamed, a new model's 1-hour writes cost 2 × input, and it caches.
        assert_eq!((new.cache_write_1h_per_mtok, new.caches), (2.0, true));
        assert!(c.version.ends_with("+config:2"), "{}", c.version);
        // A copy of the code's row changes nothing and counts for nothing
        // (theseus-vwar), whatever source it names; one figure apart, it
        // counts, and names its source.
        let code = Catalog::builtin();
        let s55 = code.get("claude-sonnet-5-5").unwrap();
        let copy = CatalogRow {
            input_per_mtok: Some(s55.input_per_mtok),
            output_per_mtok: Some(s55.output_per_mtok),
            source: Some("a price sheet".into()),
            ..Default::default()
        };
        assert!(copy.copies(s55));
        o.insert("claude-sonnet-5-5".into(), copy.clone());
        let c = Catalog::with_overrides(&o);
        assert_eq!(c.get("claude-sonnet-5-5"), Some(s55));
        assert!(c.version.ends_with("+config:2"), "{}", c.version);
        let apart = CatalogRow {
            output_per_mtok: Some(s55.output_per_mtok + 1.0),
            ..copy
        };
        assert!(!apart.copies(s55));
        o.insert("claude-sonnet-5-5".into(), apart);
        let c = Catalog::with_overrides(&o);
        let got = c.get("claude-sonnet-5-5").unwrap();
        assert_eq!(
            (got.output_per_mtok, got.source.as_str()),
            (s55.output_per_mtok + 1.0, "a price sheet")
        );
        assert!(c.version.ends_with("+config:3"), "{}", c.version);
        // Named, they replace the defaults; over a built-in row, the rest stays.
        let named = CatalogRow {
            cache_write_1h_per_mtok: Some(1.5),
            caches: Some(false),
            ..full
        };
        let e = named.over(None).unwrap();
        assert_eq!((e.cache_write_1h_per_mtok, e.caches), (1.5, false));
        let s55 = CatalogRow {
            caches: Some(false),
            ..Default::default()
        }
        .over(Catalog::builtin().get("claude-sonnet-5-5"))
        .unwrap();
        assert_eq!((s55.cache_write_1h_per_mtok, s55.caches), (4.0, false));
    }

    /// A 1-hour cache write costs 2 × input, against 1.25 × for a 5-minute
    /// one (theseus-ev1). `cache_creation_input_tokens` counts both, and
    /// `cache_creation_1h_input_tokens` the 1-hour part, so 10,000 writes of
    /// which 4,000 are 1-hour cost 6,000 × $2.50 + 4,000 × $4.00 on Sonnet
    /// 5.5: 15,000 + 16,000 µ$. Priced all at the 5-minute rate, as before,
    /// they would cost 25,000 µ$, 37.5% short on the 1-hour part.
    #[test]
    fn one_hour_cache_writes_cost_twice_the_input_price() {
        let s55 = Catalog::builtin().get("claude-sonnet-5-5").unwrap().clone();
        assert_eq!(
            (s55.cache_write_per_mtok, s55.cache_write_1h_per_mtok),
            (2.5, 4.0)
        );
        let u = Usage {
            cache_creation_input_tokens: 10_000,
            cache_creation_1h_input_tokens: 4_000,
            ..Default::default()
        };
        assert_eq!(s55.cost_micros(&u), 31_000);
        assert!((s55.cost_usd(&u) - 0.031).abs() < 1e-12);
        let all_5m = Usage {
            cache_creation_1h_input_tokens: 0,
            ..u
        };
        assert_eq!(s55.cost_micros(&all_5m), 25_000);
        // Every write an hour long: 2 × input exactly.
        let all_1h = Usage {
            cache_creation_input_tokens: 1_000_000,
            cache_creation_1h_input_tokens: 1_000_000,
            ..Default::default()
        };
        assert_eq!(
            s55.cost_micros(&all_1h),
            2 * s55.cost_micros(&Usage {
                input_tokens: 1_000_000,
                ..Default::default()
            })
        );
        // A count above the total is held to it, never a negative 5-minute part.
        let odd = Usage {
            cache_creation_input_tokens: 100,
            cache_creation_1h_input_tokens: 300,
            ..Default::default()
        };
        assert_eq!(s55.cost_micros(&odd), 400);
        // Opus 5.5 and Haiku 4.5: $8 and $2 per million 1-hour writes.
        let c = Catalog::builtin();
        assert_eq!(
            c.get("claude-opus-5-5").unwrap().cache_write_1h_per_mtok,
            8.0
        );
        assert_eq!(
            c.get("claude-haiku-4-5").unwrap().cache_write_1h_per_mtok,
            2.0
        );
    }

    /// The 1-hour count is new in `Usage`: a row or record written before it
    /// reads as none, and a usage without 1-hour writes serializes as before.
    #[test]
    fn a_usage_from_before_the_1h_count_reads_as_none() {
        let old = r#"{"input_tokens":5,"output_tokens":7,"cache_read_input_tokens":11,"cache_creation_input_tokens":13}"#;
        let u: Usage = serde_json::from_str(old).unwrap();
        assert_eq!(u.cache_creation_1h_input_tokens, 0);
        assert_eq!(u.cache_creation_input_tokens, 13);
        assert_eq!(serde_json::to_string(&u).unwrap(), old);
        let older: Usage = serde_json::from_str(r#"{"input_tokens":1,"output_tokens":2}"#).unwrap();
        assert_eq!(
            older,
            Usage {
                input_tokens: 1,
                output_tokens: 2,
                ..Default::default()
            }
        );
        let with = Usage {
            cache_creation_1h_input_tokens: 3,
            ..u
        };
        let text = serde_json::to_string(&with).unwrap();
        assert!(
            text.ends_with(r#""cache_creation_1h_input_tokens":3}"#),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<Usage>(&text).unwrap(), with);
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
            ..Default::default()
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

    /// The figures against the provider's counts of whole first requests
    /// (theseus-f5hf; the tokens lane's live check, 2026-10-01): Eddie's 15
    /// tools and header with a small context file, the same with 6.5 KB of
    /// prose in it, the same again as a tool loop's first call, and with no
    /// tools; on Sonnet 5.5, then on GLM-5.3 Flash. Each estimate is within
    /// 7 % of the count, and its bound (`compiler::MARGIN_PERCENT`) is over
    /// it. chars/4, the estimate before, read Sonnet 5.5's at 66 to 71 %.
    #[test]
    fn the_figures_hold_for_recorded_first_requests() {
        use crate::provider::Census;
        // (model, JSON bytes, text bytes, the provider's count, chars/4)
        let recorded = [
            ("claude-sonnet-5-5", 11_898, 1_631, 5_236, 3_436),
            ("claude-sonnet-5-5", 11_898, 8_191, 7_196, 5_082),
            ("claude-sonnet-5-5", 11_898, 1_678, 5_253, 3_448),
            ("claude-sonnet-5-5", 0, 444, 155, 139),
            ("glm-5.3-flash", 11_898, 1_631, 3_525, 3_436),
            ("glm-5.3-flash", 11_898, 8_191, 4_971, 5_082),
            ("glm-5.3-flash", 11_898, 1_678, 3_536, 3_448),
            ("glm-5.3-flash", 0, 444, 112, 157),
        ];
        let c = Catalog::builtin();
        for (model, json, text, count, chars4) in recorded {
            // Two system blocks and one message of one block.
            let census = Census {
                json,
                text,
                messages: 3,
                blocks: 1,
                ..Default::default()
            };
            let est = census.tokens(c.get(model).unwrap().bytes_per_token);
            let off = est as f64 / count as f64 - 1.0;
            assert!(
                off.abs() < 0.07,
                "{model}, {json} + {text} bytes: {est} against {count}"
            );
            assert!(est * 140 / 100 >= count, "{model}: {est}");
            if model.starts_with("claude") && json > 0 {
                assert!(
                    (chars4 as f64) < count as f64 * 0.72,
                    "{chars4} against {count}"
                );
            }
        }
    }

    /// Speech's prices are the code's (45b): nova-3 by the millisecond at
    /// $0.0043 a minute, Aura-2 by the character at $0.030 a thousand, and a
    /// voice priced by its family, the longest that names it.
    #[test]
    fn speech_is_priced_by_the_minute_heard_and_the_characters_said() {
        use std::time::Duration;
        let nova = speech_price("deepgram", "nova-3").unwrap();
        assert_eq!(nova.unit, SpeechUnit::Minute);
        assert_eq!(nova.cost_micros(Duration::from_secs(60), 0), 4_300);
        assert_eq!(nova.cost_micros(Duration::from_secs(3), 99), 215);
        // A millisecond's fraction of a micro-dollar rounds up, once.
        assert_eq!(nova.cost_micros(Duration::from_millis(1), 0), 1);
        let andromeda = speech_price("deepgram", "aura-2-andromeda-en").unwrap();
        assert_eq!(andromeda.model, "aura-2", "Aura-2's, not the first Aura's");
        assert_eq!(andromeda.cost_micros(Duration::from_secs(9), 1_000), 30_000);
        assert_eq!(andromeda.cost_micros(Duration::ZERO, 79), 2_370);
        assert_eq!(
            speech_price("deepgram", "aura-asteria-en").unwrap().usd,
            0.015
        );
        assert!(
            speech_price("deepgram", "nova-30").is_none(),
            "a family is a whole word"
        );
        assert!(speech_price("deepgram", "whisper-large").is_none());
        assert!(speech_price("stand-in", "nova-3").is_none());
        for p in SPEECH_PRICES {
            assert!(p.usd > 0.0 && p.source.contains("to confirm"), "{p:?}");
        }
    }
}
