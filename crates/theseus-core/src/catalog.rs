//! The model catalog (spec Part II P5, decided with the owner 2026-09-26): per model,
//! the serving provider, context window, output ceiling, prices, and the
//! capabilities that change how a request is built. A versioned table: prices
//! and limits cannot self-update (the provider's models endpoint lists ids, not
//! prices), so every priced ledger row names the catalog version that priced it.
//!
//! Sources for the built-in rows: the Claude API reference (Anthropic models;
//! cache writes at 1.25 × input for the 5-minute TTL and 2 × for the 1-hour
//! one), docs.z.ai (GLM) and api-docs.deepseek.com (DeepSeek), read 2026-10-17.
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
//!
//! Haiku 5.5 (released 2026-10-07) comes from the pricing page and its model
//! page, read on release day, and the Haiku lane's live probe (theseus-3okf):
//! the Models API's entry (1M window, 128K out, adaptive thinking, effort),
//! its token counts (the same as Sonnet 5.5's on prose, Rust and JSON), and its
//! caching minimum (a 498-token prefix was not cached, a 542-token one was).
//! It is the first model priced in tiers: see [`LongPrompt`].
//!
//! `off_peak` (theseus-9f72b) is a provider's cheaper hours (DeepSeek's: half of
//! input, output and cache read outside 01:00 to 04:00 and 06:00 to 10:00 UTC on
//! weekdays, [`price_period_at`]). A row's own prices stay the peak ones, which
//! every reserve and budget uses; only the ledger's `cost_*_at` reads the hour a
//! call was made. `default_effort` is the effort a model runs at when Jev leaves
//! it to the model.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use theseus_judge::builders::PricePeriod;
use theseus_kernel::{usd_to_micros, Micros, MICROS_PER_USD};
use theseus_protocol::Usage;

use crate::config::Effort;

mod peak;
pub use peak::price_period_at;
mod switch;
pub use switch::{switch_cost, CANDIDATES as SWITCH_CANDIDATES};

pub const BUILTIN_VERSION: &str = "2026-10-29.1";

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
    /// The model thinks without being asked; Theseus sends no `thinking`
    /// field (GLM always thinks, DeepSeek thinks by default). Its thinking
    /// blocks come back like any model's.
    Implicit,
}

/// The `thinking` that turns `model`'s thinking off for a short answer
/// (theseus-050g2): a model that thinks by default spends a small cap on
/// thinking and stops on `max_tokens` with a fragment of text. Sonnet 5.5
/// refuses `disabled` (a 400 that names `between_tools`, seen live
/// 2026-10-23) and takes `between_tools`; the other adaptive models take
/// `disabled` at their default effort. A model that always thinks (Fable
/// 5.x, Opus 5.5) cannot turn it off, so its thinking counts in the cap; and
/// one Theseus sends no `thinking` (GLM, Haiku 4.5, an unknown model) gets
/// none.
pub fn thinking_off(model: &str, entry: Option<&CatalogEntry>) -> Option<serde_json::Value> {
    match entry?.thinking {
        ThinkingMode::Adaptive => {
            let family = model.rsplit('/').next().unwrap_or(model);
            let off = match family.starts_with("claude-sonnet-5-5") {
                true => "between_tools",
                false => "disabled",
            };
            Some(serde_json::json!({ "type": off }))
        }
        ThinkingMode::None
        | ThinkingMode::Always
        | ThinkingMode::Budget
        | ThinkingMode::Implicit => None,
    }
}

/// Where a model was made (theseus-a1wvq): the owner's data rule reads it.
/// A model of `cn` or `unknown` origin is restricted: it sees only a request
/// cleared as open. Named `ModelOrigin` because `crate::node::Origin` exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin {
    Us,
    Eu,
    Cn,
    #[default]
    Unknown,
}

/// The origins private data may go to (theseus-xs5ah, the owner: such data
/// "must be routed to us-based models"): a motif drawn from private sessions
/// (`rsi::mint::data_rule`), which is all that reads it now. An allowlist, so
/// an origin not listed here, `unknown` included, is refused. A name in
/// `[privacy] sensitive_names` and a Google result are not read against it:
/// each raises a removable label (theseus-gyjyo.4, theseus-gyjyo.5).
const SENSITIVE_ORIGINS: [ModelOrigin; 1] = [ModelOrigin::Us];

impl ModelOrigin {
    pub fn restricted(self) -> bool {
        matches!(self, ModelOrigin::Cn | ModelOrigin::Unknown)
    }

    /// Whether a model of this origin may be sent private data: never a
    /// restricted one, and then only an origin on the allowlist.
    pub fn cleared_for_sensitive(self) -> bool {
        !self.restricted() && SENSITIVE_ORIGINS.contains(&self)
    }
}

/// Hosts of China-origin endpoints: the host or any subdomain of it.
const CN_HOSTS: [&str; 7] = [
    "deepseek.com",
    "z.ai",
    "bigmodel.cn",
    "moonshot.ai",
    "moonshot.cn",
    "kimi.ai",
    "kimi.com",
];

/// A model's origin, first match wins (theseus-a1wvq): a China-origin
/// endpoint, whatever model id it is asked for (DeepSeek's Anthropic endpoint
/// serves any `claude*` id with a DeepSeek model); a China-origin family on
/// any host (OpenRouter, Bedrock, self-hosted weights); the catalog row's `cn`;
/// on Anthropic's first party or a loopback host, the row's origin; any other
/// host, what the provider vouches for, else `Unknown`. A row's `us` never
/// vouches for a host it was not served from.
pub fn origin_of(
    api_base: &str,
    vouched: Option<ModelOrigin>,
    model: &str,
    entry: Option<&CatalogEntry>,
) -> ModelOrigin {
    if is_cn_endpoint(api_base) {
        return ModelOrigin::Cn;
    }
    let host = host_of(api_base);
    let id = model.to_ascii_lowercase();
    let family = id.rsplit('/').next().unwrap_or(&id);
    if id.contains("deepseek")
        || id.contains("kimi")
        || id.contains("moonshot")
        || family.starts_with("glm")
        || id.contains("z-ai")
        || id.contains("zai-org")
    {
        return ModelOrigin::Cn;
    }
    let row = entry.map_or(ModelOrigin::Unknown, |e| e.origin);
    if row == ModelOrigin::Cn {
        return ModelOrigin::Cn;
    }
    if host == "api.anthropic.com" || matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost") {
        return row;
    }
    vouched.unwrap_or(ModelOrigin::Unknown)
}

/// Whether private data may go to `model` (theseus-xs5ah): its origin is on the allowlist, and the catalog's
/// own row says `us` as well. A host the operator vouches `us` for (a proxy,
/// an aggregator) may serve families the catalog does not know; their rows
/// are `unknown`, so the vouch alone clears nothing.
pub fn cleared_for_sensitive(
    api_base: &str,
    vouched: Option<ModelOrigin>,
    model: &str,
    entry: Option<&CatalogEntry>,
) -> bool {
    origin_of(api_base, vouched, model, entry).cleared_for_sensitive()
        && entry.is_some_and(|e| e.origin == ModelOrigin::Us)
}

/// Whether a URL's host is a China-origin endpoint or a subdomain of one.
pub fn is_cn_endpoint(api_base: &str) -> bool {
    let host = host_of(api_base);
    CN_HOSTS
        .iter()
        .any(|h| host == *h || host.strip_suffix(h).is_some_and(|p| p.ends_with('.')))
}

