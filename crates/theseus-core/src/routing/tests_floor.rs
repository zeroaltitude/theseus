//! The coding floor (theseus-mn5dr, the TB-hard report's fix 5): a coding
//! verdict under its bar may hold `sonnethi` instead of falling back to the
//! session's base profile, when `[routing.coding_floor]` names one. It ships
//! off: its A/B waits for the benchmarks after 1.0.4.

use super::tests::{ask, five, verdict};
use super::*;

/// `five()` with a cheap base (`haiku`) and the floor's profile (`sonnethi`,
/// Sonnet's price: its own effort is the profile's setting, not the pick's).
fn profiles() -> Profiles {
    let mut ps = five();
    let mut sonnethi = ps["sonnet"].clone();
    sonnethi.model = "claude-sonnet-5-5".into();
    ps.insert("sonnethi".into(), sonnethi);
    let mut haiku = ps["sonnet"].clone();
    haiku.short_cost = haiku.short_cost.map(|c| c / 3.0);
    haiku.input_per_mtok = haiku.input_per_mtok.map(|c| c / 3.0);
    ps.insert("haiku".into(), haiku);
    ps
}

/// A `[routing]` table with the floor named, and `extra` lines of the floor's own.
fn floored(extra: &str) -> RoutingConfig {
    toml::from_str(&format!("[coding_floor]\nprofile = \"sonnethi\"\n{extra}"))
        .expect("[routing.coding_floor] parses")
}

#[test]
fn an_unsure_coding_verdict_holds_the_floor_instead_of_the_base() {
    let cfg = floored("");
    for mode in ["routine_coding", "deep_coding"] {
        let v = verdict(mode, 0.45);
        let d = decide(&cfg, &profiles(), &ask(&v, "haiku", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason.as_str(), d.switch, d.detour),
            ("sonnethi", "floor", true, false),
            "{mode} at 0.45, under the bar of 0.6 and over the floor's own"
        );
    }
}

#[test]
fn only_a_coding_turn_holds_it() {
    let cfg = floored("");
    for mode in [
        "run_tool",
        "commentary",
        "trivial",
        "quick",
        "chat",
        "sophisticated",
        "review",
        "other",
    ] {
        // 0.35: over the floor's own bar and under every class's (trivial's is 0.4).
        let v = verdict(mode, 0.35);
        let d = decide(&cfg, &profiles(), &ask(&v, "haiku", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason, d.switch),
            ("haiku", Reason::Unsure, false),
            "{mode} is not a coding turn"
        );
    }
}

#[test]
fn a_verdict_too_unsure_for_the_floor_stays_on_the_base() {
    let cfg = floored("");
    let v = verdict("routine_coding", 0.29);
    let d = decide(&cfg, &profiles(), &ask(&v, "haiku", 10));
    assert_eq!((d.profile.as_str(), d.reason), ("haiku", Reason::Unsure));
    let own = floored("min_confidence = 0.5");
    let v = verdict("routine_coding", 0.45);
    let d = decide(&own, &profiles(), &ask(&v, "haiku", 10));
    assert_eq!(
        (d.profile.as_str(), d.reason),
        ("haiku", Reason::Unsure),
        "the floor's own bar is the config's"
    );
}

#[test]
fn a_sure_verdict_is_the_classs_not_the_floors() {
    let cfg = floored("");
    for (mode, want) in [("routine_coding", "sonnet"), ("deep_coding", "opus")] {
        let v = verdict(mode, 0.9);
        let d = decide(&cfg, &profiles(), &ask(&v, "haiku", 10));
        assert_eq!(
            (d.profile.as_str(), d.reason),
            (want, Reason::Verdict),
            "{mode}"
        );
    }
}

