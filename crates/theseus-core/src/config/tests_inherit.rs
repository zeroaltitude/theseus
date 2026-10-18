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
