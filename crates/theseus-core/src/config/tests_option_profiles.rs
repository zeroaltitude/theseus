//! The profile named for a `model` option and the router's option must name one model, and a config written when
//! `fable` meant Fable 5.1 is told so at load, in words that name the line to change (issue 5: `fable` is 5.2).

use super::*;
use crate::routing::model::{model_of, OPTIONS};

/// A document with the keys a profile needs and the given profile tables.
fn doc(profiles: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"env:THESEUS_TEST_KEY\"\n{profiles}")
}

fn warnings(profiles: &str) -> Vec<String> {
    Config::parse(&doc(profiles)).unwrap().1
}

/// The two places that say which model `fable` is cannot drift apart: every profile of the template that is
/// named for an option runs the model the router's option names.
#[test]
fn the_templates_profiles_run_the_models_the_routers_options_name() {
    let cfg = Config::example();
    let mut named = 0;
    for (option, model) in OPTIONS {
        if let Some(p) = cfg.all_profiles().get(option) {
            assert_eq!(p.model, model, "[profiles.{option}] of the template");
            named += 1;
        }
    }
    assert!(
        named >= 4,
        "the template names a profile for the options it runs: {named}"
    );
    assert_eq!(cfg.profile("fable").unwrap().model, "claude-fable-5-2");
    assert_eq!(model_of("fable"), Some("claude-fable-5-2"));
}

/// The template, as shipped, has nothing to warn about.
#[test]
fn the_template_loads_without_an_option_warning() {
    let (_, w) = Config::parse(Config::EXAMPLE_TOML).unwrap();
    assert!(w.is_empty(), "{w:?}");
}

/// The owner's old shape, `[profiles.fable]` at Fable 5.1 with nothing running 5.2: the router's `fable` pick
/// would find no profile, so the load says so and names the line.
#[test]
fn a_fable_profile_still_at_5_1_is_told_so_at_load() {
    let w = warnings("[profiles.fable]\nmodel = \"claude-fable-5-1\"\n");
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].starts_with("[profiles.fable] runs claude-fable-5-1, but Theseus's `fable` choice is now claude-fable-5-2"), "{}", w[0]);
    assert!(
        w[0].contains("change that profile's model to \"claude-fable-5-2\""),
        "{}",
        w[0]
    );
}

/// A profile that runs Fable 5.2 under any name is enough for the router, so a config with a `fable52` profile
/// (the template's name for it until 1.0.7) and a `fable` at 5.1 pinned on purpose loads quietly.
#[test]
fn a_profile_running_5_2_under_another_name_silences_it() {
    let w = warnings(
        "[profiles.fable]\nmodel = \"claude-fable-5-1\"\n[profiles.fable52]\nmodel = \"claude-fable-5-2\"\n",
    );
    assert!(w.is_empty(), "{w:?}");
}

/// No profile of the option's name is the operator's choice (a config with no `fable` at all, or Flash left
/// commented out): nothing to say.
#[test]
fn an_option_with_no_profile_of_its_name_says_nothing() {
    assert!(warnings("").is_empty());
    assert!(warnings("[profiles.pinned]\nmodel = \"claude-fable-5-1\"\n").is_empty());
}

/// Fable 5.1 stays a model a profile may pin: the catalog prices it and the loader takes the profile.
#[test]
fn fable_5_1_can_still_be_pinned_by_a_profile() {
    let cfg = Config::parse(&doc(
        "[profiles.fable]\nmodel = \"claude-fable-5-2\"\n[profiles.fable51]\nmodel = \"claude-fable-5-1\"\n",
    ))
    .unwrap()
    .0;
    assert_eq!(cfg.profile("fable51").unwrap().model, "claude-fable-5-1");
    assert!(crate::catalog::Catalog::builtin()
        .get("claude-fable-5-1")
        .is_some());
}