#[test]
fn the_floor_never_steps_a_session_down_or_past_a_cap_or_onto_a_restricted_profile() {
    let cfg = floored("");
    let v = verdict("deep_coding", 0.45);
    // Already there: nothing to hold.
    let d = decide(&cfg, &profiles(), &ask(&v, "sonnethi", 10));
    assert_eq!(
        (d.profile.as_str(), d.reason, d.switch),
        ("sonnethi", Reason::Unsure, false)
    );
    // Dearer than the place's cap.
    let ps = profiles();
    let cap = Some(ps["haiku"].short_cost.unwrap());
    let d = decide(
        &cfg,
        &ps,
        &Ask {
            cap,
            ..ask(&v, "haiku", 10)
        },
    );
    assert_eq!((d.profile.as_str(), d.reason), ("haiku", Reason::Unsure));
    // Unusable.
    let mut ps = profiles();
    ps.get_mut("sonnethi").unwrap().unusable = Some("key");
    let d = decide(&cfg, &ps, &ask(&v, "haiku", 10));
    assert_eq!((d.profile.as_str(), d.reason), ("haiku", Reason::Unsure));
    // A China-origin model never answers a coding turn, cleared or not.
    let mut ps = profiles();
    ps.get_mut("sonnethi").unwrap().restricted = true;
    let d = decide(
        &cfg,
        &ps,
        &Ask {
            cleared: true,
            ..ask(&v, "haiku", 10)
        },
    );
    assert_eq!((d.profile.as_str(), d.reason), ("haiku", Reason::Unsure));
}

#[test]
fn the_floor_keeps_the_caches_rule_for_a_switch() {
    let cfg = floored("");
    let v = verdict("routine_coding", 0.45);
    let big = cfg.cold_switch_tokens + 1;
    let d = decide(&cfg, &profiles(), &ask(&v, "haiku", big));
    assert_eq!(
        (d.profile.as_str(), d.reason, d.switch),
        ("haiku", Reason::CacheHold, false),
        "a long context waits for a second turn that agrees"
    );
    let HoldNext::Set(hold) = d.hold else {
        panic!("the held switch sets its row: {:?}", d.hold);
    };
    assert_eq!(hold.profile, "sonnethi");
    let d = decide(
        &cfg,
        &profiles(),
        &Ask {
            hold: Some(&hold),
            ..ask(&v, "haiku", big)
        },
    );
    assert_eq!(
        (d.profile.as_str(), d.reason.as_str(), d.switch),
        ("sonnethi", "floor", true)
    );
}

#[test]
fn the_floor_ships_off() {
    // Nothing named: the table is there, and nothing is held.
    let empty: RoutingConfig = toml::from_str("[coding_floor]\n").expect("an empty table parses");
    assert_eq!(empty, RoutingConfig::default());
    for cfg in [RoutingConfig::default(), empty] {
        let v = verdict("deep_coding", 0.45);
        let d = decide(&cfg, &profiles(), &ask(&v, "haiku", 10));
        assert_eq!((d.profile.as_str(), d.reason), ("haiku", Reason::Unsure));
    }
    // And the template says so, where an operator reads the knob.
    let template = include_str!("../../config/theseus.example.toml");
    assert!(
        template.contains("# [routing.coding_floor]"),
        "the template documents the knob, commented"
    );
}