/// The lowercased host of a URL: no scheme, userinfo, port, path, or the
/// brackets of an IPv6 literal.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let auth = auth.rsplit('@').next().unwrap_or(auth);
    let host = match auth.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => auth.split(':').next().unwrap_or(""),
    };
    host.trim_end_matches('.').to_ascii_lowercase()
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
    /// The prices above a prompt length, where the model has a second tier
    /// (Haiku 5.5): see [`LongPrompt`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_prompt: Option<LongPrompt>,
    /// The provider reads repeated prefixes from its cache and reports it
    /// (the module's notes); without it a request carries no breakpoints.
    #[serde(default = "yes")]
    pub caches: bool,
    #[serde(default)]
    pub thinking: ThinkingMode,
    /// `output_config.effort` is accepted.
    #[serde(default)]
    pub effort: bool,
    /// The model takes an effort-only system message in its history
    /// (per-message effort, theseus-o719), so a turn's level changes no top-level
    /// effort and the messages' cache stays; without it Jev's level is sticky.
    #[serde(default)]
    pub per_message_effort: bool,
    /// Server-side refusal fallbacks (`fallbacks: "default"`) are supported.
    #[serde(default)]
    pub refusal_fallbacks: bool,
    /// The model a refused request is made again on, client-side
    /// (theseus-7gir.18): once, by the same provider, and the rest of its turn
    /// runs there. Where the server-side fallback applies, it is used instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_fallback_model: Option<String>,
    /// Shortest prefix the provider will cache; shorter prefixes silently do not.
    #[serde(default)]
    pub cache_min_tokens: u32,
    #[serde(default)]
    pub vision: bool,
    /// The model reads a PDF as a `document` block, its pages' text and
    /// pictures both (theseus-c9l6); any other reads the PDF's text.
    #[serde(default)]
    pub pdf: bool,
    /// The provider's tool search takes `defer_loading` tools on this model
    /// (theseus-7m7w.2); without it, hidden tools are left out and the model
    /// is offered `tools_find`.
    #[serde(default)]
    pub tool_search: bool,
    /// Where the figures came from.
    #[serde(default)]
    pub source: String,
    /// How densely the model's tokenizer reads a request (theseus-f5hf).
    #[serde(default)]
    pub bytes_per_token: TokenRates,
    /// Where the model was made; `unknown` (restricted) unless a row says.
    #[serde(default)]
    pub origin: ModelOrigin,
    /// The effort the model runs at when Jev answers `default` or is under the
    /// confirm bar (theseus-9f72b): the model's own default, none where no
    /// vendor figure is on record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<Effort>,
    /// The provider's off-peak prices, where it has them (DeepSeek's): the
    /// ledger prices a call at the rate in force when it was made. The row's
    /// own prices stay the peak ones, which reserves and budgets use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_peak: Option<OffPeak>,
}

/// A provider's off-peak prices, in dollars per million tokens (DeepSeek's
/// are half its peak ones). Cache writes follow the input price: a model whose
/// writes cost its input price (DeepSeek's) pays the off-peak input price for
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffPeak {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}

fn yes() -> bool {
    true
}

/// A model's second price tier (theseus-3okf): a call whose prompt is longer
/// than `above_tokens` pays these prices for every class of its tokens, its
/// output included. Haiku 5.5's is 5 × its base above 100,000 tokens (the
/// pricing page, 2026-10-07).
///
/// Which tokens count toward the threshold is not published. Theseus counts
/// the whole prompt, the uncached input, the cache reads, and the cache
/// writes, which is the reading that never under-charges; the owner's bill is
/// what settles it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LongPrompt {
    /// The tier applies to a prompt of more than this many tokens.
    pub above_tokens: u64,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
    pub cache_write_1h_per_mtok: f64,
}

/// The five prices a call pays: the base row's, or its long tier's.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tier {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    cache_write_1h: f64,
}

/// Bytes a model's tokenizer reads as one token, by what the bytes are
/// (theseus-f5hf), for the compiler's estimate of a request
/// (`provider::Census`). Measured on Theseus's own requests against the
/// provider's count (the tokens lane, 2026-10-01). A tokenizer reads JSON,
/// code, and command output far more densely than prose. On Sonnet 5.5 the
/// 15 tool schemas, with the provider's tool prompt, ran 2.5 bytes a token,
/// a Rust file read by a tool 2.32, the tool results in the owner's DM 1.76 to
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
        } else if model.starts_with("deepseek-") || model.starts_with("kimi-") {
            // Unmeasured: Claude's densest rates over-estimate, the safe
            // side for the keel. A probe of DeepSeek's own count is owed.
            Self::CLAUDE
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
    ///
    /// At the row's own prices, which are the peak ones where the model has an
    /// off-peak row: what a reserve and a budget count (the safe direction).
    pub fn cost_usd(&self, u: &Usage) -> f64 {
        sum_usd(&self.terms_at(u, None))
    }

    /// What a call's usage costs in micro-dollars, the budget's unit: input,
    /// output, cache reads, and cache writes each at its own price, rounded
    /// up to the next micro-dollar (theseus-0sg). At the row's own prices, as
    /// [`cost_usd`](Self::cost_usd).
    pub fn cost_micros(&self, u: &Usage) -> Micros {
        micros_of(&self.terms_at(u, None))
    }

    /// [`cost_usd`](Self::cost_usd) at the rate in force at `at_ms` (Unix
    /// milliseconds): a model with an off-peak row pays it in the provider's
    /// off-peak hours ([`price_period_at`]), every other the row's prices.
    pub fn cost_usd_at(&self, u: &Usage, at_ms: u64) -> f64 {
        sum_usd(&self.terms_at(u, Some(at_ms)))
    }

    /// [`cost_micros`](Self::cost_micros) at the rate in force at `at_ms`: what
    /// the ledger charges a call made then.
    pub fn cost_micros_at(&self, u: &Usage, at_ms: u64) -> Micros {
        micros_of(&self.terms_at(u, Some(at_ms)))
    }

    /// The uncached input price per million tokens of a prompt of
    /// `prompt_tokens`: the long tier's past its threshold (theseus-h66v4.1).
    pub fn input_price_at(&self, prompt_tokens: u64) -> f64 {
        self.tier(prompt_tokens).input
    }

    /// The cache-read price per million tokens of a prompt of `prompt_tokens`.
    pub fn cache_read_price_at(&self, prompt_tokens: u64) -> f64 {
        self.tier(prompt_tokens).cache_read
    }

    /// The prices a call with a prompt of `prompt_tokens` pays: the long
    /// tier's past its threshold, the row's own otherwise.
    fn tier(&self, prompt_tokens: u64) -> Tier {
        match self.long_prompt {
            Some(l) if prompt_tokens > l.above_tokens => Tier {
                input: l.input_per_mtok,
                output: l.output_per_mtok,
                cache_read: l.cache_read_per_mtok,
                cache_write: l.cache_write_per_mtok,
                cache_write_1h: l.cache_write_1h_per_mtok,
            },
            _ => Tier {
                input: self.input_per_mtok,
                output: self.output_per_mtok,
                cache_read: self.cache_read_per_mtok,
                cache_write: self.cache_write_per_mtok,
                cache_write_1h: self.cache_write_1h_per_mtok,
            },
        }
    }

    /// The prices a call pays in a provider's off-peak hours: `off`'s three,
    /// and the cache writes at the same share of their price that the off-peak
    /// input price is of the row's (DeepSeek bills a write as input).
    fn off_peak_tier(&self, t: Tier, off: OffPeak) -> Tier {
        let share = if self.input_per_mtok > 0.0 {
            off.input_per_mtok / self.input_per_mtok
        } else {
            1.0
        };
        Tier {
            input: off.input_per_mtok,
            output: off.output_per_mtok,
            cache_read: off.cache_read_per_mtok,
            cache_write: t.cache_write * share,
            cache_write_1h: t.cache_write_1h * share,
        }
    }

    /// Each token class of a usage with its price, at the tier its prompt
    /// (input, cache reads and cache writes: [`LongPrompt`]) picks, and, with
    /// `at_ms` in a provider's off-peak hours, the off-peak prices of a model
    /// that has them. The writes are split by TTL: those of
    /// `cache_creation_1h_input_tokens` at the 1-hour price, the rest at the
    /// 5-minute one (theseus-ev1).
    fn terms_at(&self, u: &Usage, at_ms: Option<u64>) -> [(u64, f64); 5] {
        let w1h = u
            .cache_creation_1h_input_tokens
            .min(u.cache_creation_input_tokens);
        let prompt = u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens;
        let mut t = self.tier(prompt);
        if let (Some(off), Some(at)) = (self.off_peak, at_ms) {
            if price_period_at(at) == PricePeriod::OffPeak {
                t = self.off_peak_tier(t, off);
            }
        }
        [
            (u.input_tokens, t.input),
            (u.output_tokens, t.output),
            (u.cache_read_input_tokens, t.cache_read),
            (u.cache_creation_input_tokens - w1h, t.cache_write),
            (w1h, t.cache_write_1h),
        ]
    }

    /// What a call reserves before it runs: its output cap at the output
    /// price plus its input estimate at the input price, both at the tier
    /// the estimate picks (an estimate past a long tier's threshold reserves
    /// at the long prices).
    pub fn reserve_micros(&self, max_output_tokens: u32, input_estimate: u64) -> Micros {
        let t = self.tier(input_estimate);
        micros_of(&[
            (max_output_tokens as u64, t.output),
            (input_estimate, t.input),
        ])
    }

    /// The reservation's two parts, its output and its input, each at the
    /// tier the input estimate picks (the turn's `model.calling` row).
    pub fn reserve_parts(&self, max_output_tokens: u32, input_estimate: u64) -> (Micros, Micros) {
        let t = self.tier(input_estimate);
        (
            micros_of(&[(max_output_tokens as u64, t.output)]),
            micros_of(&[(input_estimate, t.input)]),
        )
    }
}

