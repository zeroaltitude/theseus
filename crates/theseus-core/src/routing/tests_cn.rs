//! China-origin profiles in routing's grammar (theseus-a1wvq.4): a restricted
//! profile runs only for a cleared ask, enters a class only by name in
//! `cn_profiles`, and is never `cheapest`'s pick. Profiles are built here
//! with `restricted` set, since the turn's own fill waits for the cn-floor join.

use std::borrow::Cow;

use super::tests::{ask, five, seven, verdict};
use super::*;
use crate::config::routing::ClassProfiles;

/// `seven()` with the `zai` profiles restricted, as their origin makes them.
fn cn() -> Profiles {
    let mut ps = seven();
    for p in ps.values_mut() {
        p.restricted = p.provider == "zai";
    }
    ps
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

/// `trivial` as the owner's config has it: Haiku, `cheapest`, and GLM behind Haiku for an open ask.
fn trivial_cfg() -> RoutingConfig {
    let mut cfg = RoutingConfig::default();
    cfg.classes.trivial = ClassProfiles {
        profiles: names(&["haiku", "cheapest"]),
        cn_profiles: names(&["glm"]),
        cn_after: Some("haiku".into()),
        ..Default::default()
    };
    cfg
}

fn cleared<'a>(a: Ask<'a>) -> Ask<'a> {
    Ask { cleared: true, ..a }
}

#[test]
fn a_restricted_profile_runs_only_when_cleared() {
    let ps = cn();
    let list = names(&["glm", "sonnet"]);
    assert_eq!(
        pick(&list, &ps, false, None, false),
        Picked::Profile("sonnet".into()),
        "uncleared, the restricted name is passed over"
    );
    assert_eq!(
        pick(&list, &ps, false, None, true),
        Picked::Profile("glm".into()),
        "cleared, it is picked by name"
    );
    assert_eq!(
        pick(&names(&["glm"]), &ps, false, None, false),
        Picked::Fallback
    );
    // The dear guard never counts a restricted profile usable uncleared: opus
    // alone is not skipped for GLM's lower price, but is once GLM may run.
    let dear = names(&["opus", "glm"]);
    assert_eq!(dear_guard(&dear, &ps, false, false), None);
    assert_eq!(dear_guard(&dear, &ps, false, true), Some(names(&["glm"])));
    // A step back down lands on a restricted base only for a cleared ask.
    let v = verdict("chat", 0.9);
    let a = Ask {
        from: Some("glm"),
        ..ask(&v, "opus", 10)
    };
    assert!(step_down(&ps, &a, &Picked::Own).is_none());
    let d = step_down(&ps, &cleared(a), &Picked::Own).unwrap();
    assert_eq!(d.profile, "opus", "the first of two agreeing turns holds");
    // A correction to a restricted profile stays.
    let cfg = RoutingConfig::default();
    let a = ask(&v, "opus", 10);
    let d = steer_to(&cfg, &ps, &a, "glm", true);
    assert_eq!(
        (d.profile.as_str(), d.reason, d.switch),
        ("opus", Reason::Fallback, false)
    );
    let d = steer_to(&cfg, &ps, &cleared(a), "glm", true);
    assert_eq!(
        (d.profile.as_str(), d.reason, d.switch),
        ("glm", Reason::Correction, true)
    );
}

#[test]
fn cheapest_never_picks_a_restricted_profile() {
    let mut five = five();
    for p in five.values_mut() {
        p.restricted = p.provider == "zai";
    }
    // GLM is cheapest, and restricted: the next cheapest is Sonnet, cleared or not.
    assert_eq!(cheapest(&five, false, None).as_deref(), Some("sonnet"));
    for cleared in [false, true] {
        assert_eq!(
            pick(&names(&["cheapest"]), &five, false, None, cleared),
            Picked::Profile("sonnet".into()),
            "cleared: {cleared}"
        );
        assert_eq!(
            pick(&names(&["glm", "cheapest"]), &five, false, None, cleared),
            Picked::Profile(if cleared { "glm" } else { "sonnet" }.into()),
            "a name still enters, cleared; cheapest never does"
        );
    }
    // A list ending in `cheapest` never reaches GLM, whatever the class.
    let d = decide(
        &RoutingConfig::default(),
        &five,
        &cleared(ask(&verdict("trivial", 0.9), "opus", 10)),
    );
    assert_eq!(d.profile, "sonnet");
}