#[test]
fn the_floors_own_settings_are_checked_at_load() {
    let ok = |c: &RoutingConfig| c.validate(|_| false);
    assert!(ok(&floored("")).is_ok());
    let err = ok(&floored("min_confidence = 0")).unwrap_err().to_string();
    assert!(err.contains("coding_floor.min_confidence"), "{err}");
    let err = ok(&floored("min_confidence = 0.6"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("coding_floor.min_confidence") && err.contains("switch_confidence"),
        "a floor at the bar never fires: {err}"
    );
    let blank: RoutingConfig = toml::from_str("[coding_floor]\nprofile = \" \"\n").unwrap();
    let err = ok(&blank).unwrap_err().to_string();
    assert!(err.contains("coding_floor.profile"), "{err}");
}

/// An off floor (the shipped default) is not checked at load: its
/// `min_confidence` default must not refuse a legal `switch_confidence`
/// (theseus-mn5dr, review H-1a).
#[test]
fn an_off_floor_never_refuses_a_legal_config() {
    let ok = |c: &RoutingConfig| c.validate(|_| false);
    let eager: RoutingConfig = toml::from_str("switch_confidence = 0.25\n").unwrap();
    assert!(eager.coding_floor.is_off());
    assert!(ok(&eager).is_ok(), "{:?}", ok(&eager));
    // Nor does a floor's own setting matter while no profile is named.
    let silent: RoutingConfig = toml::from_str("[coding_floor]\nmin_confidence = 0.9\n").unwrap();
    assert!(ok(&silent).is_ok(), "{:?}", ok(&silent));
}

/// The floor fires in `[min_confidence, the class's own bar)`, so the load
/// reads the bars `decide` reads (`confidence_for`), not the section's alone
/// (theseus-mn5dr, review H-1b). It is refused only when no coding class has
/// room: one with room still has a floor that fires.
#[test]
fn the_floors_window_is_the_coding_classes_own_bars() {
    let ok = |c: &RoutingConfig| c.validate(|_| false);
    let with = |t: &str| -> RoutingConfig {
        toml::from_str(&format!("[coding_floor]\nprofile = \"sonnethi\"\n{t}")).unwrap()
    };
    // Both coding classes' own bars under the floor's 0.30: it never fires.
    let both = "[classes.routine_coding]\nswitch_confidence = 0.25\n\
                [classes.deep_coding]\nswitch_confidence = 0.25\n";
    let err = ok(&with(both)).unwrap_err().to_string();
    assert!(
        err.contains("coding_floor.min_confidence") && err.contains("never fires"),
        "{err}"
    );
    // The boundary is the bar itself: a bar equal to the floor is an empty window.
    let at = "[classes.routine_coding]\nswitch_confidence = 0.30\n\
              [classes.deep_coding]\nswitch_confidence = 0.30\n";
    assert!(ok(&with(at)).is_err(), "a bar equal to the floor never fires");
    let over = "[classes.routine_coding]\nswitch_confidence = 0.30\n\
                [classes.deep_coding]\nswitch_confidence = 0.31\n";
    assert!(ok(&with(over)).is_ok(), "{:?}", ok(&with(over)));
    // One coding class with room is a working floor, whatever the other says.
    let one = "[classes.deep_coding]\nswitch_confidence = 0.25\n";
    assert!(ok(&with(one)).is_ok(), "{:?}", ok(&with(one)));
    // A low section bar with classes that set their own is judged on theirs.
    let own: RoutingConfig = toml::from_str(
        "switch_confidence = 0.25\n\
         [classes.routine_coding]\nswitch_confidence = 0.5\n\
         [classes.deep_coding]\nswitch_confidence = 0.5\n\
         [coding_floor]\nprofile = \"sonnethi\"\n",
    )
    .unwrap();
    assert!(ok(&own).is_ok(), "{:?}", ok(&own));
    // With no class of its own, the section's bar is the window's top.
    let low: RoutingConfig =
        toml::from_str("switch_confidence = 0.25\n[coding_floor]\nprofile = \"sonnethi\"\n")
            .unwrap();
    assert!(ok(&low).is_err(), "the section's 0.25 is under the floor's 0.30");
}

#[test]
fn a_floor_lifts_a_session_and_never_lowers_one() {
    let cfg = floored("");
    let ps = profiles();
    // `opus` costs more than `sonnethi`: holding the floor would be a step down.
    let v = verdict("deep_coding", 0.45);
    let d = decide(&cfg, &ps, &ask(&v, "opus", 10));
    assert_eq!(
        (d.profile.as_str(), d.reason, d.switch),
        ("opus", Reason::Unsure, false)
    );
    // And a base with no price cannot be told to be below it.
    let mut ps = profiles();
    ps.get_mut("haiku").unwrap().short_cost = None;
    let d = decide(&cfg, &ps, &ask(&v, "haiku", 10));
    assert_eq!((d.profile.as_str(), d.reason), ("haiku", Reason::Unsure));
}
