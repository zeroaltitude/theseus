//! route.v10's `model` path, purely (theseus-h66v4 row 3, design §1.4 steps 3, 4,
//! 6 and 7): the pick Jev names, the floor, the cache hold, a `short` pick, and
//! the rules that are gone (the place cap, the class walk, the step down).

use super::*;

const COLD: u64 = 30_000;

fn catalog_profile(provider: &str, model: &str, restricted: bool) -> Profile {
    let c = crate::catalog::Catalog::builtin();
    let e = c.get(model).expect("in the catalog");
    Profile {
        provider: provider.into(),
        model: model.into(),
        unusable: None,
        short_cost: Some(short_cost(e)),
        input_per_mtok: Some(e.input_per_mtok),
        vision: e.vision,
        restricted,
    }
}

/// The five option models, each under the name the template gives it, and
/// Flash restricted (China-origin) as its provider's origin makes it.
fn five_options() -> Profiles {
    [
        ("haiku", "anthropic", "claude-haiku-5-5", false),
        ("sonnet", "anthropic", "claude-sonnet-5-5", false),
        ("opus", "anthropic", "claude-opus-5-5", false),
        ("fable", "anthropic", "claude-fable-5-1", false),
        ("dsflash", "deepseek", "deepseek-flash", true),
    ]
    .into_iter()
    .map(|(n, p, m, r)| (n.to_string(), catalog_profile(p, m, r)))
    .collect()
}

fn pick(choice: &str, confidence: f64, sure: bool) -> Pick {
    Pick {
        choice: choice.into(),
        confidence,
        sure,
    }
}

/// route.v10's verdict: a sure `model` answer at 0.95, a sure `context` answer.
fn open(model: &str, context: &str) -> Verdict {
    Verdict {
        mode: model.into(),
        confidence: 0.95,
        model: Some(pick(model, 0.95, true)),
        context: Some(pick(context, 0.95, true)),
        ..super::tests::verdict("", 0.95)
    }
}

fn ask<'a>(v: &'a Verdict, base: &'a str, est: u64) -> Ask<'a> {
    super::tests::ask(v, base, est)
}

fn cfg() -> RoutingConfig {
    let mut c = RoutingConfig::default();
    c.cold_switch_tokens = COLD;
    c
}

fn d(v: &Verdict, base: &str, est: u64) -> Decision {
    decide(&cfg(), &five_options(), &ask(v, base, est))
}

#[test]
fn an_option_names_a_catalog_model_and_the_profile_that_runs_it() {
    for (option, model) in [
        ("haiku", "claude-haiku-5-5"),
        ("sonnet", "claude-sonnet-5-5"),
        ("opus", "claude-opus-5-5"),
        ("fable", "claude-fable-5-1"),
        ("deepseek_flash", "deepseek-flash"),
    ] {
        assert_eq!(model::model_of(option), Some(model), "{option}");
        assert!(crate::catalog::Catalog::builtin().get(model).is_some());
    }
    assert_eq!(
        model::model_of("kimi"),
        None,
        "a model nowhere best is a pin"
    );
    let ps = five_options();
    assert_eq!(model::profile_for("opus", &ps), Some("opus"));
    assert_eq!(model::profile_for("deepseek_flash", &ps), Some("dsflash"));
    assert_eq!(model::profile_for("kimi", &ps), None);
    // Two profiles run one model: the one named for the option wins, then the
    // first by name; an unusable one is passed over for a usable one.
    let mut two = five_options();
    two.insert(
        "haikuhi".into(),
        catalog_profile("anthropic", "claude-haiku-5-5", false),
    );
    assert_eq!(model::profile_for("haiku", &two), Some("haiku"));
    two.get_mut("haiku").unwrap().unusable = Some("key");
    assert_eq!(model::profile_for("haiku", &two), Some("haikuhi"));
    two.remove("haiku");
    two.remove("haikuhi");
    assert_eq!(
        model::profile_for("haiku", &two),
        None,
        "no profile runs it"
    );
}

#[test]
fn a_sure_pick_runs_and_switches_a_small_session() {
    let v = open("opus", "full");
    let got = d(&v, "sonnet", 12_000);
    assert_eq!(
        got,
        Decision {
            profile: "opus".into(),
            reason: Reason::Verdict,
            detour: false,
            switch: true,
            hold: HoldNext::Clear,
        }
    );
}

#[test]
fn a_pick_at_the_base_stays_and_clears_a_hold() {
    let v = open("sonnet", "full");
    let got = d(&v, "sonnet", 400_000);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch, got.detour),
        ("sonnet", Reason::Verdict, false, false)
    );
    assert_eq!(got.hold, HoldNext::Clear);
}

