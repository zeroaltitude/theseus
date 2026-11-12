//! OpenAI in the config (theseus-bq73s): `kind` takes a second wire protocol,
//! so a provider block may name OpenAI's Responses API. The template declares
//! the provider commented out, as Moonshot's is, and nothing is called until a
//! profile names a model on it.

use super::*;

/// The second `kind` value loads, and the key the block names is the one the
/// provider reads.
#[test]
fn a_provider_may_name_the_openai_responses_protocol() {
    let text = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\
                openai_api_key = \"op://v/openai/f\"\n\n\
                [providers.openai]\napi_base = \"https://api.openai.com/v1\"\n\
                api_key_secret = \"openai_api_key\"\nkind = \"openai_responses\"\n";
    let (cfg, _) = Config::parse(text).unwrap();
    let p = &cfg.providers["openai"];
    assert_eq!(p.kind, ProviderKind::OpenAiResponses);
    assert_eq!(p.api_base, "https://api.openai.com/v1");
    assert_eq!(p.api_key_secret, "openai_api_key");
}

/// The template carries the block commented out, with its key under
/// `[secrets]`, so un-commenting it is the whole of the operator's work.
#[test]
fn the_template_declares_the_openai_block_commented_out() {
    let t = Config::EXAMPLE_TOML;
    assert!(
        t.contains("# [providers.openai]"),
        "no openai provider block"
    );
    assert!(
        t.contains("# kind = \"openai_responses\""),
        "the block does not name the protocol"
    );
    assert!(
        t.contains("openai_api_key ="),
        "the key is not under [secrets]"
    );
    // Commented out: the shipped config calls OpenAI nowhere.
    let cfg = Config::example();
    assert!(!cfg.providers.contains_key("openai"), "shipped switched on");
    // Every profile the template ships is on a provider that is declared.
    for (name, pr) in &cfg.profiles {
        assert_ne!(pr.provider, "openai", "profile {name} names openai");
    }
}
