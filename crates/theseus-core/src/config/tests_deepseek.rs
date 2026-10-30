//! DeepSeek in the config (theseus-9f72b): the provider and its key's
//! `op://` SecretRef are in the template, and a config with no key for a
//! provider still loads and says "no key" instead of failing.

use super::*;
use crate::catalog::{Catalog, ModelOrigin};

/// The template with the deepseek provider block and its secret left out.
fn without_deepseek() -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in Config::EXAMPLE_TOML.lines() {
        if line.trim() == "[providers.deepseek]" {
            skipping = true;
            continue;
        }
        if skipping && (line.trim().is_empty() || line.starts_with('[') || line.starts_with('#')) {
            skipping = false;
        }
        if skipping || line.starts_with("deepseek_api_key") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn the_template_declares_deepseek_and_its_key_as_a_secret_ref() {
    let cfg = Config::example();
    let p = &cfg.providers["deepseek"];
    assert_eq!(p.api_base, "https://api.deepseek.com/anthropic");
    assert_eq!(p.api_key_secret, "deepseek_api_key");
    assert_eq!(p.kind, ProviderKind::AnthropicMessages);
    assert_eq!(p.origin, Some(ModelOrigin::Cn));
    let r = &cfg.secrets["deepseek_api_key"];
    assert!(r.starts_with("op://"), "{r}");
    cfg.validate().unwrap();
    assert_eq!(cfg.provider_without_key("deepseek"), None);
}

/// No provider block, or a block whose secret entry is gone: the config
/// loads, validates the rest, and the provider reads as keyless.
#[test]
fn a_provider_without_a_key_reads_as_no_key_and_nothing_fails() {
    let cfg = Config::from_toml(&without_deepseek()).unwrap();
    cfg.validate().unwrap();
    assert_eq!(
        cfg.provider_without_key("deepseek").as_deref(),
        Some("deepseek: no key")
    );
    // Declared, its secret entry removed after the load: the same words.
    let mut cfg = Config::example();
    cfg.secrets.remove("deepseek_api_key");
    assert_eq!(
        cfg.provider_without_key("deepseek").as_deref(),
        Some("deepseek: no key")
    );
    // The catalog still names the provider of the model a pick would ask for.
    let c = Catalog::builtin();
    assert_eq!(c.get("deepseek-flash").unwrap().provider, "deepseek");
    // Providers with keys are not keyless.
    assert_eq!(cfg.provider_without_key("anthropic"), None);
    assert_eq!(cfg.provider_without_key("zai"), None);
}
