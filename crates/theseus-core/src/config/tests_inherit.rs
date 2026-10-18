//! One inheritance rule for profiles (theseus-rwpbg.7): a profile field a profile leaves unset takes
//! `[model]`'s value. The snapshot is the before/after proof: it was written on the code that inherited only
//! the cache fields, and the rule's change left it as it was.

use super::*;

const SNAPSHOT: &str = "src/config/snapshots/resolved_profiles.txt";

/// Every resolved profile of each document, every field, in name order.
fn resolved(docs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (name, text) in docs {
        let cfg = Config::parse(text).unwrap().0;
        out.push_str(&format!("== {name}\n"));
        for (profile, p) in cfg.all_profiles() {
            out.push_str(&format!("-- {profile}\n{p:#?}\n"));
        }
    }
    out
}

fn documents() -> [(&'static str, &'static str); 4] {
    [
        (
            "owner_shape",
            include_str!("../../fixtures/owner_shape.toml"),
        ),
        ("template", Config::EXAMPLE_TOML),
        (
            "bench",
            include_str!("../../../../bench/theseus-bench.toml"),
        ),
        (
            "bench_routed",
            include_str!("../../../../bench/theseus-bench-routed.toml"),
        ),
    ]
}

/// The owner-shaped config, the template, and the bench's configs resolve to the profiles they always did.
#[test]
fn the_owner_shaped_config_resolves_to_the_same_profiles() {
    let got = resolved(&documents());
    let path = format!("{}/{SNAPSHOT}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, &got).unwrap();
    }
    assert!(
        got == std::fs::read_to_string(&path).unwrap(),
        "the resolved profiles moved; THESEUS_GOLDEN=write rewrites {SNAPSHOT}, for a change you mean"
    );
}

const KEY: &str = "[secrets]\nanthropic_api_key = \"env:THESEUS_TEST_KEY\"\nzai_api_key = \"env:THESEUS_TEST_KEY\"\n\
    [providers.zai]\napi_base = \"https://api.z.ai/api/anthropic\"\napi_key_secret = \"zai_api_key\"\n";

fn load(model: &str, profile: &str) -> Config {
    load_cached("", model, profile)
}

/// `load` with a `[cache]` section, where a profile's cache defaults live (theseus-rwpbg.10).
fn load_cached(cache: &str, model: &str, profile: &str) -> Config {
    let text = format!(
        "{KEY}[cache]\n{cache}\n[model]\n{model}\n[profiles.p]\nmodel = \"claude-sonnet-5-5\"\n{profile}\n"
    );
    Config::parse(&text).unwrap().0
}

/// Each field `[model]` shares with a profile reaches a profile that says nothing of it, and so do
/// `[cache]`'s two.
#[test]
fn a_model_field_reaches_every_profile_that_leaves_it_unset() {
    let cfg = load_cached(
        "ttl = \"1h\"\nkeep_warm_hours = 3.0",
        "provider = \"zai\"\nmax_output_tokens = 9000\nsystem = \"Be brief.\"\neffort = \"low\"\n\
         thinking_display = \"omitted\"\nmax_loops = 7\non_max_loops = \"end\"\n\
         refusal_fallbacks = false",
        "",
    );
    let p = cfg.profile("p").unwrap();
    assert_eq!(p.provider, "zai");
    assert_eq!(p.max_output_tokens, Some(9000));
    assert_eq!(p.system.as_deref(), Some("Be brief."));
    assert_eq!(p.effort, Some(Effort::Low));
    assert_eq!(p.thinking_display, ThinkingDisplay::Omitted);
    assert_eq!((p.max_loops, p.on_max_loops), (7, MaxLoopsMode::End));
    assert!(!p.refusal_fallbacks);
    assert_eq!(
        (p.cache_ttl, p.keep_warm_hours),
        (Some(CacheTtl::OneHour), Some(3.0))
    );
}

/// The bug the review found: `[model] max_loops_mode` (now `on_max_loops`) was ignored by every named profile.
#[test]
fn on_max_loops_in_model_is_inherited() {
    let cfg = load("on_max_loops = \"end\"", "");
    assert_eq!(cfg.profile("p").unwrap().on_max_loops, MaxLoopsMode::End);
}

/// What a profile says is its own, whatever `[model]` says.
#[test]
fn a_profile_value_wins_over_model() {
    let cfg = load_cached(
        "ttl = \"1h\"",
        "max_loops = 7\non_max_loops = \"end\"",
        "max_loops = 90\non_max_loops = \"notify\"\ncache_ttl = \"5m\"",
    );
    let p = cfg.profile("p").unwrap();
    assert_eq!((p.max_loops, p.on_max_loops), (90, MaxLoopsMode::Notify));
    assert_eq!(p.cache_ttl, Some(CacheTtl::FiveMinutes));
}

const OWNER: &str = "[cache]\nttl = \"1h\"\n[model]\nmax_loops = 40\n\
    [profiles.a]\nmodel = \"claude-sonnet-5-5\"\ncache_ttl = \"1h\"\nmax_loops = 40\n\
    [profiles.b]\nmodel = \"claude-sonnet-5-5\"\ncache_ttl = \"1h\"\nmax_loops = 12\n";

/// A key a profile restates from `[model]` or `[cache]` is not written by `--sparse`; one that differs stays.
#[test]
fn sparse_drops_inherited_profile_keys() {
    let note = sparse::sparse_note(&format!("{KEY}{OWNER}")).unwrap();
    let doc: toml::Table = note.parse().unwrap();
    assert_eq!(doc["profiles"]["a"].as_table().unwrap().len(), 1, "{note}");
    let b = doc["profiles"]["b"].as_table().unwrap();
    assert_eq!(
        b.keys().collect::<Vec<_>>(),
        ["max_loops", "model"],
        "{note}"
    );
}

/// The sparse note loads to the profiles the whole note does.
#[test]
fn sparse_output_loads_to_the_same_config() {
    for (_, text) in documents() {
        let note = sparse::sparse_note(text).unwrap();
        assert_eq!(
            resolved(&[("x", text)]),
            resolved(&[("x", &note)]),
            "{note}"
        );
    }
    let text = format!("{KEY}{OWNER}");
    let note = sparse::sparse_note(&text).unwrap();
    assert_eq!(resolved(&[("x", &text)]), resolved(&[("x", &note)]));
}
