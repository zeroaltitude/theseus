//! route.v10's `model` path, purely (theseus-h66v4 row 3, design §1.4 steps 3, 4,
//! 6 and 7): the pick Jev names, the floor, the cache hold, and the rules that
//! are gone (the place cap, the class walk, the step down, and the detour: a
//! pick never trims the context, theseus-h66v4.1).

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

fn pick(choice: &str, confidence: f64, sure: bool) -> Box<Pick> {
    Box::new(Pick {
        choice: choice.into(),
        confidence,
        sure,
    })
}

/// route.v10's verdict: a sure `model` answer at 0.95.
fn open(model: &str) -> Verdict {
    Verdict {
        mode: model.into(),
        confidence: 0.95,
        model: Some(pick(model, 0.95, true)),
        ..super::tests::verdict("", 0.95)
    }
}

fn ask<'a>(v: &'a Verdict, base: &'a str, est: u64) -> Ask<'a> {
    super::tests::ask(v, base, est)
}

fn cfg() -> RoutingConfig {
    RoutingConfig {
        cold_switch_tokens: COLD,
        ..RoutingConfig::default()
    }
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
    let v = open("opus");
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
    let v = open("sonnet");
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
        ..open("opus")
    };
    let got = d(&v, "sonnet", 1_000);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::Unsure, false)
    );
}

#[test]
fn the_floor_refuses_a_restricted_pick_whatever_jev_said() {
    let v = open("deepseek_flash");
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
    let v = open("opus");
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
    let v = open("kimi");
    let got = d(&v, "sonnet", 1_000);
    assert_eq!(
        (got.profile.as_str(), got.reason),
        ("sonnet", Reason::Fallback)
    );
}

#[test]
fn the_cache_hold_defers_a_full_switch_one_turn_and_never_longer() {
    let v = open("opus");
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

/// theseus-h66v4.1: the `context` question is gone, and with it the detour. A
/// pick, sure or not, on any size of session, is never a detour: every turn
/// compiles the whole working context on the model that answers it.
#[test]
fn a_pick_is_never_a_detour_every_turn_compiles_the_whole_context() {
    for (pick, base) in [("haiku", "sonnet"), ("sonnet", "sonnet"), ("opus", "sonnet")] {
        for est in [0, 1_000, 29_999, 200_000, 900_000] {
            let got = d(&open(pick), base, est);
            assert!(!got.detour, "{pick} on {base} at {est}");
        }
    }
    // Nor an unsure one, nor one the floor refused.
    let v = Verdict {
        model: Some(pick("opus", 0.4, false)),
        ..open("opus")
    };
    assert!(!d(&v, "sonnet", 100_000).detour);
}

#[test]
fn nothing_of_the_old_walk_is_left() {
    // A cap that would refuse every profile is not read.
    let v = open("fable");
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
    let v = open("sonnet");
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
    let v = open("haiku");
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
fn a_late_pick_carries_like_a_switch_and_v8s_detour_modes_do_not() {
    assert!(carries(&open("haiku")), "no pick is its message's alone");
    assert!(carries(&open("opus")));
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

fn fixture() -> theseus_judge::Pack {
    theseus_judge::Pack::parse(include_str!("../../fixtures/packs/route.open.toml")).unwrap()
}

/// Every option a pack's `model` question offers is one the daemon can run, and
/// the table names no model the pack does not offer: the two lists are one. The
/// fixture is route.v10's shape, so the check bites here; the embedded sweep
/// below bites on the real route.v10 once it is in the build, and on any pack
/// that later asks `model`.
#[test]
fn the_models_options_and_the_daemons_table_are_one_list() {
    let pack = fixture();
    assert_eq!(model::unknown_options(&pack), Vec::<String>::new());
    let offered: Vec<String> = pack
        .questions
        .iter()
        .find(|q| q.id == "model")
        .unwrap()
        .options
        .iter()
        .map(|o| o.id.clone())
        .collect();
    let named: Vec<String> = model::OPTIONS.iter().map(|(o, _)| o.to_string()).collect();
    assert_eq!(offered, named, "same options, same order");
}

#[test]
fn a_foreign_option_is_found() {
    let text = include_str!("../../fixtures/packs/route.open.toml")
        .replace("{ id = \"fable\"", "{ id = \"kimi\"");
    let pack = theseus_judge::Pack::parse(&text).unwrap();
    assert_eq!(model::unknown_options(&pack), ["kimi"]);
}

/// Any embedded pack that asks `model` (route.v10 once it is embedded) names only
/// options the table can run: a pack that gained a `model` question with foreign
/// options would flip the whole regime and fall back on every turn.
#[test]
fn every_embedded_pack_that_asks_model_names_only_what_the_table_runs() {
    let packs = theseus_judge::pack::embedded().as_ref().unwrap();
    for p in packs {
        assert_eq!(
            model::unknown_options(p),
            Vec::<String>::new(),
            "{} offers a model the daemon cannot run",
            p.name()
        );
    }
}

// The seam between row 3's `model` path and row 4's checkpoint and brief rules
// (theseus-h66v4's join): each was green alone, against a different `decide`.

/// A task's checkpoint takes Jev's `model` pick whatever the place's cap, up or
/// down, with no base to return to: the cap field is still carried by the ask
/// but the `model` path never reads it.
#[test]
fn a_checkpoint_takes_jevs_pick_up_or_down_and_no_cap_refuses_it() {
    let ps = five_options();
    for (pick, from, to) in [("opus", "sonnet", "opus"), ("haiku", "opus", "haiku")] {
        let v = open(pick);
        let got = checkpoint::decide(
            &cfg(),
            &ps,
            &checkpoint::CheckpointAsk {
                verdict: &v,
                base: "sonnet",
                current: from,
                est_tokens: 1_000,
                hold: None,
                images: false,
                cap: Some(0.000_001),
                moves: 9,
                open: true,
            },
        );
        assert_eq!(
            (got.profile.as_str(), got.reason, got.switch),
            (to, Reason::Verdict, true),
            "{from} -> {pick}"
        );
    }
}

/// The checkpoint's own cache hold sits on top of the `model` path's: a move at
/// or above the cold-switch estimate waits one agreeing checkpoint, and the
/// agreeing one moves (the `model` path's own hold is switched off for it, so
/// the two never stack into a wait of two).
#[test]
fn a_checkpoint_move_over_the_cold_size_waits_for_one_agreeing_verdict() {
    let ps = five_options();
    let v = open("opus");
    let ask = |hold| checkpoint::CheckpointAsk {
        verdict: &v,
        base: "sonnet",
        current: "sonnet",
        est_tokens: COLD + 1,
        hold,
        images: false,
        cap: None,
        moves: 0,
        open: true,
    };
    let first = checkpoint::decide(&cfg(), &ps, &ask(None));
    assert_eq!(
        (first.profile.as_str(), first.reason),
        ("sonnet", Reason::CacheHold)
    );
    let HoldNext::Set(held) = first.hold else {
        panic!("the first sets a hold");
    };
    let second = checkpoint::decide(&cfg(), &ps, &ask(Some(&held)));
    assert_eq!(
        (second.profile.as_str(), second.reason, second.switch),
        ("opus", Reason::Verdict, true)
    );
}

/// A task's brief takes Jev's pick as the task's starting model, whatever the
/// parent runs on and however big the parent's conversation is.
#[test]
fn a_brief_starts_a_task_on_jevs_pick() {
    let v = open("haiku");
    let got = brief::brief_base(&cfg(), &five_options(), &ask(&v, "opus", 1_000_000), true);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("haiku", Reason::Verdict, true)
    );
}