/// Σ tokens × price, in dollars.
fn sum_usd(terms: &[(u64, f64)]) -> f64 {
    terms
        .iter()
        .map(|&(tokens, per_mtok)| tokens as f64 * per_mtok)
        .sum::<f64>()
        / 1_000_000.0
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
    /// A second price tier, whole (`[catalog."<id>".long_prompt]`): it
    /// replaces the built-in row's. Omitted: the built-in row's, or none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_prompt: Option<LongPrompt>,
    /// Omitted on a new model: true, so its requests carry breakpoints as
    /// every request did before the catalog said (theseus-ev1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caches: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_message_effort: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_fallbacks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_fallback_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_min_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_search: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Omitted on a new model: its family's figures ([`TokenRates::of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_per_token: Option<TokenRates>,
    /// Omitted on a new model: `unknown`, which is restricted. A built-in
    /// `cn` row's cannot be loosened (`Config::validate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ModelOrigin>,
    /// The effort a model runs at when Jev leaves it to the model; omitted on a
    /// new model: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<Effort>,
    /// The provider's off-peak prices, whole (`[catalog."<id>".off_peak]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_peak: Option<OffPeak>,
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
                    long_prompt: None,
                    caches: true,
                    thinking: ThinkingMode::None,
                    effort: false,
                    per_message_effort: false,
                    refusal_fallbacks: false,
                    refusal_fallback_model: None,
                    cache_min_tokens: 0,
                    vision: false,
                    pdf: false,
                    tool_search: false,
                    source: String::new(),
                    bytes_per_token: TokenRates::default(),
                    origin: ModelOrigin::Unknown,
                    default_effort: None,
                    off_peak: None,
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
            long_prompt: r.long_prompt.or(entry.long_prompt),
            caches: r.caches.unwrap_or(entry.caches),
            thinking: r.thinking.unwrap_or(entry.thinking),
            effort: r.effort.unwrap_or(entry.effort),
            per_message_effort: r.per_message_effort.unwrap_or(entry.per_message_effort),
            refusal_fallbacks: r.refusal_fallbacks.unwrap_or(entry.refusal_fallbacks),
            refusal_fallback_model: r.refusal_fallback_model.or(entry.refusal_fallback_model),
            cache_min_tokens: r.cache_min_tokens.unwrap_or(entry.cache_min_tokens),
            vision: r.vision.unwrap_or(entry.vision),
            pdf: r.pdf.unwrap_or(entry.pdf),
            tool_search: r.tool_search.unwrap_or(entry.tool_search),
            source: r.source.unwrap_or_else(|| "config".into()),
            bytes_per_token: r.bytes_per_token.unwrap_or(entry.bytes_per_token),
            origin: r.origin.unwrap_or(entry.origin),
            default_effort: r.default_effort.or(entry.default_effort),
            off_peak: r.off_peak.or(entry.off_peak),
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
        long_prompt: None,
        caches: true,
        thinking,
        effort: thinking != ThinkingMode::Budget,
        per_message_effort: false,
        refusal_fallbacks: fallbacks,
        refusal_fallback_model: None,
        cache_min_tokens: cache_min,
        vision: true,
        pdf: true,
        tool_search: true,
        source: "Claude API reference (2026-06-24 model table; model-migration pricing)".into(),
        bytes_per_token: TokenRates::CLAUDE,
        origin: ModelOrigin::Us,
        default_effort: None,
        off_peak: None,
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
        long_prompt: None,
        caches: true,
        // GLM always thinks, and `thinking: disabled` fails (docs.z.ai).
        thinking: ThinkingMode::Implicit,
        effort: true,
        per_message_effort: false,
        refusal_fallbacks: false,
        refusal_fallback_model: None,
        cache_min_tokens: 0,
        vision: true,
        // Z.ai's Anthropic-compatible endpoint takes no document blocks:
        // GLM reads a PDF's text (theseus-c9l6).
        pdf: false,
        tool_search: false,
        source: "docs.z.ai pricing and model pages, 2026-10-17".into(),
        bytes_per_token: TokenRates::GLM,
        origin: ModelOrigin::Cn,
        default_effort: None,
        off_peak: None,
    }
}