#[test]
fn a_pick_under_the_bar_stays_and_says_unsure() {
    let v = Verdict {
        model: Some(pick("opus", 0.45, false)),
        ..open("opus", "full")
    };
    let got = d(&v, "sonnet", 1_000);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::Unsure, false)
    );
}

#[test]
fn the_floor_refuses_a_restricted_pick_whatever_jev_said() {
    let v = open("deepseek_flash", "full");
    // The daemon's own marking: the floor made the profile unusable.
    let mut ps = five_options();
    ps.get_mut("dsflash").unwrap().unusable = Some("data_gate");
    let a = Ask {
        cleared: false,
        ..ask(&v, "sonnet", 1_000)
    };
    let got = decide(&cfg(), &ps, &a);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::DataGate, false)
    );
    // Not marked, but restricted and the ask not cleared: the rule is the
    // routing's own too, never only the daemon's marking.
    let got = decide(&cfg(), &five_options(), &a);
    assert_eq!(
        (got.profile.as_str(), got.reason),
        ("sonnet", Reason::DataGate)
    );
    // Cleared: it runs.
    let a = Ask {
        cleared: true,
        ..ask(&v, "sonnet", 1_000)
    };
    let got = decide(&cfg(), &five_options(), &a);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("dsflash", Reason::Verdict, true)
    );
}

#[test]
fn a_pick_that_cannot_run_says_fallback() {
    let v = open("opus", "full");
    // No key: the profile is unusable for it.
    let mut ps = five_options();
    ps.get_mut("opus").unwrap().unusable = Some("key");
    let got = decide(&cfg(), &ps, &ask(&v, "sonnet", 1_000));
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::Fallback, false)
    );
    // No profile for the model.
    let mut ps = five_options();
    ps.remove("opus");
    let got = decide(&cfg(), &ps, &ask(&v, "sonnet", 1_000));
    assert_eq!(
        (got.profile.as_str(), got.reason),
        ("sonnet", Reason::Fallback)
    );
    // A model that cannot read the turn's images.
    let mut ps = five_options();
    ps.get_mut("opus").unwrap().vision = false;
    let a = Ask {
        images: true,
        ..ask(&v, "sonnet", 1_000)
    };
    let got = decide(&cfg(), &ps, &a);
    assert_eq!(
        (got.profile.as_str(), got.reason),
        ("sonnet", Reason::Fallback)
    );
    // A pick no option names (a pack newer than the table).
    let v = open("kimi", "full");
    let got = d(&v, "sonnet", 1_000);
    assert_eq!(
        (got.profile.as_str(), got.reason),
        ("sonnet", Reason::Fallback)
    );
}

#[test]
fn the_cache_hold_defers_a_full_switch_one_turn_and_never_longer() {
    let v = open("opus", "full");
    // Over the cold line: held one turn.
    let first = d(&v, "sonnet", COLD);
    assert_eq!(
        (first.profile.as_str(), first.reason, first.switch),
        ("sonnet", Reason::CacheHold, false)
    );
    let HoldNext::Set(h) = &first.hold else {
        panic!("a hold is set: {first:?}");
    };
    assert_eq!(
        (h.mode.as_str(), h.profile.as_str(), h.turn.as_str()),
        ("opus", "opus", "turn_1")
    );
    // The next agreeing pick switches, at any size.
    let a = Ask {
        hold: Some(h),
        ..ask(&v, "sonnet", 900_000)
    };
    let second = decide(&cfg(), &five_options(), &a);
    assert_eq!(
        (
            second.profile.as_str(),
            second.reason,
            second.switch,
            second.hold
        ),
        ("opus", Reason::Verdict, true, HoldNext::Clear)
    );
    // Under the cold line: no hold at all.
    let small = d(&v, "sonnet", COLD - 1);
    assert_eq!((small.reason, small.switch), (Reason::Verdict, true));
    // A hold for another model does not agree.
    let other = Hold {
        profile: "fable".into(),
        ..h.clone()
    };
    let a = Ask {
        hold: Some(&other),
        ..ask(&v, "sonnet", 900_000)
    };
    assert_eq!(
        decide(&cfg(), &five_options(), &a).reason,
        Reason::CacheHold
    );
}

#[test]
fn a_short_pick_is_a_detour_that_never_holds_and_never_switches() {
    let v = open("haiku", "short");
    let got = d(&v, "sonnet", 200_000);
    assert_eq!(
        got,
        Decision {
            profile: "haiku".into(),
            reason: Reason::Verdict,
            detour: true,
            switch: false,
            hold: HoldNext::Keep,
        },
        "200K on Sonnet, a thank-you: Haiku for this message alone, no hold"
    );
    // The session's own model: the detour compile still answers from the
    // last exchanges (the context question is its own).
    let v = open("sonnet", "short");
    let got = d(&v, "sonnet", 200_000);
    assert_eq!(
        (got.profile.as_str(), got.detour, got.switch),
        ("sonnet", true, false)
    );
    // A hold set by an earlier full pick is left as it was.
    assert_eq!(got.hold, HoldNext::Keep);
}