#[test]
fn cn_profiles_come_first_only_when_cleared_and_obey_the_cap() {
    let mut cfg = trivial_cfg();
    cfg.classes.trivial.cn_after = None;
    let c = &cfg.classes;
    let plain = c.effective("trivial", false);
    assert!(matches!(plain, Cow::Borrowed(_)), "nothing allocated");
    assert_eq!(*plain, names(&["haiku", "cheapest"]));
    assert_eq!(
        *c.effective("trivial", true),
        names(&["glm", "haiku", "cheapest"])
    );
    assert!(
        matches!(c.effective("chat", true), Cow::Borrowed(_)),
        "a class with none borrows, cleared too"
    );
    // A place's cap still skips a dear cn profile.
    let mut ps = cn();
    ps.get_mut("glm").unwrap().short_cost = Some(0.05);
    let cap = ps["haiku"].short_cost.map(|c| c + 1e-9);
    let v = verdict("trivial", 0.9);
    let d = decide(
        &cfg,
        &ps,
        &cleared(Ask {
            cap,
            ..ask(&v, "opus", 10)
        }),
    );
    assert_eq!(
        (d.profile.as_str(), d.reason, d.detour),
        ("haiku", Reason::Capped, true)
    );
}

#[test]
fn cn_after_places_cn_profiles_after_the_named_profile() {
    let cfg = trivial_cfg();
    assert_eq!(
        *cfg.classes.effective("trivial", true),
        names(&["haiku", "glm", "cheapest"])
    );
    let mut end = trivial_cfg();
    end.classes.trivial.cn_after = Some("cheapest".into());
    assert_eq!(
        *end.classes.effective("trivial", true),
        names(&["haiku", "cheapest", "glm"])
    );
    assert_eq!(
        *cfg.classes.effective("trivial", false),
        names(&["haiku", "cheapest"])
    );
}

#[test]
fn a_detour_to_a_cn_profile_needs_clearance() {
    let cfg = trivial_cfg();
    let v = verdict("trivial", 0.9);
    let run = |haiku_usable: bool, clear: bool, v: &Verdict| {
        let mut ps = cn();
        if !haiku_usable {
            ps.get_mut("haiku").unwrap().unusable = Some("key");
        }
        let a = ask(v, "opus", 10);
        let d = decide(
            &cfg,
            &ps,
            &Ask {
                cleared: clear,
                ..a
            },
        );
        (d, ps)
    };
    let (d, _) = run(true, false, &v);
    assert_eq!(
        (d.profile.as_str(), d.reason, d.detour),
        ("haiku", Reason::Detour, true)
    );
    // Haiku out and uncleared: `cheapest`'s pick, never GLM.
    let (d, ps) = run(false, false, &v);
    assert_eq!(Some(d.profile.clone()), cheapest(&ps, false, None));
    assert_ne!(d.profile, "glm");
    let (d, _) = run(true, true, &v);
    assert_eq!(d.profile, "haiku", "cleared with Haiku usable");
    let (d, _) = run(false, true, &v);
    assert_eq!(
        (d.profile.as_str(), d.reason, d.detour),
        ("glm", Reason::Detour, true),
        "cleared with Haiku out: GLM, ahead of cheapest"
    );
    // `quick` has no cn_profiles: never GLM, cleared or not.
    let quick = verdict("quick", 0.9);
    for clear in [false, true] {
        let (d, _) = run(false, clear, &quick);
        assert_eq!(d.reason, Reason::Fallback, "cleared: {clear}");
        assert_eq!(d.profile, "opus", "the session's own stays");
    }
}