/// A DeepSeek model on its Anthropic-compatible endpoint (api-docs.deepseek.com,
/// 2026-10-17). Prices are the peak rates (off-peak is half): the catalog has
/// no time of day, and peak is the safe direction. Caching is automatic, a
/// miss is billed at the input rate, and `cache_control` is ignored. The
/// endpoint takes no document blocks.
fn deepseek(input: f64, output: f64, cache_read: f64, vision: bool, served: &str) -> CatalogEntry {
    CatalogEntry {
        provider: "deepseek".into(),
        context_window: 1_000_000,
        max_output_tokens: 384_000,
        input_per_mtok: input,
        output_per_mtok: output,
        cache_read_per_mtok: cache_read,
        cache_write_per_mtok: input,
        cache_write_1h_per_mtok: input,
        long_prompt: None,
        caches: true,
        thinking: ThinkingMode::Implicit,
        effort: true,
        per_message_effort: false,
        refusal_fallbacks: false,
        refusal_fallback_model: None,
        cache_min_tokens: 0,
        vision,
        pdf: false,
        tool_search: false,
        source: format!(
            "api-docs.deepseek.com pricing and Anthropic API pages, 2026-10-17; peak rates \
             (off-peak is half); {served}"
        ),
        bytes_per_token: TokenRates::of("deepseek-"),
        origin: ModelOrigin::Cn,
        default_effort: None,
        // Half of each of input, output and cache read, outside the peak hours
        // (`price_period_at`); the prices above stay the peak ones.
        off_peak: Some(OffPeak {
            input_per_mtok: input / 2.0,
            output_per_mtok: output / 2.0,
            cache_read_per_mtok: cache_read / 2.0,
        }),
    }
}

/// A Kimi model on Moonshot AI's Anthropic-compatible endpoint
/// (api.moonshot.ai/anthropic, vendor docs and a live probe, 2026-10-24).
/// Prices are flat over the whole context; the 5-minute and 1-hour cache
/// writes have their own, which the catalog keeps in its two write fields
/// (the 1-hour one at twice the input price, as the vendor's pro row lists
/// it). It always thinks, effort low/high/max is its one knob, caching is
/// automatic, and the endpoint takes no document blocks, no URL images and
/// no forced tool (`tool_choice` is auto, any or none).
fn kimi(input: f64, output: f64, served: &str) -> CatalogEntry {
    CatalogEntry {
        provider: "moonshot".into(),
        context_window: 1_048_576,
        max_output_tokens: 131_072,
        input_per_mtok: input,
        output_per_mtok: output,
        cache_read_per_mtok: 0.20,
        cache_write_per_mtok: input,
        cache_write_1h_per_mtok: 2.0 * input,
        long_prompt: None,
        caches: true,
        thinking: ThinkingMode::Implicit,
        effort: true,
        per_message_effort: false,
        refusal_fallbacks: false,
        refusal_fallback_model: None,
        cache_min_tokens: 0,
        vision: true,
        pdf: false,
        tool_search: false,
        source: format!("Moonshot AI Kimi API docs and a live probe, 2026-10-24; {served}"),
        bytes_per_token: TokenRates::of("kimi-"),
        origin: ModelOrigin::Cn,
        default_effort: None,
        off_peak: None,
    }
}

/// Kimi's two rows, the model and its faster variant at double the price.
fn kimi_rows(e: &mut BTreeMap<String, CatalogEntry>) {
    e.insert(
        "kimi-k3.1-pro".into(),
        kimi(2.00, 10.00, "cache writes 2.00 (5m) and 4.00 (1h)"),
    );
    e.insert(
        "kimi-k3.1-pro-highspeed".into(),
        kimi(
            4.00,
            20.00,
            "highspeed, double the price (its 1h write is assumed 8.00)",
        ),
    );
}

/// DeepSeek's two rows. `deepseek-flash` floats: DeepSeek-V4.1-Flash-1017
/// since 2026-10-17.
fn deepseek_rows(e: &mut BTreeMap<String, CatalogEntry>) {
    e.insert(
        "deepseek-flash".into(),
        deepseek(
            0.30,
            1.20,
            0.006,
            true,
            "`deepseek-flash` is a floating alias, served by DeepSeek-V4.1-Flash-1017 since \
             2026-10-17",
        ),
    );
    e.insert(
        "deepseek-v4-pro".into(),
        deepseek(1.32, 3.96, 0.044, false, "served by DeepSeek-V4-Pro-0813"),
    );
}

/// Each option model's own default effort (theseus-9f72b, design §1.4 step 5),
/// what a turn runs at when Jev answers `default` or is under the confirm bar.
/// The four Claude figures are each vendor's own model notes (the claude-api
/// reference, cached 2026-10-06): Haiku 5.5 medium, Sonnet 5.5 high, Opus 5.5
/// medium, Fable 5.1 high. Flash's low is an inference, `[I]` in the design:
/// the cheapest forced think, as DeepSeek names no default. The older rows
/// name none: no choice reaches them but a pin, and a pin sets its own effort.
fn default_efforts(e: &mut BTreeMap<String, CatalogEntry>) {
    for (id, effort) in [
        ("claude-haiku-5-5", Effort::Medium),
        ("claude-sonnet-5-5", Effort::High),
        ("claude-opus-5-5", Effort::Medium),
        ("claude-fable-5-1", Effort::High),
        ("deepseek-flash", Effort::Low),
    ] {
        if let Some(row) = e.get_mut(id) {
            row.default_effort = Some(effort);
        }
    }
}

impl Catalog {
    /// [`thinking_off`] for `model` at this catalog's row.
    pub fn thinking_off(&self, model: &str) -> Option<serde_json::Value> {
        thinking_off(model, self.get(model))
    }

    /// `model` thinks whether asked or not: its row is [`ThinkingMode::Always`]
    /// (Fable 5.x, Opus 5.5) or [`ThinkingMode::Implicit`] (GLM, DeepSeek, Kimi),
    /// so [`thinking_off`] has nothing to send and a small cap is spent
    /// thinking first (theseus-ohiu8).
    pub fn thinks_unasked(&self, model: &str) -> bool {
        self.get(model)
            .is_some_and(|e| matches!(e.thinking, ThinkingMode::Always | ThinkingMode::Implicit))
    }

    /// Whether a call to `model` may force its tool (`tool_choice` of type
    /// `tool`): the model's thinking is off in the plain sense, so
    /// [`thinking_off`] sends `disabled`, or it sends none and does not think
    /// unasked (an unknown model is today's behaviour). Sonnet 5.5
    /// (`between_tools`), Opus 5.5, Fable and the implicit thinkers refuse a
    /// forced tool or think through it (theseus-yym79).
    pub fn may_force(&self, model: &str) -> bool {
        match self.thinking_off(model) {
            Some(t) => t["type"] == "disabled",
            None => !self.thinks_unasked(model),
        }
    }

    /// How a one-tool call to `model` asks: its thinking off where it can be
    /// and whether the tool may be forced.
    pub fn forcing(&self, model: &str) -> crate::provider::one_tool::Forcing {
        crate::provider::one_tool::Forcing {
            thinking: self.thinking_off(model),
            force: self.may_force(model),
        }
    }

