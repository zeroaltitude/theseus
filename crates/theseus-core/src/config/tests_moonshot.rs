//! The template's Moonshot block (theseus-rgbtz): it parses un-commented, and a
//! Kimi profile is China-origin whatever the config says.

use super::*;
use crate::catalog::{Catalog, ModelOrigin};

/// The template with the moonshot provider, its two profiles and its secret
/// un-commented, and nothing else.
fn with_moonshot() -> String {
    let mut out = String::new();
    let mut open = false;
    for line in Config::EXAMPLE_TOML.lines() {
        let rest = line
            .trim_start()
            .strip_prefix("# ")
            .or_else(|| line.trim_start().strip_prefix('#'));
        let starts = [
            "[providers.moonshot]",
            "[profiles.kimi]",
            "[profiles.kimimax]",
            "moonshot_api_key =",
        ];
        if let Some(r) = rest {
            if starts.iter().any(|s| r.starts_with(s)) {
                open = !r.starts_with("moonshot_api_key");
                out.push_str(r);
                out.push('\n');
                continue;
            }
            if open && r.contains(" = ") {
                out.push_str(r);
                out.push('\n');
                continue;
            }
        }
        if rest.is_none_or(|r| r.is_empty() || r == "#") {
            open = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn the_templates_moonshot_block_uncommented_parses_and_is_cn() {
    let cfg = Config::from_toml(&with_moonshot()).unwrap();
    let p = &cfg.providers["moonshot"];
    assert_eq!(p.api_base, "https://api.moonshot.ai/anthropic");
    assert_eq!(p.api_key_secret, "moonshot_api_key");
    assert_eq!(p.kind, ProviderKind::AnthropicMessages);
    assert_eq!(p.origin, Some(ModelOrigin::Cn));
    assert!(cfg.secrets.contains_key("moonshot_api_key"));
    let catalog = Catalog::builtin();
    for (name, effort) in [("kimi", Effort::High), ("kimimax", Effort::Max)] {
        let prof = cfg.profile(name).unwrap();
        assert_eq!(
            (prof.provider.as_str(), prof.model.as_str()),
            ("moonshot", "kimi-k3.1-pro")
        );
        assert_eq!(prof.effort, Some(effort), "{name}");
        assert_eq!(
            cfg.profile_origin(name, &catalog),
            ModelOrigin::Cn,
            "{name}"
        );
    }
}

/// A provider's word never makes the Moonshot host anything but `cn`, and a
/// config table cannot loosen a Kimi row's origin.
#[test]
fn the_moonshot_host_and_rows_cannot_be_loosened_to_us() {
    for loose in [ModelOrigin::Us, ModelOrigin::Eu, ModelOrigin::Unknown] {
        // Only the moonshot block's word: the template's deepseek block has its own.
        let full = with_moonshot();
        let (head, moonshot) = full.split_once("[providers.moonshot]").unwrap();
        let toml = format!(
            "{head}[providers.moonshot]{}",
            moonshot.replacen(
                "origin = \"cn\"",
                &format!(
                    "origin = \"{}\"",
                    match loose {
                        ModelOrigin::Us => "us",
                        ModelOrigin::Eu => "eu",
                        _ => "unknown",
                    }
                ),
                1,
            )
        );
        let cfg = Config::from_toml(&toml).unwrap();
        let e = format!("{:#}", cfg.validate().unwrap_err());
        assert!(
            e.contains("providers.moonshot.origin") && e.contains("China-origin endpoint"),
            "{loose:?}: {e}"
        );
    }
    let mut cfg = Config::from_toml(&with_moonshot()).unwrap();
    cfg.catalog.insert(
        "kimi-k3.1-pro".into(),
        crate::catalog::CatalogRow {
            origin: Some(ModelOrigin::Us),
            ..Default::default()
        },
    );
    let e = format!("{:#}", cfg.validate().unwrap_err());
    assert!(
        e.contains("a China-origin model's origin is not configurable"),
        "{e}"
    );
}