#[test]
fn a_short_answer_under_the_bar_is_full() {
    let v = Verdict {
        context: Some(pick("short", 0.40, false)),
        ..open("haiku", "short")
    };
    assert!(!v.short());
    let got = d(&v, "sonnet", 200_000);
    assert_eq!(
        (got.reason, got.detour, got.switch),
        (Reason::CacheHold, false, false),
        "no_match is full: a full pick over the line holds"
    );
}

#[test]
fn a_short_context_applies_whatever_became_of_the_model() {
    // An unsure model and a sure `short`: the base answers from the last exchanges.
    let v = Verdict {
        model: Some(pick("opus", 0.4, false)),
        ..open("opus", "short")
    };
    let got = d(&v, "sonnet", 100_000);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.detour, got.switch),
        ("sonnet", Reason::Unsure, true, false)
    );
}

#[test]
fn nothing_of_the_old_walk_is_left() {
    // A cap that would refuse every profile is not read.
    let v = open("fable", "full");
    let a = Ask {
        cap: Some(0.0),
        ..ask(&v, "haiku", 1_000)
    };
    let got = decide(&cfg(), &five_options(), &a);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("fable", Reason::Verdict, true)
    );
    // No step down: a session Jev moved up goes down only because Jev says so,
    // at once, with no row of agreeing verdicts.
    let v = open("sonnet", "full");
    let a = Ask {
        from: Some("sonnet"),
        ..ask(&v, "opus", 1_000)
    };
    let got = decide(&cfg(), &five_options(), &a);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::Verdict, true)
    );
    // No class list: the answer names a model, whatever `[routing.classes]` says.
    let mut c = cfg();
    c.classes.trivial.profiles = vec!["sonnet".into()];
    let v = open("haiku", "full");
    let got = decide(&c, &five_options(), &ask(&v, "sonnet", 1_000));
    assert_eq!(got.profile, "haiku");
}

#[test]
fn a_verdict_without_a_model_answer_still_walks_the_classes() {
    let v = super::tests::verdict("sophisticated", 0.95);
    assert!(!v.open());
    let ps = super::tests::five();
    let got = decide(&cfg(), &ps, &super::tests::ask(&v, "sonnet", 1_000));
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("opus", Reason::Verdict, true)
    );
}

#[test]
fn a_late_short_verdict_carries_to_no_other_message() {
    assert!(
        !carries(&open("haiku", "short")),
        "a short pick is its message's alone"
    );
    assert!(carries(&open("opus", "full")));
    assert!(
        carries(&super::tests::verdict("chat", 0.9)),
        "v8's rule is unchanged"
    );
    assert!(!carries(&super::tests::verdict("trivial", 0.9)));
}

#[test]
fn a_blocks_why_is_its_first_sentence_and_the_one_that_says_what_it_is_for() {
    let opus = "Opus 5.5 (Anthropic, US). Twice Sonnet's price: $4 in, $20 out, $0.20 a cache read per million. Always thinks; its maker's default effort is medium. For reviewing code, a plan or an argument for mistakes; comparing things on their merits. Not for programming itself, where Sonnet solves nearly as much for far less.";
    assert_eq!(
        model::block_why(opus),
        "Opus 5.5 (Anthropic, US). … For reviewing code, a plan or an argument for mistakes; comparing things on their merits."
    );
    let fable = "Fable 5.1 (Anthropic, US), the most capable: $10 in, $50 out. Always thinks. Only for the hardest design or reasoning problems: novel, spanning many parts. Never for ordinary programming.";
    assert_eq!(
        model::block_why(fable),
        "Fable 5.1 (Anthropic, US), the most capable: $10 in, $50 out. … Only for the hardest design or reasoning problems: novel, spanning many parts."
    );
    // Prices hold decimal points, which end no sentence.
    let sonnet = "Sonnet 5.5 (Anthropic, US). The usual choice: $2 in, $10 out, $0.20 a cache read per million. For conversation about the work. Choose it when unsure.";
    assert_eq!(
        model::block_why(sonnet),
        "Sonnet 5.5 (Anthropic, US). … For conversation about the work."
    );
    // A block with no "for" sentence is its first sentence alone.
    assert_eq!(model::block_why("Only a name."), "Only a name.");
    assert_eq!(model::block_why(""), "");
}
