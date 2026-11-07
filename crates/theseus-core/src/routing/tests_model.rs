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

/// route.v10's own declared default for its `model` question, read out of the
/// pack. Every pick this file builds carries it, as a live one does.
fn declared() -> String {
    theseus_judge::pack::by_name("route.v10")
        .expect("route.v10 is embedded")
        .question("model")
        .expect("route.v10 asks `model`")
        .no_match
        .clone()
        .expect("route.v10 declares a no_match")
}

fn pick(choice: &str, confidence: f64, sure: bool) -> Box<Pick> {
    carrying(choice, confidence, sure, Some(declared()))
}

fn carrying(choice: &str, confidence: f64, sure: bool, no_match: Option<String>) -> Box<Pick> {
    Box::new(Pick {
        choice: choice.into(),
        confidence,
        sure,
        no_match,
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
    let v = open("opus");
    let got = d(&v, "sonnet", 12_000);
    assert_eq!(
        got,
        Decision {
            profile: "opus".into(),
            reason: Reason::Verdict,
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
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::Verdict, false)
    );
    assert_eq!(got.hold, HoldNext::Clear);
}

/// A pick under the `model` question's bar stays on the turn's own profile and
/// says `unsure`.
///
/// **The pack says otherwise, and the two are knowingly apart**
/// (theseus-ig13o). route.v10's `model` question declares
/// `no_match = "sonnet"` and the replay grades an under-bar answer as
/// `sonnet`; this path runs neither. See
/// `the_model_question_declares_a_default_the_live_path_does_not_run`.
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

/// **The divergence, pinned** (theseus-ig13o). The `model` question declares a
/// default, the answer carries it, `Pick::ran` says what it would run — and
/// `decide` runs none of it, standing on the turn's own profile instead. Three
/// facts in one test, because each alone is misleading:
///
/// 1. the pack declares `sonnet`, and the answer carries it off the pack, so
///    the value is genuinely in hand on the live path;
/// 2. `Pick::ran` applies it, so honouring it is one line in `decide`;
/// 3. `decide` does not, for any declared value — a planted `haiku` changes
///    nothing — so this is not a value that happens to agree, it is a value
///    that is not read.
///
/// Running it moves twelve tests, two of which are named rules in the opposite
/// direction (`tests_task_brief::a_verdict_under_its_bar_leaves_the_parents_model`
/// and `tests_data_open::a_judged_clean_session_runs_on_the_restricted_model`),
/// so which of the pack and the daemon is wrong is the owner's call. This test
/// exists so that the day it is decided, closing the gap is a deliberate edit
/// here and not a silent one.
#[test]
fn the_model_question_declares_a_default_the_live_path_does_not_run() {
    // 1. Declared, and carried onto the answer by the ask.
    assert_eq!(declared(), "sonnet");
    let carried = theseus_judge::pack::by_name("route.v10")
        .expect("route.v10")
        .ask(&theseus_judge::pack::Dynamic::default())
        .iter()
        .find(|a| a.def == "model")
        .and_then(|a| a.no_match.clone());
    assert_eq!(carried.as_deref(), Some("sonnet"));
    // 2. `ran` applies it.
    let under = carrying("opus", 0.45, false, carried);
    assert_eq!(under.ran(), Some("sonnet"));
    assert_eq!(pick("opus", 0.95, true).ran(), Some("opus"));
    // 3. `decide` does not, for the declared value or for a planted one.
    for planted in [Some("sonnet"), Some("haiku"), None] {
        let v = Verdict {
            model: Some(carrying("opus", 0.45, false, planted.map(str::to_string))),
            ..open("opus")
        };
        let got = d(&v, "haiku", 1_000);
        assert_eq!(
            (got.profile.as_str(), got.reason, got.switch),
            ("haiku", Reason::Unsure, false),
            "declared {planted:?}: the turn's own profile, whatever the pack says"
        );
    }
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

#[test]
fn nothing_of_the_old_walk_is_left() {
    // No step down: a session Jev moved up goes down only because Jev says so,
    // at once, with no row of agreeing verdicts.
    let v = open("sonnet");
    let got = decide(&cfg(), &five_options(), &ask(&v, "opus", 1_000));
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::Verdict, true)
    );
    // The answer names a model: the dearest on offer, from the cheapest.
    let v = open("fable");
    let got = decide(&cfg(), &five_options(), &ask(&v, "haiku", 1_000));
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("fable", Reason::Verdict, true)
    );
}

#[test]
fn a_verdict_without_a_model_answer_stays_on_the_base() {
    let v = super::tests::verdict("sophisticated", 0.95);
    assert!(v.model.is_none());
    let got = decide(
        &cfg(),
        &five_options(),
        &super::tests::ask(&v, "sonnet", 1_000),
    );
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("sonnet", Reason::NoVerdict, false)
    );
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

// A task's checkpoint and brief ask the same rule as a turn does (`decide`).

/// A task's checkpoint takes Jev's `model` pick, up or down, with no base to
/// return to and no place cap to refuse it.
#[test]
fn a_checkpoint_takes_jevs_pick_up_or_down_and_no_cap_refuses_it() {
    let ps = five_options();
    for (pick, from, to) in [("opus", "sonnet", "opus"), ("haiku", "opus", "haiku")] {
        let v = open(pick);
        let got = decide(
            &cfg(),
            &ps,
            &Ask {
                verdict: &v,
                base: from,
                context_tokens: 1_000,
                hold: None,
                images: false,
                cleared: false,
            },
        );
        assert_eq!(
            (got.profile.as_str(), got.reason, got.switch),
            (to, Reason::Verdict, true),
            "{from} -> {pick}"
        );
    }
}

/// A checkpoint's cache hold is the rule's: a move at or above the cold-switch
/// estimate waits one agreeing checkpoint, and the agreeing one moves.
#[test]
fn a_checkpoint_move_over_the_cold_size_waits_for_one_agreeing_verdict() {
    let ps = five_options();
    let v = open("opus");
    let ask = |hold| Ask {
        verdict: &v,
        base: "sonnet",
        context_tokens: COLD + 1,
        hold,
        images: false,
        cleared: false,
    };
    let first = decide(&cfg(), &ps, &ask(None));
    assert_eq!(
        (first.profile.as_str(), first.reason),
        ("sonnet", Reason::CacheHold)
    );
    let HoldNext::Set(held) = first.hold else {
        panic!("the first sets a hold");
    };
    let second = decide(&cfg(), &ps, &ask(Some(&held)));
    assert_eq!(
        (second.profile.as_str(), second.reason, second.switch),
        ("opus", Reason::Verdict, true)
    );
}

/// A task's brief takes Jev's pick as the task's starting model, whatever the
/// parent runs on: the step asks with nothing to keep warm (`context_tokens: 0`)
/// and nothing held.
#[test]
fn a_brief_starts_a_task_on_jevs_pick() {
    let v = open("haiku");
    let got = d(&v, "opus", 0);
    assert_eq!(
        (got.profile.as_str(), got.reason, got.switch),
        ("haiku", Reason::Verdict, true)
    );
}
