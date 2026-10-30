//! `[ontology]` (theseus-xzxqv): its defaults, what it refuses, and that it
//! stays out of a sparse note when it is the default.

use super::{ontology::Request, Config};

fn doc(rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n")
}

#[test]
fn the_defaults_are_the_documented_ones() {
    let (cfg, _) = Config::parse(&doc("")).unwrap();
    let c = &cfg.ontology;
    assert_eq!(
        (
            c.judge,
            c.act,
            c.confirm,
            c.name_profile.as_deref(),
            c.request
        ),
        (true, 0.80, 0.55, None, Request::Batch)
    );
    assert_eq!(c.recall_boost, 0.25);
    assert!(c.is_default());
    // The private mark ships off until the owner says on (theseus-und4g).
    assert!(!c.private_mark);
    // The nightly sweep ships on: it touches only what a machine wrote.
    assert!(c.sweep);
}

#[test]
fn the_sweep_is_a_key_that_is_on_by_default() {
    let (cfg, _) = Config::parse(&doc("[ontology]\nsweep = false\n")).unwrap();
    assert!(!cfg.ontology.sweep);
    assert!(!cfg.ontology.is_default());
}

#[test]
fn the_private_mark_is_a_key_that_is_off_by_default() {
    let (cfg, _) = Config::parse(&doc("[ontology]\nprivate_mark = true\n")).unwrap();
    assert!(cfg.ontology.private_mark);
    assert!(!cfg.ontology.is_default());
}

#[test]
fn a_key_is_read_and_a_bad_one_is_refused_with_its_name() {
    let (cfg, _) = Config::parse(&doc(
        "[ontology]\njudge = false\nact = 0.9\nconfirm = 0.6\nname_profile = \"sonnet\"\nrequest = \"own\"\n",
    ))
    .unwrap();
    let c = &cfg.ontology;
    assert_eq!(
        (
            c.judge,
            c.act,
            c.confirm,
            c.name_profile.as_deref(),
            c.request
        ),
        (false, 0.9, 0.6, Some("sonnet"), Request::Own)
    );
    for (bad, word) in [
        ("confirm = 0.9\nact = 0.8", "confirm"),
        ("act = 1.5", "act"),
        ("name_profile = \" \"", "name_profile"),
        ("recall_boost = 2.0", "recall_boost"),
        ("request = \"third\"", "third"),
        ("nightly = true", "unknown field"),
    ] {
        let e = Config::parse(&doc(&format!("[ontology]\n{bad}\n"))).unwrap_err();
        assert!(format!("{e:#}").contains(word), "{bad}: {e:#}");
    }
}

#[test]
fn the_sweeps_daily_cap_is_a_dollar_and_zero_turns_the_model_off() {
    let (cfg, _) = Config::parse(&doc("")).unwrap();
    assert_eq!(cfg.ontology.sweep_usd_per_day, 1.0);
    let (cfg, _) = Config::parse(&doc("[ontology]\nsweep_usd_per_day = 0\n")).unwrap();
    assert_eq!(cfg.ontology.sweep_usd_per_day, 0.0);
    assert!(!cfg.ontology.is_default());
    let e = Config::parse(&doc("[ontology]\nsweep_usd_per_day = -1\n")).unwrap_err();
    assert!(format!("{e:#}").contains("sweep_usd_per_day"), "{e:#}");
}

/// The dream's guidance (theseus-23wd8) ships on, has a switch of its own, and
/// the template's `[ontology]` names it.
#[test]
fn the_dreams_guidance_is_a_key_that_is_on_by_default() {
    let (cfg, _) = Config::parse(&doc("")).unwrap();
    assert!(cfg.ontology.dream_guidance);
    let (cfg, _) = Config::parse(&doc("[ontology]\ndream_guidance = false\n")).unwrap();
    assert!(!cfg.ontology.dream_guidance);
    assert!(!cfg.ontology.is_default());
    let section = Config::EXAMPLE_TOML
        .split("# [ontology]\n")
        .nth(1)
        .and_then(|s| s.split("\n\n").next())
        .unwrap();
    assert!(section.contains("\n# dream_guidance = true "), "{section}");
}

/// `not_topics` (theseus-648f8) ships empty, is read, and the template's
/// `[ontology]` names it; a bad key beside it is still refused.
#[test]
fn not_topics_is_a_key_that_is_empty_by_default() {
    let (cfg, _) = Config::parse(&doc("")).unwrap();
    assert!(cfg.ontology.not_topics.is_empty());
    let (cfg, _) = Config::parse(&doc(
        "[ontology]\nnot_topics = [\"Small Talk\", \"status checks\"]\n",
    ))
    .unwrap();
    assert_eq!(cfg.ontology.not_topics, ["Small Talk", "status checks"]);
    assert!(!cfg.ontology.is_default());
    let e = Config::parse(&doc("[ontology]\nnot_topic = []\n")).unwrap_err();
    assert!(format!("{e:#}").contains("not_topic"), "{e:#}");
    let section = Config::EXAMPLE_TOML
        .split("# [ontology]\n")
        .nth(1)
        .and_then(|s| s.split("\n\n").next())
        .unwrap();
    assert!(section.contains("\n# not_topics = [] "), "{section}");
}
