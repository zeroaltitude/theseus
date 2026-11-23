//! Claude Mythos 5.2's row: each figure is the maker's published one, the router
//! cannot choose the model, and a profile that pins it loads and is priced.

use theseus_protocol::Usage;

use super::super::*;
use super::ID;
use crate::config::Config;
use crate::routing::model::OPTIONS;

/// The name the provider takes, and every figure the maker publishes for the
/// model (its page, the pricing page, the caching, effort and tool-search pages,
/// 2026-11-18). A figure here that moved would be one the maker moved.
#[test]
fn mythos_5_2_is_the_makers_published_row() {
    let c = Catalog::builtin();
    assert_eq!(ID, "claude-mythos-5-2");
    let m = c.get(ID).expect("Mythos 5.2 is in the model list");
    assert_eq!(
        (m.context_window, m.max_output_tokens),
        (1_000_000, 128_000)
    );
    assert_eq!(
        (
            m.input_per_mtok,
            m.output_per_mtok,
            m.cache_write_per_mtok,
            m.cache_write_1h_per_mtok,
            m.cache_read_per_mtok
        ),
        (8.0, 40.0, 10.0, 16.0, 0.20)
    );
    assert_eq!(m.long_prompt, None);
    assert_eq!(m.provider, "anthropic");
    assert_eq!(m.origin, ModelOrigin::Us);
    assert_eq!(
        origin_of("https://api.anthropic.com", None, Some(m)),
        ModelOrigin::Us
    );
    // "Adaptive (always on)", default effort high, effort changed per message.
    assert_eq!(m.thinking, ThinkingMode::Always);
    assert_eq!(m.default_effort, Some(Effort::High));
    assert!(m.effort && m.per_message_effort);
    assert!(c.thinks_unasked(ID));
    assert_eq!(c.thinking_off(ID), None);
    assert!(!c.may_force(ID));
    // Text and images in, PDFs ("All models"), tool search (its id is listed),
    // a 512-token caching minimum (the caching page names it).
    assert!(m.vision && m.pdf && m.tool_search && m.caches);
    assert_eq!(m.cache_min_tokens, 512);
}

/// What the maker does not publish for it is not claimed: no server-side
/// refusal fallback is asked for (the refusals page does not say it has one), and
/// the tokenizer's density is Theseus's estimate for a Claude id, said so in the
/// row's source, with the date the pages were read.
#[test]
fn what_the_maker_does_not_publish_is_left_out_and_the_row_says_so() {
    let c = Catalog::builtin();
    let m = c.get(ID).unwrap();
    assert!(!m.refusal_fallbacks);
    assert_eq!(m.refusal_fallback_model, None);
    assert_eq!(m.bytes_per_token, TokenRates::CLAUDE);
    for want in [
        "2026-11-18",
        "model page",
        "pricing page",
        "Not published for it, so left out",
        "refusal fallback",
        "tokenizer",
    ] {
        assert!(m.source.contains(want), "{want}: {}", m.source);
    }
    // It is not among the models that take the provider's own fallback.
    assert!(c
        .entries
        .iter()
        .filter(|(_, e)| e.refusal_fallbacks)
        .all(|(id, _)| id != ID));
}

/// A million of each class: 8 + 40 + 0.20 + 10, and the reservation a call takes
/// is the output cap at $40 plus the input estimate at the highest cache-write rate ($16).
#[test]
fn mythos_5_2_is_priced_as_published() {
    let c = Catalog::builtin();
    let u = Usage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_read_input_tokens: 1_000_000,
        cache_creation_input_tokens: 1_000_000,
        ..Default::default()
    };
    let usd = c.cost_usd(ID, &u).unwrap();
    assert!((usd - 58.20).abs() < 1e-9, "{usd}");
    assert_eq!(
        c.get(ID).unwrap().reserve_micros(128_000, 200_000),
        8_320_000
    );
}

/// The router does not choose Mythos by itself: no option of the router's, and
/// no model the switch-cost estimate weighs, is this one. A profile pins it.
#[test]
fn the_router_cannot_choose_mythos_5_2() {
    assert!(OPTIONS.iter().all(|(_, model)| *model != ID));
}

/// The template's commented profile that pins it: uncommented, it loads
/// without a warning, runs the model by its published name, and the model list
/// prices it. As shipped (commented) it adds no profile of its own.
#[test]
fn the_templates_mythos_pin_loads_and_is_priced() {
    let shipped = Config::example();
    assert!(shipped.all_profiles().get("mythos").is_none());
    let block = [
        "# [profiles.mythos]\n",
        "# provider = \"anthropic\"\n",
        "# model = \"claude-mythos-5-2\"\n",
    ];
    let mut toml = Config::EXAMPLE_TOML.to_string();
    let at = toml
        .find(block[0])
        .expect("the template shows a profile that pins Mythos 5.2");
    let end = at + block.concat().len();
    assert_eq!(&toml[at..end], block.concat(), "the three lines, as shown");
    let uncommented: String = block.iter().map(|l| l.trim_start_matches("# ")).collect();
    toml.replace_range(at..end, &uncommented);
    let (cfg, warnings) = Config::parse(&toml).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let p = cfg.profile("mythos").expect("the pin loads");
    assert_eq!(p.model, ID);
    assert!(Catalog::builtin()
        .cost_usd(&p.model, &Usage::default())
        .is_some());
    // It is a pin and no more: the router's options still name the others.
    assert!(OPTIONS.iter().all(|(option, _)| *option != "mythos"));
}