    pub fn builtin() -> Self {
        use ThinkingMode::*;
        let mut e = BTreeMap::new();
        let m = 1_000_000;
        e.insert(
            "claude-fable-5-1".into(),
            CatalogEntry {
                // Seen to take it live (theseus-posww).
                per_message_effort: true,
                ..claude(m, 128_000, 10.0, 50.0, 0.25, 12.50, Always, true, 512)
            },
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
        // table said 1,024, Sonnet 5's, until 2026-10-01. A refusal goes to
        // Sonnet 5, as Claude Code's does (the owner, 2026-10-04, theseus-7gir.18).
        e.insert(
            "claude-sonnet-5-5".into(),
            CatalogEntry {
                source: "Anthropic Models API and pricing page, 2026-09-28; cache reads $0.20, \
                         the pricing page's 0.1x of input, as before the 2026-10-17 change to \
                         $0.10 that undercounted them by half (the owner, 2026-10-25: \
                         \"Do make the sonnet fix.\")"
                    .into(),
                refusal_fallback_model: Some("claude-sonnet-5".into()),
                // The live probe of 2026-10-17: a per-message change keeps the
                // cache, a top-level one restarts it (theseus-rwpbg.4). Its
                // 400s of 2026-10-11 and 2026-10-21 were requests that carried
                // a message without the beta (theseus-blzt6).
                per_message_effort: true,
                ..claude(m, 128_000, 2.0, 10.0, 0.20, 2.50, Adaptive, false, 512)
            },
        );
        e.insert(
            "claude-sonnet-5".into(),
            claude(m, 128_000, 2.0, 10.0, 0.20, 2.50, Adaptive, false, 1024),
        );
        // Haiku 5.5 (theseus-3okf): 5 × every price above a 100,000-token
        // prompt. No server-side refusal fallback; a refusal goes to Sonnet 5,
        // whose 1M window holds any request Haiku 5.5's does (Haiku 4.5's
        // 200k would not). Its tokenizer is Sonnet 5.5's (the lane's probe:
        // the same counts on 2 KB of prose, Rust and JSON).
        e.insert(
            "claude-haiku-5-5".into(),
            CatalogEntry {
                source: "Anthropic pricing page, Haiku 5.5 model page, and Models API, 2026-10-07"
                    .into(),
                refusal_fallback_model: Some("claude-sonnet-5".into()),
                long_prompt: Some(LongPrompt {
                    above_tokens: 100_000,
                    input_per_mtok: 0.50,
                    output_per_mtok: 2.50,
                    cache_read_per_mtok: 0.05,
                    cache_write_per_mtok: 0.625,
                    cache_write_1h_per_mtok: 1.00,
                }),
                tool_search: false,
                ..claude(m, 128_000, 0.10, 0.50, 0.01, 0.125, Adaptive, false, 512)
            },
        );
        e.insert(
            "claude-haiku-4-5".into(),
            CatalogEntry {
                bytes_per_token: TokenRates::CLAUDE_OLD,
                tool_search: false,
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
            glm(1_048_576, 131_072, 0.15, 0.50, 0.03, 0.15),
        );
        e.insert(
            "glm-5.3-flashx".into(),
            glm(1_048_576, 131_072, 0.37, 1.25, 0.075, 0.37),
        );
        deepseek_rows(&mut e);
        kimi_rows(&mut e);
        default_efforts(&mut e);
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

    /// [`cost_usd`](Self::cost_usd) at the rate in force at `at_ms`.
    pub fn cost_usd_at(&self, model: &str, usage: &Usage, at_ms: u64) -> Option<f64> {
        self.get(model).map(|e| e.cost_usd_at(usage, at_ms))
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

/// Deepgram's pay-as-you-go list prices, as best known (2025's), until the owner
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
        model: "flux-general",
        unit: SpeechUnit::Minute,
        usd: 0.0077,
        source: "Deepgram pay-as-you-go list price, Flux English streaming (2026; a promotional $0.0065 was listed), to confirm",
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

/// Whether `served`, the model id a provider echoed, is the `configured` one:
/// equal; or one side carries a vendor prefix (its text up to the last `/`)
/// the other lacks (two prefixes must be equal: another vendor's model of the
/// same name is another model); or, past the prefix, one is the other plus a
/// dated-snapshot suffix, `-` or `@` then `YYYYMMDD` or `YYYY-MM-DD`. Nothing
/// else counts (`model-a-5` and `model-ab` are not `model-a`). Case-sensitive.
pub fn same_model(configured: &str, served: &str) -> bool {
    fn split(id: &str) -> (Option<&str>, &str) {
        match id.rsplit_once('/') {
            Some((vendor, name)) => (Some(vendor), name),
            None => (None, id),
        }
    }
    fn snapshot_of(base: &str, full: &str) -> bool {
        let Some(rest) = full.strip_prefix(base) else {
            return false;
        };
        let Some(date) = rest.strip_prefix(['-', '@']) else {
            return false;
        };
        let b = date.as_bytes();
        let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
        !base.is_empty()
            && ((b.len() == 8 && digits(b))
                || (b.len() == 10
                    && digits(&b[..4])
                    && b[4] == b'-'
                    && digits(&b[5..7])
                    && b[7] == b'-'
                    && digits(&b[8..])))
    }
    let ((cv, cn), (sv, sn)) = (split(configured), split(served));
    if cv.is_some() && sv.is_some() && cv != sv {
        return false;
    }
    cn == sn || snapshot_of(cn, sn) || snapshot_of(sn, cn)
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
            } else if e.provider == "moonshot" {
                // Moonshot prices a 1-hour write apart: 2 × input, writes at input.
                assert_eq!(e.cache_write_per_mtok, e.input_per_mtok, "{id}");
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
            "claude-haiku-5-5",
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
        // Sonnet 5 and Sonnet 5.5 alike: 2 + 10 + 0.20 + 2.50.
        for (id, usd) in [("claude-sonnet-5", 14.70), ("claude-sonnet-5-5", 14.70)] {
            let s = c.cost_usd(id, &u).unwrap();
            assert!((s - usd).abs() < 1e-9, "{id}: {s}");
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

    /// The client-side refusal fallback (theseus-7gir.18): the owner approved
    /// Sonnet 5.5's, Sonnet 5, kept apart from the provider's own
    /// (`refusal_fallbacks`), which Fable 5.1 and Opus 5 take; Haiku 5.5's,
    /// Sonnet 5 too (theseus-3okf); a config table can name one for another
    /// model.
    #[test]
    fn sonnet_and_haiku_5_5_name_a_client_side_fallback_and_a_table_can_name_one() {
        let c = Catalog::builtin();
        let named: Vec<(&str, &str)> = c
            .entries
            .iter()
            .filter_map(|(id, e)| Some((id.as_str(), e.refusal_fallback_model.as_deref()?)))
            .collect();
        assert_eq!(
            named,
            [
                ("claude-haiku-5-5", "claude-sonnet-5"),
                ("claude-sonnet-5-5", "claude-sonnet-5")
            ]
        );
        assert!(!c.get("claude-sonnet-5-5").unwrap().refusal_fallbacks);
        let server: Vec<&str> = c
            .entries
            .iter()
            .filter(|(_, e)| e.refusal_fallbacks)
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(server, ["claude-fable-5-1", "claude-opus-5"]);
        let row = CatalogRow {
            refusal_fallback_model: Some("claude-opus-5".into()),
            ..Default::default()
        };
        let o: BTreeMap<String, CatalogRow> = [("claude-opus-5-5".to_string(), row)].into();
        let c = Catalog::with_overrides(&o);
        let e = c.get("claude-opus-5-5").unwrap();
        assert_eq!(e.refusal_fallback_model.as_deref(), Some("claude-opus-5"));
        assert!(c.version.ends_with("+config:1"), "{}", c.version);
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
        assert_eq!(blind, vec!["deepseek-v4-pro", "glm-5.2", "glm-5.3"]);
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
    /// (theseus-f5hf; the tokens lane's live check, 2026-10-01): the owner's 15
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

    /// A fallback takes the refused request whole (`turn::fallback_step`), so
    /// its window holds any request its model's does: Haiku 5.5's 1M would
    /// not fit Haiku 4.5's 200k, so its fallback is Sonnet 5 (theseus-3okf).
    #[test]
    fn every_refusal_fallbacks_window_holds_its_models_requests() {
        let c = Catalog::builtin();
        for (id, e) in &c.entries {
            let Some(to) = e.refusal_fallback_model.as_deref() else {
                continue;
            };
            let f = c.get(to).unwrap();
            assert!(
                f.context_window >= e.context_window,
                "{id}'s fallback {to} has a {} window against its {}",
                f.context_window,
                e.context_window
            );
            assert_eq!(f.provider, e.provider, "{id}");
        }
        assert!(c.get("claude-haiku-4-5").unwrap().context_window < 1_000_000);
    }

    /// Haiku 5.5's row (theseus-3okf): the pricing page's figures, the Models
    /// API's window and cap, adaptive thinking with effort, and the probe's
    /// tokenizer, which is Sonnet 5.5's.
    #[test]
    fn haiku_5_5_is_priced_as_published_and_reads_as_sonnet_5_5_does() {
        let c = Catalog::builtin();
        let h = c.get("claude-haiku-5-5").unwrap();
        assert_eq!(
            (h.context_window, h.max_output_tokens),
            (1_000_000, 128_000)
        );
        assert_eq!(
            (
                h.input_per_mtok,
                h.output_per_mtok,
                h.cache_read_per_mtok,
                h.cache_write_per_mtok,
                h.cache_write_1h_per_mtok
            ),
            (0.10, 0.50, 0.01, 0.125, 0.20)
        );
        assert_eq!(h.thinking, ThinkingMode::Adaptive);
        assert!(h.effort && h.vision && h.pdf && !h.refusal_fallbacks);
        assert_eq!(h.bytes_per_token, TokenRates::CLAUDE);
        assert_eq!(TokenRates::of("claude-haiku-5-5"), TokenRates::CLAUDE);
        assert_eq!(image_tokens("claude-haiku-5-5", 1920, 1080), 2_691);
        assert_eq!(c.version, "2026-10-29.1");
        // Sonnet 5.5's cache read is $0.20, as Sonnet 5's is.
        assert_eq!(
            c.get("claude-sonnet-5-5").unwrap().cache_read_per_mtok,
            0.20
        );
    }

    /// Thinking off for a short answer (theseus-050g2): Sonnet 5.5's is
    /// `between_tools` (its `disabled` is a 400), the other adaptive models'
    /// `disabled`; a model that always thinks, or one sent no `thinking`,
    /// gets no field.
    #[test]
    fn thinking_off_is_the_field_each_model_takes() {
        let c = Catalog::builtin();
        let off = |m: &str| thinking_off(m, c.get(m));
        assert_eq!(
            off("claude-sonnet-5-5"),
            Some(serde_json::json!({"type": "between_tools"}))
        );
        for m in [
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-haiku-5-5",
        ] {
            assert_eq!(off(m), Some(serde_json::json!({"type": "disabled"})), "{m}");
        }
        for m in [
            "claude-opus-5-5",
            "claude-fable-5-1",
            "glm-5.3-flash",
            "no-such-model",
        ] {
            assert_eq!(off(m), None, "{m}");
        }
    }

    /// A model thinks unasked when its row is Always or Implicit (theseus-ohiu8);
    /// one that can be turned off, one left off (Haiku 4.5), and an unknown
    /// model do not.
    #[test]
    fn a_model_thinks_unasked_when_its_row_is_always_or_implicit() {
        let c = Catalog::builtin();
        for m in [
            "claude-opus-5-5",
            "claude-fable-5-1",
            "glm-5.3-flash",
            "deepseek-v4-pro",
        ] {
            assert!(c.thinks_unasked(m), "{m}");
        }
        for m in [
            "claude-sonnet-5-5",
            "claude-opus-5",
            "claude-haiku-5-5",
            "claude-haiku-4-5",
            "no-such-model",
        ] {
            assert!(!c.thinks_unasked(m), "{m}");
        }
    }

    /// A tool may be forced only where thinking is off in the plain sense
    /// (theseus-yym79). (Planted revert: always true.)
    #[test]
    fn a_tool_may_be_forced_only_where_thinking_is_plainly_off() {
        let c = Catalog::builtin();
        for m in [
            "claude-haiku-5-5",
            "claude-haiku-4-5",
            "claude-opus-5",
            "no-such-model",
        ] {
            assert!(c.may_force(m), "{m}");
        }
        for m in [
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-fable-5-1",
            "glm-5.3-flash",
            "deepseek-v4-pro",
        ] {
            assert!(!c.may_force(m), "{m}");
        }
        let f = c.forcing("claude-sonnet-5-5");
        assert_eq!(
            (f.thinking, f.force),
            (Some(serde_json::json!({"type": "between_tools"})), false)
        );
    }

    /// Every built-in row says where its model was made (theseus-a1wvq.1):
    /// Claude's `us`, GLM's and DeepSeek's `cn`; none is `unknown`.
    #[test]
    fn every_builtin_row_names_its_origin_and_glm_and_deepseek_are_cn() {
        let c = Catalog::builtin();
        for (id, e) in &c.entries {
            let want = match e.provider.as_str() {
                "anthropic" => ModelOrigin::Us,
                "zai" | "deepseek" | "moonshot" => ModelOrigin::Cn,
                other => panic!("{id}: provider {other} has no origin rule"),
            };
            assert_eq!(e.origin, want, "{id}");
            assert_eq!(e.origin.restricted(), want == ModelOrigin::Cn, "{id}");
        }
        assert!(ModelOrigin::Unknown.restricted() && !ModelOrigin::Eu.restricted());
        assert_eq!(ModelOrigin::default(), ModelOrigin::Unknown);
    }

    /// The sensitive names go only to a model of US origin (theseus-xs5ah): an
    /// allowlist, so `eu` is refused as well as the restricted `cn` and
    /// `unknown`, and an origin added later is refused until it is listed.
    #[test]
    fn only_a_us_model_is_cleared_for_private_data() {
        assert!(ModelOrigin::Us.cleared_for_sensitive());
        for o in [ModelOrigin::Eu, ModelOrigin::Cn, ModelOrigin::Unknown] {
            assert!(!o.cleared_for_sensitive(), "{o:?}");
        }
    }

    /// A new model's row without `origin` is unknown, so restricted; the
    /// operator may say `us` or `eu`; a word that is none of the four fails.
    #[test]
    fn a_config_model_without_origin_is_unknown_and_restricted() {
        let row = |origin: Option<ModelOrigin>| CatalogRow {
            provider: Some("anthropic".into()),
            context_window: Some(200_000),
            max_output_tokens: Some(64_000),
            input_per_mtok: Some(1.0),
            output_per_mtok: Some(5.0),
            cache_read_per_mtok: Some(0.1),
            cache_write_per_mtok: Some(1.25),
            origin,
            ..Default::default()
        };
        let c = Catalog::with_overrides(&BTreeMap::from([
            ("acme-1".to_string(), row(None)),
            ("acme-2".to_string(), row(Some(ModelOrigin::Eu))),
        ]));
        assert_eq!(c.get("acme-1").unwrap().origin, ModelOrigin::Unknown);
        assert!(c.get("acme-1").unwrap().origin.restricted());
        assert_eq!(c.get("acme-2").unwrap().origin, ModelOrigin::Eu);
        // A table over a built-in row keeps its origin unless it names one.
        let over = CatalogRow {
            input_per_mtok: Some(3.0),
            ..Default::default()
        };
        let c = Catalog::with_overrides(&BTreeMap::from([("glm-5.3".to_string(), over)]));
        assert_eq!(c.get("glm-5.3").unwrap().origin, ModelOrigin::Cn);
        let bad: Result<CatalogRow, _> = toml::from_str("origin = \"china\"");
        assert!(bad.is_err());
        let good: CatalogRow = toml::from_str("origin = \"eu\"").unwrap();
        assert_eq!(good.origin, Some(ModelOrigin::Eu));
    }

    /// Origin is judged by the endpoint and the family as well as by the row
    /// (the owner, 2026-10-17): DeepSeek's Anthropic endpoint serves any
    /// `claude*` id with a DeepSeek model, so the id proves nothing there.
    #[test]
    fn a_china_endpoint_or_family_is_cn_whatever_the_model_id_says() {
        use ModelOrigin::*;
        let c = Catalog::builtin();
        let of = |base: &str, vouched: Option<ModelOrigin>, model: &str| {
            origin_of(base, vouched, model, c.get(model))
        };
        let sonnet = "claude-sonnet-5-5";
        assert_eq!(of("https://api.deepseek.com/anthropic", None, sonnet), Cn);
        assert_eq!(
            of("https://api.deepseek.com/anthropic", Some(Us), sonnet),
            Cn
        );
        assert_eq!(of("https://api.z.ai/api/anthropic", None, sonnet), Cn);
        assert_eq!(
            of("https://open.bigmodel.cn/api/anthropic", None, sonnet),
            Cn
        );
        assert_eq!(of("https://DEEPSEEK.com./x", None, sonnet), Cn);
        assert_eq!(of("https://u:p@api.z.ai:443/x", None, sonnet), Cn);
        // The family, on any host.
        assert_eq!(
            of(
                "https://proxy.example.com",
                None,
                "deepseek/deepseek-v4-flash"
            ),
            Cn
        );
        assert_eq!(
            of("https://proxy.example.com", Some(Us), "z-ai/glm-5.3"),
            Cn
        );
        assert_eq!(
            of("https://proxy.example.com", Some(Us), "zai-org/GLM-5.3"),
            Cn
        );
        assert_eq!(
            of("https://api.z.ai/api/anthropic", None, "glm-5.3-flash"),
            Cn
        );
        assert_eq!(of("http://localhost:9", None, "glm-5.3-flash"), Cn);
        // A GLM the catalog does not know, behind a host that vouches `us`:
        // the family alone says where it was made.
        assert_eq!(of("https://proxy.example.com", Some(Us), "glm-6"), Cn);
        assert_eq!(
            of("https://proxy.example.com", Some(Us), "acme/GLM-6-Air"),
            Cn
        );
        // Anthropic's first party and loopback: the row's origin.
        assert_eq!(of("https://api.anthropic.com", None, sonnet), Us);
        assert_eq!(of("http://127.0.0.1:8080", None, sonnet), Us);
        assert_eq!(of("http://[::1]:8080", None, sonnet), Us);
        assert_eq!(of("http://localhost:8080", None, sonnet), Us);
        // A model the catalog lacks is unknown even there.
        assert_eq!(
            of("https://api.anthropic.com", None, "claude-future-9"),
            Unknown
        );
        // Any other host: the provider's word, else unknown; a row's `us`
        // never vouches for a host.
        assert_eq!(of("https://proxy.example.com", None, sonnet), Unknown);
        assert_eq!(of("https://proxy.example.com", Some(Us), sonnet), Us);
        assert_eq!(of("https://proxy.example.com", Some(Eu), sonnet), Eu);
        // A look-alike is not a subdomain.
        assert_eq!(of("https://notdeepseek.com", None, sonnet), Unknown);
        assert_eq!(
            of("https://deepseek.com.evil.example", None, sonnet),
            Unknown
        );
    }

    /// DeepSeek's two rows, priced as its pricing page says at peak rates
    /// (theseus-a1wvq.1), and the rows' shape: no PDF, thinking implicit.
    #[test]
    fn deepseek_rows_are_priced_as_research_says() {
        let c = Catalog::builtin();
        let f = c.get("deepseek-flash").unwrap();
        let p = c.get("deepseek-v4-pro").unwrap();
        let prices = |e: &CatalogEntry| {
            (
                e.input_per_mtok,
                e.output_per_mtok,
                e.cache_read_per_mtok,
                e.cache_write_per_mtok,
                e.cache_write_1h_per_mtok,
            )
        };
        assert_eq!(prices(f), (0.30, 1.20, 0.006, 0.30, 0.30));
        assert_eq!(prices(p), (1.32, 3.96, 0.044, 1.32, 1.32));
        for e in [f, p] {
            assert_eq!(e.provider, "deepseek");
            assert_eq!(
                (e.context_window, e.max_output_tokens),
                (1_000_000, 384_000)
            );
            assert_eq!(
                (e.caches, e.thinking, e.effort),
                (true, ThinkingMode::Implicit, true)
            );
            assert!(!e.pdf && !e.tool_search && !e.refusal_fallbacks);
            assert_eq!(e.origin, ModelOrigin::Cn);
            assert_eq!(e.bytes_per_token, TokenRates::CLAUDE);
        }
        assert!(f.vision && !p.vision);
        assert!(f.source.contains("V4.1-Flash-1017") && p.source.contains("V4-Pro-0813"));
        assert_eq!(TokenRates::of("deepseek-flash"), TokenRates::CLAUDE);
    }

    /// The GLM rows' cache writes are priced at their input (no premium, a
    /// write is billed as uncached input), flash and flashx included.
    #[test]
    fn glm_cache_writes_are_priced_at_input() {
        let c = Catalog::builtin();
        for id in ["glm-5.3", "glm-5.2", "glm-5.3-flash", "glm-5.3-flashx"] {
            let e = c.get(id).unwrap();
            assert_eq!(e.cache_write_per_mtok, e.input_per_mtok, "{id}");
            assert!(e.source.contains("docs.z.ai"), "{id}");
        }
    }

    /// Sonnet 5.5's cache reads are $0.20 a million, the pricing page's 0.1x
    /// of its $2.00 input and Sonnet 5's price (theseus-rko44). From
    /// 2026-10-17 the table said $0.10, so every reservation, settlement,
    /// ledger cost and spend limit undercounted the cache-read share; the
    /// owner decided on 2026-10-25 that it is $0.20.
    #[test]
    fn sonnet_5_5_cache_reads_are_twenty_cents() {
        let c = Catalog::builtin();
        assert_eq!(
            c.get("claude-sonnet-5-5").unwrap().cache_read_per_mtok,
            0.20
        );
        assert_eq!(c.get("claude-sonnet-5").unwrap().cache_read_per_mtok, 0.20);
        let source = &c.get("claude-sonnet-5-5").unwrap().source;
        assert!(source.contains("cache reads $0.20"), "{source}");
        assert!(!source.contains("since Haiku 5.5's launch"), "{source}");
    }

    /// The price against a worked bill (theseus-rko44): the 10-24 Bug Hunt's
    /// Theseus run read 62.24M tokens from the cache, which is $12.45 at
    /// $0.20 a million and $6.22 at $0.10, and a million cache reads cost the
    /// same on Sonnet 5.5 as on Sonnet 5.
    #[test]
    fn a_bug_hunts_cache_reads_cost_twelve_dollars_forty_five() {
        let c = Catalog::builtin();
        let reads = Usage {
            cache_read_input_tokens: 62_240_000,
            ..Default::default()
        };
        let usd = c.cost_usd("claude-sonnet-5-5", &reads).unwrap();
        assert!((usd - 12.448).abs() < 1e-9, "{usd}");
        assert_eq!(usd, c.cost_usd("claude-sonnet-5", &reads).unwrap());
    }

    /// The long tier (theseus-3okf): at 100,000 prompt tokens every class
    /// costs the base price, and at 100,001 every class costs 5 ×, the
    /// prompt counting the uncached input, the cache reads, and the cache
    /// writes alike. A model without a tier never changes price.
    #[test]
    fn a_prompt_past_the_threshold_pays_the_long_tier_for_every_class() {
        let h = Catalog::builtin().get("claude-haiku-5-5").unwrap().clone();
        let at = |input, read, write| Usage {
            input_tokens: input,
            output_tokens: 1_000,
            cache_read_input_tokens: read,
            cache_creation_input_tokens: write,
            ..Default::default()
        };
        // 100,000: 10,000 × 0.10 + 1,000 × 0.50 + 80,000 × 0.01 + 10,000 × 0.125
        let base = at(10_000, 80_000, 10_000);
        assert_eq!(h.cost_micros(&base), 1_000 + 500 + 800 + 1_250);
        // One more token, of any class, and each is at the long price.
        for over in [
            at(10_001, 80_000, 10_000),
            at(10_000, 80_001, 10_000),
            at(10_000, 80_000, 10_001),
        ] {
            let long = 5_000 + 2_500 + 4_000 + 6_250;
            let one_more = h.cost_micros(&over) - long;
            assert!(one_more <= 1, "{over:?}: {}", h.cost_micros(&over));
        }
        // 1-hour writes take the long tier's 1-hour price.
        let w1h = Usage {
            cache_creation_input_tokens: 200_000,
            cache_creation_1h_input_tokens: 200_000,
            ..Default::default()
        };
        assert_eq!(h.cost_micros(&w1h), 200_000);
        // A model with no tier: the same price per token at any length.
        let s55 = Catalog::builtin().get("claude-sonnet-5-5").unwrap().clone();
        let big = Usage {
            input_tokens: 400_000,
            ..Default::default()
        };
        assert_eq!(s55.cost_micros(&big), 800_000);
    }

    /// The reservation (theseus-3okf): an input estimate past the threshold
    /// reserves its input and its output cap at the long prices, so a long
    /// call is never under-reserved; at the threshold, the base prices. Its
    /// two parts, as the turn's row shows them, add up to it.
    #[test]
    fn an_estimate_past_the_threshold_reserves_at_the_long_tier() {
        let h = Catalog::builtin().get("claude-haiku-5-5").unwrap().clone();
        // 8,000 out at $0.50 and 100,000 in at $0.10: 4,000 + 10,000.
        assert_eq!(h.reserve_micros(8_000, 100_000), 14_000);
        // 8,000 out at $2.50 and 100,001 in at $0.50: 20,000 + 50,000.5.
        assert_eq!(h.reserve_micros(8_000, 100_001), 70_001);
        assert_eq!(h.reserve_parts(8_000, 100_001), (20_000, 50_001));
        assert_eq!(h.reserve_parts(8_000, 100_000), (4_000, 10_000));
        assert_eq!(h.reserve_micros(8_000, 0), 4_000);
    }

    /// A config's `[catalog."<id>".long_prompt]` replaces the row's tier
    /// whole, or gives a tier to a model that has none; a table naming no
    /// tier keeps the row's.
    #[test]
    fn a_config_table_sets_or_keeps_the_long_tier() {
        let tier = LongPrompt {
            above_tokens: 200_000,
            input_per_mtok: 4.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.4,
            cache_write_per_mtok: 5.0,
            cache_write_1h_per_mtok: 8.0,
        };
        let o: BTreeMap<String, CatalogRow> = [
            (
                "claude-sonnet-5-5".to_string(),
                CatalogRow {
                    long_prompt: Some(tier),
                    ..Default::default()
                },
            ),
            (
                "claude-haiku-5-5".to_string(),
                CatalogRow {
                    output_per_mtok: Some(0.6),
                    ..Default::default()
                },
            ),
        ]
        .into();
        let c = Catalog::with_overrides(&o);
        assert_eq!(c.get("claude-sonnet-5-5").unwrap().long_prompt, Some(tier));
        let h = c.get("claude-haiku-5-5").unwrap();
        assert_eq!(h.long_prompt.unwrap().above_tokens, 100_000);
        assert_eq!(h.output_per_mtok, 0.6);
        let row: CatalogRow = toml::from_str(
            "[long_prompt]\nabove_tokens = 200000\ninput_per_mtok = 4.0\noutput_per_mtok = 15.0\n\
             cache_read_per_mtok = 0.4\ncache_write_per_mtok = 5.0\ncache_write_1h_per_mtok = 8.0\n",
        )
        .unwrap();
        assert_eq!(row.long_prompt, Some(tier));
        assert!(toml::from_str::<CatalogRow>("[long_prompt]\nabove_tokens = 1\n").is_err());
        // The catalog's listing carries it; a row without one names none.
        let listed = serde_json::to_value(Catalog::builtin().get("claude-haiku-5-5")).unwrap();
        assert_eq!(listed["long_prompt"]["above_tokens"], 100_000);
        let s5 = serde_json::to_value(Catalog::builtin().get("claude-sonnet-5")).unwrap();
        assert!(s5.get("long_prompt").is_none());
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

    /// The exam's model match (theseus-nbgi4): a dated snapshot or a vendor
    /// prefix of the configured id is the same model; another vendor's, or
    /// anything else appended, is not.
    #[test]
    fn same_model_takes_a_dated_snapshot_or_a_vendor_prefix() {
        for (a, b) in [
            ("model-a", "model-a"),
            ("model-a", "model-a-20261001"),
            ("model-a", "model-a@20261001"),
            ("model-a", "model-a-2026-10-01"),
            ("model-a", "vendor-x/model-a"),
            ("vendor-x/model-a", "model-a"),
            ("model-a", "vendor-x/model-a-20261001"),
        ] {
            assert!(same_model(a, b), "{a} vs {b}");
        }
        for (a, b) in [
            ("model-a", "model-b"),
            ("model-a-5", "model-a"),
            ("model-a", "model-a-5"),
            ("model-a", "model-a-2026"),
            ("model-a", "model-ab"),
            ("vendor-x/model-a", "vendor-y/model-a"),
            ("model-a", ""),
        ] {
            assert!(!same_model(a, b), "{a} vs {b}");
        }
    }
}

#[cfg(test)]
mod tests_moonshot;
#[cfg(test)]
mod tests_peak;
#[cfg(test)]
mod tests_switch;
