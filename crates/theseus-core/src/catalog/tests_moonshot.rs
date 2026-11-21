//! Kimi (Moonshot AI) as China-origin rows (theseus-rgbtz): the rows' own
//! `cn`, the two rows' figures, what a Kimi-shaped usage costs, and that no
//! path forces a tool on a Kimi model.

use super::*;
use serde_json::json;
use theseus_protocol::Usage;

const KIMI: [&str; 2] = ["kimi-k3.1-pro", "kimi-k3.1-pro-highspeed"];

/// A Kimi row reads `cn` wherever it is served, as its row says; a Claude id
/// on Moonshot's host is not guessed at (2026-11-21): unknown, unless the
/// provider says.
#[test]
fn a_kimi_row_is_cn_wherever_it_is_served() {
    use ModelOrigin::*;
    let c = Catalog::builtin();
    let of = |base: &str, vouched: Option<ModelOrigin>, model: &str| {
        origin_of(base, vouched, c.get(model))
    };
    for m in KIMI {
        assert_eq!(of("https://api.moonshot.ai/anthropic", None, m), Cn, "{m}");
        assert_eq!(of("https://proxy.example.com", Some(Us), m), Cn, "{m}");
        assert_eq!(of("http://localhost:9", None, m), Cn, "{m}");
    }
    let sonnet = "claude-sonnet-5-5";
    assert_eq!(
        of("https://api.moonshot.ai/anthropic", None, sonnet),
        Unknown
    );
    assert_eq!(
        of("https://api.moonshot.ai/anthropic", Some(Cn), sonnet),
        Cn
    );
}

/// The rows, as the vendor's pages and the live probe say (2026-10-24), and
/// the shape a China-origin, always-thinking, text-and-image row takes.
#[test]
fn kimi_rows_are_priced_and_shaped_as_the_vendor_says() {
    let c = Catalog::builtin();
    let prices = |e: &CatalogEntry| {
        (
            e.input_per_mtok,
            e.output_per_mtok,
            e.cache_read_per_mtok,
            e.cache_write_per_mtok,
            e.cache_write_1h_per_mtok,
        )
    };
    assert_eq!(prices(c.get(KIMI[0]).unwrap()), (2.0, 10.0, 0.20, 2.0, 4.0));
    assert_eq!(prices(c.get(KIMI[1]).unwrap()), (4.0, 20.0, 0.20, 4.0, 8.0));
    for id in KIMI {
        let e = c.get(id).unwrap();
        assert_eq!(e.provider, "moonshot", "{id}");
        assert_eq!(
            (e.context_window, e.max_output_tokens),
            (1_048_576, 131_072)
        );
        assert_eq!(
            (e.caches, e.thinking, e.effort, e.long_prompt),
            (true, ThinkingMode::Implicit, true, None)
        );
        assert!(
            e.vision && !e.pdf && !e.tool_search && !e.refusal_fallbacks,
            "{id}"
        );
        assert_eq!(e.origin, ModelOrigin::Cn, "{id}");
        assert_eq!(e.bytes_per_token, TokenRates::CLAUDE, "{id}");
        assert!(e.source.contains("Moonshot"), "{id}");
    }
    for skipped in ["kimi-k3", "kimi-k2", "kimi-k2.5"] {
        assert!(c.get(skipped).is_none(), "{skipped}");
    }
    assert_eq!(TokenRates::of("kimi-k3.1-pro"), TokenRates::CLAUDE);
}

/// A Kimi response's usage, parsed and priced: cached input at its own rate,
/// 5-minute and 1-hour writes apart, and the thinking tokens inside
/// `output_tokens` billed as output (`output_tokens_details` is read by no one
/// and adds nothing).
#[test]
fn a_kimi_usage_parses_and_prices_every_class() {
    let v = json!({
        "input_tokens": 10_000,
        "output_tokens": 4_000,
        "cache_read_input_tokens": 200_000,
        "cache_creation_input_tokens": 50_000,
        "cache_creation": {
            "ephemeral_5m_input_tokens": 30_000,
            "ephemeral_1h_input_tokens": 20_000
        },
        "output_tokens_details": {"thinking_tokens": 3_000},
        "service_tier": "standard",
        "inference_geo": "global"
    });
    let mut u = Usage::default();
    crate::provider::apply_usage(&mut u, &v);
    assert_eq!(
        u,
        Usage {
            input_tokens: 10_000,
            output_tokens: 4_000,
            cache_read_input_tokens: 200_000,
            cache_creation_input_tokens: 50_000,
            cache_creation_1h_input_tokens: 20_000,
        }
    );
    // Dollars: 10k in at 2, 4k out at 10, 200k cached at 0.20, 30k 5m writes
    // at 2, 20k 1h writes at 4: 0.02 + 0.04 + 0.04 + 0.06 + 0.08.
    let c = Catalog::builtin();
    let pro = c.get(KIMI[0]).unwrap();
    assert!(
        (pro.cost_usd(&u) - 0.24).abs() < 1e-9,
        "{}",
        pro.cost_usd(&u)
    );
    assert_eq!(pro.cost_micros(&u), 240_000);
    // Highspeed: 0.04 + 0.08 + 0.04 + 0.12 + 0.16.
    let fast = c.get(KIMI[1]).unwrap();
    assert!(
        (fast.cost_usd(&u) - 0.44).abs() < 1e-9,
        "{}",
        fast.cost_usd(&u)
    );
    // Flat over the whole context: a prompt past 200k pays the same rates.
    let long = Usage {
        input_tokens: 900_000,
        output_tokens: 1_000_000,
        ..Default::default()
    };
    assert!((pro.cost_usd(&long) - (1.8 + 10.0)).abs() < 1e-9);
}

/// Kimi's endpoint takes `tool_choice` of auto, any or none and never a tool
/// by name, and the row thinks unasked: no one-tool call to it forces a tool,
/// and none sends a `thinking` field.
#[test]
fn no_call_forces_a_named_tool_on_a_kimi_model() {
    let c = Catalog::builtin();
    for id in KIMI {
        assert!(c.thinks_unasked(id), "{id}");
        assert!(!c.may_force(id), "{id}");
        let f = c.forcing(id);
        assert!(!f.force && f.thinking.is_none(), "{id}");
        let (tool, choice) = crate::provider::one_tool::one_tool(
            json!({"name": "mark", "input_schema": {"type": "object", "properties": {}}}),
            f.force,
        );
        assert_eq!(choice, json!({"type": "auto"}), "{id}");
        assert_eq!(tool["strict"], true, "{id}");
    }
    // The control: a model that may be forced is.
    assert!(c.may_force("claude-haiku-5-5"));
}
